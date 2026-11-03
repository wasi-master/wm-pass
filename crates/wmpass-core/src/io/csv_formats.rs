//! CSV exports. Each app has its own columns, read by header name.

use std::collections::HashMap;

use serde_json::Value;

use super::{ColumnMap, Draft, Format, Imported, ImportedEntry, base_entry, non_empty, note_item};
use crate::clock::Hlc;
use crate::model::{Card, Field, Identity, Kind, Lww, Note};
use crate::{Error, Result, Secret};

struct Row<'a> {
    record: &'a csv::StringRecord,
    columns: &'a HashMap<String, usize>,
    /// Header names as written, for formats that keep unknown columns as fields.
    names: &'a [String],
}

impl Row<'_> {
    fn get(&self, name: &str) -> String {
        self.columns
            .get(name)
            .and_then(|at| self.record.get(*at))
            .unwrap_or_default()
            .to_owned()
    }

    /// Every column whose lowercased name is not in `known`, by its written name.
    fn others(&self, known: &[&str]) -> Vec<(String, String, bool)> {
        self.names
            .iter()
            .enumerate()
            .filter(|(_, name)| !known.contains(&name.to_lowercase().as_str()))
            .filter_map(|(at, name)| {
                let value = self.record.get(at)?.to_owned();
                (!value.is_empty()).then(|| (name.clone(), value, false))
            })
            .collect()
    }
}

fn reader(text: &str, has_header: bool) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(has_header)
        .from_reader(text.as_bytes())
}

fn written_header(text: &str) -> Option<Vec<String>> {
    let mut reader = reader(text, true);
    let headers = reader.headers().ok()?;
    Some(headers.iter().map(|h| h.trim().to_owned()).collect())
}

fn header_of(text: &str) -> Option<Vec<String>> {
    Some(
        written_header(text)?
            .into_iter()
            .map(|h| h.to_lowercase())
            .collect(),
    )
}

fn has_all(headers: &[String], names: &[&str]) -> bool {
    names.iter().all(|name| headers.iter().any(|h| h == name))
}

pub(super) fn detect(text: &str) -> Option<Format> {
    let headers = header_of(text)?;
    if looks_like_msecure(text) {
        Some(Format::MSecureCsv)
    } else if has_all(&headers, &["wmpass_kind"]) {
        Some(Format::WmPassCsv)
    } else if has_all(&headers, &["login_uri", "login_password"]) {
        Some(Format::BitwardenCsv)
    } else if has_all(&headers, &["grouping", "extra", "password"]) {
        Some(Format::LastPassCsv)
    } else if has_all(
        &headers,
        &["group", "title", "username", "password", "url", "notes"],
    ) {
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
    } else if has_all(&headers, &["secretdata"])
        && (has_all(&headers, &["password name"]) || has_all(&headers, &["secret name"]))
    {
        Some(Format::ZohoVaultCsv)
    } else if has_all(&headers, &["!group_name", "title", "password"]) {
        Some(Format::ButtercupCsv)
    } else if has_all(&headers, &["website", "secondary_username", "password"]) {
        Some(Format::AviraCsv)
    } else if has_all(&headers, &["name", "web", "login", "password"]) {
        Some(Format::AvastCsv)
    } else if has_all(&headers, &["name", "url", "username", "password"]) {
        Some(Format::ChromeCsv)
    } else if headers == ["url", "username", "password"] {
        // Chrome's columns without the name, which Avast writes as its "Chrome" CSV;
        // each login is named after its site.
        Some(Format::ChromeCsv)
    } else {
        None
    }
}

pub(super) fn import(format: Format, text: &str, at: Hlc) -> Result<Imported> {
    if format == Format::MSecureCsv {
        return msecure(text, at);
    }
    let names = written_header(text).ok_or_else(|| Error::Import("no header row".into()))?;
    let columns: HashMap<String, usize> = names
        .iter()
        .enumerate()
        .map(|(i, h)| (h.to_lowercase(), i))
        .collect();
    let mut out = Imported::default();
    for record in reader(text, true).records() {
        let record = record.map_err(|_| Error::Import("a row could not be read".into()))?;
        let row = Row {
            record: &record,
            columns: &columns,
            names: &names,
        };
        match format {
            Format::BitwardenCsv => bitwarden_row(&row, at, &mut out),
            Format::LastPassCsv => lastpass_row(&row, at, &mut out),
            Format::WmPassCsv => wmpass_row(&row, at, &mut out),
            Format::NordPassCsv => nordpass_row(&row, at, &mut out),
            Format::ZohoVaultCsv => zoho_row(&row, at, &mut out),
            _ => {
                let draft = simple_row(format, &row);
                let entry = draft.into_login(at, &mut out.skipped);
                out.entries.push(entry);
            }
        }
    }
    Ok(out)
}

fn simple_row(format: Format, row: &Row<'_>) -> Draft {
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
            Draft {
                title,
                urls: vec![url],
                username: row.get("username"),
                password: row.get("password"),
                ..Draft::default()
            }
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
                // Newer exports name the column otpUrl and hold a whole link.
                otp: match row.get("otpurl") {
                    url if url.is_empty() => row.get("otpsecret"),
                    url => url,
                },
                folder: row.get("category"),
                fields,
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
        Format::ButtercupCsv => Draft {
            title: row.get("title"),
            username: row.get("username"),
            password: row.get("password"),
            urls: vec![row.get("url")],
            notes: match row.get("note") {
                note if note.is_empty() => row.get("notes"),
                note => note,
            },
            folder: row.get("!group_name"),
            fields: row.others(&[
                "!group_id",
                "!group_name",
                "!type",
                "title",
                "username",
                "password",
                "url",
                "note",
                "notes",
                "id",
            ]),
            ..Draft::default()
        },
        Format::AviraCsv => {
            // The second username fills in when the first is empty, and is kept otherwise.
            let (first, second) = (row.get("username"), row.get("secondary_username"));
            let (username, extra) = if first.is_empty() {
                (second, String::new())
            } else {
                (first, second)
            };
            Draft {
                title: row.get("name"),
                urls: vec![row.get("website")],
                username,
                password: row.get("password"),
                fields: vec![("Secondary username".into(), extra, false)],
                ..Draft::default()
            }
        }
        Format::AvastCsv => Draft {
            title: row.get("name"),
            urls: vec![row.get("web")],
            username: row.get("login"),
            password: row.get("password"),
            ..Draft::default()
        },
        _ => Draft::default(),
    }
}

fn bitwarden_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let kind = row.get("type");
    if kind == "note" {
        let mut entry = base_entry(&row.get("name"), "", at);
        entry.kind = Kind::Note(Note {
            body: Lww::new(Secret::from(row.get("notes")), at),
        });
        entry.common.favourite = Lww::new(row.get("favorite") == "1", at);
        let folder = super::non_empty(row.get("folder"));
        out.entries.push(super::ImportedEntry {
            entry,
            folder,
            files: Vec::new(),
        });
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
        entry.kind = Kind::Note(Note {
            body: Lww::new(Secret::from(row.get("extra")), at),
        });
        let folder = super::non_empty(row.get("grouping"));
        out.entries.push(super::ImportedEntry {
            entry,
            folder,
            files: Vec::new(),
        });
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
        fields.insert(
            0,
            Field {
                name: "Exported kind".into(),
                value: kind.as_str().into(),
                ..Field::default()
            },
        );
        entry.common.custom_fields = Lww::new(fields, at);
        entry.kind = Kind::Note(Note::default());
        out.skipped.push(format!(
            "{}: {kind} entries come back from CSV as notes; a vault file keeps their type",
            row.get("title")
        ));
        let folder = super::non_empty(row.get("folder"));
        out.entries.push(super::ImportedEntry {
            entry,
            folder,
            files: Vec::new(),
        });
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
        tags: row
            .get("tags")
            .split(',')
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect(),
        ..Draft::default()
    };
    let mut item = draft.into_login(at, &mut out.skipped);
    item.entry.common.custom_fields = Lww::new(extra, at);
    out.entries.push(item);
}

/// NordPass writes every kind of item into one CSV, told apart by `type`, with extra
/// addresses and custom fields as JSON in their own columns. A row with nothing but a
/// name and no type is a folder, and makes no entry.
fn nordpass_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let (title, note, folder) = (row.get("name"), row.get("note"), row.get("folder"));
    let custom: Vec<(String, String, bool)> =
        serde_json::from_str::<Value>(&row.get("custom_fields"))
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|f| {
                let text = |key: &str| {
                    f.get(key)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                (text("label"), text("value"), text("type") == "hidden")
            })
            .collect();
    let kind = row.get("type");
    let only_a_name = row.record.iter().filter(|cell| !cell.is_empty()).count() <= 1;
    if kind.is_empty() && only_a_name {
        return;
    }
    let item = match kind.as_str() {
        "note" => note_item(&title, &note, custom, non_empty(folder), at),
        "credit_card" => {
            let mut entry = base_entry(&title, &note, at);
            entry.kind = Kind::Card(Card {
                holder: Lww::new(row.get("cardholdername"), at),
                number: Lww::new(Secret::from(row.get("cardnumber")), at),
                cvv: Lww::new(Secret::from(row.get("cvc")), at),
                pin: Lww::new(Secret::from(row.get("pin")), at),
                expiry: Lww::new(row.get("expirydate").replace(' ', ""), at),
                billing: Lww::new(row.get("zipcode"), at),
                ..Card::default()
            });
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            ImportedEntry {
                entry,
                folder: non_empty(folder),
                files: Vec::new(),
            }
        }
        "identity" => {
            let address = [
                "address1", "address2", "city", "state", "zipcode", "country",
            ]
            .into_iter()
            .map(|key| row.get(key))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
            let mut entry = base_entry(&title, &note, at);
            entry.kind = Kind::Identity(Identity {
                full_name: Lww::new(row.get("full_name"), at),
                address: Lww::new(address, at),
                phone: Lww::new(row.get("phone_number"), at),
                email: Lww::new(row.get("email"), at),
                ..Identity::default()
            });
            entry.common.custom_fields = Lww::new(super::fields(custom), at);
            ImportedEntry {
                entry,
                folder: non_empty(folder),
                files: Vec::new(),
            }
        }
        other => {
            if !other.is_empty() && other != "password" {
                out.skipped.push(format!(
                    "{title}: kept as a login, its NordPass type was {other}"
                ));
            }
            let mut urls = vec![row.get("url")];
            urls.extend(
                serde_json::from_str::<Vec<String>>(&row.get("additional_urls"))
                    .unwrap_or_default(),
            );
            let draft = Draft {
                title,
                urls,
                username: row.get("username"),
                password: row.get("password"),
                notes: note,
                folder,
                fields: custom,
                ..Draft::default()
            };
            draft.into_login(at, &mut out.skipped)
        }
    };
    out.entries.push(item);
}

/// Zoho Vault keeps the login itself in `SecretData` (and extra fields in
/// `CustomData`), one `Name:value` line each.
fn zoho_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let pick = |a: &str, b: &str| match row.get(a) {
        value if value.is_empty() => row.get(b),
        value => value,
    };
    let title = pick("password name", "secret name");
    if title.is_empty() {
        return;
    }
    let mut draft = Draft {
        title,
        urls: vec![pick("password url", "secret url")],
        notes: [row.get("notes"), row.get("description")]
            .into_iter()
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        otp: pick("totp", "login_totp"),
        folder: row.get("folder name"),
        favourite: row.get("favorite") == "1",
        tags: row
            .get("tags")
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect(),
        ..Draft::default()
    };
    for line in row
        .get("secretdata")
        .lines()
        .chain(row.get("customdata").lines())
        .map(str::to_owned)
        .collect::<Vec<_>>()
    {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if value.trim().is_empty() || name == "SecretType" {
            continue;
        }
        let lower = name.trim().to_lowercase();
        if draft.username.is_empty()
            && ["user name", "username", "login", "email"].contains(&lower.as_str())
        {
            draft.username = value.to_owned();
        } else if draft.password.is_empty() && lower == "password" {
            draft.password = value.to_owned();
        } else {
            let hidden = lower.contains("password") || lower.contains("pin");
            draft
                .fields
                .push((name.trim().to_owned(), value.to_owned(), hidden));
        }
    }
    out.entries.push(draft.into_login(at, &mut out.skipped));
}

/// mSecure's field cells: `Label|type number|value`. The value may itself hold `|`.
fn msecure_cell(cell: &str) -> Option<(&str, &str)> {
    let mut parts = cell.splitn(3, '|');
    let label = parts.next()?;
    let number = parts.next()?;
    let value = parts.next()?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some((label, value))
}

/// mSecure has no header row. The first cell is `name|id`, then the type, the folder,
/// the notes, and the fields.
fn looks_like_msecure(text: &str) -> bool {
    let mut rows = reader(text, false).into_records();
    let Some(Ok(first)) = rows.next() else {
        return false;
    };
    let named = first
        .get(0)
        .and_then(|cell| cell.rsplit_once('|'))
        .is_some_and(|(_, id)| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()));
    named
        && first.len() >= 4
        && first
            .iter()
            .skip(4)
            .all(|cell| cell.is_empty() || msecure_cell(cell).is_some())
}

fn msecure(text: &str, at: Hlc) -> Result<Imported> {
    let mut out = Imported::default();
    for record in reader(text, false).records() {
        let record = record.map_err(|_| Error::Import("a row could not be read".into()))?;
        if record.len() < 3 {
            continue;
        }
        let title = record
            .get(0)
            .unwrap_or_default()
            .split('|')
            .next()
            .unwrap_or_default()
            .to_owned();
        let kind = record.get(1).unwrap_or_default().to_owned();
        let folder = record.get(2).unwrap_or_default();
        let folder = if folder == "Unassigned" {
            String::new()
        } else {
            folder.to_owned()
        };
        let notes = record.get(3).unwrap_or_default().replace("\\n", "\n");
        let cells: Vec<(String, String)> = record
            .iter()
            .skip(4)
            .filter_map(msecure_cell)
            .map(|(label, value)| (label.to_owned(), value.to_owned()))
            .collect();
        let value_at = |index: usize| {
            record
                .get(index)
                .and_then(msecure_cell)
                .map(|(_, value)| value.to_owned())
                .unwrap_or_default()
        };
        let secret = |label: &str| {
            let lower = label.to_lowercase();
            ["password", "pin", "security code", "cvv"]
                .iter()
                .any(|word| lower.contains(word))
        };
        match kind.as_str() {
            "Login" | "Web Logins" => {
                // The website, username and password are always the first three fields.
                let rest = cells
                    .iter()
                    .skip(3)
                    .map(|(label, value)| (label.clone(), value.clone(), secret(label)))
                    .collect();
                let draft = Draft {
                    title,
                    urls: vec![value_at(4)],
                    username: value_at(5),
                    password: value_at(6),
                    notes,
                    folder,
                    fields: rest,
                    ..Draft::default()
                };
                out.entries.push(draft.into_login(at, &mut out.skipped));
            }
            "Credit Card" => {
                let mut payment = Card {
                    number: Lww::new(Secret::from(value_at(4)), at),
                    expiry: Lww::new(value_at(5), at),
                    ..Card::default()
                };
                let mut extra = Vec::new();
                for (label, value) in cells.iter().skip(2) {
                    match label.as_str() {
                        "Security Code" => payment.cvv = Lww::new(Secret::from(value.clone()), at),
                        "Name on Card" => payment.holder = Lww::new(value.clone(), at),
                        "PIN" => payment.pin = Lww::new(Secret::from(value.clone()), at),
                        _ => extra.push((label.clone(), value.clone(), secret(label))),
                    }
                }
                let mut entry = base_entry(&title, &notes, at);
                entry.kind = Kind::Card(payment);
                entry.common.custom_fields = Lww::new(super::fields(extra), at);
                out.entries.push(ImportedEntry {
                    entry,
                    folder: non_empty(folder),
                    files: Vec::new(),
                });
            }
            _ => {
                let mut extra: Vec<(String, String, bool)> = cells
                    .iter()
                    .map(|(label, value)| (label.clone(), value.clone(), secret(label)))
                    .collect();
                if !kind.is_empty() {
                    extra.insert(0, ("mSecure type".into(), kind.clone(), false));
                }
                out.entries
                    .push(note_item(&title, &notes, extra, non_empty(folder), at));
            }
        }
    }
    Ok(out)
}

pub(super) fn mapped(text: &str, map: &ColumnMap, has_header: bool, at: Hlc) -> Result<Imported> {
    let mut out = Imported::default();
    for record in reader(text, has_header).records() {
        let record = record.map_err(|_| Error::Import("a row could not be read".into()))?;
        let cell = |column: Option<usize>| {
            column
                .and_then(|at| record.get(at))
                .unwrap_or_default()
                .to_owned()
        };
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
