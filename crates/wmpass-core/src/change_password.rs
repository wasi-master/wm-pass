//! Where a site lets you change your password.
//!
//! Apple's list of known pages answers at once. Any other website may have a
//! `/.well-known/change-password` address, as the W3C proposal has it, but most do not, and
//! offering that address for every site sends people to home pages and missing pages. So a site
//! off the list only gets a domain to [`ask`], and the network layer asks it, trusting the answer
//! only when the site first refuses an address no site has. Apps and addresses on the local
//! network get nothing: a router's page has no such address.

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

/// The host of a saved address on the internet, without `www.`; `None` for an app, a local
/// address or anything that is not a website.
fn web_host(site: &str) -> Option<String> {
    let site = site.trim();
    if site.is_empty() || app_of_address(site).is_some() {
        return None;
    }
    let host = host_of(site)?;
    let host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    if !host.contains('.') || is_local_host(&host) || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    Some(host)
}

/// The page on the list for a saved address, or `None` when the list does not know the site.
pub fn url(site: &str) -> Option<String> {
    let host = web_host(site)?;
    let list = known();
    list.get(&host).or_else(|| list.get(&registrable_domain(&host))).cloned()
}

/// For a website the list does not know: the domain to ask whether it has a page, which the
/// network layer does with `site.probe_change_password`. `None` for a listed site, an app or a
/// local address.
pub fn ask(site: &str) -> Option<String> {
    if url(site).is_some() {
        return None;
    }
    web_host(site).map(|host| registrable_domain(&host))
}

/// The listed page for the first website a login is saved for that has one.
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
    fn other_sites_are_asked_rather_than_guessed() {
        assert_eq!(url("https://login.example.co.uk/signin?x=1"), None);
        assert_eq!(ask("https://login.example.co.uk/signin?x=1").as_deref(), Some("example.co.uk"));
        assert_eq!(ask("example.com").as_deref(), Some("example.com"));
        // A listed site needs no asking.
        assert_eq!(ask("https://23andme.com/login"), None);
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
        assert_eq!(ask("http://printer.home.arpa"), None);
        assert_eq!(ask("androidapp://com.example.app"), None);
    }

    #[test]
    fn a_login_uses_its_first_website() {
        let matchers = vec![
            SiteMatcher::AndroidApp { package: "com.example.app".into(), cert_sha256: None },
            SiteMatcher::Url("http://192.168.0.1".into()),
            SiteMatcher::Domain("example.org".into()),
            SiteMatcher::Domain("amazon.com".into()),
        ];
        // example.org is not on the list, so the next website that is answers.
        assert_eq!(for_matchers(&matchers).as_deref(), Some("https://www.amazon.com/ax/account/manage"));
        assert_eq!(for_matchers(&matchers[..3]), None);
        assert_eq!(for_matchers(&[]), None);
    }

    #[test]
    fn the_list_parses() {
        assert!(known().len() > 100);
    }
}
