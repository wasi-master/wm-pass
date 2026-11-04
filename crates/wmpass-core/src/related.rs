//! What goes with a login's name, websites and apps, so each can suggest the others: Facebook
//! brings facebook.com, messenger.com and the Facebook, Lite and Messenger apps.
//!
//! Families of sites come from `data/related/accounts.txt` and Apple's
//! `shared-credentials.json`. Apps are matched to a family by the company in their package name,
//! by their own name and by the website `data/related/apps.tsv` gives them. A phone passes the
//! apps it has installed; elsewhere the apps in that table are offered.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::matching::{host_of, registrable_domain};

static ACCOUNTS: &str = include_str!("../../../data/related/accounts.txt");
static SHARED: &str = include_str!("../../../data/related/shared-credentials.json");
static APPS: &str = include_str!("../../../data/related/apps.tsv");

/// The most of each kind of suggestion offered at once.
const MOST: usize = 6;

/// An app, by its package and the name people know it by.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct App {
    pub package: String,
    pub name: String,
}

/// A brand the shell recognised: its name and its websites.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Brand {
    pub name: String,
    pub domains: Vec<String>,
}

/// What is suggested for a login.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Found {
    /// Websites, as registrable domains.
    pub sites: Vec<String>,
    pub apps: Vec<App>,
    /// A name, only when the login has none yet.
    pub name: Option<String>,
}

/// What a login already has, and what the shell knows around it.
#[derive(Clone, Debug, Default)]
pub struct Have<'a> {
    pub title: &'a str,
    /// Websites as typed.
    pub sites: &'a [String],
    /// Packages of the apps already chosen.
    pub apps: &'a [String],
    /// The brand the name points to, whose websites seed the family.
    pub brand: Option<&'a Brand>,
    /// Names of the brands of the websites.
    pub site_brands: &'a [String],
    /// The apps installed here, when the shell can list them. `None` offers the known apps.
    pub installed: Option<&'a [App]>,
}

struct KnownApp {
    package: &'static str,
    name: &'static str,
    site: &'static str,
}

fn families() -> &'static [BTreeSet<String>] {
    static FAMILIES: OnceLock<Vec<BTreeSet<String>>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut out: Vec<BTreeSet<String>> = ACCOUNTS
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.split_whitespace().map(str::to_owned).collect())
            .collect();
        if let Ok(serde_json::Value::Array(groups)) = serde_json::from_str::<serde_json::Value>(SHARED) {
            for group in groups {
                let domains: BTreeSet<String> = ["shared", "from", "to"]
                    .iter()
                    .filter_map(|key| group.get(*key)?.as_array())
                    .flatten()
                    .filter_map(|d| d.as_str().map(str::to_owned))
                    .collect();
                if domains.len() > 1 {
                    out.push(domains);
                }
            }
        }
        out
    })
}

fn known() -> &'static [KnownApp] {
    static KNOWN: OnceLock<Vec<KnownApp>> = OnceLock::new();
    KNOWN.get_or_init(|| {
        APPS.lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let mut parts = line.split('\t');
                Some(KnownApp { package: parts.next()?, name: parts.next()?, site: parts.next().unwrap_or("") })
            })
            .collect()
    })
}

/// The apps this table knows, for naming them where none are installed.
pub fn known_apps() -> Vec<App> {
    known().iter().map(|app| App { package: app.package.into(), name: app.name.into() }).collect()
}

/// The name of a well-known app, such as Messenger for `com.facebook.orca`.
pub fn app_name(package: &str) -> Option<&'static str> {
    known().iter().find(|app| app.package == package).map(|app| app.name)
}

/// The part of an address a sign-in belongs to: `m.facebook.com/login` is facebook.com. None
/// for local names and addresses, which belong to no family.
pub fn registrable(address: &str) -> Option<String> {
    let host = host_of(address)?;
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') || !host.contains('.') {
        return None;
    }
    let domain = registrable_domain(&host);
    domain.contains('.').then_some(domain)
}

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The website a package's company runs: `com.facebook.katana` belongs to facebook.com.
fn company_site(package: &str) -> Option<String> {
    let mut parts = package.split('.');
    let root = parts.next()?;
    let company = parts.next()?;
    (["com", "org", "net", "io"].contains(&root) && !company.is_empty()).then(|| format!("{company}.{root}"))
}

fn push(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// Suggestions for what a login does not have yet. Nothing it already has comes back.
pub fn find(have: &Have<'_>) -> Found {
    let had: Vec<String> = have.sites.iter().filter_map(|s| registrable(s)).collect();
    let mut seeds: Vec<String> = had.clone();
    let mut brand_names: Vec<String> = Vec::new();
    if let Some(brand) = have.brand {
        for domain in &brand.domains {
            if let Some(d) = registrable(domain) {
                push(&mut seeds, d);
            }
        }
        if !brand.name.is_empty() {
            push(&mut brand_names, brand.name.clone());
        }
    }
    for package in have.apps {
        if let Some(site) = company_site(package) {
            push(&mut seeds, site);
        }
        if let Some(app) = known().iter().find(|app| app.package == *package && !app.site.is_empty()) {
            push(&mut seeds, app.site.to_owned());
        }
    }
    for name in have.site_brands {
        if !name.is_empty() {
            push(&mut brand_names, name.clone());
        }
    }
    let mut family = seeds.clone();
    for group in families() {
        if group.iter().any(|d| seeds.contains(d)) {
            for d in group {
                push(&mut family, d.clone());
            }
        }
    }
    let sites: Vec<String> = family
        .iter()
        .filter(|d| !had.contains(d) && registrable(d).as_deref() == Some(d.as_str()))
        .take(MOST)
        .cloned()
        .collect();

    // Each site's own word, and each brand's name, to know its apps by.
    let words: BTreeSet<String> = family
        .iter()
        .map(|d| d.split('.').next().unwrap_or("").to_owned())
        .chain(brand_names.iter().map(|n| squeeze(n)))
        .filter(|w| w.chars().count() >= 3)
        .collect();
    let known_apps: Vec<(App, &str)> = known()
        .iter()
        .map(|app| (App { package: app.package.into(), name: app.name.into() }, app.site))
        .collect();
    let candidates: Vec<(App, &str)> = match have.installed {
        Some(installed) => installed.iter().map(|app| (app.clone(), "")).collect(),
        None => known_apps,
    };
    let named = |app: &App| {
        let label = squeeze(&app.name);
        words.iter().any(|w| label.starts_with(w.as_str()))
    };
    let mut apps: Vec<App> = candidates
        .into_iter()
        .filter(|(app, site)| {
            if have.apps.contains(&app.package) {
                return false;
            }
            let company = app.package.split('.').nth(1).unwrap_or("");
            words.contains(company) || named(app) || (!site.is_empty() && family.iter().any(|d| d == site))
        })
        .map(|(app, _)| app)
        .collect();
    // Those named for the family first, such as Messenger before a company's other apps.
    apps.sort_by_key(|app| !named(app));
    apps.truncate(MOST);

    let name = if have.title.trim().is_empty() {
        brand_names
            .first()
            .cloned()
            .or_else(|| {
                have.apps.iter().find_map(|package| {
                    have.installed
                        .and_then(|installed| installed.iter().find(|app| app.package == *package).map(|app| app.name.clone()))
                        .or_else(|| app_name(package).map(str::to_owned))
                })
            })
            .or_else(|| {
                had.first().map(|d| {
                    let word = d.split('.').next().unwrap_or("");
                    let mut chars = word.chars();
                    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
                })
            })
            .filter(|n: &String| !n.is_empty())
    } else {
        None
    };
    Found { sites, apps, name }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(found: &Found) -> Vec<&str> {
        found.apps.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn a_site_brings_its_family_and_apps() {
        let sites = vec!["https://www.facebook.com/login".to_owned()];
        let found = find(&Have { sites: &sites, ..Have::default() });
        assert!(found.sites.contains(&"messenger.com".to_owned()), "{:?}", found.sites);
        assert!(!found.sites.contains(&"facebook.com".to_owned()), "what it has is not offered");
        let apps = names(&found);
        for want in ["Facebook", "Facebook Lite", "Messenger"] {
            assert!(apps.contains(&want), "{apps:?}");
        }
        assert!(!apps.contains(&"Instagram"), "another family's app is not offered");
        assert_eq!(found.name.as_deref(), Some("Facebook"), "a login without a name gets one");
    }

    #[test]
    fn a_name_brings_its_brand_sites_and_nothing_once_named() {
        let brand = Brand { name: "Facebook".into(), domains: vec!["facebook.com".into()] };
        let found = find(&Have { title: "Facebook", brand: Some(&brand), ..Have::default() });
        assert!(found.sites.contains(&"facebook.com".to_owned()));
        assert!(found.sites.contains(&"messenger.com".to_owned()));
        assert_eq!(found.name, None, "a name already typed is kept");
    }

    #[test]
    fn an_app_brings_its_site_and_name() {
        let apps = vec!["com.facebook.orca".to_owned()];
        let found = find(&Have { apps: &apps, ..Have::default() });
        assert!(found.sites.contains(&"messenger.com".to_owned()), "{:?}", found.sites);
        assert!(found.sites.contains(&"facebook.com".to_owned()));
        assert!(!names(&found).contains(&"Messenger"), "a chosen app is not offered again");
        assert_eq!(found.name.as_deref(), Some("Messenger"));
    }

    #[test]
    fn installed_apps_replace_the_known_ones() {
        let installed = vec![App { package: "com.facebook.katana".into(), name: "Facebook".into() }, App { package: "org.example.notes".into(), name: "Notes".into() }];
        let sites = vec!["facebook.com".to_owned()];
        let found = find(&Have { sites: &sites, installed: Some(&installed), ..Have::default() });
        assert_eq!(names(&found), vec!["Facebook"]);
    }

    #[test]
    fn local_hosts_have_no_family() {
        assert_eq!(registrable("http://192.168.1.1/admin"), None);
        assert_eq!(registrable("http://router"), None);
        assert_eq!(registrable("https://m.facebook.com/x"), Some("facebook.com".into()));
        assert_eq!(registrable("shop.amazon.co.uk"), Some("amazon.co.uk".into()));
        let sites = vec!["http://nas.local:5000".to_owned()];
        assert_eq!(find(&Have { title: "NAS", sites: &sites, ..Have::default() }), Found::default());
    }

    #[test]
    fn known_apps_have_names() {
        assert_eq!(app_name("com.facebook.orca"), Some("Messenger"));
        assert_eq!(app_name("com.example.nothing"), None);
        assert!(known().iter().all(|a| !a.name.is_empty() && a.package.contains('.')));
    }
}
