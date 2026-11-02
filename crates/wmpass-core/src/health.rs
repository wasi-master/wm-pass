//! The vault's health report: weak, reused, old, breached, duplicated and more.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::matching::{host_of, registrable_domain};
use crate::model::{Entry, Id, Kind, Lww, SiteMatcher};
use crate::strength;
use crate::vault::Vault;

const DAY_MS: u64 = 86_400_000;
static DOMAINS: &str = include_str!("../../../data/domains.tsv");

/// What the report needs beyond the vault.
#[derive(Clone, Debug, Default)]
pub struct HealthInput {
    /// Now, in milliseconds.
    pub now_ms: u64,
    /// Passwords older than this count as old. Zero uses 365.
    pub old_after_days: u32,
    /// Breach counts by entry id, from an opt-in check.
    pub breached: BTreeMap<Id, u64>,
}

/// The report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HealthReport {
    /// Weak passwords.
    pub weak: Vec<Id>,
    /// Groups of entries sharing a password.
    pub reused: Vec<Vec<Id>>,
    /// Passwords past their age limit or their own change reminder.
    pub old: Vec<Id>,
    /// Passwords seen in breaches, with counts.
    pub breached: Vec<(Id, u64)>,
    /// Sites that offer two-factor when this entry has no code saved.
    pub missing_two_factor: Vec<Id>,
    /// Saved with a plain `http://` address.
    pub insecure_url: Vec<Id>,
    /// Groups of entries for one site, user and password.
    pub duplicates: Vec<Vec<Id>>,
    /// Entries past or nearing a fixed expiry.
    pub expiring: Vec<Id>,
    /// 0 to 100.
    pub score: u8,
}

fn matcher_domain(matcher: &SiteMatcher) -> Option<String> {
    match matcher {
        SiteMatcher::Domain(text) | SiteMatcher::Host(text) | SiteMatcher::Url(text) => {
            Some(registrable_domain(&host_of(text)?))
        }
        _ => None,
    }
}

/// Registrable domains a login is saved for.
pub fn entry_domains(entry: &Entry) -> Vec<String> {
    entry.login().map_or_else(Vec::new, |login| {
        let mut out: Vec<String> = login.site_matchers.v.iter().filter_map(matcher_domain).collect();
        out.sort();
        out.dedup();
        out
    })
}

/// Domains known to offer two-factor authentication.
pub fn two_factor_domains() -> HashSet<&'static str> {
    DOMAINS
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let domain = columns.next()?;
            let methods = columns.nth(1)?;
            (methods != "-").then_some(domain)
        })
        .collect()
}

fn fingerprint(value: &str) -> [u8; 32] {
    *blake3::hash(value.as_bytes()).as_bytes()
}

/// Builds the report over everything not in the trash, shelved entries included.
pub fn report(vault: &Vault, input: &HealthInput) -> HealthReport {
    let mut out = HealthReport::default();
    let old_after = u64::from(if input.old_after_days == 0 { 365 } else { input.old_after_days });
    let tfa = two_factor_domains();
    let mut by_password: HashMap<[u8; 32], Vec<Id>> = HashMap::new();
    let mut by_identity: HashMap<(String, String, [u8; 32]), Vec<Id>> = HashMap::new();
    let mut logins = 0usize;

    for entry in vault.doc.entries.values().filter(|e| !e.is_trashed()) {
        let reminders = &entry.common.reminders.v;
        if let Some(expires) = reminders.expires_ms {
            let warn = u64::from(reminders.warn_days) * DAY_MS;
            if expires.saturating_sub(warn) <= input.now_ms {
                out.expiring.push(entry.id.clone());
            }
        }
        let Kind::Login(login) = &entry.kind else { continue };
        let password = login.password.v.expose();
        if password.is_empty() {
            continue;
        }
        logins += 1;
        let domains = entry_domains(entry);
        let mut context: Vec<&str> = domains.iter().map(String::as_str).collect();
        context.push(login.username.v.as_str());
        context.push(entry.common.title.v.as_str());
        if strength::assess(password, &context).score < strength::STRONG_ENOUGH {
            out.weak.push(entry.id.clone());
        }
        let print = fingerprint(password);
        by_password.entry(print).or_default().push(entry.id.clone());
        let site = domains.first().cloned().unwrap_or_else(|| entry.common.title.v.to_lowercase());
        by_identity
            .entry((site, login.username.v.to_lowercase(), print))
            .or_default()
            .push(entry.id.clone());

        let limit = reminders.change_every_days.map_or(old_after, u64::from);
        if input.now_ms.saturating_sub(login.password.at.ms) > limit * DAY_MS {
            out.old.push(entry.id.clone());
        }
        if let Some(count) = input.breached.get(&entry.id) {
            out.breached.push((entry.id.clone(), *count));
        }
        let has_second_factor = login.otp.v.is_some() || !login.passkeys.v.is_empty();
        if !has_second_factor && domains.iter().any(|domain| tfa.contains(domain.as_str())) {
            out.missing_two_factor.push(entry.id.clone());
        }
        let plain = |m: &SiteMatcher| matches!(m, SiteMatcher::Url(url) if url.to_ascii_lowercase().starts_with("http://"));
        if login.site_matchers.v.iter().any(plain) {
            out.insecure_url.push(entry.id.clone());
        }
    }
    out.reused = groups(by_password);
    out.duplicates = groups(by_identity);

    let bad: HashSet<&Id> = out
        .weak
        .iter()
        .chain(out.reused.iter().flatten())
        .chain(out.breached.iter().map(|(id, _)| id))
        .chain(out.old.iter())
        .collect();
    out.score = if logins == 0 {
        100
    } else {
        let good = logins.saturating_sub(bad.len());
        u8::try_from(good * 100 / logins).unwrap_or(100)
    };
    out
}

fn groups<K: std::hash::Hash + Eq>(map: HashMap<K, Vec<Id>>) -> Vec<Vec<Id>> {
    let mut out: Vec<Vec<Id>> = map.into_values().filter(|ids| ids.len() > 1).collect();
    for group in &mut out {
        group.sort();
    }
    out.sort();
    out
}

/// Folds `remove` into `keep` and sends `remove` to the trash. Newer values win
/// field by field, and any password that loses lands in history, so clearing a
/// duplicate cannot lose a credential.
pub fn merge_duplicates(vault: &mut Vault, keep: &str, remove: &str, now_ms: u64) -> crate::Result<()> {
    let other = vault.get(remove).cloned().ok_or_else(|| crate::Error::Invalid("no such entry".into()))?;
    vault.edit(keep, now_ms, |entry, at| {
        let mut tags = entry.common.tags.v.clone();
        for tag in &other.common.tags.v {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
        let mut merged = other.clone();
        merged.id.clone_from(&entry.id);
        entry.merge_from(&merged);
        entry.common.tags = Lww::new(tags, at);
    })?;
    vault.trash(remove, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lww, Reminders};
    use crate::otp::OtpConfig;
    use crate::vault::login_entry;
    use crate::vault::test_support::fast_vault;

    const STRONG: &str = "xK9#mP2$vL7@qR4!tN8&";
    const NOW: u64 = 2_000_000_000_000;

    fn add(vault: &mut Vault, title: &str, site: Option<&str>, user: &str, password: &str) -> Id {
        let at = vault.tick(NOW);
        let mut entry = login_entry(title, user, password, at);
        if let Some(site) = site {
            entry.login_mut().unwrap().site_matchers = Lww::new(vec![SiteMatcher::Url(site.into())], at);
        }
        vault.add(entry, NOW).unwrap()
    }

    fn run(vault: &Vault) -> HealthReport {
        report(vault, &HealthInput { now_ms: NOW, ..HealthInput::default() })
    }

    #[test]
    fn weak_reused_and_duplicate_are_told_apart() {
        let mut vault = fast_vault(1);
        let weak = add(&mut vault, "Weak", Some("https://weak.example"), "me", "password");
        let a = add(&mut vault, "A", Some("https://a.example"), "me", STRONG);
        let b = add(&mut vault, "B", Some("https://b.example"), "me", STRONG);
        let c = add(&mut vault, "C", Some("https://a.example"), "ME", STRONG);
        let result = run(&vault);
        assert_eq!(result.weak, vec![weak]);
        let mut reused = vec![a.clone(), b, c.clone()];
        reused.sort();
        assert_eq!(result.reused, vec![reused]);
        let mut duplicate = vec![a, c];
        duplicate.sort();
        assert_eq!(result.duplicates, vec![duplicate]);
        assert!(result.score < 100);
    }

    #[test]
    fn old_uses_the_entrys_own_reminder_first() {
        let mut vault = fast_vault(1);
        let at = crate::clock::Hlc { ms: NOW - 100 * DAY_MS, counter: 0, device: 1 };
        let mut entry = login_entry("Fresh enough", "u", STRONG, at);
        entry.id = "a".into();
        let mut strict = entry.clone();
        strict.id = "b".into();
        strict.common.reminders = Lww::new(Reminders { change_every_days: Some(30), ..Reminders::default() }, at);
        vault.add(entry, NOW).unwrap();
        vault.add(strict, NOW).unwrap();
        assert_eq!(run(&vault).old, vec!["b".to_owned()]);
    }

    #[test]
    fn breach_counts_come_through_and_lower_the_score() {
        let mut vault = fast_vault(1);
        let id = add(&mut vault, "A", None, "u", STRONG);
        let breached = BTreeMap::from([(id.clone(), 42)]);
        let result = report(&vault, &HealthInput { now_ms: NOW, breached, ..HealthInput::default() });
        assert_eq!(result.breached, vec![(id, 42)]);
        assert_eq!(result.score, 0);
    }

    #[test]
    fn missing_two_factor_uses_the_directory_and_respects_saved_codes() {
        let mut vault = fast_vault(1);
        let bare = add(&mut vault, "GitHub", Some("https://github.com/login"), "u", STRONG);
        let covered = add(&mut vault, "GitHub 2", Some("https://github.com/login"), "v", "yT5!pQ8@zM3#kL9$wR2&");
        vault
            .edit(&covered, NOW, |entry, at| {
                entry.login_mut().unwrap().otp = Lww::new(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP")), at);
            })
            .unwrap();
        add(&mut vault, "Nowhere", Some("https://no-such-site.example"), "w", "uH7%nB4^cX6*dF1!sA3#");
        assert_eq!(run(&vault).missing_two_factor, vec![bare]);
    }

    #[test]
    fn plain_http_and_expiry_are_flagged() {
        let mut vault = fast_vault(1);
        let http = add(&mut vault, "Old", Some("http://intranet.example"), "u", STRONG);
        let expiring = add(&mut vault, "Licence", None, "x", "mN2@bV5#cX8$zL1!kJ4%");
        vault
            .edit(&expiring, NOW, |entry, at| {
                let reminders = Reminders { expires_ms: Some(NOW + 3 * DAY_MS), warn_days: 7, ..Reminders::default() };
                entry.common.reminders = Lww::new(reminders, at);
            })
            .unwrap();
        let result = run(&vault);
        assert_eq!(result.insecure_url, vec![http]);
        assert_eq!(result.expiring, vec![expiring]);
    }

    #[test]
    fn trash_is_ignored_and_an_empty_vault_is_healthy() {
        let mut vault = fast_vault(1);
        assert_eq!(run(&vault).score, 100);
        let id = add(&mut vault, "Weak", None, "u", "password");
        assert_eq!(run(&vault).weak.len(), 1);
        vault.trash(&id, NOW).unwrap();
        assert!(run(&vault).weak.is_empty());
    }

    #[test]
    fn clearing_a_duplicate_keeps_every_password_and_all_tags() {
        let mut vault = fast_vault(1);
        let keep = add(&mut vault, "A", Some("https://a.example"), "me", "old-password-1");
        let remove = add(&mut vault, "B", Some("https://a.example"), "me", "newer-password-2");
        vault
            .edit(&remove, NOW + 5, |entry, at| {
                entry.set_password("newest-password-3".into(), at);
                entry.common.tags = Lww::new(vec!["work".into()], at);
            })
            .unwrap();
        merge_duplicates(&mut vault, &keep, &remove, NOW + 10).unwrap();
        let entry = vault.get(&keep).unwrap();
        assert_eq!(entry.login().unwrap().password.v.expose(), "newest-password-3");
        let past: Vec<_> = entry.history_of("password").map(|r| r.value.expose().to_owned()).collect();
        assert!(past.contains(&"old-password-1".to_owned()), "{past:?}");
        assert!(past.contains(&"newer-password-2".to_owned()), "{past:?}");
        assert_eq!(entry.common.tags.v, vec!["work".to_owned()]);
        assert!(vault.get(&remove).unwrap().is_trashed());
    }

    #[test]
    fn directory_loads() {
        let tfa = two_factor_domains();
        assert!(tfa.len() > 1000);
        assert!(tfa.contains("github.com"));
    }
}
