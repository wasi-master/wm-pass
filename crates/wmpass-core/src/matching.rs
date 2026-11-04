//! Which saved logins may be offered where. Getting this wrong hands a password to
//! the wrong site, so the rules lean towards offering less.

use base64::Engine;
use regex::Regex;
use url::Url;

use crate::model::{Entry, SiteMatcher};

/// Where a fill was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target<'a> {
    /// A page. The URL must come from a trusted source: the browser's own API.
    Web {
        /// Full URL.
        url: &'a str,
    },
    /// An Android app, with the certificate digest the system reports.
    App {
        /// Package name.
        package: &'a str,
        /// SHA-256 of its signing certificate.
        cert_sha256: &'a str,
    },
}

/// How firmly a login matches, strongest last. Anything below
/// [`MatchLevel::Domain`] must not be filled without the person confirming.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchLevel {
    /// The package name matches but no certificate is on file, and a package name
    /// alone proves nothing. Requires a tap naming what is being filled.
    Unverified,
    /// Same registrable domain, different host.
    Domain,
    /// Exactly the saved host, or the saved app with its certificate.
    Exact,
}

/// First labels of package names as people write them: a top-level domain, reversed.
const PACKAGE_ROOTS: &[&str] = &[
    "com", "org", "net", "io", "app", "dev", "me", "co", "de", "uk", "bd", "in", "us", "tv", "info", "ru", "fr",
    "jp", "cn", "br", "edu", "gov", "nl", "it", "es", "ca", "au", "kr", "tw", "pl", "ch", "se", "at", "be", "mobi",
    "ai", "gg", "xyz", "eu", "id", "my", "pk", "vn", "tr", "ir", "mx", "ar", "za", "ua", "cz", "fi", "no", "dk",
    "gr", "hu", "ro", "pt", "il", "sg", "hk", "th", "ph", "nz",
];

/// Whether text saved where a website goes is really an Android package name, such as
/// `com.bkash.customerapp`: no scheme, path or port, a top-level domain first and a last label
/// that is no top-level domain. A guess, so it only decides when nothing else says.
pub fn looks_like_package(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || !text.is_ascii() || text.contains(['/', ':', '@', '?', '#']) || text.contains(char::is_whitespace) {
        return false;
    }
    let labels: Vec<&str> = text.split('.').collect();
    if labels.len() < 2
        || labels
            .iter()
            .any(|label| label.is_empty() || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
    {
        return false;
    }
    let first = labels[0].to_ascii_lowercase();
    let last = labels[labels.len() - 1].to_ascii_lowercase();
    PACKAGE_ROOTS.contains(&first.as_str()) && !psl::suffix(last.as_bytes()).is_some_and(|suffix| suffix.is_known())
}

/// The Android app an address saved for a login names, in the forms exports write it:
/// `androidapp://package` (Bitwarden), `android://<certificate digest>@package/` (Google
/// Password Manager, the digest in base64) and a bare package name. The digest comes back as
/// lowercase hex, as the system reports it.
pub fn app_of_address(address: &str) -> Option<(String, Option<String>)> {
    let address = address.trim();
    let lower = address.to_ascii_lowercase();
    if lower.starts_with("androidapp://") {
        let package = address["androidapp://".len()..].trim_end_matches('/');
        return (!package.is_empty()).then(|| (package.to_owned(), None));
    }
    if lower.starts_with("android://") {
        let rest = address["android://".len()..].trim_end_matches('/');
        let (digest, package) = rest.rsplit_once('@').unwrap_or(("", rest));
        if package.is_empty() {
            return None;
        }
        let engines = [base64::engine::general_purpose::STANDARD, base64::engine::general_purpose::URL_SAFE];
        let cert = engines
            .iter()
            .find_map(|engine| engine.decode(digest).ok())
            .filter(|bytes| bytes.len() == 32)
            .map(|bytes| crate::vault::hex(&bytes));
        return Some((package.to_owned(), cert));
    }
    looks_like_package(address).then(|| (address.to_owned(), None))
}

/// What an importer saves for an address: the app it names, or the address as a website.
pub fn matcher_from_address(address: &str) -> SiteMatcher {
    match app_of_address(address) {
        Some((package, cert_sha256)) => SiteMatcher::AndroidApp { package, cert_sha256 },
        None => SiteMatcher::Url(address.to_owned()),
    }
}

/// A matcher as it is meant: an app saved as a website address reads as the app. Entries
/// imported before importers knew these forms keep them, so they are read this way.
pub fn normalized_matcher(matcher: &SiteMatcher) -> SiteMatcher {
    match matcher {
        SiteMatcher::Url(address) => matcher_from_address(address),
        other => other.clone(),
    }
}

/// The host of a saved value that may lack a scheme.
pub fn host_of(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let parsed = Url::parse(trimmed)
        .ok()
        .filter(|url| url.has_host())
        .or_else(|| Url::parse(&format!("https://{trimmed}")).ok())?;
    parsed
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

/// The registrable domain (eTLD+1), or the host itself for IPs and local names.
pub fn registrable_domain(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return host;
    }
    psl::domain_str(&host).map_or(host.clone(), str::to_owned)
}

fn scheme_downgrade(saved: &str, page: &Url) -> bool {
    saved
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("https://")
        && page.scheme() == "http"
}

fn web_level(matcher: &SiteMatcher, page: &Url, url: &str) -> Option<MatchLevel> {
    let page_host = page.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    match matcher {
        SiteMatcher::Domain(domain) => {
            let saved = registrable_domain(&host_of(domain)?);
            let same = registrable_domain(&page_host) == saved;
            same.then_some(if page_host == saved {
                MatchLevel::Exact
            } else {
                MatchLevel::Domain
            })
        }
        SiteMatcher::Host(host) => (host_of(host)? == page_host).then_some(MatchLevel::Exact),
        SiteMatcher::Url(saved) => {
            if scheme_downgrade(saved, page) || app_of_address(saved).is_some() {
                return None;
            }
            let host = host_of(saved)?;
            if host == page_host {
                Some(MatchLevel::Exact)
            } else {
                (registrable_domain(&host) == registrable_domain(&page_host))
                    .then_some(MatchLevel::Domain)
            }
        }
        SiteMatcher::Regex(pattern) => Regex::new(pattern)
            .ok()?
            .is_match(url)
            .then_some(MatchLevel::Domain),
        SiteMatcher::AndroidApp { .. } | SiteMatcher::Never => None,
    }
}

fn app_level(matcher: &SiteMatcher, package: &str, cert: &str) -> Option<MatchLevel> {
    let SiteMatcher::AndroidApp {
        package: saved,
        cert_sha256,
    } = normalized_matcher(matcher)
    else {
        return None;
    };
    if saved != package {
        return None;
    }
    match &cert_sha256 {
        Some(digest) => digest
            .eq_ignore_ascii_case(cert)
            .then_some(MatchLevel::Exact),
        None => Some(MatchLevel::Unverified),
    }
}

/// How an entry matches a target, if at all. Trashed and shelved entries never do.
pub fn match_entry(entry: &Entry, target: Target<'_>) -> Option<MatchLevel> {
    if entry.is_trashed() || entry.common.hidden.v {
        return None;
    }
    let matchers = &entry.login()?.site_matchers.v;
    if matchers.iter().any(|m| matches!(m, SiteMatcher::Never)) {
        return None;
    }
    match target {
        Target::Web { url } => {
            let page = Url::parse(url).ok()?;
            if !matches!(page.scheme(), "http" | "https") {
                return None;
            }
            matchers
                .iter()
                .filter_map(|m| web_level(m, &page, url))
                .max()
        }
        Target::App {
            package,
            cert_sha256,
        } => matchers
            .iter()
            .filter_map(|m| app_level(m, package, cert_sha256))
            .max(),
    }
}

/// Every matching entry, strongest first.
pub fn matches_for<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    target: Target<'_>,
) -> Vec<(&'a Entry, MatchLevel)> {
    let mut found: Vec<_> = entries
        .into_iter()
        .filter_map(|e| match_entry(e, target).map(|level| (e, level)))
        .collect();
    found.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.common.title.v.cmp(&b.0.common.title.v))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Hlc;
    use crate::model::Lww;
    use crate::vault::login_entry;

    fn login(matchers: Vec<SiteMatcher>) -> Entry {
        let at = Hlc::default();
        let mut entry = login_entry("t", "u", "p", at);
        entry.login_mut().unwrap().site_matchers = Lww::new(matchers, at);
        entry
    }

    fn web(url: &str) -> Target<'_> {
        Target::Web { url }
    }

    #[test]
    fn domain_matching_uses_the_public_suffix_list() {
        let entry = login(vec![SiteMatcher::Domain("example.co.uk".into())]);
        assert_eq!(
            match_entry(&entry, web("https://example.co.uk/x")),
            Some(MatchLevel::Exact)
        );
        assert_eq!(
            match_entry(&entry, web("https://login.example.co.uk/")),
            Some(MatchLevel::Domain)
        );
        assert_eq!(match_entry(&entry, web("https://other.co.uk/")), None);
    }

    #[test]
    fn lookalikes_and_shared_suffix_tenants_do_not_match() {
        let entry = login(vec![SiteMatcher::Domain("example.com".into())]);
        for bad in [
            "https://example.com.evil.net/",
            "https://evilexample.com/",
            "https://example.com@evil.net/",
            "https://evil.net/example.com",
        ] {
            assert_eq!(match_entry(&entry, web(bad)), None, "{bad}");
        }
        let tenant = login(vec![SiteMatcher::Domain("alice.github.io".into())]);
        assert_eq!(
            match_entry(&tenant, web("https://mallory.github.io/")),
            None
        );
    }

    #[test]
    fn host_matcher_is_strict() {
        let entry = login(vec![SiteMatcher::Host("login.example.com".into())]);
        assert_eq!(
            match_entry(&entry, web("https://login.example.com/")),
            Some(MatchLevel::Exact)
        );
        assert_eq!(match_entry(&entry, web("https://example.com/")), None);
        assert_eq!(
            match_entry(&entry, web("https://a.login.example.com/")),
            None
        );
    }

    #[test]
    fn https_login_is_never_offered_on_http() {
        let entry = login(vec![SiteMatcher::Url("https://example.com/login".into())]);
        assert_eq!(
            match_entry(&entry, web("https://example.com/")),
            Some(MatchLevel::Exact)
        );
        assert_eq!(match_entry(&entry, web("http://example.com/")), None);
    }

    #[test]
    fn only_web_schemes_fill() {
        let entry = login(vec![SiteMatcher::Domain("example.com".into())]);
        for bad in [
            "javascript:alert(1)",
            "file:///example.com",
            "data:text/html,example.com",
            "not a url",
        ] {
            assert_eq!(match_entry(&entry, web(bad)), None, "{bad}");
        }
    }

    #[test]
    fn app_needs_the_certificate_to_be_exact() {
        let pinned = login(vec![SiteMatcher::AndroidApp {
            package: "com.bank".into(),
            cert_sha256: Some("AB:CD".into()),
        }]);
        let app = |package, cert_sha256| Target::App {
            package,
            cert_sha256,
        };
        assert_eq!(
            match_entry(&pinned, app("com.bank", "ab:cd")),
            Some(MatchLevel::Exact)
        );
        assert_eq!(
            match_entry(&pinned, app("com.bank", "FF:FF")),
            None,
            "a repackaged app must not match"
        );
        assert_eq!(match_entry(&pinned, app("com.other", "AB:CD")), None);
        let unpinned = login(vec![SiteMatcher::AndroidApp {
            package: "com.bank".into(),
            cert_sha256: None,
        }]);
        assert_eq!(
            match_entry(&unpinned, app("com.bank", "FF")),
            Some(MatchLevel::Unverified)
        );
    }

    #[test]
    fn never_trashed_and_shelved_are_left_out() {
        let target = web("https://example.com/");
        let never = login(vec![
            SiteMatcher::Domain("example.com".into()),
            SiteMatcher::Never,
        ]);
        assert_eq!(match_entry(&never, target), None);
        let mut trashed = login(vec![SiteMatcher::Domain("example.com".into())]);
        trashed.common.deleted_at_ms = Lww::new(Some(1), Hlc::default());
        assert_eq!(match_entry(&trashed, target), None);
        let mut shelved = login(vec![SiteMatcher::Domain("example.com".into())]);
        shelved.common.hidden = Lww::new(true, Hlc::default());
        assert_eq!(match_entry(&shelved, target), None);
    }

    #[test]
    fn regex_matches_and_a_bad_pattern_matches_nothing() {
        let good = login(vec![SiteMatcher::Regex(
            r"^https://[a-z]+\.corp\.example/".into(),
        )]);
        assert_eq!(
            match_entry(&good, web("https://wiki.corp.example/")),
            Some(MatchLevel::Domain)
        );
        let bad = login(vec![SiteMatcher::Regex("(".into())]);
        assert_eq!(match_entry(&bad, web("https://x.example/")), None);
    }

    #[test]
    fn ip_addresses_match_only_themselves() {
        let entry = login(vec![SiteMatcher::Domain("192.168.1.1".into())]);
        assert_eq!(
            match_entry(&entry, web("http://192.168.1.1/")),
            Some(MatchLevel::Exact)
        );
        assert_eq!(match_entry(&entry, web("http://192.168.1.2/")), None);
    }

    #[test]
    fn strongest_match_sorts_first() {
        let at = Hlc::default();
        let mut a = login(vec![SiteMatcher::Domain("example.com".into())]);
        a.common.title = Lww::new("A".into(), at);
        let mut b = login(vec![SiteMatcher::Host("sso.example.com".into())]);
        b.common.title = Lww::new("B".into(), at);
        let found = matches_for([&a, &b], web("https://sso.example.com/"));
        assert_eq!(found[0].0.common.title.v, "B");
        assert_eq!(found[0].1, MatchLevel::Exact);
    }

    #[test]
    fn apps_saved_as_addresses_read_as_apps() {
        assert_eq!(app_of_address("androidapp://com.discord"), Some(("com.discord".into(), None)));
        // Google Password Manager writes the certificate digest in base64 before the package.
        let digest = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
        let (package, cert) = app_of_address(&format!("android://{digest}@com.twitter.android/")).unwrap();
        assert_eq!(package, "com.twitter.android");
        assert_eq!(cert.as_deref(), Some("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"));
        for package in ["com.bkash.customerapp", "bd.com.sonalibank.sw", "us.zoom.videomeetings"] {
            assert_eq!(app_of_address(package).map(|(p, _)| p).as_deref(), Some(package), "{package}");
        }
        for site in ["x.com", "payment.bkash.app", "co.uk", "me.com", "https://com.example.org/", "github.com/login", "localhost"] {
            assert_eq!(app_of_address(site), None, "{site}");
        }
    }

    #[test]
    fn an_app_saved_as_an_address_fills_the_app_and_no_site() {
        let entry = login(vec![SiteMatcher::Url("android://AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=@com.example.app/".into())]);
        let cert = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        assert_eq!(
            match_entry(&entry, Target::App { package: "com.example.app", cert_sha256: cert }),
            Some(MatchLevel::Exact)
        );
        assert_eq!(match_entry(&entry, Target::App { package: "com.example.app", cert_sha256: "ff" }), None);
        let bare = login(vec![SiteMatcher::Url("com.example.wallet".into())]);
        assert_eq!(match_entry(&bare, web("https://wallet.example.com/")), None);
        assert_eq!(
            match_entry(&bare, Target::App { package: "com.example.wallet", cert_sha256: cert }),
            Some(MatchLevel::Unverified)
        );
    }
}
