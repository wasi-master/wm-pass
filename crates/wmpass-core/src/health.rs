//! The vault's health report: weak, reused, old, breached, duplicated and more.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
    /// Entries the person has said to leave out of one check, by check: `weak`, `reused`,
    /// `old`, `breached`, `twofactor`, `insecure`, `expiring`, `duplicates` or `similar`. Such an entry
    /// no longer counts against the score for that check.
    pub ignored: BTreeMap<String, BTreeSet<Id>>,
    /// Leaves out logins saved only for addresses on the local network, such as a router's
    /// page, which a firewall already keeps from the internet.
    pub skip_local: bool,
}

/// Whether a host is on the local network: a private, loopback or link-local address, a name
/// without a dot, or a name under one of the suffixes home networks use.
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80,
        };
    }
    !host.contains('.')
        || [".local", ".lan", ".home", ".home.arpa", ".internal", ".localhost", ".localdomain"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
}

/// Whether a host is this machine: `localhost`, a name under it, or a loopback address.
fn is_loopback(host: &str) -> bool {
    let host = host.trim_end_matches('.').trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether every address a login is saved for is on the local network. One with no address,
/// or with an app, is not.
fn local_only(entry: &Entry) -> bool {
    let Some(login) = entry.login() else { return false };
    let hosts: Vec<Option<String>> = login
        .site_matchers
        .v
        .iter()
        .map(|matcher| match matcher {
            SiteMatcher::Url(text) if crate::matching::app_of_address(text).is_some() => None,
            SiteMatcher::Domain(text) | SiteMatcher::Host(text) | SiteMatcher::Url(text) => host_of(text),
            _ => None,
        })
        .collect();
    !hosts.is_empty() && hosts.iter().all(|host| host.as_deref().is_some_and(is_local_host))
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
    /// Groups that share a password and look like one account saved more than once: the same
    /// site or name, under usernames that differ only in spelling, or by one being the email.
    pub similar: Vec<Vec<Id>>,
    /// Entries past or nearing a fixed expiry.
    pub expiring: Vec<Id>,
    /// 0 to 100.
    pub score: u8,
}

fn matcher_domain(matcher: &SiteMatcher) -> Option<String> {
    match matcher {
        SiteMatcher::Url(text) if crate::matching::app_of_address(text).is_some() => None,
        SiteMatcher::Domain(text) | SiteMatcher::Host(text) | SiteMatcher::Url(text) => {
            Some(registrable_domain(&host_of(text)?))
        }
        _ => None,
    }
}

/// Registrable domains a login is saved for.
pub fn entry_domains(entry: &Entry) -> Vec<String> {
    entry.login().map_or_else(Vec::new, |login| {
        let mut out: Vec<String> = login
            .site_matchers
            .v
            .iter()
            .filter_map(matcher_domain)
            .collect();
        out.sort();
        out.dedup();
        out
    })
}

/// The registrable domain of the first site a login is saved for, in the order it was saved:
/// the one its icon and its group go by. [`entry_domains`] sorts them.
pub fn first_domain(entry: &Entry) -> Option<String> {
    entry.login()?.site_matchers.v.iter().find_map(matcher_domain)
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
    let old_after = u64::from(if input.old_after_days == 0 {
        365
    } else {
        input.old_after_days
    });
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
        let Kind::Login(login) = &entry.kind else {
            continue;
        };
        if input.skip_local && local_only(entry) {
            continue;
        }
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
        let site = domains
            .first()
            .cloned()
            .unwrap_or_else(|| entry.common.title.v.to_lowercase());
        by_identity
            .entry((site, login.username.v.to_lowercase(), print))
            .or_default()
            .push(entry.id.clone());

        // Zero days is no reminder, as every screen shows it, not one that is always due.
        let limit = reminders.change_every_days.filter(|days| *days > 0).map_or(old_after, u64::from);
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
        // Plain http to this same machine never crosses a network, and browsers count it as
        // secure. A router's page on the local network does cross one; leaving those out is
        // what `skip_local` is for.
        let plain = |m: &SiteMatcher| {
            matches!(m, SiteMatcher::Url(url) if url.trim_start().to_ascii_lowercase().starts_with("http://")
                && !host_of(url).is_some_and(|host| is_loopback(&host)))
        };
        if login.site_matchers.v.iter().any(plain) {
            out.insecure_url.push(entry.id.clone());
        }
    }
    out.similar = similar_groups(vault, by_password.values());
    out.reused = groups(by_password);
    out.duplicates = groups(by_identity);
    let skip = |check: &str, id: &Id| input.ignored.get(check).is_some_and(|ids| ids.contains(id));
    out.weak.retain(|id| !skip("weak", id));
    out.old.retain(|id| !skip("old", id));
    out.breached.retain(|(id, _)| !skip("breached", id));
    out.missing_two_factor.retain(|id| !skip("twofactor", id));
    out.insecure_url.retain(|id| !skip("insecure", id));
    out.expiring.retain(|id| !skip("expiring", id));
    let keep = |check: &str, sets: Vec<Vec<Id>>| -> Vec<Vec<Id>> {
        sets.into_iter()
            .map(|set| set.into_iter().filter(|id| !skip(check, id)).collect::<Vec<_>>())
            .filter(|set| set.len() > 1)
            .collect()
    };
    out.reused = keep("reused", std::mem::take(&mut out.reused));
    out.duplicates = keep("duplicates", std::mem::take(&mut out.duplicates));
    let exact = out.duplicates.clone();
    out.similar = keep("similar", std::mem::take(&mut out.similar));
    out.similar.retain(|group| !exact.contains(group));

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

/// Whether two edits or fewer turn one into the other, scaled to length: a slip of a key in a
/// short name, two in a long one. Under four letters only an exact match counts.
fn near(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let shorter = a.len().min(b.len());
    let allowed = match shorter {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    };
    if a.len().abs_diff(b.len()) > allowed {
        return false;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == cb { previous } else { 1 + previous.min(above).min(row[j]) };
            previous = above;
        }
    }
    row[b.len()] <= allowed
}

/// A username with case, dots, dashes and underscores gone, for telling spellings apart.
fn squeezed(text: &str) -> String {
    text.trim().to_lowercase().chars().filter(|c| !matches!(c, '.' | '-' | '_' | ' ')).collect()
}

/// Whether two usernames could be one person's on one site. Either left empty could be; an
/// email and a name are when the name is the email's own first part; two different emails are
/// two accounts unless one is a slip of the other.
fn same_user(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim().to_lowercase(), b.trim().to_lowercase());
    if a.is_empty() || b.is_empty() || a == b {
        return true;
    }
    let local = |text: &str| squeezed(text.split('@').next().unwrap_or(text));
    match (a.contains('@'), b.contains('@')) {
        (true, true) => near(&a, &b),
        (false, false) => squeezed(&a) == squeezed(&b) || near(&a, &b),
        _ => local(&a) == local(&b),
    }
}

/// The name a site goes by in a title: the domain's own label for an address, the letters and
/// digits otherwise. "github.com" and "GitHub" both come out as "github".
fn site_name(title: &str) -> String {
    let title = title.trim().to_lowercase();
    if title.contains('.') && !title.contains(' ') {
        let domain = registrable_domain(&host_of(&title).unwrap_or(title));
        return domain.split('.').next().unwrap_or_default().to_owned();
    }
    title.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// What a username is: an email, a phone number, or a plain name.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Handle {
    Email,
    Phone,
    Name,
}

fn handle_of(text: &str) -> Handle {
    let text = text.trim();
    if text.contains('@') {
        return Handle::Email;
    }
    let digits = text.chars().filter(char::is_ascii_digit).count();
    let phoneish = text.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | ' ' | '-' | '(' | ')' | '.'));
    if phoneish && digits >= 6 {
        Handle::Phone
    } else {
        Handle::Name
    }
}

/// Whether two logins that share a password look like one account saved twice.
fn same_account(a: &Entry, b: &Entry) -> bool {
    let (Some(first), Some(second)) = (a.login(), b.login()) else { return false };
    let (ours, theirs) = (entry_domains(a), entry_domains(b));
    let (name_a, name_b) = (site_name(&a.common.title.v), site_name(&b.common.title.v));
    let named = |domains: &[String], name: &str| {
        !name.is_empty() && domains.iter().any(|domain| domain.split('.').next() == Some(name))
    };
    // The same site for certain: a shared domain, or one name for it.
    let surely = ours.iter().any(|domain| theirs.contains(domain))
        || (!name_a.is_empty() && name_a == name_b)
        || named(&ours, &name_b)
        || named(&theirs, &name_a);
    if same_user(&first.username.v, &second.username.v) {
        return surely || (!name_a.is_empty() && near(&name_a, &name_b));
    }
    // Different handles of different kinds, an email and a phone number or a name, on surely
    // the same site with the same password: most sites take any of them to sign in to one
    // account. Two different emails, or two different names, stay two accounts.
    surely && handle_of(&first.username.v) != handle_of(&second.username.v)
}

/// Within each set sharing a password, the logins that look like one account, grouped.
fn similar_groups<'a>(vault: &Vault, sets: impl Iterator<Item = &'a Vec<Id>>) -> Vec<Vec<Id>> {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut out = Vec::new();
    for set in sets.filter(|set| set.len() > 1) {
        let entries: Vec<&Entry> = set.iter().filter_map(|id| vault.get(id)).collect();
        let mut parent: Vec<usize> = (0..entries.len()).collect();
        for i in 0..entries.len() {
            for j in i + 1..entries.len() {
                if same_account(entries[i], entries[j]) {
                    let (x, y) = (root(&mut parent, i), root(&mut parent, j));
                    parent[x] = y;
                }
            }
        }
        let mut clusters: BTreeMap<usize, Vec<Id>> = BTreeMap::new();
        for (i, entry) in entries.iter().enumerate() {
            let at = root(&mut parent, i);
            clusters.entry(at).or_default().push(entry.id.clone());
        }
        out.extend(clusters.into_values().filter(|group| group.len() > 1).map(|mut group| {
            group.sort();
            group
        }));
    }
    out.sort();
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
pub fn merge_duplicates(
    vault: &mut Vault,
    keep: &str,
    remove: &str,
    now_ms: u64,
) -> crate::Result<()> {
    // Folding an entry into itself would only send it to the trash.
    if keep == remove {
        return Err(crate::Error::Invalid("choose two different entries to merge".into()));
    }
    let other = vault
        .get(remove)
        .cloned()
        .ok_or_else(|| crate::Error::Invalid("no such entry".into()))?;
    vault.edit(keep, now_ms, |entry, at| {
        fn joined<T: Clone + PartialEq>(mine: &[T], theirs: &[T], same: impl Fn(&T, &T) -> bool) -> Vec<T> {
            let mut out = mine.to_vec();
            for item in theirs {
                if !out.iter().any(|held| same(held, item)) {
                    out.push(item.clone());
                }
            }
            out
        }
        // Lists join rather than one replacing the other: a newer list of files or fields on
        // either side would otherwise drop the rest, and the files of the one sent to the trash
        // with it.
        let tags = joined(&entry.common.tags.v, &other.common.tags.v, PartialEq::eq);
        let files = joined(&entry.common.attachments.v, &other.common.attachments.v, |a, b| a.blob == b.blob);
        let fields = joined(&entry.common.custom_fields.v, &other.common.custom_fields.v, |a, b| {
            a.name == b.name && a.value == b.value
        });
        let mut merged = other.clone();
        merged.id.clone_from(&entry.id);
        // The one kept stays where it is: a copy already in the trash or on the shelf must not
        // take it there too.
        merged.common.deleted_at_ms.clone_from(&entry.common.deleted_at_ms);
        merged.common.hidden.clone_from(&entry.common.hidden);
        // Nor may the other's self-destruct timer come across and take the kept one with it.
        merged.common.destroy_at_ms.clone_from(&entry.common.destroy_at_ms);
        entry.absorb_item(&merged, at);
        if entry.common.tags.v != tags {
            entry.common.tags = Lww::new(tags, at);
        }
        if entry.common.attachments.v != files {
            entry.common.attachments = Lww::new(files, at);
        }
        if entry.common.custom_fields.v != fields {
            entry.common.custom_fields = Lww::new(fields, at);
        }
    })?;
    vault.trash(remove, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Hlc;
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
            entry.login_mut().unwrap().site_matchers =
                Lww::new(vec![SiteMatcher::Url(site.into())], at);
        }
        vault.add(entry, NOW).unwrap()
    }

    fn run(vault: &Vault) -> HealthReport {
        report(
            vault,
            &HealthInput {
                now_ms: NOW,
                ..HealthInput::default()
            },
        )
    }

    #[test]
    fn the_first_domain_is_the_first_saved() {
        let at = Hlc::default();
        let mut entry = login_entry("Outlook", "me", "pw", at);
        entry.login_mut().unwrap().site_matchers = Lww::new(
            vec![
                SiteMatcher::Url("androidapp://com.microsoft.office.outlook".into()),
                SiteMatcher::Url("https://account.microsoft.com".into()),
                SiteMatcher::Url("https://outlook.live.com/mail".into()),
            ],
            at,
        );
        assert_eq!(first_domain(&entry).as_deref(), Some("microsoft.com"));
        assert_eq!(entry_domains(&entry), vec!["live.com".to_owned(), "microsoft.com".to_owned()]);
    }

    #[test]
    fn one_account_saved_twice_is_told_from_a_reused_password() {
        let mut vault = fast_vault(1);
        let a = add(&mut vault, "GitHub", Some("https://github.com"), "jane.doe", STRONG);
        let b = add(&mut vault, "github.com", None, "janedoe@example.com", STRONG);
        let c = add(&mut vault, "Githb", Some("https://gist.github.com"), "", STRONG);
        // The same password, but somebody else's site and another email: plain reuse.
        add(&mut vault, "Bank", Some("https://bank.example"), "other@example.com", STRONG);
        let found = run(&vault);
        let mut expected = vec![a, b, c];
        expected.sort();
        assert_eq!(found.similar, vec![expected]);
        assert!(same_user("Jane.Doe", "janedoe@mail.example"));
        assert!(!same_user("jane@one.example", "jane@two.example"));
        assert!(near("netflix", "netflx"));
        assert!(!near("ebay", "etsy"));
    }

    #[test]
    fn an_email_and_a_phone_or_name_for_one_site_look_like_one_account() {
        let mut vault = fast_vault(1);
        let other = "Zq7!pL3#wK9$mN2@vB5&";
        let a = add(&mut vault, "Apex", None, "someone@example.com", STRONG);
        let b = add(&mut vault, "Apex", None, "01715 378075", STRONG);
        let c = add(&mut vault, "TheTVDB", None, "WasiMaster", other);
        let d = add(&mut vault, "The TV DB", Some("https://thetvdb.com"), "someone@example.com", other);
        // Two different emails, or two different names, on one site stay two accounts.
        let third = "Hy4%rT8^cX1*jD6(sF0)";
        add(&mut vault, "Shop", None, "one@example.com", third);
        add(&mut vault, "Shop", None, "two@example.com", third);
        let fourth = "Gw2)eU7(kR5*aM9&nQ3^";
        add(&mut vault, "Forum", None, "alice", fourth);
        add(&mut vault, "Forum", None, "robert", fourth);
        // A different kind of handle on a site whose name is only close is not enough.
        let fifth = "Lp8&dV4^tJ1%oZ6$cW3#";
        add(&mut vault, "Netflix", None, "someone@example.com", fifth);
        add(&mut vault, "Netflx", None, "01715378075", fifth);
        let found = run(&vault);
        let pair = |x: &Id, y: &Id| {
            let mut group = vec![x.clone(), y.clone()];
            group.sort();
            group
        };
        let mut expected = vec![pair(&a, &b), pair(&c, &d)];
        expected.sort();
        assert_eq!(found.similar, expected);
        assert!(handle_of("+880 1715-378075") == Handle::Phone);
        assert!(handle_of("user123") == Handle::Name);
    }

    #[test]
    fn weak_reused_and_duplicate_are_told_apart() {
        let mut vault = fast_vault(1);
        let weak = add(
            &mut vault,
            "Weak",
            Some("https://weak.example"),
            "me",
            "password",
        );
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
        let at = crate::clock::Hlc {
            ms: NOW - 100 * DAY_MS,
            counter: 0,
            device: 1,
        };
        let mut entry = login_entry("Fresh enough", "u", STRONG, at);
        entry.id = "a".into();
        let mut strict = entry.clone();
        strict.id = "b".into();
        strict.common.reminders = Lww::new(
            Reminders {
                change_every_days: Some(30),
                ..Reminders::default()
            },
            at,
        );
        let mut zero = entry.clone();
        zero.id = "c".into();
        zero.common.reminders = Lww::new(
            Reminders {
                change_every_days: Some(0),
                ..Reminders::default()
            },
            at,
        );
        vault.add(entry, NOW).unwrap();
        vault.add(strict, NOW).unwrap();
        vault.add(zero, NOW).unwrap();
        assert_eq!(run(&vault).old, vec!["b".to_owned()]);
    }

    #[test]
    fn breach_counts_come_through_and_lower_the_score() {
        let mut vault = fast_vault(1);
        let id = add(&mut vault, "A", None, "u", STRONG);
        let breached = BTreeMap::from([(id.clone(), 42)]);
        let result = report(
            &vault,
            &HealthInput {
                now_ms: NOW,
                breached,
                ..HealthInput::default()
            },
        );
        assert_eq!(result.breached, vec![(id, 42)]);
        assert_eq!(result.score, 0);
    }

    #[test]
    fn missing_two_factor_uses_the_directory_and_respects_saved_codes() {
        let mut vault = fast_vault(1);
        let bare = add(
            &mut vault,
            "GitHub",
            Some("https://github.com/login"),
            "u",
            STRONG,
        );
        let covered = add(
            &mut vault,
            "GitHub 2",
            Some("https://github.com/login"),
            "v",
            "yT5!pQ8@zM3#kL9$wR2&",
        );
        vault
            .edit(&covered, NOW, |entry, at| {
                entry.login_mut().unwrap().otp =
                    Lww::new(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP")), at);
            })
            .unwrap();
        add(
            &mut vault,
            "Nowhere",
            Some("https://no-such-site.example"),
            "w",
            "uH7%nB4^cX6*dF1!sA3#",
        );
        assert_eq!(run(&vault).missing_two_factor, vec![bare]);
    }

    #[test]
    fn plain_http_and_expiry_are_flagged() {
        let mut vault = fast_vault(1);
        let http = add(
            &mut vault,
            "Old",
            Some("http://intranet.example"),
            "u",
            STRONG,
        );
        let expiring = add(&mut vault, "Licence", None, "x", "mN2@bV5#cX8$zL1!kJ4%");
        vault
            .edit(&expiring, NOW, |entry, at| {
                let reminders = Reminders {
                    expires_ms: Some(NOW + 3 * DAY_MS),
                    warn_days: 7,
                    ..Reminders::default()
                };
                entry.common.reminders = Lww::new(reminders, at);
            })
            .unwrap();
        let result = run(&vault);
        assert_eq!(result.insecure_url, vec![http]);
        assert_eq!(result.expiring, vec![expiring]);
    }

    #[test]
    fn local_logins_can_be_left_out_and_items_ignored_per_check() {
        let mut vault = fast_vault(1);
        let router = add(&mut vault, "Router", Some("http://192.168.0.1"), "admin", "admin");
        let nas = add(&mut vault, "NAS", Some("http://nas.local:5000"), "me", "admin");
        let site = add(&mut vault, "Shop", Some("http://shop.example"), "me", STRONG);
        let all = run(&vault);
        assert!(all.insecure_url.contains(&router) && all.weak.contains(&nas));
        // This machine's own server is no plain-http risk, whatever the setting.
        let dev = add(&mut vault, "Dev", Some("http://localhost:3000"), "me", STRONG);
        let loop6 = add(&mut vault, "Loop", Some("http://[::1]:8080"), "me", STRONG);
        let app = add(&mut vault, "App", Some("http://app.localhost"), "me", STRONG);
        let found = run(&vault).insecure_url;
        assert!(![dev, loop6, app].iter().any(|id| found.contains(id)), "{found:?}");
        let input = HealthInput {
            now_ms: NOW,
            skip_local: true,
            ignored: BTreeMap::from([("insecure".to_owned(), BTreeSet::from([site.clone()]))]),
            ..HealthInput::default()
        };
        let trimmed = report(&vault, &input);
        assert!(trimmed.weak.is_empty() && trimmed.reused.is_empty());
        assert!(trimmed.insecure_url.is_empty());
        assert_eq!(trimmed.score, 100);
        assert!(is_local_host("localhost") && is_local_host("10.0.0.2") && is_local_host("fd00::1"));
        assert!(!is_local_host("example.com") && !is_local_host("8.8.8.8"));
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
        let keep = add(
            &mut vault,
            "A",
            Some("https://a.example"),
            "me",
            "old-password-1",
        );
        let remove = add(
            &mut vault,
            "B",
            Some("https://a.example"),
            "me",
            "newer-password-2",
        );
        vault
            .edit(&remove, NOW + 5, |entry, at| {
                entry.set_password("newest-password-3".into(), at);
                entry.common.tags = Lww::new(vec!["work".into()], at);
            })
            .unwrap();
        merge_duplicates(&mut vault, &keep, &remove, NOW + 10).unwrap();
        let entry = vault.get(&keep).unwrap();
        assert_eq!(
            entry.login().unwrap().password.v.expose(),
            "newest-password-3"
        );
        let past: Vec<_> = entry
            .history_of("password")
            .map(|r| r.value.expose().to_owned())
            .collect();
        assert!(past.contains(&"old-password-1".to_owned()), "{past:?}");
        assert!(past.contains(&"newer-password-2".to_owned()), "{past:?}");
        assert_eq!(entry.common.tags.v, vec!["work".to_owned()]);
        assert!(vault.get(&remove).unwrap().is_trashed());
    }

    #[test]
    fn clearing_a_duplicate_from_the_same_import_keeps_what_only_it_held() {
        let mut vault = fast_vault(1);
        // One import stamps every item with the same moment.
        let at = vault.tick(NOW);
        let mut keep = login_entry("A", "me", STRONG, at);
        keep.login_mut().unwrap().site_matchers = Lww::new(vec![SiteMatcher::Url("https://a.example".into())], at);
        // As an importer writes it: every field stamped, the empty ones too.
        keep.login_mut().unwrap().otp = Lww::new(None, at);
        let mut remove = keep.clone();
        remove.login_mut().unwrap().otp = Lww::new(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP")), at);
        remove.common.notes = Lww::new("recovery codes: 1234".into(), at);
        keep.common.notes = Lww::new("kept note".into(), at);
        let keep = vault.add(keep, NOW).unwrap();
        let remove = vault.add(remove, NOW).unwrap();
        merge_duplicates(&mut vault, &keep, &remove, NOW + 10).unwrap();
        let entry = vault.get(&keep).unwrap();
        assert!(entry.login().unwrap().otp.v.is_some(), "the code only the other held is kept");
        assert!(entry.login().unwrap().otp.at > at, "and written anew, so other copies take it");
        assert_eq!(entry.common.notes.v.expose(), "kept note");
        let past: Vec<_> = entry.history_of("notes").map(|r| r.value.expose().to_owned()).collect();
        assert_eq!(past, vec!["recovery codes: 1234".to_owned()]);
    }

    #[test]
    fn clearing_a_duplicate_keeps_the_files_and_fields_of_both() {
        use crate::model::{Field, FieldKind};
        let mut vault = fast_vault(1);
        let keep = add(&mut vault, "A", Some("https://a.example"), "me", STRONG);
        let remove = add(&mut vault, "B", Some("https://a.example"), "me", STRONG);
        let (mine, _) = Vault::seal_attachment("mine.txt", "text/plain", b"1").unwrap();
        let (theirs, _) = Vault::seal_attachment("theirs.txt", "text/plain", b"2").unwrap();
        let field = |name: &str| Field { name: name.into(), value: "v".into(), kind: FieldKind::Hidden };
        vault
            .edit(&keep, NOW + 1, |entry, at| {
                entry.common.attachments = Lww::new(vec![mine.clone()], at);
                entry.common.custom_fields = Lww::new(vec![field("PIN")], at);
            })
            .unwrap();
        // The newer lists are on the copy.
        vault
            .edit(&remove, NOW + 2, |entry, at| {
                entry.common.attachments = Lww::new(vec![theirs.clone()], at);
                entry.common.custom_fields = Lww::new(vec![field("Recovery key")], at);
            })
            .unwrap();
        merge_duplicates(&mut vault, &keep, &remove, NOW + 10).unwrap();
        let entry = vault.get(&keep).unwrap();
        let blobs: Vec<&str> = entry.common.attachments.v.iter().map(|a| a.blob.as_str()).collect();
        assert_eq!(blobs, vec![mine.blob.as_str(), theirs.blob.as_str()]);
        let names: Vec<&str> = entry.common.custom_fields.v.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["PIN", "Recovery key"]);
        // Emptying the trash later leaves the file the kept item now holds.
        assert!(vault.purge(&remove, NOW + 20).is_empty());
    }

    #[test]
    fn a_self_destruct_timer_on_the_copy_does_not_come_across() {
        let mut vault = fast_vault(1);
        let keep = add(&mut vault, "A", Some("https://a.example"), "me", STRONG);
        let remove = add(&mut vault, "B", Some("https://a.example"), "me", STRONG);
        vault
            .edit(&remove, NOW + 5, |entry, at| entry.common.destroy_at_ms = Lww::new(Some(NOW + DAY_MS), at))
            .unwrap();
        merge_duplicates(&mut vault, &keep, &remove, NOW + 10).unwrap();
        assert_eq!(vault.get(&keep).unwrap().common.destroy_at_ms.v, None);
    }

    #[test]
    fn merging_never_trashes_the_one_kept() {
        let mut vault = fast_vault(1);
        let keep = add(&mut vault, "A", Some("https://a.example"), "me", STRONG);
        assert!(merge_duplicates(&mut vault, &keep, &keep, NOW + 1).is_err());
        assert!(!vault.get(&keep).unwrap().is_trashed());
        let remove = add(&mut vault, "B", Some("https://a.example"), "me", STRONG);
        vault.trash(&remove, NOW + 5).unwrap();
        merge_duplicates(&mut vault, &keep, &remove, NOW + 10).unwrap();
        assert!(!vault.get(&keep).unwrap().is_trashed());
    }

    #[test]
    fn directory_loads() {
        let tfa = two_factor_domains();
        assert!(tfa.len() > 1000);
        assert!(tfa.contains("github.com"));
    }
}
