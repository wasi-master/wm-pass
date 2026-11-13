//! The search box's query language, for example:
//! `github tag:work kind:login has:totp modified>30d -old "two words"`
//!
//! Secret values are never matched, except note bodies with `in:notes`, because a
//! search box must not become a way to test what a password is.

use std::collections::HashSet;

use crate::health::entry_domains;
use crate::matching::{app_of_address, host_of};
use crate::model::{Entry, Id, Kind, SiteMatcher};

const DAY_MS: u64 = 86_400_000;

/// Text as search compares it: lower case, with accents taken off Latin letters, so `cafe`
/// finds Café and `strasse` finds Straße. Marks that make another letter elsewhere, as in
/// Japanese kana, are kept. Lists sorted by name use it too, so Écran sorts among the e's.
pub fn fold(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let lower = text.to_lowercase();
    if lower.is_ascii() {
        return lower;
    }
    let mut out = String::with_capacity(lower.len());
    for ch in lower.nfd() {
        match ch {
            '\u{300}'..='\u{36f}' => {}
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            'ø' => out.push('o'),
            'ł' => out.push('l'),
            'đ' | 'ð' => out.push('d'),
            'ħ' => out.push('h'),
            'ı' => out.push('i'),
            'þ' => out.push_str("th"),
            ch => out.push(ch),
        }
    }
    out.nfc().collect()
}

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
    /// A filter turned around by a leading minus.
    Exclude(Box<Term>),
}

/// A parsed query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    terms: Vec<Term>,
}

fn days(text: &str) -> Option<u64> {
    // The unit is one ASCII letter; anything else, a multi-byte letter included, is no span.
    if !text.is_char_boundary(text.len().checked_sub(1)?) {
        return None;
    }
    let (number, unit) = text.split_at(text.len() - 1);
    let number: u64 = number.parse().ok()?;
    let days = match unit {
        "d" => Some(number),
        "w" => number.checked_mul(7),
        "m" => number.checked_mul(30),
        "y" => number.checked_mul(365),
        _ => return None,
    }?;
    days.checked_mul(DAY_MS)
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
            // `tag:` with nothing after it yet is a filter still being typed, and keeps everything
            // rather than emptying the list for a moment.
            if let Some((key, "")) = lower.trim_start_matches('-').split_once(':')
                && matches!(key, "tag" | "kind" | "site" | "folder" | "has" | "in")
            {
                continue;
            }
            // A minus leaves out what the rest would keep: `-old`, `-tag:work`, `-modified<7d`.
            let term = match lower.strip_prefix('-').filter(|rest| !rest.is_empty()) {
                Some(rest) => Some(match Self::filter(rest) {
                    Some(filter) => Term::Exclude(Box::new(filter)),
                    None => Term::Not(fold(rest)),
                }),
                None => Self::filter(&lower),
            };
            terms.push(term.unwrap_or_else(|| Term::Text(fold(&lower))));
        }
        Self { terms }
    }

    fn filter(lower: &str) -> Option<Term> {
        // An address pasted into the box, as from a browser's address bar, finds the logins for
        // its site: `https://www.github.com/login` is no text any field holds.
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return host_of(lower).map(|host| Term::Site(crate::matching::registrable_domain(&host)));
        }
        if let Some(rest) = lower.strip_prefix("modified>") {
            days(rest).map(Term::ModifiedBefore)
        } else if let Some(rest) = lower.strip_prefix("modified<") {
            days(rest).map(Term::ModifiedWithin)
        } else if let Some(rest) = lower.strip_prefix("created>") {
            days(rest).map(Term::CreatedBefore)
        } else if let Some(rest) = lower.strip_prefix("created<") {
            days(rest).map(Term::CreatedWithin)
        } else if let Some((key, value)) = lower.split_once(':') {
            match key {
                "tag" => Some(Term::Tag(fold(value))),
                // The help spells it the American way, as many people do.
                "kind" => Some(Term::Kind(if value == "license" { "licence".to_owned() } else { value.to_owned() })),
                // `site:https://github.com/login` means the site, not that text.
                // `site:https://github.com/login` means the site, not that text, and `www.` is
                // no part of a site's name: a login saved for github.com is found by either.
                "site" => {
                    let host = if value.contains('/') { host_of(value)? } else { value.to_owned() };
                    Some(Term::Site(host.strip_prefix("www.").map_or_else(|| host.clone(), str::to_owned)))
                }
                "folder" => Some(Term::Folder(fold(value))),
                "has" => Some(Term::Has(value.to_owned())),
                "in" if value == "notes" => Some(Term::InNotes),
                _ => None,
            }
        } else {
            match lower {
                "weak" | "reused" | "breached" | "old" | "duplicate" | "pinned" | "favourite" | "favorite"
                | "hidden" | "trashed" => Some(Term::Flag(lower.replace("favorite", "favourite"))),
                _ => None,
            }
        }
    }

    /// Whether the query filters on a health check, either way round (`weak`, `-reused`), so
    /// the caller knows to work out [`Flags`] first: without them such a filter keeps nothing,
    /// or turned around, leaves nothing out.
    pub fn wants_health(&self) -> bool {
        fn health(term: &Term) -> bool {
            match term {
                Term::Flag(flag) => matches!(flag.as_str(), "weak" | "reused" | "breached" | "old" | "duplicate"),
                Term::Exclude(inner) => health(inner),
                _ => false,
            }
        }
        self.terms.iter().any(health)
    }

    /// Whether the query asks for trashed or shelved entries, which are otherwise left out.
    fn wants(&self, flag: &str) -> bool {
        self.terms
            .iter()
            .any(|t| matches!(t, Term::Flag(f) if f == flag))
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

/// The full hosts a login is saved for, so `mail.example` finds a login for mail.example.com
/// and not only its registrable domain does.
fn entry_hosts(entry: &Entry) -> Vec<String> {
    entry.login().map_or_else(Vec::new, |login| {
        login
            .site_matchers
            .v
            .iter()
            .filter_map(|matcher| match matcher {
                SiteMatcher::Url(text) if app_of_address(text).is_some() => None,
                SiteMatcher::Domain(text) | SiteMatcher::Host(text) | SiteMatcher::Url(text) => host_of(text),
                _ => None,
            })
            .collect()
    })
}

fn searchable(entry: &Entry) -> Vec<(String, u32)> {
    let mut out = vec![(fold(&entry.common.title.v), 4)];
    for domain in entry_domains(entry) {
        out.push((domain, 3));
    }
    for host in entry_hosts(entry) {
        out.push((host, 3));
    }
    for tag in &entry.common.tags.v {
        out.push((fold(tag), 2));
    }
    match &entry.kind {
        Kind::Login(login) => out.push((fold(&login.username.v), 3)),
        Kind::Card(card) => {
            out.push((fold(&card.holder.v), 2));
            out.push((fold(&card.issuer.v), 3));
        }
        Kind::Identity(identity) => {
            out.push((fold(&identity.full_name.v), 3));
            out.push((fold(&identity.email.v), 2));
        }
        Kind::ApiKey(api) => out.push((fold(&api.provider.v), 3)),
        Kind::Database(db) => {
            out.push((fold(&db.host.v), 3));
            out.push((fold(&db.name.v), 3));
            out.push((fold(&db.user.v), 2));
        }
        Kind::Licence(licence) => out.push((fold(&licence.product.v), 3)),
        Kind::Wifi(wifi) => out.push((fold(&wifi.ssid.v), 3)),
        Kind::Otp(otp) => {
            if let Some(config) = &otp.config.v {
                out.push((fold(&config.issuer), 3));
                out.push((fold(&config.account), 2));
            }
        }
        Kind::SshKey(ssh) => out.push((fold(&ssh.fingerprint.v), 1)),
        Kind::Document(document) => out.push((fold(&document.label.v), 3)),
        Kind::Note(_) | Kind::Recovery(_) => {}
    }
    for field in &entry.common.custom_fields.v {
        out.push((fold(&field.name), 1));
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
            reminders.change_every_days.is_some_and(|days| days > 0) || reminders.expires_ms.is_some()
        }
        _ => false,
    }
}

fn flagged(entry: &Entry, flag: &str, context: &Context<'_>) -> bool {
    let health =
        |set: fn(&Flags) -> &HashSet<Id>| context.flags.is_some_and(|f| set(f).contains(&entry.id));
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

/// What one entry offers a query: the fields it may be found by, and its notes when asked.
struct Seen<'e> {
    entry: &'e Entry,
    fields: Vec<(String, u32)>,
    notes: Option<String>,
}

fn score(entry: &Entry, query: &Query, context: &Context<'_>) -> Option<u32> {
    let in_notes = query.terms.contains(&Term::InNotes);
    let notes = in_notes.then(|| {
        let mut text = fold(entry.common.notes.v.expose());
        if let Kind::Note(note) = &entry.kind {
            text.push(' ');
            text.push_str(&fold(note.body.v.expose()));
        }
        text
    });
    let seen = Seen { entry, fields: searchable(entry), notes };
    let mut total = 0u32;
    for term in &query.terms {
        total += term_score(&seen, term, context)?;
    }
    if entry.common.pinned.v {
        total += 2;
    }
    if entry.common.favourite.v {
        total += 1;
    }
    Some(total)
}

/// How well one term finds an entry, or `None` when it rules the entry out.
fn term_score(seen: &Seen<'_>, term: &Term, context: &Context<'_>) -> Option<u32> {
    let entry = seen.entry;
    let modified = entry.latest().ms;
    let keep = |kept: bool| kept.then_some(0);
    match term {
        Term::Text(text) => seen
            .fields
            .iter()
            .filter_map(|(field, weight)| subsequence_score(text, field).map(|s| s * weight))
            .max()
            .or_else(|| {
                seen.notes
                    .as_ref()
                    .filter(|body| body.contains(text.as_str()))
                    .map(|_| 1)
            }),
        Term::Not(text) => keep(!seen.fields.iter().any(|(field, _)| field.contains(text.as_str()))),
        Term::Exclude(inner) => keep(term_score(seen, inner, context).is_none()),
        Term::Tag(tag) => keep(entry.common.tags.v.iter().any(|t| fold(t) == *tag)),
        Term::Kind(kind) => keep(entry.kind.name() == kind),
        Term::Site(site) => keep(
            entry_domains(entry)
                .iter()
                .chain(&entry_hosts(entry))
                .any(|domain| domain.contains(site.as_str())),
        ),
        Term::Folder(name) => {
            let folder = entry.common.folder.v.as_deref()?;
            let found = context.folder_name?.folder_name(folder)?;
            keep(fold(&found).contains(name.as_str()))
        }
        Term::Has(what) => keep(has(entry, what)),
        Term::Flag(flag) => keep(flagged(entry, flag, context)),
        Term::ModifiedWithin(span) => keep(context.now_ms.saturating_sub(modified) <= *span),
        Term::ModifiedBefore(span) => keep(context.now_ms.saturating_sub(modified) >= *span),
        Term::CreatedWithin(span) => keep(context.now_ms.saturating_sub(entry.created_ms) <= *span),
        Term::CreatedBefore(span) => keep(context.now_ms.saturating_sub(entry.created_ms) >= *span),
        Term::InNotes => Some(0),
    }
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
    // Equal scores read in title order as a person reads it: "apple" before "Zebra".
    found.sort_by_cached_key(|(entry, score)| (std::cmp::Reverse(*score), fold(&entry.common.title.v)));
    found.into_iter().map(|(entry, _)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Hlc;
    use crate::model::{Field, Lww, Note};
    use crate::otp::OtpConfig;
    use crate::vault::login_entry;

    const NOW: u64 = 1_900_000_000_000;

    fn entry(id: &str, title: &str, user: &str, site: &str) -> Entry {
        let at = Hlc {
            ms: NOW - 40 * DAY_MS,
            counter: 0,
            device: 1,
        };
        let mut entry = login_entry(title, user, "SuperSecretValue", at);
        entry.id = id.into();
        entry.created_ms = NOW - 40 * DAY_MS;
        entry.login_mut().unwrap().site_matchers =
            Lww::new(vec![SiteMatcher::Url(site.into())], at);
        entry
    }

    fn fixture() -> Vec<Entry> {
        let at = Hlc {
            ms: NOW - DAY_MS,
            counter: 0,
            device: 1,
        };
        let mut hub = entry("hub", "GitHub", "octo", "https://github.com");
        hub.common.tags = Lww::new(vec!["Work".into()], at);
        hub.login_mut().unwrap().otp = Lww::new(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP")), at);
        let lab = entry("lab", "GitLab", "octo", "https://gitlab.com");
        let bank = entry("bank", "City Bank", "me", "https://citybank.example");
        let mut note = Entry {
            id: "note".into(),
            created_ms: NOW - DAY_MS,
            ..Entry::default()
        };
        note.common.title = Lww::new("Door code".into(), at);
        note.kind = Kind::Note(Note {
            body: Lww::new("the code is 1234".into(), at),
        });
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
        let context = Context {
            now_ms: NOW,
            flags,
            folder_name: None,
        };
        search(&entries, &Query::parse(query), &context)
            .iter()
            .map(|e| e.id.clone())
            .collect()
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
        let flags = Flags {
            weak: HashSet::from(["lab".to_owned()]),
            ..Flags::default()
        };
        assert_eq!(ids_with("weak", Some(&flags)), vec!["lab"]);
        assert!(ids("weak").is_empty());
    }

    #[test]
    fn custom_field_names_match_but_their_values_do_not() {
        let at = Hlc::default();
        let mut entries = fixture();
        entries[0].common.custom_fields = Lww::new(
            vec![Field {
                name: "Recovery PIN".into(),
                value: "998877".into(),
                ..Field::default()
            }],
            at,
        );
        let context = Context {
            now_ms: NOW,
            ..Context::default()
        };
        assert_eq!(
            search(&entries, &Query::parse("recovery"), &context).len(),
            1
        );
        assert_eq!(search(&entries, &Query::parse("998877"), &context).len(), 0);
    }

    #[test]
    fn unreadable_filters_become_plain_text() {
        assert_eq!(
            Query::parse("foo:bar").terms,
            vec![Term::Text("foo:bar".into())]
        );
        assert_eq!(
            Query::parse("modified>soon").terms,
            vec![Term::Text("modified>soon".into())]
        );
        assert_eq!(Query::parse("-").terms, vec![Term::Text("-".into())]);
    }

    #[test]
    fn a_span_ending_in_a_wide_letter_or_too_large_is_plain_text() {
        for query in ["modified>5é", "created<日", "modified>99999999999999999y"] {
            assert_eq!(Query::parse(query).terms, vec![Term::Text(fold(query))], "{query}");
        }
    }

    #[test]
    fn a_minus_turns_any_filter_around() {
        let flags = Flags { old: HashSet::from(["lab".to_owned()]), ..Flags::default() };
        // `-old` leaves out what the health check calls old, not titles with "old" in them.
        assert_eq!(ids_with("git -old", Some(&flags)), vec!["hub"]);
        assert_eq!(ids("git -tag:work"), vec!["lab"]);
        assert_eq!(ids("-kind:login"), vec!["note"]);
        assert_eq!(ids("kind:login -modified<7d"), vec!["bank", "lab"]);
        assert_eq!(Query::parse("-zz").terms, vec![Term::Not("zz".into())]);
    }

    #[test]
    fn a_full_host_finds_its_login() {
        let mut entries = fixture();
        entries.push(entry("mail", "Work mail", "me", "https://mail.corp.example/"));
        let context = Context { now_ms: NOW, ..Context::default() };
        let found = |query: &str| search(&entries, &Query::parse(query), &context).iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(found("mail.corp"), vec!["mail"]);
        assert_eq!(found("site:mail.corp"), vec!["mail"]);
        assert_eq!(found("site:https://mail.corp.example/inbox"), vec!["mail"]);
        assert_eq!(found("site:www.corp.example"), vec!["mail"]);
    }

    #[test]
    fn a_filter_still_being_typed_keeps_everything() {
        assert_eq!(ids("tag:"), ids(""));
        assert_eq!(ids("git has:"), ids("git"));
        assert_eq!(ids("-kind:"), ids(""));
    }

    #[test]
    fn accents_fold_on_both_sides() {
        let mut entries = fixture();
        entries.push(entry("cafe", "Café Crème", "zoë", "https://cafe.example"));
        entries.push(entry("strasse", "Hauptstraße", "x", "https://strasse.example"));
        entries.push(entry("kana", "がっこう", "x", "https://kana.example"));
        let context = Context { now_ms: NOW, ..Context::default() };
        let found = |query: &str| search(&entries, &Query::parse(query), &context).iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(found("cafe"), vec!["cafe"]);
        assert_eq!(found("CAFÉ creme"), vec!["cafe"]);
        assert_eq!(found("zoe"), vec!["cafe"]);
        assert_eq!(found("strasse"), vec!["strasse"]);
        assert_eq!(found("café -creme"), Vec::<String>::new());
        // A decomposed é typed in the box is the same letter.
        assert_eq!(found("cafe\u{301}"), vec!["cafe"]);
        // Kana keep their voicing marks: が is not か.
        assert_eq!(found("がっこう"), vec!["kana"]);
        assert!(found("かっこう").is_empty());
        assert_eq!(fold("Ångström"), "angstrom");
        assert_eq!(fold("ŁÓDŹ"), "lodz");
    }

    #[test]
    fn a_pasted_address_finds_its_site() {
        assert_eq!(ids("https://www.github.com/login?next=%2F"), vec!["hub"]);
        assert_eq!(ids("HTTPS://GITLAB.COM"), vec!["lab"]);
        assert!(ids("https://unknown.example/").is_empty());
    }

    #[test]
    fn health_filters_are_seen_either_way_round() {
        for query in ["weak", "git -reused", "\"old\"", "-Breached", "duplicate"] {
            assert!(Query::parse(query).wants_health(), "{query}");
        }
        for query in ["git", "tag:weak", "pinned", "-hidden", "weakling"] {
            assert!(!Query::parse(query).wants_health(), "{query}");
        }
    }

    #[test]
    fn equal_scores_sort_by_title_whatever_its_case() {
        let mut entries = fixture();
        entries.push(entry("zed", "Zed", "x", "https://shared.example"));
        entries.push(entry("ant", "ant", "x", "https://shared.example"));
        let context = Context { now_ms: NOW, ..Context::default() };
        let found: Vec<String> =
            search(&entries, &Query::parse("site:shared"), &context).iter().map(|e| e.id.clone()).collect();
        assert_eq!(found, vec!["ant", "zed"]);
    }

    #[test]
    fn kind_takes_either_spelling_of_licence() {
        assert_eq!(Query::parse("kind:license").terms, vec![Term::Kind("licence".into())]);
    }
}
