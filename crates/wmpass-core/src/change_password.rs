//! Where a site lets you change your password.
//!
//! Apple's list of known pages comes first. Any other website gets its
//! `/.well-known/change-password` address, which sites that follow the W3C proposal send on to
//! the right page and the rest answer with their home or a missing page. Apps and addresses on
//! the local network get nothing: a router's page has no such address.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::health::is_local_host;
use crate::matching::{app_of_address, host_of, registrable_domain};
use crate::model::SiteMatcher;

static KNOWN: &str = include_str!("../../../data/related/change-password-urls.json");

fn known() -> &'static HashMap<String, String> {
    static LIST: OnceLock<HashMap<String, String>> = OnceLock::new();
    LIST.get_or_init(|| serde_json::from_str(KNOWN).unwrap_or_default())
}

/// The page for a saved address, or `None` when it names no website on the internet.
pub fn url(site: &str) -> Option<String> {
    let site = site.trim();
    if site.is_empty() || app_of_address(site).is_some() {
        return None;
    }
    let host = host_of(site)?;
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if !host.contains('.') || is_local_host(host) || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let domain = registrable_domain(host);
    let list = known();
    Some(
        list.get(host)
            .or_else(|| list.get(&domain))
            .cloned()
            .unwrap_or_else(|| format!("https://{domain}/.well-known/change-password")),
    )
}

/// The page for the first website a login is saved for that has one.
pub fn for_matchers(matchers: &[SiteMatcher]) -> Option<String> {
    matchers.iter().find_map(|matcher| match matcher {
        SiteMatcher::Domain(text) | SiteMatcher::Host(text) | SiteMatcher::Url(text) => url(text),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sites_get_their_own_page() {
        assert_eq!(url("https://23andme.com/login").as_deref(), Some("https://you.23andme.com/user/edit/password/"));
        // A subdomain falls back to its registrable domain's entry.
        assert_eq!(url("www.23andme.com").as_deref(), Some("https://you.23andme.com/user/edit/password/"));
        assert_eq!(url("accounts.23andme.com").as_deref(), Some("https://you.23andme.com/user/edit/password/"));
    }

    #[test]
    fn other_sites_get_the_well_known_address() {
        assert_eq!(url("https://login.example.co.uk/signin?x=1").as_deref(), Some("https://example.co.uk/.well-known/change-password"));
        assert_eq!(url("example.com").as_deref(), Some("https://example.com/.well-known/change-password"));
        assert_eq!(url("github.com").as_deref(), Some("https://github.com/.well-known/change-password"));
    }

    #[test]
    fn apps_and_local_addresses_get_nothing() {
        assert_eq!(url(""), None);
        assert_eq!(url("androidapp://com.example.app"), None);
        assert_eq!(url("android://abc@com.example.app/"), None);
        assert_eq!(url("http://192.168.1.1"), None);
        assert_eq!(url("https://8.8.8.8/"), None);
        assert_eq!(url("http://router"), None);
        assert_eq!(url("https://nas.local:5001"), None);
        assert_eq!(url("http://printer.home.arpa"), None);
    }

    #[test]
    fn a_login_uses_its_first_website() {
        let matchers = vec![
            SiteMatcher::AndroidApp { package: "com.example.app".into(), cert_sha256: None },
            SiteMatcher::Url("http://192.168.0.1".into()),
            SiteMatcher::Domain("example.org".into()),
        ];
        assert_eq!(for_matchers(&matchers).as_deref(), Some("https://example.org/.well-known/change-password"));
        assert_eq!(for_matchers(&[]), None);
    }

    #[test]
    fn the_list_parses() {
        assert!(known().len() > 100);
    }
}
