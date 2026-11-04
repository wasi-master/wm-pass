//! Site icons. Off by default, because fetching one tells somebody which sites are in
//! the vault: the site itself, or DuckDuckGo. Bundled issuer icons cover common sites
//! without any of this.

use crate::http::{Call, NetError, Request};

/// Where icons come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconSource {
    /// No fetching.
    #[default]
    Off,
    /// The site's own `/favicon.ico`.
    Site,
    /// The site's own `/apple-touch-icon.png`: larger than a favicon where a site has one.
    SiteTouch,
    /// DuckDuckGo's icon service.
    DuckDuckGo,
}

/// The largest icon accepted.
pub const MAX_ICON: usize = 256 * 1024;

fn looks_like_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG")
        || bytes.starts_with(&[0, 0, 1, 0])
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF8")
        || (bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
}

/// Fetches an icon for a host. SVG is refused, because it can carry scripts.
pub fn fetch(source: IconSource, host: &str) -> Result<Call<Vec<u8>>, NetError> {
    let valid = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    if !valid {
        return Err(NetError::Invalid("that is not a host name".into()));
    }
    let url = match source {
        IconSource::Off => return Err(NetError::Blocked),
        IconSource::Site => format!("https://{host}/favicon.ico"),
        IconSource::SiteTouch => format!("https://{host}/apple-touch-icon.png"),
        IconSource::DuckDuckGo => format!("https://icons.duckduckgo.com/ip3/{host}.ico"),
    };
    Ok(Call::new(Request::get(url), |response| {
        let body = &response.ok()?.body;
        if body.len() > MAX_ICON || !looks_like_image(body) {
            return Err(NetError::BadAnswer);
        }
        Ok(body.clone())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::NetPolicy;
    use crate::http::fake::FakeHttp;

    #[test]
    fn off_fetches_nothing_and_images_are_checked() {
        assert!(fetch(IconSource::Off, "x.example").is_err());
        let png = FakeHttp::new().answer("https://x.example", 200, "\u{0}\u{0}\u{1}\u{0}data");
        assert!(
            fetch(IconSource::Site, "x.example")
                .unwrap()
                .run(&png, NetPolicy::Any)
                .is_ok()
        );
        let svg = FakeHttp::new().answer("https://x.example", 200, "<svg onload=alert(1)>");
        assert_eq!(
            fetch(IconSource::Site, "x.example")
                .unwrap()
                .run(&svg, NetPolicy::Any),
            Err(NetError::BadAnswer)
        );
        assert!(fetch(IconSource::Site, "x.example/../evil").is_err());
    }
}
