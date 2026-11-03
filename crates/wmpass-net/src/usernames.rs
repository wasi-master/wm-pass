//! Is a username taken? Each check tells that site the name being looked at, so this is
//! opt-in and runs one site at a time. Sites that block anonymous lookups or answer the
//! same for every name, including X, Instagram, Reddit, Twitch, npm and PyPI, are left
//! out: a guess is worse than no answer here. `tests/live.rs` checks that each listed
//! signal still holds.
//!
//! Some sites only show their answer once their own scripts have run. [`PAGE_SITES`]
//! are read by an app that can load them in a private, hidden browser view, and
//! [`LINK_SITES`] give no answer anything can read, so they are only opened for a person
//! to look at.

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
    /// 200 means taken; this status with this text in the body means free.
    ErrorMeansFree(u16, &'static str),
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
    Site {
        name: "Bitbucket",
        url: "https://api.bitbucket.org/2.0/workspaces/{}",
        signal: Signal::Status,
    },
    Site {
        name: "SourceHut",
        url: "https://sr.ht/~{}/",
        signal: Signal::Status,
    },
    Site {
        name: "SourceForge",
        url: "https://sourceforge.net/u/{}/profile",
        signal: Signal::Status,
    },
    Site {
        name: "Launchpad",
        url: "https://api.launchpad.net/devel/~{}",
        signal: Signal::Status,
    },
    Site {
        name: "Hugging Face",
        url: "https://huggingface.co/api/users/{}/overview",
        signal: Signal::Status,
    },
    Site {
        name: "RubyGems",
        url: "https://rubygems.org/api/v1/profiles/{}.json",
        signal: Signal::Status,
    },
    Site {
        name: "DEV",
        url: "https://dev.to/api/users/by_username?url={}",
        signal: Signal::Status,
    },
    Site {
        name: "Lobsters",
        url: "https://lobste.rs/~{}.json",
        signal: Signal::Status,
    },
    Site {
        name: "Codewars",
        url: "https://www.codewars.com/api/v1/users/{}",
        signal: Signal::Status,
    },
    Site {
        name: "HackerOne",
        url: "https://hackerone.com/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Pastebin",
        url: "https://pastebin.com/u/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Bluesky",
        url: "https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile?actor={}.bsky.social",
        signal: Signal::ErrorMeansFree(400, "Profile not found"),
    },
    Site {
        name: "Wikipedia",
        url: "https://en.wikipedia.org/w/api.php?action=query&list=users&format=json&ususers={}",
        signal: Signal::BodyMeansFree(r#""missing":""#),
    },
    Site {
        name: "Linktree",
        url: "https://linktr.ee/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Vimeo",
        url: "https://vimeo.com/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Dribbble",
        url: "https://dribbble.com/{}/about",
        signal: Signal::Status,
    },
    Site {
        name: "Unsplash",
        url: "https://unsplash.com/@{}/collections",
        signal: Signal::Status,
    },
    Site {
        name: "Lichess",
        url: "https://lichess.org/api/user/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Chess.com",
        url: "https://api.chess.com/pub/player/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Minecraft",
        url: "https://api.mojang.com/users/profiles/minecraft/{}",
        signal: Signal::Status,
    },
    Site {
        name: "Duolingo",
        url: "https://www.duolingo.com/2017-06-30/users?username={}",
        signal: Signal::BodyMeansFree(r#"{"users":[]}"#),
    },
    Site {
        name: "MyAnimeList",
        url: "https://myanimelist.net/profile/{}",
        signal: Signal::Status,
    },
    Site {
        name: "YouTube",
        url: "https://www.youtube.com/@{}",
        signal: Signal::Status,
    },
    Site {
        name: "Snapchat",
        url: "https://www.snapchat.com/@{}",
        signal: Signal::Status,
    },
];

/// A site read from the page it draws, after its scripts have run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageSite {
    /// Display name.
    pub name: &'static str,
    /// Address, with `{}` for the name. It asks for English, which the markers are in.
    pub url: &'static str,
    /// The page may only load frames from this domain and the hosts under it.
    pub domain: &'static str,
    /// Lowercase text on the page that means nobody has the name.
    pub free: &'static str,
    /// Lowercase text on the page that means somebody has it.
    pub taken: &'static str,
}

/// Every site read from its page. A login wall shows neither marker, so it reads as no
/// answer rather than a guess.
pub const PAGE_SITES: &[PageSite] = &[
    PageSite {
        name: "TikTok",
        url: "https://www.tiktok.com/@{}?lang=en",
        domain: "tiktok.com",
        free: "find this account",
        taken: "followers",
    },
    PageSite {
        name: "Instagram",
        url: "https://www.instagram.com/{}/?hl=en",
        domain: "instagram.com",
        free: "the link may be broken",
        taken: "followers",
    },
];

/// A site whose answer cannot be read: it needs a login, or says the same for a free name
/// as for a suspended one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkSite {
    /// Display name.
    pub name: &'static str,
    /// The profile's address, with `{}` for the name.
    pub url: &'static str,
}

/// Every site that is only opened for a person to look at.
pub const LINK_SITES: &[LinkSite] = &[
    LinkSite {
        name: "X",
        url: "https://x.com/{}",
    },
    LinkSite {
        name: "Threads",
        url: "https://www.threads.com/@{}",
    },
    LinkSite {
        name: "Reddit",
        url: "https://www.reddit.com/user/{}",
    },
    LinkSite {
        name: "Facebook",
        url: "https://www.facebook.com/{}",
    },
    LinkSite {
        name: "Pinterest",
        url: "https://www.pinterest.com/{}/",
    },
    LinkSite {
        name: "Twitch",
        url: "https://www.twitch.tv/{}",
    },
    LinkSite {
        name: "Telegram",
        url: "https://t.me/{}",
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
        (Signal::ErrorMeansFree(..), 200) => Ok(Availability::Taken),
        (Signal::ErrorMeansFree(code, marker), status) if status == code => {
            Ok(if response.text()?.contains(marker) {
                Availability::Available
            } else {
                Availability::Unknown
            })
        }
        _ => Ok(Availability::Unknown),
    }
}

/// Fills a name into an address. Names outside letters, digits, dot, dash and
/// underscore are refused, since none of these sites allow others, and so are names
/// of dots alone, which a path would read as `.` or `..`.
fn address(template: &str, name: &str) -> Result<String, NetError> {
    let valid = name.chars().any(|c| c != '.')
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !valid {
        return Err(NetError::Invalid(
            "usernames here are letters, digits, dots, dashes and underscores".into(),
        ));
    }
    Ok(template.replace("{}", &percent_encode(name)))
}

/// Builds the check for one site.
pub fn check(site: Site, name: &str) -> Result<Call<Availability>, NetError> {
    let mut request = Request::get(address(site.url, name)?);
    request.timeout_ms = 8_000;
    Ok(Call::new(request, move |response| {
        read(site.signal, response)
    }))
}

/// The page to load for one name.
pub fn page_url(site: &PageSite, name: &str) -> Result<String, NetError> {
    address(site.url, name)
}

/// The profile to open for one name.
pub fn link_url(site: &LinkSite, name: &str) -> Result<String, NetError> {
    address(site.url, name)
}

/// What the page's text says so far, or `None` while it shows neither marker.
pub fn read_page(site: &PageSite, text: &str) -> Option<Availability> {
    let text = text.to_lowercase().replace('\u{2019}', "'");
    if text.contains(site.free) {
        Some(Availability::Available)
    } else if text.contains(site.taken) {
        Some(Availability::Taken)
    } else {
        None
    }
}

/// Whether the hidden view may load this address into a frame: only https on the site's
/// own domain, so a page cannot send it to the app's own pages or anywhere else.
pub fn may_visit(site: &PageSite, url: &str) -> bool {
    if url == "about:blank" || url == "about:srcdoc" {
        return true;
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains(['@', '\\']) {
        return false;
    }
    let host = authority
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    host == site.domain
        || host
            .strip_suffix(site.domain)
            .is_some_and(|start| start.ends_with('.'))
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
    fn error_sites() {
        let sky = site("Bluesky");
        let host = "https://public.api.bsky.app";
        let taken = FakeHttp::new().answer(host, 200, r#"{"did":"did:plc:x"}"#);
        assert_eq!(
            check(sky, "x").unwrap().run(&taken, ANY),
            Ok(Availability::Taken)
        );
        let free = FakeHttp::new().answer(
            host,
            400,
            r#"{"error":"InvalidRequest","message":"Profile not found"}"#,
        );
        assert_eq!(
            check(sky, "x").unwrap().run(&free, ANY),
            Ok(Availability::Available)
        );
        let refused = FakeHttp::new().answer(
            host,
            400,
            r#"{"error":"InvalidRequest","message":"Invalid AT identifier"}"#,
        );
        assert_eq!(
            check(sky, "x").unwrap().run(&refused, ANY),
            Ok(Availability::Unknown)
        );
    }

    fn page(name: &str) -> PageSite {
        *PAGE_SITES.iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn pages_read_their_markers() {
        let tok = page("TikTok");
        assert_eq!(read_page(&tok, "TikTok\n\nLoading"), None);
        assert_eq!(
            read_page(&tok, "Couldn\u{2019}t find this account"),
            Some(Availability::Available)
        );
        assert_eq!(
            read_page(&tok, "khaby.lame\n82\nFollowing\n163M\nFollowers"),
            Some(Availability::Taken)
        );
        let gram = page("Instagram");
        assert_eq!(
            read_page(
                &gram,
                "Log in\nProfile isn't available\nThe link may be broken, or the profile may have been removed."
            ),
            Some(Availability::Available)
        );
        assert_eq!(read_page(&gram, "Log in\nContinue with Facebook"), None);
        assert_eq!(
            page_url(&gram, "natgeo").unwrap(),
            "https://www.instagram.com/natgeo/?hl=en"
        );
        assert!(page_url(&gram, "../x").is_err());
    }

    #[test]
    fn pages_stay_on_their_own_domain() {
        let tok = page("TikTok");
        for good in [
            "https://www.tiktok.com/@x",
            "https://tiktok.com",
            "https://m.tiktok.com:443/a?b",
            "about:blank",
        ] {
            assert!(may_visit(&tok, good), "{good}");
        }
        for bad in [
            "http://www.tiktok.com/",
            "tauri://localhost/index.html",
            "https://tauri.localhost/",
            "https://evil.com/?tiktok.com",
            "https://eviltiktok.com/",
            "https://tiktok.com.evil.com/",
            "https://tiktok.com@evil.com/",
            "https://evil.com\\@tiktok.com/",
            "javascript:alert(1)",
            "data:text/html,x",
            "blob:https://www.tiktok.com/1",
        ] {
            assert!(!may_visit(&tok, bad), "{bad}");
        }
    }

    #[test]
    fn links_fill_the_name() {
        let x = LINK_SITES.iter().find(|s| s.name == "X").unwrap();
        assert_eq!(link_url(x, "nasa").unwrap(), "https://x.com/nasa");
        assert!(link_url(x, "..").is_err());
    }

    #[test]
    fn site_names_are_unique() {
        let names: Vec<&str> = SITES
            .iter()
            .map(|s| s.name)
            .chain(PAGE_SITES.iter().map(|s| s.name))
            .chain(LINK_SITES.iter().map(|s| s.name))
            .collect();
        for (i, a) in names.iter().enumerate() {
            assert!(!names[i + 1..].contains(a), "{a}");
        }
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
            ".",
            "..",
            &"a".repeat(65),
        ] {
            assert!(check(site("GitHub"), bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_site_uses_https_and_has_a_slot() {
        let urls = SITES
            .iter()
            .map(|s| s.url)
            .chain(PAGE_SITES.iter().map(|s| s.url))
            .chain(LINK_SITES.iter().map(|s| s.url));
        for url in urls {
            assert!(url.starts_with("https://") && url.contains("{}"), "{url}");
        }
        for site in PAGE_SITES {
            assert!(
                may_visit(site, &page_url(site, "x").unwrap()),
                "{}",
                site.name
            );
            assert_eq!(site.free, site.free.to_lowercase());
            assert_eq!(site.taken, site.taken.to_lowercase());
        }
    }
}
