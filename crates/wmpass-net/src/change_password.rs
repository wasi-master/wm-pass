//! Where a site lets its users change their password, found the way the W3C's well-known
//! address proposes: `/.well-known/change-password` sends a browser on to the right page.
//!
//! Most sites have no such address, and many answer every address with their home page, so an
//! answer is trusted only when the site first says no to an address that cannot exist. Redirects
//! are followed here, by hand, because the client follows none: each hop has to stay on https.

use url::Url;

use crate::http::{Http, NetError, NetPolicy, Request, Response, check};

/// Redirects followed before giving up.
const HOPS: usize = 5;
/// An address no site has. A site that answers it with a page answers everything that way.
const PROBE: &str = "/.well-known/resource-that-should-not-exist-whose-status-code-should-not-be-200";
const CHANGE: &str = "/.well-known/change-password";
/// Shorter than an ordinary call: this runs while a page is open, and a slow site just has no link.
const TIMEOUT_MS: u64 = 8_000;

/// A host this may ask: a dotted name of letters, digits and hyphens. Anything else, such as an
/// address with a user or a path in it, is not sent.
fn plain_host(host: &str) -> bool {
    host.contains('.')
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// GETs `url` and follows its redirects. The address it ended at, and that answer.
fn follow(http: &dyn Http, policy: NetPolicy, url: String) -> Result<(Url, Response), NetError> {
    let mut at = Url::parse(&url).map_err(|_| NetError::Invalid("that address is not valid".into()))?;
    for _ in 0..=HOPS {
        let mut request = Request::get(at.as_str());
        request.timeout_ms = TIMEOUT_MS;
        check(&request, policy)?;
        let response = http.send(&request)?;
        if !(300..400).contains(&response.status) {
            return Ok((at, response));
        }
        let Some(location) = response.header("location") else {
            return Ok((at, response));
        };
        let next = at.join(location).map_err(|_| NetError::BadAnswer)?;
        if next.scheme() != "https" {
            return Err(NetError::Insecure);
        }
        at = next;
    }
    Err(NetError::BadAnswer)
}

/// The page where `host` changes passwords, or `None` when the site does not offer one this way.
/// An error means no answer, such as no network, and is worth asking again later; `None` is the
/// site's answer.
pub fn find(http: &dyn Http, policy: NetPolicy, host: &str) -> Result<Option<String>, NetError> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if !plain_host(&host) {
        return Err(NetError::Invalid("that is not a site's name".into()));
    }
    let (_, probe) = follow(http, policy, format!("https://{host}{PROBE}"))?;
    if (200..300).contains(&probe.status) {
        return Ok(None);
    }
    let (page, answer) = match follow(http, policy, format!("https://{host}{CHANGE}")) {
        Ok(found) => found,
        // A redirect off https, or round in circles, is a site without a page to offer.
        Err(NetError::Insecure | NetError::BadAnswer) => return Ok(None),
        Err(error) => return Err(error),
    };
    // Landing on the home page is the site not knowing the address, not the site pointing at it.
    if !(200..300).contains(&answer.status) || page.path() == "/" {
        return Ok(None);
    }
    Ok(Some(page.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;

    const SITE: &str = "https://example.com";

    fn probe_missing(fake: FakeHttp) -> FakeHttp {
        fake.answer(&format!("{SITE}{PROBE}"), 404, "")
    }

    #[test]
    fn a_site_that_sends_the_address_on_is_found() {
        let fake = probe_missing(FakeHttp::new())
            .answer_with(&format!("{SITE}{CHANGE}"), 302, b"", &[("Location", "/account/security?tab=password")])
            .answer(&format!("{SITE}/account/security"), 200, "<html>");
        let found = find(&fake, NetPolicy::Any, "Example.com").unwrap();
        assert_eq!(found.as_deref(), Some("https://example.com/account/security?tab=password"));
    }

    #[test]
    fn a_site_that_answers_everything_is_not_believed() {
        let fake = FakeHttp::new().answer(SITE, 200, "<html>");
        assert_eq!(find(&fake, NetPolicy::Any, "example.com").unwrap(), None);
    }

    #[test]
    fn a_site_without_the_address_has_no_page() {
        let fake = FakeHttp::new().answer(SITE, 404, "");
        assert_eq!(find(&fake, NetPolicy::Any, "example.com").unwrap(), None);
    }

    #[test]
    fn landing_on_the_home_page_is_no_page() {
        let fake = probe_missing(FakeHttp::new())
            .answer_with(&format!("{SITE}{CHANGE}"), 301, b"", &[("Location", "https://www.example.com/")])
            .answer("https://www.example.com/", 200, "<html>");
        assert_eq!(find(&fake, NetPolicy::Any, "example.com").unwrap(), None);
    }

    #[test]
    fn a_redirect_off_https_is_no_page() {
        let fake = probe_missing(FakeHttp::new())
            .answer_with(&format!("{SITE}{CHANGE}"), 302, b"", &[("Location", "http://example.com/password")]);
        assert_eq!(find(&fake, NetPolicy::Any, "example.com").unwrap(), None);
    }

    #[test]
    fn nothing_is_asked_offline_or_of_an_odd_host() {
        let fake = FakeHttp::new().answer(SITE, 404, "");
        assert_eq!(find(&fake, NetPolicy::Offline, "example.com"), Err(NetError::Blocked));
        assert!(find(&fake, NetPolicy::Any, "user@example.com").is_err());
        assert!(find(&fake, NetPolicy::Any, "example.com/path").is_err());
        assert!(fake.asked.lock().unwrap().is_empty());
    }
}
