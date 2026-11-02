//! Zip-based exports: 1Password's 1PUX and Proton Pass.

use std::io::{Cursor, Read};

use serde_json::Value;

use super::json_formats::{list, parse, text};
use super::{Draft, Format, Imported, ImportedEntry, base_entry, non_empty};
use crate::clock::Hlc;
use crate::model::{Card, Kind, Lww, Note};
use crate::{Error, Result, Secret};

const MAX_MEMBER: u64 = 512 * 1024 * 1024;

fn member(data: &[u8], name: &str) -> Result<Option<Vec<u8>>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(|_| Error::Import("the archive could not be read".into()))?;
    let Ok(file) = archive.by_name(name) else { return Ok(None) };
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
    let names: Vec<String> = archive.file_names().filter_map(|name| name.ok().map(|n| n.into_owned())).collect();
    if names.iter().any(|name| name == "export.data") {
        return Some(Format::OnePux);
    }
    names.iter().any(|name| name.ends_with("data.json")).then_some(Format::ProtonPass)
}

pub(super) fn import_proton_zip(data: &[u8], at: Hlc) -> Result<Imported> {
    let archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(|_| Error::Import("the archive could not be read".into()))?;
    let name = archive
        .file_names()
        .filter_map(|name| name.ok().map(|n| n.into_owned()))
        .find(|name| name.ends_with("data.json"))
        .ok_or_else(|| Error::Import("no data.json in the archive".into()))?;
    let bytes = member(data, &name)?.ok_or_else(|| Error::Import("no data.json in the archive".into()))?;
    let json = String::from_utf8(bytes).map_err(|_| Error::Import("the file is not text".into()))?;
    super::json_formats::proton(&json, at)
}

fn field_value(value: &Value) -> (String, bool) {
    let Some(map) = value.as_object() else { return (String::new(), false) };
    for (kind, inner) in map {
        let text = match inner {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => continue,
        };
        return (text, kind == "concealed" || kind == "totp");
    }
    (String::new(), false)
}

pub(super) fn import_1pux(data: &[u8], at: Hlc) -> Result<Imported> {
    let bytes = member(data, "export.data")?.ok_or_else(|| Error::Import("no export.data in the archive".into()))?;
    let json = String::from_utf8(bytes).map_err(|_| Error::Import("the file is not text".into()))?;
    let root = parse(&json)?;
    let mut out = Imported::default();
    for account in list(&root, "accounts") {
        for vault in list(account, "vaults") {
            let vault_name = vault.get("attrs").map_or_else(String::new, |attrs| text(attrs, "name"));
            for item in list(vault, "items") {
                if text(item, "state") == "archived" {
                    continue;
                }
                let overview = item.get("overview").cloned().unwrap_or(Value::Null);
                let details = item.get("details").cloned().unwrap_or(Value::Null);
                read_item(item, &overview, &details, &vault_name, at, &mut out);
            }
        }
    }
    Ok(out)
}

fn read_item(item: &Value, overview: &Value, details: &Value, vault: &str, at: Hlc, out: &mut Imported) {
    let title = text(overview, "title");
    let notes = text(details, "notesPlain");
    let mut totp = String::new();
    let mut custom = Vec::new();
    let mut section_values: std::collections::HashMap<String, String> = std::collections::HashMap::new();
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
                tags: list(overview, "tags").iter().filter_map(|t| t.as_str().map(str::to_owned)).collect(),
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
                match text(field, "designation").as_str() {
                    "username" => draft.username = text(field, "value"),
                    "password" => draft.password = text(field, "value"),
                    _ => {}
                }
            }
            if draft.password.is_empty() {
                draft.password = text(details, "password");
            }
            draft.history = list(details, "passwordHistory")
                .iter()
                .map(|h| (text(h, "value"), h.get("time").and_then(Value::as_u64).unwrap_or(0) * 1000))
                .collect();
            let entry = draft.into_login(at, &mut out.skipped);
            out.entries.push(entry);
        }
        "003" => {
            let mut entry = base_entry(&title, "", at);
            entry.kind = Kind::Note(Note { body: Lww::new(Secret::from(notes), at) });
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            out.entries.push(ImportedEntry { entry, folder: non_empty(vault.to_owned()), files: Vec::new() });
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
            out.entries.push(ImportedEntry { entry, folder: non_empty(vault.to_owned()), files: Vec::new() });
        }
        _ => {
            // Anything else keeps its fields on a note, so no data is dropped.
            let mut entry = base_entry(&title, &notes, at);
            entry.kind = Kind::Note(Note::default());
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            out.skipped.push(format!("{title}: kept as a note with its fields, because this 1Password category has no direct match"));
            out.entries.push(ImportedEntry { entry, folder: non_empty(vault.to_owned()), files: Vec::new() });
        }
    }
}
