//! Zip-based exports: 1Password's 1PUX and Proton Pass.

use std::io::{Cursor, Read};

use serde_json::Value;

use super::json_formats::{list, parse, text};
use super::{Draft, Format, Imported, ImportedEntry, base_entry, non_empty};
use crate::clock::Hlc;
use crate::model::{Card, Database, Identity, Kind, Lww, Note, SshKey, Wifi, WifiSecurity};
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
    if names.iter().any(|name| name.ends_with("credentials.csv") || name.ends_with("securenotes.csv")) {
        return Some(Format::DashlaneCsv);
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

/// The fields a kind has no place of its own for, by their ids.
fn unmapped(custom: Vec<(String, String, bool)>, ids: &[String], mapped: &[&str]) -> Vec<(String, String, bool)> {
    custom.into_iter().zip(ids).filter(|(_, id)| !mapped.contains(&id.as_str())).map(|(field, _)| field).collect()
}

/// Dashlane's export: a zip of CSVs, one for each kind of item.
pub(super) fn import_dashlane_zip(data: &[u8], at: Hlc) -> Result<Imported> {
    let archive = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|_| Error::Import("the archive could not be read".into()))?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|name| name.ok().map(|n| n.into_owned()))
        .filter(|name| name.to_ascii_lowercase().ends_with(".csv"))
        .collect();
    let mut out = Imported::default();
    for name in names {
        let Some(bytes) = member(data, &name)? else {
            continue;
        };
        let text = String::from_utf8(bytes).map_err(|_| Error::Import(format!("{name} is not UTF-8 text")))?;
        let text = text.trim_start_matches('\u{feff}');
        if text.trim().is_empty() || super::csv_formats::detect(text) != Some(Format::DashlaneCsv) {
            continue;
        }
        let part = super::csv_formats::import(Format::DashlaneCsv, text, at)?;
        out.entries.extend(part.entries);
        out.skipped.extend(part.skipped);
    }
    Ok(out)
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
                let overview = item.get("overview").cloned().unwrap_or(Value::Null);
                let details = item.get("details").cloned().unwrap_or(Value::Null);
                let before = out.entries.len();
                read_item(item, &overview, &details, &vault_name, at, &mut out);
                // There is no archive here: an archived item comes in tagged so, to be found
                // and cleared out, as 1Password's CSV brings it.
                if text(item, "state") == "archived"
                    && let Some(added) = out.entries.get_mut(before)
                    && !added.entry.common.tags.v.iter().any(|t| t.eq_ignore_ascii_case("archived"))
                {
                    let mut tags = added.entry.common.tags.v.clone();
                    tags.push("Archived".to_owned());
                    added.entry.common.tags = Lww::new(tags, at);
                }
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
    // Each custom field's id beside it, so a kind can leave out the ones it maps to fields.
    let mut custom_ids = Vec::new();
    let mut section_values: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for section in list(details, "sections") {
        for field in list(section, "fields") {
            let value = field.get("value").cloned().unwrap_or(Value::Null);
            // The first code fills a login's; any more are kept as hidden fields.
            if let Some(code) = value.get("totp").and_then(Value::as_str) {
                if totp.is_empty() {
                    totp = code.to_owned();
                } else {
                    custom_ids.push(text(field, "id"));
                    custom.push((text(field, "title"), code.to_owned(), true));
                }
                continue;
            }
            let (shown, hidden) = field_value(&value);
            section_values.insert(text(field, "id"), shown.clone());
            custom_ids.push(text(field, "id"));
            custom.push((text(field, "title"), shown, hidden));
        }
    }
    let category = text(item, "categoryUuid");
    // Only a login has a place for a code: on anything else it is kept as a field.
    if !matches!(category.as_str(), "001" | "005") && !totp.is_empty() {
        custom_ids.push(String::new());
        custom.push(("One-time code".to_owned(), std::mem::take(&mut totp), true));
    }
    let favourite = item.get("favIndex").and_then(Value::as_i64).unwrap_or(0) > 0;
    let tags: Vec<String> = list(overview, "tags").iter().filter_map(|t| t.as_str().map(str::to_owned)).collect();
    let before = out.entries.len();
    read_kind(&category, &title, &notes, totp, custom, &custom_ids, &section_values, details, overview, vault, at, out);
    // Every kind is a favourite or tagged as it was in 1Password.
    if let Some(added) = out.entries.get_mut(before) {
        added.entry.common.favourite = Lww::new(favourite, at);
        added.entry.common.tags = Lww::new(tags, at);
        added.entry.created_ms = super::json_formats::seconds(item.get("createdAt"));
    }
}

#[allow(clippy::too_many_arguments)]
fn read_kind(
    category: &str,
    title: &str,
    notes: &str,
    totp: String,
    custom: Vec<(String, String, bool)>,
    custom_ids: &[String],
    section_values: &std::collections::HashMap<String, String>,
    details: &Value,
    overview: &Value,
    vault: &str,
    at: Hlc,
    out: &mut Imported,
) {
    let (title, notes) = (title.to_owned(), notes.to_owned());
    match category {
        "001" | "005" => {
            let mut draft = Draft {
                title,
                notes,
                otp: totp,
                fields: custom,
                folder: vault.to_owned(),
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
                        h.get("time").and_then(Value::as_u64).unwrap_or(0).saturating_mul(1000),
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
        // A wireless router: its network, and the base station's own password beside it.
        "109" => {
            let cell = |key: &str| section_values.get(key).cloned().unwrap_or_default();
            let security = match cell("wireless_security").trim().to_ascii_lowercase().as_str() {
                "none" => Some(WifiSecurity::Open),
                "wep" => Some(WifiSecurity::Wep),
                "wpa" | "wpa2p" => Some(WifiSecurity::Wpa),
                "wpa3p" => Some(WifiSecurity::Sae),
                "wpa2e" | "wpa3e" => Some(WifiSecurity::Eap),
                _ => None,
            };
            // A security setting not known here stays as the field it came in.
            let mut mapped = vec!["network_name", "wireless_password"];
            if security.is_some() {
                mapped.push("wireless_security");
            }
            let mut entry = base_entry(&title, &notes, at);
            entry.kind = Kind::Wifi(Wifi {
                ssid: Lww::new(cell("network_name"), at),
                password: Lww::new(Secret::from(cell("wireless_password")), at),
                security: Lww::new(security.unwrap_or_default(), at),
                ..Wifi::default()
            });
            entry.common.custom_fields = Lww::new(super::fields(unmapped(custom, custom_ids, &mapped)), at);
            out.entries.push(ImportedEntry { entry, folder: non_empty(vault.to_owned()), files: Vec::new() });
        }
        "102" => {
            let cell = |key: &str| section_values.get(key).cloned().unwrap_or_default();
            let port = cell("port").trim().parse::<u16>().ok();
            // A port that is no number stays as the field it came in.
            let mut mapped = vec!["database_type", "hostname", "database", "username", "password"];
            if port.is_some() {
                mapped.push("port");
            }
            let mut entry = base_entry(&title, &notes, at);
            entry.kind = Kind::Database(Database {
                engine: Lww::new(cell("database_type"), at),
                host: Lww::new(cell("hostname"), at),
                port: Lww::new(port, at),
                name: Lww::new(cell("database"), at),
                user: Lww::new(cell("username"), at),
                password: Lww::new(Secret::from(cell("password")), at),
                ..Database::default()
            });
            entry.common.custom_fields = Lww::new(super::fields(unmapped(custom, custom_ids, &mapped)), at);
            out.entries.push(ImportedEntry { entry, folder: non_empty(vault.to_owned()), files: Vec::new() });
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
                issuer: Lww::new(cell("type"), at),
                ..Card::default()
            });
            // The card's other fields, such as the issuing bank and its phone numbers, are kept
            // as fields rather than dropped.
            const MAPPED: [&str; 6] = ["cardholder", "ccnum", "cvv", "expiry", "pin", "type"];
            let rest = custom
                .into_iter()
                .zip(custom_ids)
                .filter(|(_, id)| !MAPPED.contains(&id.as_str()))
                .map(|(field, _)| field)
                .collect();
            entry.common.custom_fields = Lww::new(super::fields(rest), at);
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
