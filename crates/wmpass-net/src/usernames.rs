//! Is a username taken? Each check tells that site the name being looked at, so this is
//! opt-in and runs one site at a time. Sites that block anonymous lookups or answer the
//! same for every name, including X, Instagram, npm and PyPI, are left out: a guess is
//! worse than no answer here. `tests/live.rs` checks that each listed signal still holds.

use crate::http::{Call, NetError, Request, Response};
use wmpass_core::otp::percent_encode;

/// What a site said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Nobody has it.
    Available,
    /// Somebody has it.
    Taken,
    /// The site gave no clear answer.
    Unknown,
}

/// How to tell from an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// 200 means taken, 404 means free.
    Status,
    /// With a 200, this text in the body means free.
    BodyMeansFree(&'static str),
}

/// A site that can be asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    /// Display name.
    pub name: &'static str,
    /// Address, with `{}` for the name.
    pub url: &'static str,
    /// How to read the answer.
    pub signal: Signal,
}

/// Every supported site.
pub const SITES: &[Site] = &[
    Site {
        name: "GitHub",
        url: "https://api.github.com/users/{}",
        signal: Signal::Status,
    },
    Site {
        name: "GitLab",
        url: "https://gitlab.com/api/v4/users?username={}",
        signal: Signal::BodyMeansFree("[]"),
    },
    Site {
        name: "Codeberg",
        url: "https://codeberg.org/api/v1/users/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Docker Hub",
        url: "https://hub.docker.com/v2/users/{}/",
        signal: Signal::Status,
    },
    Site {
        name: "crates.io",
        url: "https://crates.io/api/v1/users/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Mastodon",
        url: "https://mastodon.social/api/v1/accounts/lookup?acct={}",
        signal: Signal::Status,
    },
    Site {
        name: "Hacker News",
        url: "https://hacker-news.firebaseio.com/v0/user/{}.json",
        signal: Signal::BodyMeansFree("null"),
    },
    Site {
        name: "Steam",
        url: "https://steamcommunity.com/id/{}",
        signal: Signal::BodyMeansFree("The specified profile could not be found"),
    },
];

fn read(signal: Signal, response: &Response) -> Result<Availability, NetError> {
    match (signal, response.status) {
        (_, 429) => Err(NetError::RateLimited(response.retry_after)),
        (Signal::Status, 200) => Ok(Availability::Taken),
        (Signal::Status, 404) => Ok(Availability::Available),
        (Signal::BodyMeansFree(marker), 200) => {
            let body = response.text()?.trim();
            let free = if marker.len() <= 4 {
                body == marker
            } else {
                body.contains(marker)
            };
            Ok(if free {
                Availability::Available
            } else {
                Availability::Taken
            })
        }
        (Signal::BodyMeansFree(_), 404) => Ok(Availability::Available),
        _ => Ok(Availability::Unknown),
    }
}

/// Builds the check for one site. Names outside letters, digits, dot, dash and
/// underscore are refused, since none of these sites allow others.
pub fn check(site: Site, name: &str) -> Result<Call<Availability>, NetError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid {
        return Err(NetError::Invalid(
            "usernames here are letters, digits, dots, dashes and underscores".into(),
        ));
    }
    let url = site.url.replace("{}", &percent_encode(name));
    let mut request = Request::get(url);
    request.timeout_ms = 8_000;
    Ok(Call::new(request, move |response| {
        read(site.signal, response)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::NetPolicy;
    use crate::http::fake::FakeHttp;

    const ANY: NetPolicy = NetPolicy::Any;

    fn site(name: &str) -> Site {
        *SITES.iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn status_sites() {
        let hub = site("GitHub");
        let taken = FakeHttp::new().answer("https://api.github.com", 200, "{}");
        assert_eq!(
            check(hub, "octo").unwrap().run(&taken, ANY),
            Ok(Availability::Taken)
        );
        let free = FakeHttp::new().answer("https://api.github.com", 404, "");
        assert_eq!(
            check(hub, "octo").unwrap().run(&free, ANY),
            Ok(Availability::Available)
        );
        let odd = FakeHttp::new().answer("https://api.github.com", 403, "");
        assert_eq!(
            check(hub, "octo").unwrap().run(&odd, ANY),
            Ok(Availability::Unknown)
        );
        let limited = FakeHttp::new().answer("https://api.github.com", 429, "");
        assert!(matches!(
            check(hub, "octo").unwrap().run(&limited, ANY),
            Err(NetError::RateLimited(_))
        ));
    }

    #[test]
    fn body_sites() {
        let lab = site("GitLab");
        let free = FakeHttp::new().answer("https://gitlab.com", 200, "[]\n");
        assert_eq!(
            check(lab, "x").unwrap().run(&free, ANY),
            Ok(Availability::Available)
        );
        let taken = FakeHttp::new().answer("https://gitlab.com", 200, r#"[{"id":1}]"#);
        assert_eq!(
            check(lab, "x").unwrap().run(&taken, ANY),
            Ok(Availability::Taken)
        );
        let steam = site("Steam");
        let gone = FakeHttp::new().answer(
            "https://steamcommunity.com",
            200,
            "<html>The specified profile could not be found.</html>",
        );
        assert_eq!(
            check(steam, "x").unwrap().run(&gone, ANY),
            Ok(Availability::Available)
        );
    }

    #[test]
    fn hostile_names_never_reach_a_url() {
        for bad in [
            "",
            "a/b",
            "../admin",
            "a b",
            "a?b=c",
            "a#b",
            "%2e",
            "x@y",
            &"a".repeat(65),
        ] {
            assert!(check(site("GitHub"), bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_site_uses_https_and_has_a_slot() {
        for site in SITES {
            assert!(
                site.url.starts_with("https://") && site.url.contains("{}"),
                "{}",
                site.name
            );
        }
    }
}
