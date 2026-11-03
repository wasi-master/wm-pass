//! Zip-based exports: 1Password's 1PUX and Proton Pass.

use std::io::{Cursor, Read};

use serde_json::Value;

use super::json_formats::{list, parse, text};
use super::{Draft, Format, Imported, ImportedEntry, base_entry, non_empty};
use crate::clock::Hlc;
use crate::model::{Card, Identity, Kind, Lww, Note, SshKey};
use crate::{Error, Result, Secret};

const MAX_MEMBER: u64 = super::MAX_INFLATED;

fn member(data: &[u8], name: &str) -> Result<Option<Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|_| Error::Import("the archive could not be read".into()))?;
    let Ok(file) = archive.by_name(name) else {
        return Ok(None);
    };
    if file.size() > MAX_MEMBER {
        return Err(Error::Import("a file in the archive is too large".into()));
    }
    let mut out = Vec::new();
    file.take(MAX_MEMBER)
        .read_to_end(&mut out)
        .map_err(|_| Error::Import("the archive could not be read".into()))?;
    Ok(Some(out))
}

pub(super) fn detect_zip(data: &[u8]) -> Option<Format> {
    let archive = zip::ZipArchive::new(Cursor::new(data)).ok()?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|name| name.ok().map(|n| n.into_owned()))
        .collect();
    if names.iter().any(|name| name == "export.data") {
        return Some(Format::OnePux);
    }
    names
        .iter()
        .any(|name| name.ends_with("data.json"))
        .then_some(Format::ProtonPass)
}

pub(super) fn import_proton_zip(data: &[u8], at: Hlc) -> Result<Imported> {
    let archive = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|_| Error::Import("the archive could not be read".into()))?;
    let name = archive
        .file_names()
        .filter_map(|name| name.ok().map(|n| n.into_owned()))
        .find(|name| name.ends_with("data.json"))
        .ok_or_else(|| Error::Import("no data.json in the archive".into()))?;
    let bytes =
        member(data, &name)?.ok_or_else(|| Error::Import("no data.json in the archive".into()))?;
    let json =
        String::from_utf8(bytes).map_err(|_| Error::Import("the file is not text".into()))?;
    super::json_formats::proton(&json, at)
}

/// A section field's value as text, and whether it is secret. Most values are a
/// string under a key naming their kind; emails, addresses, dates and SSH keys are not.
fn field_value(value: &Value) -> (String, bool) {
    let Some(map) = value.as_object() else {
        return (String::new(), false);
    };
    for (kind, inner) in map {
        let shown = match (kind.as_str(), inner) {
            (_, Value::String(s)) => s.clone(),
            // Year and month as one number: 202605.
            ("monthYear", Value::Number(n)) => match n.as_u64() {
                Some(v) if v >= 100 => format!("{:02}/{}", v % 100, v / 100),
                _ => continue,
            },
            ("date", Value::Number(n)) => n
                .as_i64()
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            (_, Value::Number(n)) => n.to_string(),
            ("email", Value::Object(_)) => text(inner, "email_address"),
            ("address", Value::Object(_)) => ["street", "city", "state", "zip", "country"]
                .into_iter()
                .map(|key| text(inner, key))
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(", "),
            ("sshKey", Value::Object(_)) => inner
                .get("metadata")
                .map(|m| text(m, "privateKey"))
                .filter(|key| !key.is_empty())
                .unwrap_or_else(|| text(inner, "privateKey")),
            _ => continue,
        };
        let secret = matches!(
            kind.as_str(),
            "concealed" | "totp" | "creditCardNumber" | "sshKey"
        );
        return (shown, secret);
    }
    (String::new(), false)
}

/// The attachment a field or a document points at, from the archive's `files/`
/// folder, where each file's name starts with its document id.
fn attachment(data: &[u8], names: &[String], file: &Value) -> Option<(String, Vec<u8>)> {
    let id = text(file, "documentId");
    if id.is_empty() {
        return None;
    }
    let prefix = format!("files/{id}");
    let name = names.iter().find(|n| n.starts_with(&prefix))?;
    let bytes = member(data, name).ok()??;
    let file_name = match text(file, "fileName") {
        n if n.is_empty() => name
            .trim_start_matches(&prefix)
            .trim_start_matches('_')
            .to_owned(),
        n => n,
    };
    Some((file_name, bytes))
}

pub(super) fn import_1pux(data: &[u8], at: Hlc) -> Result<Imported> {
    let bytes = member(data, "export.data")?
        .ok_or_else(|| Error::Import("no export.data in the archive".into()))?;
    let json =
        String::from_utf8(bytes).map_err(|_| Error::Import("the file is not text".into()))?;
    let root = parse(&json)?;
    let names: Vec<String> = zip::ZipArchive::new(Cursor::new(data))
        .map(|archive| {
            archive
                .file_names()
                .filter_map(|name| name.ok().map(|n| n.into_owned()))
                .filter(|name| name.starts_with("files/"))
                .collect()
        })
        .unwrap_or_default();
    let mut out = Imported::default();
    for account in list(&root, "accounts") {
        for vault in list(account, "vaults") {
            let vault_name = vault
                .get("attrs")
                .map_or_else(String::new, |attrs| text(attrs, "name"));
            for item in list(vault, "items") {
                if text(item, "state") == "archived" {
                    continue;
                }
                let overview = item.get("overview").cloned().unwrap_or(Value::Null);
                let details = item.get("details").cloned().unwrap_or(Value::Null);
                let before = out.entries.len();
                read_item(item, &overview, &details, &vault_name, at, &mut out);
                // Files hang off fields, or off the item itself for a document.
                let mut files: Vec<Value> = list(&details, "sections")
                    .iter()
                    .flat_map(|section| list(section, "fields"))
                    .filter_map(|field| field.get("value")?.get("file").cloned())
                    .collect();
                files.extend(details.get("documentAttributes").cloned());
                if let Some(added) = out.entries.get_mut(before) {
                    for file in &files {
                        match attachment(data, &names, file) {
                            Some(found) => added.files.push(found),
                            None => out.skipped.push(format!(
                                "{}: the attachment {} was not in the archive",
                                added.entry.common.title.v,
                                text(file, "fileName")
                            )),
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

fn read_item(
    item: &Value,
    overview: &Value,
    details: &Value,
    vault: &str,
    at: Hlc,
    out: &mut Imported,
) {
    let title = text(overview, "title");
    let notes = text(details, "notesPlain");
    let mut totp = String::new();
    let mut custom = Vec::new();
    let mut section_values: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for section in list(details, "sections") {
        for field in list(section, "fields") {
            let value = field.get("value").cloned().unwrap_or(Value::Null);
            if let Some(code) = value.get("totp").and_then(Value::as_str) {
                totp = code.to_owned();
                continue;
            }
            let (shown, hidden) = field_value(&value);
            section_values.insert(text(field, "id"), shown.clone());
            custom.push((text(field, "title"), shown, hidden));
        }
    }
    match text(item, "categoryUuid").as_str() {
        "001" | "005" => {
            let mut draft = Draft {
                title,
                notes,
                otp: totp,
                fields: custom,
                folder: vault.to_owned(),
                favourite: item.get("favIndex").and_then(Value::as_i64).unwrap_or(0) > 0,
                tags: list(overview, "tags")
                    .iter()
                    .filter_map(|t| t.as_str().map(str::to_owned))
                    .collect(),
                ..Draft::default()
            };
            draft.urls.push(text(overview, "url"));
            for url in list(overview, "urls") {
                let extra = text(url, "url");
                if !draft.urls.contains(&extra) {
                    draft.urls.push(extra);
                }
            }
            for field in list(details, "loginFields") {
                let value = text(field, "value");
                match text(field, "designation").as_str() {
                    "username" if draft.username.is_empty() => draft.username = value,
                    "password" if draft.password.is_empty() => draft.password = value,
                    // Other fields of the sign-in form, kept rather than dropped.
                    _ => draft.fields.push((
                        text(field, "name"),
                        value,
                        text(field, "fieldType") == "P",
                    )),
                }
            }
            if draft.password.is_empty() {
                draft.password = text(details, "password");
            }
            draft.history = list(details, "passwordHistory")
                .iter()
                .map(|h| {
                    (
                        text(h, "value"),
                        h.get("time").and_then(Value::as_u64).unwrap_or(0) * 1000,
                    )
                })
                .collect();
            let entry = draft.into_login(at, &mut out.skipped);
            out.entries.push(entry);
        }
        "003" => {
            let mut entry = base_entry(&title, "", at);
            entry.kind = Kind::Note(Note {
                body: Lww::new(Secret::from(notes), at),
            });
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            out.entries.push(ImportedEntry {
                entry,
                folder: non_empty(vault.to_owned()),
                files: Vec::new(),
            });
        }
        "004" => {
            let cell = |key: &str| section_values.get(key).cloned().unwrap_or_default();
            let name = [cell("firstname"), cell("initial"), cell("lastname")]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let mut entry = base_entry(&title, &notes, at);
            entry.kind = Kind::Identity(Identity {
                full_name: Lww::new(name, at),
                address: Lww::new(cell("address"), at),
                phone: Lww::new(cell("defphone"), at),
                email: Lww::new(cell("email"), at),
                birth_date: Lww::new(Secret::from(cell("birthdate")), at),
                ..Identity::default()
            });
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            out.entries.push(ImportedEntry {
                entry,
                folder: non_empty(vault.to_owned()),
                files: Vec::new(),
            });
        }
        "114" => {
            let key = list(details, "sections")
                .iter()
                .flat_map(|section| list(section, "fields"))
                .find_map(|field| field.get("value")?.get("sshKey").cloned())
                .unwrap_or(Value::Null);
            let meta = key.get("metadata").cloned().unwrap_or(Value::Null);
            let private = match text(&meta, "privateKey") {
                k if k.is_empty() => text(&key, "privateKey"),
                k => k,
            };
            let rest = custom
                .into_iter()
                .filter(|(_, value, _)| *value != private)
                .collect();
            let mut entry = base_entry(&title, &notes, at);
            entry.kind = Kind::SshKey(SshKey {
                private_key: Lww::new(Secret::from(private), at),
                public_key: Lww::new(text(&meta, "publicKey"), at),
                fingerprint: Lww::new(text(&meta, "fingerprint"), at),
                ..SshKey::default()
            });
            entry.common.custom_fields = Lww::new(super::fields(rest), at);
            out.entries.push(ImportedEntry {
                entry,
                folder: non_empty(vault.to_owned()),
                files: Vec::new(),
            });
        }
        "006" => {
            let mut entry = base_entry(&title, "", at);
            entry.kind = Kind::Note(Note {
                body: Lww::new(Secret::from(notes), at),
            });
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            out.entries.push(ImportedEntry {
                entry,
                folder: non_empty(vault.to_owned()),
                files: Vec::new(),
            });
        }
        "002" => {
            let cell = |key: &str| section_values.get(key).cloned().unwrap_or_default();
            let mut entry = base_entry(&title, &notes, at);
            entry.kind = Kind::Card(Card {
                holder: Lww::new(cell("cardholder"), at),
                number: Lww::new(Secret::from(cell("ccnum")), at),
                cvv: Lww::new(Secret::from(cell("cvv")), at),
                expiry: Lww::new(cell("expiry"), at),
                pin: Lww::new(Secret::from(cell("pin")), at),
                ..Card::default()
            });
            out.entries.push(ImportedEntry {
                entry,
                folder: non_empty(vault.to_owned()),
                files: Vec::new(),
            });
        }
        _ => {
            // Anything else keeps its fields on a note, so no data is dropped.
            let mut entry = base_entry(&title, &notes, at);
            entry.kind = Kind::Note(Note::default());
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            out.skipped.push(format!("{title}: kept as a note with its fields, because this 1Password category has no direct match"));
            out.entries.push(ImportedEntry {
                entry,
                folder: non_empty(vault.to_owned()),
                files: Vec::new(),
            });
        }
    }
}
