//! Filing entries automatically. Rules first, then the domain list, then a small
//! offline classifier. An AI provider is only ever asked at the owner's request,
//! and that code is not here, because this crate has no network.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::health::entry_domains;
use crate::model::{Entry, Kind};

/// The default categories.
pub const CATEGORIES: [&str; 16] = [
    "Banking",
    "Finance",
    "Crypto",
    "Developer",
    "Cloud",
    "Email",
    "Social",
    "Shopping",
    "Gaming",
    "Entertainment",
    "Work",
    "Education",
    "Government",
    "Health",
    "Travel",
    "Utilities",
];

/// Categories for kinds that need no guess.
pub const KIND_CATEGORIES: [&str; 11] = [
    "Payment cards",
    "Identities",
    "Notes",
    "Documents",
    "SSH keys",
    "API keys",
    "Databases",
    "Licenses",
    "Recovery",
    "Wi-Fi",
    "Authenticator",
];

static DOMAINS: &str = include_str!("../../../data/domains.tsv");
/// Games people keep accounts for, by name: "Genshin Impact" has no domain to go by.
static GAMES: &str = include_str!("../../../data/games.tsv");
/// More of them, by how widely they are known, from Wikidata.
static MORE_GAMES: &str = include_str!("../../../data/wikidata/games.tsv");
static MODEL: &[u8] = include_bytes!("../../../data/categoriser.bin");

/// Buckets in the hashed feature space.
pub const BUCKETS: usize = 4096;
/// Log-probability units per quantised step.
pub const SCALE: f32 = 8.0;
/// Lowest probability the classifier must reach to say anything. A wrong guess
/// costs more than no guess, because the owner then has to find and fix it.
pub const MIN_CONFIDENCE: f32 = 0.60;

/// Where a category came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The entry's kind decides it.
    Kind,
    /// The site is on the bundled list.
    DomainList,
    /// The item's name is a listed site's: "PyPI" for pypi.org. Nearly as sure as the list,
    /// but a name can be a coincidence where an address cannot.
    Name,
    /// The classifier's guess.
    Classifier,
}

impl Source {
    /// The word shells use for it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Kind => "kind",
            Self::DomainList => "list",
            Self::Name => "name",
            Self::Classifier => "guess",
        }
    }
}

/// A suggestion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// Category name.
    pub category: &'static str,
    /// How it was decided.
    pub source: Source,
}

fn fnv(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Hashed features of a text: its words plus character 3- to 5-grams.
pub fn features(text: &str) -> Vec<usize> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for word in lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let framed = format!("^{word}$");
        out.push(fnv(framed.as_bytes()) as usize % BUCKETS);
        let chars: Vec<char> = framed.chars().collect();
        for size in 3..=5 {
            for window in chars.windows(size) {
                let gram: String = window.iter().collect();
                out.push(fnv(gram.as_bytes()) as usize % BUCKETS);
            }
        }
    }
    out
}

/// Probability per category from a raw model. Exposed for the training tool.
pub fn classify_with(model: &[u8], text: &str) -> Option<(usize, f32)> {
    let classes = CATEGORIES.len();
    let expected = 4 + classes + classes * BUCKETS;
    if model.len() != expected || &model[..4] != b"WMCG" {
        return None;
    }
    let feats = features(text);
    if feats.is_empty() {
        return None;
    }
    let priors = &model[4..4 + classes];
    let weights = &model[4 + classes..];
    let mut scores = vec![0f32; classes];
    for (class, score) in scores.iter_mut().enumerate() {
        *score = -f32::from(priors[class]) / SCALE;
        let row = &weights[class * BUCKETS..(class + 1) * BUCKETS];
        for feat in &feats {
            *score += -f32::from(row[*feat]) / SCALE;
        }
    }
    // Scale by the feature count so long titles do not become overconfident.
    let damp = (feats.len() as f32).sqrt().max(1.0);
    let top = scores.iter().copied().fold(f32::MIN, f32::max);
    let total: f32 = scores.iter().map(|s| ((s - top) / damp).exp()).sum();
    let (best, score) = scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    Some((best, ((score - top) / damp).exp() / total))
}

/// The bundled list as a table, built on first use: it holds tens of thousands of sites.
fn listed() -> &'static HashMap<&'static str, &'static str> {
    static LISTED: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    LISTED.get_or_init(|| {
        DOMAINS
            .lines()
            .filter_map(|line| {
                let mut columns = line.split('\t');
                let domain = columns.next()?;
                let category = known_category(columns.next()?)?;
                Some((domain, category))
            })
            .collect()
    })
}

fn known_category(name: &str) -> Option<&'static str> {
    CATEGORIES.iter().find(|known| **known == name).copied()
}

/// The category of each first label on the list: `pypi` for pypi.org. Where several sites share
/// a label, one the 2FA directory lists wins, then one under .com, then the first. Sites only
/// Wikidata files, marked `w`, are left out: among fifty thousand colleges and hospitals some
/// label is every word, and a name is not an address.
fn labels() -> &'static HashMap<&'static str, &'static str> {
    static LABELS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    LABELS.get_or_init(|| {
        let mut best: HashMap<&'static str, ((bool, bool), &'static str)> = HashMap::new();
        for line in DOMAINS.lines() {
            let mut columns = line.split('\t');
            let (Some(domain), Some(category)) = (columns.next(), columns.next()) else {
                continue;
            };
            let Some(category) = known_category(category) else {
                continue;
            };
            let Some((label, _)) = domain.split_once('.') else {
                continue;
            };
            let methods = columns.next();
            if columns.next() == Some("w") {
                continue;
            }
            let rank = (methods.is_some_and(|methods| methods != "-"), domain.ends_with(".com"));
            match best.get(label) {
                Some((held, _)) if *held >= rank => {}
                _ => {
                    best.insert(label, (rank, category));
                }
            }
        }
        best.into_iter().map(|(label, (_, category))| (label, category)).collect()
    })
}

/// The category the bundled list gives a domain.
pub fn listed_category(domain: &str) -> Option<&'static str> {
    listed().get(domain).copied()
}

/// The category of a listed domain whose first label is `label`: `pypi` finds pypi.org.
fn label_category(label: &str) -> Option<&'static str> {
    labels().get(label).copied()
}

/// Names people give a service that are not its domain's first label.
const NAME_LABELS: &[(&str, &str)] = &[
    ("npm", "npmjs"),
    ("steam", "steampowered"),
    ("x", "x"),
    ("disney", "disneyplus"),
    ("hackernews", "ycombinator"),
    ("jira", "atlassian"),
    ("confluence", "atlassian"),
    ("appleid", "apple"),
    ("chatgpt", "openai"),
];

/// Words that begin the names of many things, so say nothing as a name's first word: "State
/// University" is no account at state.gov.
const OPENING_WORDS: &[&str] = &[
    "air", "all", "american", "bank", "best", "big", "central", "city", "civil", "college", "community",
    "county", "credit", "digital", "direct", "east", "express", "family", "federal", "first", "free",
    "general", "get", "global", "great", "health", "home", "international", "just", "medical", "mobile",
    "my", "national", "new", "north", "one", "online", "open", "plus", "prime", "private", "pro", "public",
    "royal", "school", "simple", "smart", "social", "south", "state", "super", "the", "united",
    "university", "west", "world",
];

/// Letters and digits only, lowercased: "Honkai: Star Rail" and "honkai star rail" meet.
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The bundled games by name, squeezed.
fn games() -> &'static HashSet<String> {
    static GAME_NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    GAME_NAMES.get_or_init(|| {
        GAMES
            .lines()
            .chain(MORE_GAMES.lines())
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split('\t').next())
            .map(squeeze)
            .filter(|name| !name.is_empty())
            .collect()
    })
}

/// Whether a name is a game on the bundled list, as typed or before a note such as
/// "Valorant (smurf)" or "Minecraft - server".
pub fn is_game(title: &str) -> bool {
    let whole = squeeze(title);
    let lead = squeeze(title.split(['(', '[', '|', '-', '\u{2013}', '\u{2014}']).next().unwrap_or(""));
    // A name of one or two letters says nothing on its own.
    [whole, lead]
        .iter()
        .filter(|name| name.chars().count() >= 3)
        .any(|name| games().contains(name))
}

/// What a name alone says, the way a code's issuer or a bare login's title is usually the
/// service's own: "PyPI", "Docker Hub", "GitHub (work)". The whole name squeezed together is
/// tried first, then its first word, for names of three words or fewer.
fn name_category(title: &str) -> Option<&'static str> {
    let lower = title.trim().to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let by_label = |name: &str| {
        let alias = NAME_LABELS.iter().find(|(known, _)| *known == name).map(|(_, label)| *label);
        match alias {
            Some(label) => label_category(label),
            None if name.len() >= 3 => label_category(name),
            None => None,
        }
    };
    // A game's whole name comes before its first word: "Mobile Legends" is not about phones.
    by_label(&words.concat())
        .or_else(|| is_game(title).then_some("Gaming"))
        .or_else(|| {
            let first = words.first().filter(|w| !OPENING_WORDS.contains(w))?;
            (2..=3).contains(&words.len()).then(|| by_label(first)).flatten()
        })
}

/// The classifier's opinion of a name, if it is confident enough.
pub fn classify(text: &str) -> Option<&'static str> {
    let (index, confidence) = classify_with(MODEL, text)?;
    (confidence >= MIN_CONFIDENCE).then_some(CATEGORIES[index])
}

/// The category an entry's kind gives it, whatever its site. Logins have none.
fn kind_category(kind: &Kind) -> Option<&'static str> {
    match kind {
        Kind::Card(_) => Some("Payment cards"),
        Kind::Identity(_) => Some("Identities"),
        Kind::Note(_) => Some("Notes"),
        Kind::Document(_) => Some("Documents"),
        Kind::SshKey(_) => Some("SSH keys"),
        Kind::ApiKey(_) => Some("API keys"),
        Kind::Database(_) => Some("Databases"),
        Kind::Licence(_) => Some("Licenses"),
        Kind::Recovery(_) => Some("Recovery"),
        Kind::Wifi(_) => Some("Wi-Fi"),
        Kind::Otp(_) => Some("Authenticator"),
        Kind::Login(_) => None,
    }
}

/// What the site or the name says, by the list first and the classifier after.
pub fn site_category(entry: &Entry) -> Option<(&'static str, Source)> {
    let domains = entry_domains(entry);
    if let Some(category) = domains.iter().find_map(|domain| listed_category(domain)) {
        return Some((category, Source::DomainList));
    }
    // A code has no website, only a name, and the name is usually the site's: GitHub, PyPI.
    // A website the list does not know gets the same chance before the classifier guesses.
    if let Some(category) = name_category(&entry.common.title.v) {
        return Some((category, Source::Name));
    }
    let name = domains
        .iter()
        .filter_map(|domain| domain.split('.').next())
        .collect::<Vec<_>>()
        .join(" ");
    let text = format!("{} {name}", entry.common.title.v);
    classify(&text).map(|category| (category, Source::Classifier))
}

/// Suggests a category. `None` leaves the entry unfiled rather than guessing wrong.
pub fn suggest(entry: &Entry) -> Option<Suggestion> {
    if let Some(category) = kind_category(&entry.kind) {
        return Some(Suggestion {
            category,
            source: Source::Kind,
        });
    }
    site_category(entry).map(|(category, source)| Suggestion { category, source })
}

/// The category an entry's kind gives it, whatever its site. Logins have none.
pub fn kind_suggestion(entry: &Entry) -> Option<&'static str> {
    kind_category(&entry.kind)
}

/// Every category worth offering, what the site is about before what the kind is: a code for
/// GitHub is both Developer and Authenticator.
pub fn suggest_all(entry: &Entry) -> Vec<&'static str> {
    let mut out = Vec::new();
    if let Some((category, _)) = site_category(entry) {
        out.push(category);
    }
    if let Some(category) = kind_category(&entry.kind)
        && !out.contains(&category)
    {
        out.push(category);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Hlc;
    use crate::model::{Card, Lww, SiteMatcher};
    use crate::vault::login_entry;

    fn site_entry(title: &str, site: &str) -> Entry {
        let at = Hlc::default();
        let mut entry = login_entry(title, "", "", at);
        entry.login_mut().unwrap().site_matchers =
            Lww::new(vec![SiteMatcher::Url(site.into())], at);
        entry
    }

    #[test]
    fn kinds_decide_first() {
        let entry = Entry {
            kind: Kind::Card(Card::default()),
            ..Entry::default()
        };
        assert_eq!(
            suggest(&entry),
            Some(Suggestion {
                category: "Payment cards",
                source: Source::Kind
            })
        );
    }

    #[test]
    fn listed_sites_use_the_list() {
        let found = suggest(&site_entry("whatever", "https://github.com/login")).unwrap();
        assert_eq!(
            found,
            Suggestion {
                category: "Developer",
                source: Source::DomainList
            }
        );
        assert_eq!(
            suggest(&site_entry("x", "https://www.netflix.com"))
                .unwrap()
                .category,
            "Entertainment"
        );
    }

    #[test]
    fn a_code_is_filed_by_its_site_as_well_as_its_kind() {
        let mut code = Entry {
            kind: Kind::Otp(crate::model::Otp::default()),
            ..Entry::default()
        };
        code.common.title = Lww::new("GitHub".into(), Hlc::default());
        assert_eq!(suggest_all(&code), vec!["Developer", "Authenticator"]);
        assert_eq!(suggest(&code).unwrap().category, "Authenticator");
        assert_eq!(suggest_all(&site_entry("x", "https://discord.com")), vec!["Social"]);
    }

    #[test]
    fn unknown_names_get_the_classifier() {
        let bank = suggest(&site_entry(
            "First Community Bank",
            "https://firstcommunitybank-online.example",
        ))
        .unwrap();
        assert_eq!(
            bank,
            Suggestion {
                category: "Banking",
                source: Source::Classifier
            }
        );
        let uni = suggest(&site_entry(
            "State University Portal",
            "https://portal.stateu.example",
        ))
        .unwrap();
        assert_eq!(uni.category, "Education");
    }

    #[test]
    fn a_name_alone_finds_its_service() {
        let filed = |name: &str| {
            let mut code = Entry {
                kind: Kind::Otp(crate::model::Otp::default()),
                ..Entry::default()
            };
            code.common.title = Lww::new(name.into(), Hlc::default());
            site_category(&code).map(|(category, _)| category)
        };
        for (name, category) in [
            ("PyPI", "Developer"),
            ("npm", "Developer"),
            ("crates.io", "Developer"),
            ("Docker Hub", "Developer"),
            ("Stack Overflow", "Developer"),
            ("GitHub (work)", "Developer"),
            ("Steam", "Gaming"),
            ("X", "Social"),
            ("Disney+", "Entertainment"),
            ("Jira", "Work"),
            ("Slack", "Work"),
            ("Google", "Email"),
            ("Duolingo", "Education"),
            ("Bank of America", "Banking"),
        ] {
            assert_eq!(filed(name), Some(category), "{name}");
        }
        // A generic first word is not a service, and Wikidata's sites are matched by address.
        assert_eq!(name_category("State Library"), None);
        assert_eq!(name_category("Rgsu"), None);
        assert_eq!(listed_category("rgsu.ru"), Some("Education"));
        // A word that happens to be a short label says nothing on its own.
        assert_eq!(name_category("my"), None);
    }

    #[test]
    fn games_are_known_by_name() {
        for name in [
            "Genshin Impact",
            "genshin impact",
            "Honkai: Star Rail",
            "Valorant (smurf)",
            "Clash of Clans",
            "Mobile Legends - main",
            "PUBG Mobile",
            "CS:GO",
            "Old School RuneScape",
        ] {
            assert_eq!(name_category(name), Some("Gaming"), "{name}");
        }
        // A site the list already files keeps its own heading, and words in passing are not games.
        assert_eq!(name_category("Discord"), Some("Social"));
        assert!(!is_game("Rust"));
        assert!(!is_game("my game notes"));
        assert!(!is_game("GO"));
    }

    #[test]
    fn games_on_their_own_sites_are_gaming() {
        assert_eq!(suggest(&site_entry("x", "https://www.hoyoverse.com")).unwrap().category, "Gaming");
        assert_eq!(suggest(&site_entry("x", "https://supercell.com")).unwrap().category, "Gaming");
    }

    #[test]
    fn a_name_is_told_apart_from_the_list() {
        let mut code = Entry {
            kind: Kind::Otp(crate::model::Otp::default()),
            ..Entry::default()
        };
        code.common.title = Lww::new("PyPI".into(), Hlc::default());
        assert_eq!(site_category(&code), Some(("Developer", Source::Name)));
        assert_eq!(site_category(&site_entry("x", "https://pypi.org")), Some(("Developer", Source::DomainList)));
        assert_eq!(Source::Classifier.word(), "guess");
    }

    #[test]
    fn nonsense_stays_unfiled() {
        assert_eq!(suggest(&site_entry("qqzx", "https://qqzx.example")), None);
        assert_eq!(classify(""), None);
    }

    #[test]
    fn a_truncated_model_is_ignored() {
        assert_eq!(classify_with(&MODEL[..100], "bank"), None);
    }
}
