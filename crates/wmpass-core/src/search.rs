//! The search box's query language, for example:
//! `github tag:work kind:login has:totp modified>30d -old "two words"`
//!
//! Secret values are never matched, except note bodies with `in:notes`, because a
//! search box must not become a way to test what a password is.

use std::collections::HashSet;

use crate::health::entry_domains;
use crate::model::{Entry, Id, Kind};

const DAY_MS: u64 = 86_400_000;

/// One parsed filter.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Term {
    Text(String),
    Not(String),
    Tag(String),
    Kind(String),
    Site(String),
    Folder(String),
    Has(String),
    Flag(String),
    ModifiedWithin(u64),
    ModifiedBefore(u64),
    CreatedWithin(u64),
    CreatedBefore(u64),
    InNotes,
}

/// A parsed query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
}

fn days(text: &str) -> Option<u64> {
    let (number, unit) = text.split_at(text.len().checked_sub(1)?);
    let number: u64 = number.parse().ok()?;
    let days = match unit {
        "d" => number,
        "w" => number * 7,
        "m" => number * 30,
        "y" => number * 365,
        _ => return None,
    };
    Some(days * DAY_MS)
}

fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut current, mut quoted) = (String::new(), false);
    for ch in text.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

impl Query {
    /// Parses a query. Nothing is an error: what cannot be read is plain text.
    pub fn parse(text: &str) -> Self {
        let mut terms = Vec::new();
        for token in tokens(text) {
            let lower = token.to_lowercase();
            let term = if let Some(rest) = lower.strip_prefix("modified>") {
                days(rest).map(Term::ModifiedBefore)
            } else if let Some(rest) = lower.strip_prefix("modified<") {
                days(rest).map(Term::ModifiedWithin)
            } else if let Some(rest) = lower.strip_prefix("created>") {
                days(rest).map(Term::CreatedBefore)
            } else if let Some(rest) = lower.strip_prefix("created<") {
                days(rest).map(Term::CreatedWithin)
            } else if let Some((key, value)) = lower.split_once(':') {
                match key {
                    "tag" => Some(Term::Tag(value.to_owned())),
                    "kind" => Some(Term::Kind(value.to_owned())),
                    "site" => Some(Term::Site(value.to_owned())),
                    "folder" => Some(Term::Folder(value.to_owned())),
                    "has" => Some(Term::Has(value.to_owned())),
                    "in" if value == "notes" => Some(Term::InNotes),
                    _ => None,
                }
            } else {
                match lower.as_str() {
                    "weak" | "reused" | "breached" | "old" | "duplicate" | "pinned" | "favourite"
                    | "favorite" | "hidden" | "trashed" => Some(Term::Flag(lower.replace("favorite", "favourite"))),
                    _ => lower.strip_prefix('-').filter(|rest| !rest.is_empty()).map(|rest| Term::Not(rest.to_owned())),
                }
            };
            terms.push(term.unwrap_or(Term::Text(lower)));
        }
        Self { terms }
    }

    /// Whether the query asks for trashed or shelved entries, which are otherwise left out.
    fn wants(&self, flag: &str) -> bool {
        self.terms.iter().any(|t| matches!(t, Term::Flag(f) if f == flag))
    }
}

/// Health flags the search can filter on, computed elsewhere.
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// Weak passwords.
    pub weak: HashSet<Id>,
    /// Reused passwords.
    pub reused: HashSet<Id>,
    /// Breached passwords.
    pub breached: HashSet<Id>,
    /// Old passwords.
    pub old: HashSet<Id>,
    /// Duplicates.
    pub duplicate: HashSet<Id>,
}

/// Search context.
#[derive(Clone, Debug, Default)]
pub struct Context<'a> {
    /// Now, in milliseconds.
    pub now_ms: u64,
    /// Health flags.
    pub flags: Option<&'a Flags>,
    /// Folder name by id.
    pub folder_name: Option<&'a dyn FolderNames>,
}

/// Looks up folder names.
pub trait FolderNames: std::fmt::Debug {
    /// The name of a folder.
    fn folder_name(&self, id: &str) -> Option<String>;
}

fn subsequence_score(needle: &str, haystack: &str) -> Option<u32> {
    if haystack.contains(needle) {
        let bonus = if haystack.starts_with(needle) { 40 } else { 20 };
        return Some(bonus + needle.len() as u32);
    }
    // In-order letters with few gaps, for typos like "gthub". Only for words long
    // enough that this rarely matches by accident.
    if needle.chars().count() < 4 {
        return None;
    }
    let mut chars = haystack.chars();
    let mut gaps = 0u32;
    for wanted in needle.chars() {
        let mut found = false;
        for ch in chars.by_ref() {
            if ch == wanted {
                found = true;
                break;
            }
            gaps += 1;
        }
        if !found {
            return None;
        }
    }
    (gaps <= 4 + needle.len() as u32 / 2).then_some(5)
}

fn searchable(entry: &Entry) -> Vec<(String, u32)> {
    let mut out = vec![(entry.common.title.v.to_lowercase(), 4)];
    for domain in entry_domains(entry) {
        out.push((domain, 3));
    }
    for tag in &entry.common.tags.v {
        out.push((tag.to_lowercase(), 2));
    }
    match &entry.kind {
        Kind::Login(login) => out.push((login.username.v.to_lowercase(), 3)),
        Kind::Card(card) => {
            out.push((card.holder.v.to_lowercase(), 2));
            out.push((card.issuer.v.to_lowercase(), 3));
        }
        Kind::Identity(identity) => {
            out.push((identity.full_name.v.to_lowercase(), 3));
            out.push((identity.email.v.to_lowercase(), 2));
        }
        Kind::ApiKey(api) => out.push((api.provider.v.to_lowercase(), 3)),
        Kind::Database(db) => {
            out.push((db.host.v.to_lowercase(), 3));
            out.push((db.name.v.to_lowercase(), 3));
            out.push((db.user.v.to_lowercase(), 2));
        }
        Kind::Licence(licence) => out.push((licence.product.v.to_lowercase(), 3)),
        Kind::Wifi(wifi) => out.push((wifi.ssid.v.to_lowercase(), 3)),
        Kind::Otp(otp) => {
            if let Some(config) = &otp.config.v {
                out.push((config.issuer.to_lowercase(), 3));
                out.push((config.account.to_lowercase(), 2));
            }
        }
        Kind::SshKey(ssh) => out.push((ssh.fingerprint.v.to_lowercase(), 1)),
        Kind::Document(document) => out.push((document.label.v.to_lowercase(), 3)),
        Kind::Note(_) | Kind::Recovery(_) => {}
    }
    for field in &entry.common.custom_fields.v {
        out.push((field.name.to_lowercase(), 1));
    }
    out
}

fn has(entry: &Entry, what: &str) -> bool {
    let login = entry.login();
    match what {
        "totp" | "otp" | "2fa" => {
            login.is_some_and(|l| l.otp.v.is_some()) || matches!(&entry.kind, Kind::Otp(_))
        }
        "passkey" => login.is_some_and(|l| !l.passkeys.v.is_empty()),
        "attachment" | "file" => !entry.common.attachments.v.is_empty(),
        "alias" => login.is_some_and(|l| l.alias.v.is_some()),
        "notes" => !entry.common.notes.v.is_empty(),
        "link" => !entry.common.links.v.is_empty(),
        "history" => !entry.history.is_empty(),
        "reminder" => {
            let reminders = &entry.common.reminders.v;
            reminders.change_every_days.is_some() || reminders.expires_ms.is_some()
        }
        _ => false,
    }
}

fn flagged(entry: &Entry, flag: &str, context: &Context<'_>) -> bool {
    let health = |set: fn(&Flags) -> &HashSet<Id>| context.flags.is_some_and(|f| set(f).contains(&entry.id));
    match flag {
        "pinned" => entry.common.pinned.v,
        "favourite" => entry.common.favourite.v,
        "hidden" => entry.common.hidden.v,
        "trashed" => entry.is_trashed(),
        "weak" => health(|f| &f.weak),
        "reused" => health(|f| &f.reused),
        "breached" => health(|f| &f.breached),
        "old" => health(|f| &f.old),
        "duplicate" => health(|f| &f.duplicate),
        _ => false,
    }
}

fn score(entry: &Entry, query: &Query, context: &Context<'_>) -> Option<u32> {
    let fields = searchable(entry);
    let in_notes = query.terms.contains(&Term::InNotes);
    let notes = in_notes.then(|| {
        let mut text = entry.common.notes.v.expose().to_lowercase();
        if let Kind::Note(note) = &entry.kind {
            text.push(' ');
            text.push_str(&note.body.v.expose().to_lowercase());
        }
        text
    });
    let modified = entry.latest().ms;
    let mut total = 0u32;
    for term in &query.terms {
        match term {
            Term::Text(text) => {
                let found = fields
                    .iter()
                    .filter_map(|(field, weight)| subsequence_score(text, field).map(|s| s * weight))
                    .max()
                    .or_else(|| notes.as_ref().filter(|body| body.contains(text.as_str())).map(|_| 1))?;
                total += found;
            }
            Term::Not(text) => {
                if fields.iter().any(|(field, _)| field.contains(text.as_str())) {
                    return None;
                }
            }
            Term::Tag(tag) => {
                if !entry.common.tags.v.iter().any(|t| t.to_lowercase() == *tag) {
                    return None;
                }
            }
            Term::Kind(kind) => {
                if entry.kind.name() != kind {
                    return None;
                }
            }
            Term::Site(site) => {
                if !entry_domains(entry).iter().any(|domain| domain.contains(site.as_str())) {
                    return None;
                }
            }
            Term::Folder(name) => {
                let folder = entry.common.folder.v.as_deref()?;
                let found = context.folder_name?.folder_name(folder)?;
                if !found.to_lowercase().contains(name.as_str()) {
                    return None;
                }
            }
            Term::Has(what) => {
                if !has(entry, what) {
                    return None;
                }
            }
            Term::Flag(flag) => {
                if !flagged(entry, flag, context) {
                    return None;
                }
            }
            Term::ModifiedWithin(span) => {
                if context.now_ms.saturating_sub(modified) > *span {
                    return None;
                }
            }
            Term::ModifiedBefore(span) => {
                if context.now_ms.saturating_sub(modified) < *span {
                    return None;
                }
            }
            Term::CreatedWithin(span) => {
                if context.now_ms.saturating_sub(entry.created_ms) > *span {
                    return None;
                }
            }
            Term::CreatedBefore(span) => {
                if context.now_ms.saturating_sub(entry.created_ms) < *span {
                    return None;
                }
            }
            Term::InNotes => {}
        }
    }
    if entry.common.pinned.v {
        total += 2;
    }
    if entry.common.favourite.v {
        total += 1;
    }
    Some(total)
}

/// Runs a query, best match first. Trashed and shelved entries stay out unless asked for.
pub fn search<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    query: &Query,
    context: &Context<'_>,
) -> Vec<&'a Entry> {
    let (want_trash, want_hidden) = (query.wants("trashed"), query.wants("hidden"));
    let mut found: Vec<(&Entry, u32)> = entries
        .into_iter()
        .filter(|entry| want_trash || !entry.is_trashed())
        .filter(|entry| want_hidden || !entry.common.hidden.v)
        .filter_map(|entry| score(entry, query, context).map(|s| (entry, s)))
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.common.title.v.cmp(&b.0.common.title.v)));
    found.into_iter().map(|(entry, _)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Hlc;
    use crate::model::{Field, Lww, Note, SiteMatcher};
    use crate::otp::OtpConfig;
    use crate::vault::login_entry;

    const NOW: u64 = 1_900_000_000_000;

    fn entry(id: &str, title: &str, user: &str, site: &str) -> Entry {
        let at = Hlc { ms: NOW - 40 * DAY_MS, counter: 0, device: 1 };
        let mut entry = login_entry(title, user, "SuperSecretValue", at);
        entry.id = id.into();
        entry.created_ms = NOW - 40 * DAY_MS;
        entry.login_mut().unwrap().site_matchers = Lww::new(vec![SiteMatcher::Url(site.into())], at);
        entry
    }

    fn fixture() -> Vec<Entry> {
        let at = Hlc { ms: NOW - DAY_MS, counter: 0, device: 1 };
        let mut hub = entry("hub", "GitHub", "octo", "https://github.com");
        hub.common.tags = Lww::new(vec!["Work".into()], at);
        hub.login_mut().unwrap().otp = Lww::new(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP")), at);
        let lab = entry("lab", "GitLab", "octo", "https://gitlab.com");
        let bank = entry("bank", "City Bank", "me", "https://citybank.example");
        let mut note = Entry { id: "note".into(), created_ms: NOW - DAY_MS, ..Entry::default() };
        note.common.title = Lww::new("Door code".into(), at);
        note.kind = Kind::Note(Note { body: Lww::new("the code is 1234".into(), at) });
        let mut gone = entry("gone", "GitGone", "x", "https://gone.example");
        gone.common.deleted_at_ms = Lww::new(Some(NOW), at);
        let mut shelf = entry("shelf", "GitShelf", "x", "https://shelf.example");
        shelf.common.hidden = Lww::new(true, at);
        vec![hub, lab, bank, note, gone, shelf]
    }

    fn ids(query: &str) -> Vec<String> {
        ids_with(query, None)
    }

    fn ids_with(query: &str, flags: Option<&Flags>) -> Vec<String> {
        let entries = fixture();
        let context = Context { now_ms: NOW, flags, folder_name: None };
        search(&entries, &Query::parse(query), &context).iter().map(|e| e.id.clone()).collect()
    }

    #[test]
    fn plain_words_match_titles_and_rank_prefixes_first() {
        assert_eq!(ids("git"), vec!["hub", "lab"]);
        assert_eq!(ids("github"), vec!["hub"]);
        assert_eq!(ids("octo lab"), vec!["lab"]);
    }

    #[test]
    fn small_typos_still_find_it() {
        assert_eq!(ids("gthub"), vec!["hub"]);
        assert!(ids("zzzz").is_empty());
    }

    #[test]
    fn filters_narrow() {
        assert_eq!(ids("tag:work"), vec!["hub"]);
        assert_eq!(ids("kind:note"), vec!["note"]);
        assert_eq!(ids("site:gitlab"), vec!["lab"]);
        assert_eq!(ids("has:totp"), vec!["hub"]);
        assert_eq!(ids("git -hub"), vec!["lab"]);
        assert_eq!(ids("\"city bank\""), vec!["bank"]);
    }

    #[test]
    fn dates_filter_by_age() {
        assert_eq!(ids("modified<7d"), vec!["note", "hub"]);
        assert_eq!(ids("modified>30d kind:login"), vec!["bank", "lab"]);
        assert!(ids("created>365d").is_empty());
    }

    #[test]
    fn secrets_are_not_searchable() {
        assert!(ids("SuperSecretValue").is_empty());
        assert!(ids("1234").is_empty());
        assert_eq!(ids("1234 in:notes"), vec!["note"]);
    }

    #[test]
    fn trash_and_shelf_stay_out_unless_asked_for() {
        assert_eq!(ids("trashed"), vec!["gone"]);
        assert_eq!(ids("hidden"), vec!["shelf"]);
        assert!(!ids("git").contains(&"gone".to_owned()));
        assert!(!ids("git").contains(&"shelf".to_owned()));
    }

    #[test]
    fn health_flags_filter() {
        let flags = Flags { weak: HashSet::from(["lab".to_owned()]), ..Flags::default() };
        assert_eq!(ids_with("weak", Some(&flags)), vec!["lab"]);
        assert!(ids("weak").is_empty());
    }

    #[test]
    fn custom_field_names_match_but_their_values_do_not() {
        let at = Hlc::default();
        let mut entries = fixture();
        entries[0].common.custom_fields =
            Lww::new(vec![Field { name: "Recovery PIN".into(), value: "998877".into(), ..Field::default() }], at);
        let context = Context { now_ms: NOW, ..Context::default() };
        assert_eq!(search(&entries, &Query::parse("recovery"), &context).len(), 1);
        assert_eq!(search(&entries, &Query::parse("998877"), &context).len(), 0);
    }

    #[test]
    fn unreadable_filters_become_plain_text() {
        assert_eq!(Query::parse("foo:bar").terms, vec![Term::Text("foo:bar".into())]);
        assert_eq!(Query::parse("modified>soon").terms, vec![Term::Text("modified>soon".into())]);
        assert_eq!(Query::parse("-").terms, vec![Term::Text("-".into())]);
    }
}
