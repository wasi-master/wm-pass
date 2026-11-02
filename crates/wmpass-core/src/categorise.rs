//! Filing entries automatically. Rules first, then the domain list, then a small
//! offline classifier. An AI provider is only ever asked at the owner's request,
//! and that code is not here, because this crate has no network.

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
    "Licences",
    "Recovery",
    "Wi-Fi",
    "Authenticator",
];

static DOMAINS: &str = include_str!("../../../data/domains.tsv");
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
    /// The classifier's guess.
    Classifier,
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

/// The category the bundled list gives a domain.
pub fn listed_category(domain: &str) -> Option<&'static str> {
    let line = DOMAINS
        .lines()
        .find(|line| line.split('\t').next() == Some(domain))?;
    let category = line.split('\t').nth(1)?;
    CATEGORIES.iter().find(|name| **name == category).copied()
}

/// The classifier's opinion of a name, if it is confident enough.
pub fn classify(text: &str) -> Option<&'static str> {
    let (index, confidence) = classify_with(MODEL, text)?;
    (confidence >= MIN_CONFIDENCE).then_some(CATEGORIES[index])
}

/// Suggests a category. `None` leaves the entry unfiled rather than guessing wrong.
pub fn suggest(entry: &Entry) -> Option<Suggestion> {
    let by_kind = match &entry.kind {
        Kind::Card(_) => Some("Payment cards"),
        Kind::Identity(_) => Some("Identities"),
        Kind::Note(_) => Some("Notes"),
        Kind::Document(_) => Some("Documents"),
        Kind::SshKey(_) => Some("SSH keys"),
        Kind::ApiKey(_) => Some("API keys"),
        Kind::Database(_) => Some("Databases"),
        Kind::Licence(_) => Some("Licences"),
        Kind::Recovery(_) => Some("Recovery"),
        Kind::Wifi(_) => Some("Wi-Fi"),
        Kind::Otp(_) => Some("Authenticator"),
        Kind::Login(_) => None,
    };
    if let Some(category) = by_kind {
        return Some(Suggestion {
            category,
            source: Source::Kind,
        });
    }
    let domains = entry_domains(entry);
    if let Some(category) = domains.iter().find_map(|domain| listed_category(domain)) {
        return Some(Suggestion {
            category,
            source: Source::DomainList,
        });
    }
    let name = domains
        .iter()
        .filter_map(|domain| domain.split('.').next())
        .collect::<Vec<_>>()
        .join(" ");
    let text = format!("{} {name}", entry.common.title.v);
    classify(&text).map(|category| Suggestion {
        category,
        source: Source::Classifier,
    })
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
    fn nonsense_stays_unfiled() {
        assert_eq!(suggest(&site_entry("qqzx", "https://qqzx.example")), None);
        assert_eq!(classify(""), None);
    }

    #[test]
    fn a_truncated_model_is_ignored() {
        assert_eq!(classify_with(&MODEL[..100], "bank"), None);
    }
}
