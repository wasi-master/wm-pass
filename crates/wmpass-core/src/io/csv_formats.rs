//! CSV exports. Each app has its own columns, read by header name.

use std::collections::HashMap;

use super::{ColumnMap, Draft, Format, Imported, base_entry};
use crate::clock::Hlc;
use crate::model::{Field, Kind, Lww, Note};
use crate::{Error, Result, Secret};

struct Row<'a> {
    record: &'a csv::StringRecord,
    columns: &'a HashMap<String, usize>,
}

impl Row<'_> {
    fn get(&self, name: &str) -> String {
        self.columns
            .get(name)
            .and_then(|at| self.record.get(*at))
            .unwrap_or_default()
            .to_owned()
    }
}

fn reader(text: &str, has_header: bool) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new().flexible(true).has_headers(has_header).from_reader(text.as_bytes())
}

fn header_of(text: &str) -> Option<Vec<String>> {
    let mut reader = reader(text, true);
    let headers = reader.headers().ok()?;
    Some(headers.iter().map(|h| h.trim().to_lowercase()).collect())
}

fn has_all(headers: &[String], names: &[&str]) -> bool {
    names.iter().all(|name| headers.iter().any(|h| h == name))
}

pub(super) fn detect(text: &str) -> Option<Format> {
    let headers = header_of(text)?;
    if has_all(&headers, &["wmpass_kind"]) {
        Some(Format::WmPassCsv)
    } else if has_all(&headers, &["login_uri", "login_password"]) {
        Some(Format::BitwardenCsv)
    } else if has_all(&headers, &["grouping", "extra", "password"]) {
        Some(Format::LastPassCsv)
    } else if has_all(&headers, &["group", "title", "username", "password", "url", "notes"]) {
        Some(Format::KeePassXcCsv)
    } else if has_all(&headers, &["otpauth", "title", "url"]) {
        Some(Format::AppleCsv)
    } else if has_all(&headers, &["username2", "password", "title"]) {
        Some(Format::DashlaneCsv)
    } else if has_all(&headers, &["cardholdername", "password", "name"]) {
        Some(Format::NordPassCsv)
    } else if has_all(&headers, &["matchurl", "pwd"]) {
        Some(Format::RoboFormCsv)
    } else if has_all(&headers, &["httprealm", "formactionorigin", "password"]) {
        Some(Format::FirefoxCsv)
    } else if has_all(&headers, &["name", "url", "username", "password"]) {
        Some(Format::ChromeCsv)
    } else {
        None
    }
}

pub(super) fn import(format: Format, text: &str, at: Hlc) -> Result<Imported> {
    let headers = header_of(text).ok_or_else(|| Error::Import("no header row".into()))?;
    let columns: HashMap<String, usize> = headers.into_iter().enumerate().map(|(i, h)| (h, i)).collect();
    let mut out = Imported::default();
    for record in reader(text, true).records() {
        let record = record.map_err(|_| Error::Import("a row could not be read".into()))?;
        let row = Row { record: &record, columns: &columns };
        match format {
            Format::BitwardenCsv => bitwarden_row(&row, at, &mut out),
            Format::LastPassCsv => lastpass_row(&row, at, &mut out),
            Format::WmPassCsv => wmpass_row(&row, at, &mut out),
            _ => {
                let draft = simple_row(format, &row, &mut out.skipped);
                let entry = draft.into_login(at, &mut out.skipped);
                out.entries.push(entry);
            }
        }
    }
    Ok(out)
}

fn simple_row(format: Format, row: &Row<'_>, skipped: &mut Vec<String>) -> Draft {
    match format {
        Format::ChromeCsv => Draft {
            title: row.get("name"),
            urls: vec![row.get("url")],
            username: row.get("username"),
            password: row.get("password"),
            notes: row.get("note"),
            ..Draft::default()
        },
        Format::FirefoxCsv => {
            let url = row.get("url");
            let title = crate::matching::host_of(&url).unwrap_or_else(|| url.clone());
            Draft { title, urls: vec![url], username: row.get("username"), password: row.get("password"), ..Draft::default() }
        }
        Format::KeePassXcCsv => Draft {
            title: row.get("title"),
            username: row.get("username"),
            password: row.get("password"),
            urls: vec![row.get("url")],
            notes: row.get("notes"),
            otp: row.get("totp"),
            folder: row.get("group").trim_start_matches("Root/").to_owned(),
            ..Draft::default()
        },
        Format::DashlaneCsv => {
            let mut fields = Vec::new();
            for extra in ["username2", "username3"] {
                fields.push((extra.to_owned(), row.get(extra), false));
            }
            Draft {
                title: row.get("title"),
                username: row.get("username"),
                password: row.get("password"),
                urls: vec![row.get("url")],
                notes: row.get("note"),
                otp: row.get("otpsecret"),
                folder: row.get("category"),
                fields,
                ..Draft::default()
            }
        }
        Format::NordPassCsv => {
            if !row.get("type").is_empty() && row.get("type") != "password" {
                skipped.push(format!("{}: kept as a login, its NordPass type was {}", row.get("name"), row.get("type")));
            }
            Draft {
                title: row.get("name"),
                urls: vec![row.get("url")],
                username: row.get("username"),
                password: row.get("password"),
                notes: row.get("note"),
                folder: row.get("folder"),
                ..Draft::default()
            }
        }
        Format::AppleCsv => Draft {
            title: row.get("title"),
            urls: vec![row.get("url")],
            username: row.get("username"),
            password: row.get("password"),
            notes: row.get("notes"),
            otp: row.get("otpauth"),
            ..Draft::default()
        },
        Format::RoboFormCsv => Draft {
            title: row.get("name"),
            urls: vec![row.get("url"), row.get("matchurl")],
            username: row.get("login"),
            password: row.get("pwd"),
            notes: row.get("note"),
            folder: row.get("folder").trim_start_matches('/').to_owned(),
            ..Draft::default()
        },
        _ => Draft::default(),
    }
}

fn bitwarden_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let kind = row.get("type");
    if kind == "note" {
        let mut entry = base_entry(&row.get("name"), "", at);
        entry.kind = Kind::Note(Note { body: Lww::new(Secret::from(row.get("notes")), at) });
        entry.common.favourite = Lww::new(row.get("favorite") == "1", at);
        let folder = super::non_empty(row.get("folder"));
        out.entries.push(super::ImportedEntry { entry, folder, files: Vec::new() });
        return;
    }
    let mut fields = Vec::new();
    for line in row.get("fields").lines() {
        if let Some((name, value)) = line.split_once(": ") {
            fields.push((name.to_owned(), value.to_owned(), false));
        }
    }
    let draft = Draft {
        title: row.get("name"),
        username: row.get("login_username"),
        password: row.get("login_password"),
        urls: row.get("login_uri").split(',').map(str::to_owned).collect(),
        notes: row.get("notes"),
        otp: row.get("login_totp"),
        folder: row.get("folder"),
        favourite: row.get("favorite") == "1",
        fields,
        ..Draft::default()
    };
    let entry = draft.into_login(at, &mut out.skipped);
    out.entries.push(entry);
}

fn lastpass_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let url = row.get("url");
    // LastPass marks secure notes with this placeholder address.
    if url == "http://sn" {
        let mut entry = base_entry(&row.get("name"), "", at);
        entry.kind = Kind::Note(Note { body: Lww::new(Secret::from(row.get("extra")), at) });
        let folder = super::non_empty(row.get("grouping"));
        out.entries.push(super::ImportedEntry { entry, folder, files: Vec::new() });
        return;
    }
    let draft = Draft {
        title: row.get("name"),
        urls: vec![url],
        username: row.get("username"),
        password: row.get("password"),
        notes: row.get("extra"),
        otp: row.get("totp"),
        folder: row.get("grouping"),
        favourite: row.get("fav") == "1",
        ..Draft::default()
    };
    let entry = draft.into_login(at, &mut out.skipped);
    out.entries.push(entry);
}

fn wmpass_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let extra: Vec<Field> = serde_json::from_str(&row.get("fields")).unwrap_or_default();
    let kind = row.get("wmpass_kind");
    if kind != "login" {
        let mut entry = base_entry(&row.get("title"), &row.get("notes"), at);
        let mut fields = extra;
        fields.insert(0, Field { name: "Exported kind".into(), value: kind.as_str().into(), ..Field::default() });
        entry.common.custom_fields = Lww::new(fields, at);
        entry.kind = Kind::Note(Note::default());
        out.skipped.push(format!("{}: {kind} entries come back from CSV as notes; a vault file keeps their type", row.get("title")));
        let folder = super::non_empty(row.get("folder"));
        out.entries.push(super::ImportedEntry { entry, folder, files: Vec::new() });
        return;
    }
    let draft = Draft {
        title: row.get("title"),
        username: row.get("username"),
        password: row.get("password"),
        urls: row.get("urls").split(' ').map(str::to_owned).collect(),
        notes: row.get("notes"),
        otp: row.get("otp"),
        folder: row.get("folder"),
        tags: row.get("tags").split(',').filter(|t| !t.is_empty()).map(str::to_owned).collect(),
        ..Draft::default()
    };
    let mut item = draft.into_login(at, &mut out.skipped);
    item.entry.common.custom_fields = Lww::new(extra, at);
    out.entries.push(item);
}

pub(super) fn mapped(text: &str, map: &ColumnMap, has_header: bool, at: Hlc) -> Result<Imported> {
    let mut out = Imported::default();
    for record in reader(text, has_header).records() {
        let record = record.map_err(|_| Error::Import("a row could not be read".into()))?;
        let cell = |column: Option<usize>| column.and_then(|at| record.get(at)).unwrap_or_default().to_owned();
        let draft = Draft {
            title: cell(map.title),
            username: cell(map.username),
            password: cell(map.password),
            urls: vec![cell(map.url)],
            notes: cell(map.notes),
            otp: cell(map.otp),
            folder: cell(map.folder),
            ..Draft::default()
        };
        let entry = draft.into_login(at, &mut out.skipped);
        out.entries.push(entry);
    }
    Ok(out)
}

pub(super) fn preview(text: &str, rows: usize) -> Result<Vec<Vec<String>>> {
    let mut out = Vec::new();
    for record in reader(text, false).records().take(rows) {
        let record = record.map_err(|_| Error::Import("a row could not be read".into()))?;
        out.push(record.iter().map(str::to_owned).collect());
    }
    Ok(out)
}
