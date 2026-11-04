//! Folding several items into one, field by field, with the owner choosing where they differ.
//!
//! Lists join: every website, tag, file, passkey and custom field from each item ends up on the
//! one kept. A field one item leaves empty takes the other's value. Only a field the items fill
//! differently is a question, and [`plan`] lists those. Whatever the answers, no secret is lost:
//! a password that loses goes to the kept item's history, and any other value that loses can be
//! kept as a field of its own.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};
use wmpass_core::clock::Hlc;
use wmpass_core::model::{Entry, Field, FieldKind, Kind, Lww, Revision};
use wmpass_core::otp::OtpConfig;
use wmpass_core::vault::Vault;
use wmpass_core::{Error, Result, Secret};

use crate::fields::{self, Raw};

const TITLE: &str = "title";
const NOTES: &str = "notes";
const OTP: &str = "otp";

/// The owner's answers.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Choices {
    /// The item the others fold into.
    pub keep: String,
    /// Every item in the merge, the kept one included.
    pub ids: Vec<String>,
    /// For a field the items fill differently: the item whose value it keeps. The kept item's
    /// own value wins where nothing is said.
    pub take: BTreeMap<String, String>,
    /// Fields whose losing values are kept too: as a field of their own, or for notes, joined.
    pub both: BTreeSet<String>,
}

/// The items, in the order asked, all live and all one kind.
fn gather(vault: &Vault, ids: &[String]) -> Result<Vec<Entry>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let entry = vault.get(id).filter(|e| !e.is_trashed()).ok_or_else(|| Error::Invalid("one of those items is gone".into()))?;
        out.push(entry.clone());
    }
    if out.len() < 2 {
        return Err(Error::Invalid("pick two or more items to merge".into()));
    }
    if out.iter().any(|e| e.kind.name() != out[0].kind.name()) {
        return Err(Error::Invalid("only items of one kind merge".into()));
    }
    Ok(out)
}

fn otp_of(entry: &Entry) -> Option<&OtpConfig> {
    match &entry.kind {
        Kind::Login(login) => login.otp.v.as_ref(),
        Kind::Otp(otp) => otp.config.v.as_ref(),
        _ => None,
    }
}

fn set_otp(entry: &mut Entry, config: Option<OtpConfig>, at: Hlc) {
    match &mut entry.kind {
        Kind::Login(login) => login.otp = Lww::new(config, at),
        Kind::Otp(otp) => otp.config = Lww::new(config, at),
        _ => {}
    }
}

/// How a field keeps a value that lost: in history (passwords, always), as a field of its own,
/// joined with the winner (notes), or not at all (a name, a switch, a choice).
fn keeps(key: &str, value: &Raw) -> &'static str {
    match (key, value) {
        (TITLE, _) | (_, Raw::Flag(_) | Raw::Wifi(_) | Raw::Form(_)) => "none",
        (NOTES, _) => "joined",
        ("password" | "passphrase", _) => "history",
        _ => "field",
    }
}

/// A question for each field the items fill differently, `None` when they agree.
fn differ(key: &str, label: &str, secret: bool, entries: &[Entry], read: impl Fn(&Entry) -> Option<Raw>) -> Option<Value> {
    let mut values: Vec<(Raw, Vec<&str>)> = Vec::new();
    for entry in entries {
        let Some(value) = read(entry).filter(|v| !v.is_empty()) else { continue };
        match values.iter_mut().find(|(seen, _)| *seen == value) {
            Some((_, ids)) => ids.push(entry.id.as_str()),
            None => values.push((value, vec![entry.id.as_str()])),
        }
    }
    if values.len() < 2 {
        return None;
    }
    let both = keeps(key, &values[0].0);
    Some(json!({
        "key": key,
        "label": label,
        "secret": secret,
        "both": both,
        "values": values.iter().map(|(value, ids)| json!({"ids": ids, "shown": value.shown()})).collect::<Vec<_>>(),
    }))
}

/// What the merge would ask: the items, and each field they fill differently.
pub fn plan(vault: &Vault, ids: &[String]) -> Result<Value> {
    let entries = gather(vault, ids)?;
    let kind = entries[0].kind.name();
    let mut questions = Vec::new();
    questions.extend(differ(TITLE, "Name", false, &entries, |e| Some(Raw::Text(e.common.title.v.clone()))));
    for spec in fields::fields_of(kind) {
        questions.extend(differ(spec.key, spec.label, spec.secret, &entries, |e| fields::raw(&mut e.clone(), spec.key).map(|(v, _)| v)));
    }
    questions.extend(differ(OTP, "One-time code", true, &entries, |e| otp_of(e).map(|c| Raw::Secret(c.to_uri()))));
    questions.extend(differ(NOTES, "Notes", true, &entries, |e| Some(Raw::Secret(e.common.notes.v.clone()))));
    Ok(json!({
        "kind": kind,
        "items": entries.iter().map(|e| json!({
            "id": e.id,
            "title": e.common.title.v,
            "subtitle": fields::subtitle(e),
            "changed_ms": e.latest().ms,
        })).collect::<Vec<_>>(),
        "questions": questions,
    }))
}

fn union<T: Clone + PartialEq>(mine: &[T], theirs: &[T], same: impl Fn(&T, &T) -> bool) -> Option<Vec<T>> {
    let mut out = mine.to_vec();
    for item in theirs {
        if !out.iter().any(|held| same(held, item)) {
            out.push(item.clone());
        }
    }
    (out.len() != mine.len()).then_some(out)
}

/// A field for a value that lost, named for the item it came from unless that is the winner's
/// name too.
fn kept_field(label: &str, from_title: &str, winner_title: &str, value: Secret, hidden: bool) -> Field {
    let title = from_title.trim();
    let name = if title.is_empty() || title == winner_title.trim() { format!("{label} (other)") } else { format!("{label} ({title})") };
    Field {
        name,
        value,
        kind: if hidden { FieldKind::Hidden } else { FieldKind::Text },
    }
}

fn push_field(entry: &mut Entry, field: Field, at: Hlc) {
    let mut list = entry.common.custom_fields.v.clone();
    if !list.iter().any(|held| held.value == field.value && held.kind == field.kind) {
        list.push(field);
        entry.common.custom_fields = Lww::new(list, at);
    }
}

fn remember(entry: &mut Entry, key: &str, value: Secret, at: Hlc) {
    let revision = Revision { field: key.to_owned(), value, at };
    if !entry.history.contains(&revision) {
        entry.history.push(revision);
    }
}

/// Folds every other item into the kept one, then sends the others to the trash, where they
/// can still be brought back.
pub fn merge(vault: &mut Vault, choices: &Choices, now_ms: u64) -> Result<()> {
    if !choices.ids.contains(&choices.keep) {
        return Err(Error::Invalid("the item to keep has to be one of those merged".into()));
    }
    let entries = gather(vault, &choices.ids)?;
    let others: Vec<Entry> = entries.iter().filter(|e| e.id != choices.keep).cloned().collect();
    let kind = entries[0].kind.name();
    let merged: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    vault.edit(&choices.keep, now_ms, |entry, at| fold(entry, &others, kind, &merged, choices, at))?;
    for other in &others {
        vault.trash(&other.id, now_ms)?;
    }
    Ok(())
}

fn fold(entry: &mut Entry, others: &[Entry], kind: &str, merged: &[&str], choices: &Choices, at: Hlc) {
    let kept_title = entry.common.title.v.clone();
    let wins = |key: &str, other: &Entry| choices.take.get(key).is_some_and(|id| *id == other.id);
    let both = |key: &str| choices.both.contains(key);

    // The kind's own fields.
    for spec in fields::fields_of(kind) {
        let key = spec.key;
        for other in others {
            let Some((theirs, their_at)) = fields::raw(&mut other.clone(), key) else { continue };
            let Some((mine, _)) = fields::raw(entry, key) else { continue };
            if theirs.is_empty() || theirs == mine {
                continue;
            }
            let style = keeps(key, &theirs);
            let their_title = other.common.title.v.as_str();
            if mine.is_empty() || wins(key, other) {
                let lost = mine.clone();
                fields::put_raw(entry, key, theirs, at);
                // A secret it replaced is in history already; a plain value is kept only if asked.
                if !lost.is_empty() && !spec.secret && both(key) && style == "field" {
                    push_field(entry, kept_field(spec.label, &kept_title, their_title, lost.text(), false), at);
                }
            } else if let Raw::Secret(value) = theirs {
                if both(key) && style == "field" {
                    push_field(entry, kept_field(spec.label, their_title, &kept_title, value, true), at);
                } else {
                    remember(entry, key, value, their_at);
                }
            } else if both(key) && style == "field" {
                push_field(entry, kept_field(spec.label, their_title, &kept_title, theirs.text(), false), at);
            }
        }
    }

    // The name.
    if let Some(id) = choices.take.get(TITLE)
        && let Some(other) = others.iter().find(|o| o.id == *id)
        && !other.common.title.v.is_empty()
    {
        entry.common.title = Lww::new(other.common.title.v.clone(), at);
    } else if entry.common.title.v.is_empty()
        && let Some(other) = others.iter().find(|o| !o.common.title.v.is_empty())
    {
        entry.common.title = Lww::new(other.common.title.v.clone(), at);
    }

    // Notes: one kept, or all of them one after the other.
    let mut notes = vec![entry.common.notes.v.clone()];
    for other in others {
        let theirs = &other.common.notes.v;
        if theirs.is_empty() || notes.contains(theirs) {
            continue;
        }
        if notes[0].is_empty() || wins(NOTES, other) {
            notes.insert(0, theirs.clone());
        } else {
            notes.push(theirs.clone());
        }
    }
    notes.retain(|note| !note.is_empty());
    if let Some(first) = notes.first().cloned() {
        let text = if both(NOTES) {
            Secret::new(notes.iter().map(Secret::expose).collect::<Vec<_>>().join("\n\n"))
        } else {
            for lost in &notes[1..] {
                remember(entry, NOTES, lost.clone(), at);
            }
            first
        };
        if entry.common.notes.v != text {
            entry.common.notes = Lww::new(text, at);
        }
    }

    // The one-time code.
    for other in others {
        let Some(theirs) = otp_of(other).cloned() else { continue };
        let mine = otp_of(entry).cloned();
        if mine.as_ref() == Some(&theirs) {
            continue;
        }
        let (lost, from, winner) = match mine {
            None => {
                set_otp(entry, Some(theirs), at);
                continue;
            }
            Some(mine) if wins(OTP, other) => {
                set_otp(entry, Some(theirs), at);
                (mine, kept_title.as_str(), other.common.title.v.as_str())
            }
            Some(_) => (theirs, other.common.title.v.as_str(), kept_title.as_str()),
        };
        if both(OTP) {
            push_field(entry, kept_field("One-time code", from, winner, lost.to_uri(), true), at);
        } else {
            remember(entry, OTP, lost.to_uri(), at);
        }
    }

    // Lists join.
    for other in others {
        let common = &other.common;
        if let Some(tags) = union(&entry.common.tags.v, &common.tags.v, |a, b| a == b) {
            entry.common.tags = Lww::new(tags, at);
        }
        let links: Vec<String> = common.links.v.iter().filter(|id| !merged.contains(&id.as_str())).cloned().collect();
        if let Some(links) = union(&entry.common.links.v, &links, |a, b| a == b) {
            entry.common.links = Lww::new(links, at);
        }
        if let Some(fields) = union(&entry.common.custom_fields.v, &common.custom_fields.v, |a, b| a.name == b.name && a.value == b.value) {
            entry.common.custom_fields = Lww::new(fields, at);
        }
        if let Some(files) = union(&entry.common.attachments.v, &common.attachments.v, |a, b| a.blob == b.blob) {
            entry.common.attachments = Lww::new(files, at);
        }
        if common.favourite.v && !entry.common.favourite.v {
            entry.common.favourite = Lww::new(true, at);
        }
        if common.pinned.v && !entry.common.pinned.v {
            entry.common.pinned = Lww::new(true, at);
        }
        if entry.common.folder.v.is_none() && common.folder.v.is_some() {
            entry.common.folder = Lww::new(common.folder.v.clone(), at);
        }
        if entry.common.color.v.is_none() && common.color.v.is_some() {
            entry.common.color = Lww::new(common.color.v.clone(), at);
        }
        if entry.common.icon.v.is_none() && common.icon.v.is_some() {
            entry.common.icon = Lww::new(common.icon.v.clone(), at);
        }
        if entry.common.reminders.v == Default::default() && common.reminders.v != Default::default() {
            entry.common.reminders = Lww::new(common.reminders.v.clone(), at);
        }
        match (&mut entry.kind, &other.kind) {
            (Kind::Login(mine), Kind::Login(theirs)) => {
                if let Some(sites) = union(&mine.site_matchers.v, &theirs.site_matchers.v, |a, b| a == b) {
                    mine.site_matchers = Lww::new(sites, at);
                }
                if let Some(keys) = union(&mine.passkeys.v, &theirs.passkeys.v, |a, b| a.credential_id == b.credential_id) {
                    mine.passkeys = Lww::new(keys, at);
                }
                if let Some(questions) = union(&mine.questions.v, &theirs.questions.v, |a, b| a.name == b.name && a.value == b.value) {
                    mine.questions = Lww::new(questions, at);
                }
                if mine.alias.v.is_none() && theirs.alias.v.is_some() {
                    mine.alias = Lww::new(theirs.alias.v.clone(), at);
                }
            }
            (Kind::Identity(mine), Kind::Identity(theirs)) => {
                if let Some(numbers) = union(&mine.document_numbers.v, &theirs.document_numbers.v, |a, b| a.name == b.name && a.value == b.value) {
                    mine.document_numbers = Lww::new(numbers, at);
                }
            }
            _ => {}
        }
        for revision in &other.history {
            if !entry.history.contains(revision) {
                entry.history.push(revision.clone());
            }
        }
        entry.created_ms = entry.created_ms.min(other.created_ms);
    }
    entry.history.sort_by_key(|revision| revision.at);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wmpass_core::crypto::KdfParams;
    use wmpass_core::format::KdfConfig;
    use wmpass_core::model::SiteMatcher;
    use wmpass_core::vault::login_entry;

    const NOW: u64 = 2_000_000_000_000;

    /// The lowest settings the format accepts, so each test derives once, quickly.
    fn fast_vault(device: u32) -> Vault {
        let config = KdfConfig::new(KdfParams { m_kib: 65_536, t: 3, p: 1 }).unwrap();
        Vault::create("Test", b"pw", &config, device, 1000).unwrap()
    }

    fn add(vault: &mut Vault, title: &str, site: &str, user: &str, password: &str, tag: &str) -> String {
        let at = vault.tick(NOW);
        let mut entry = login_entry(title, user, password, at);
        entry.login_mut().unwrap().site_matchers = Lww::new(vec![SiteMatcher::Url(site.into())], at);
        entry.common.tags = Lww::new(vec![tag.into()], at);
        vault.add(entry, NOW).unwrap()
    }

    fn password(vault: &Vault, id: &str) -> String {
        vault.get(id).unwrap().login().unwrap().password.v.expose().to_owned()
    }

    #[test]
    fn the_plan_asks_only_where_items_differ() {
        let mut vault = fast_vault(1);
        let a = add(&mut vault, "GitHub", "https://github.com", "jane", "one", "Developer");
        let b = add(&mut vault, "GitHub", "https://gist.github.com", "jane@example.com", "one", "Work");
        let first = plan(&vault, &[a.clone(), b.clone()]).unwrap();
        let keys: Vec<&str> = first["questions"].as_array().unwrap().iter().map(|q| q["key"].as_str().unwrap()).collect();
        assert_eq!(keys, vec!["username"]);
        let username = &first["questions"][0];
        assert_eq!(username["values"][1]["shown"], "jane@example.com");
        assert_eq!(username["both"], "field");
        // A secret is never in the plan, only who holds it.
        let c = add(&mut vault, "GitHub", "https://github.com", "jane", "two", "");
        let second = plan(&vault, &[a, c]).unwrap();
        let question = &second["questions"][0];
        assert_eq!(question["key"], "password");
        assert!(question["values"][0]["shown"].is_null());
    }

    #[test]
    fn a_merge_joins_lists_keeps_answers_and_loses_no_secret() {
        let mut vault = fast_vault(1);
        let a = add(&mut vault, "GitHub", "https://github.com", "jane", "old-one", "Developer");
        let b = add(&mut vault, "Github work", "https://gist.github.com", "jane@example.com", "new-one", "Work");
        let choices = Choices {
            keep: a.clone(),
            ids: vec![a.clone(), b.clone()],
            take: BTreeMap::from([("password".to_owned(), b.clone())]),
            both: BTreeSet::from(["username".to_owned()]),
        };
        merge(&mut vault, &choices, NOW + 10).unwrap();
        let kept = vault.get(&a).unwrap();
        let login = kept.login().unwrap();
        assert_eq!(password(&vault, &a), "new-one");
        assert!(kept.history.iter().any(|r| r.field == "password" && r.value.expose() == "old-one"));
        assert_eq!(login.username.v, "jane");
        let other = kept.common.custom_fields.v.iter().find(|f| f.name == "Username (Github work)").unwrap();
        assert_eq!(other.value.expose(), "jane@example.com");
        assert_eq!(login.site_matchers.v.len(), 2);
        assert_eq!(kept.common.tags.v, vec!["Developer".to_owned(), "Work".to_owned()]);
        assert_eq!(kept.common.title.v, "GitHub");
        assert!(vault.get(&b).unwrap().is_trashed());
    }

    #[test]
    fn a_losing_password_goes_to_history_even_when_kept_wins() {
        let mut vault = fast_vault(1);
        let a = add(&mut vault, "Site", "https://site.example", "me", "kept", "");
        let b = add(&mut vault, "Site", "https://site.example", "me", "lost", "");
        merge(&mut vault, &Choices { keep: a.clone(), ids: vec![a.clone(), b], ..Choices::default() }, NOW + 10).unwrap();
        assert_eq!(password(&vault, &a), "kept");
        assert!(vault.get(&a).unwrap().history.iter().any(|r| r.value.expose() == "lost"));
    }

    #[test]
    fn items_of_two_kinds_or_one_item_do_not_merge() {
        let mut vault = fast_vault(1);
        let a = add(&mut vault, "Site", "https://site.example", "me", "x", "");
        let at = vault.tick(NOW);
        let note = vault
            .add(Entry { kind: Kind::Note(wmpass_core::model::Note::default()), common: wmpass_core::model::Common { title: Lww::new("n".into(), at), ..Default::default() }, ..Entry::default() }, NOW)
            .unwrap();
        assert!(plan(&vault, &[a.clone(), note]).is_err());
        assert!(plan(&vault, &[a.clone(), a]).is_err());
    }
}
