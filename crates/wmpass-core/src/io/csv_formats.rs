//! CSV exports. Each app has its own columns, read by header name.

use std::collections::HashMap;

use serde_json::Value;

use super::{ColumnMap, Draft, Format, Imported, ImportedEntry, base_entry, non_empty, note_item};
use crate::clock::Hlc;
use crate::model::{Card, Field, FieldKind, Identity, Kind, Lww, Note, SiteMatcher};
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
    reader_with(text, has_header, b',')
}

fn reader_with(text: &str, has_header: bool, delimiter: u8) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(has_header)
        .delimiter(delimiter)
        .from_reader(text.as_bytes())
}

/// The separator of a CSV of unknown origin, by its first line: a spreadsheet in much of Europe
/// saves with semicolons, and some apps write tabs.
fn sniffed_delimiter(text: &str) -> u8 {
    let first = text.lines().next().unwrap_or_default();
    b",;\t"
        .iter()
        .copied()
        .max_by_key(|d| (first.bytes().filter(|b| b == d).count(), *d == b','))
        .unwrap_or(b',')
}

/// A row with nothing in it, as spreadsheets leave at the end of a sheet.
fn blank(record: &csv::StringRecord) -> bool {
    record.iter().all(|cell| cell.trim().is_empty())
}

fn unreadable(error: &csv::Error) -> Error {
    match error.position() {
        Some(at) => Error::Import(format!("line {} of the file could not be read", at.line())),
        None => Error::Import("a row could not be read".into()),
    }
}

/// A reader for an export read by its header. A file opened in a spreadsheet and saved again
/// comes back with semicolons in much of Europe, or tabs as Excel's "Unicode Text", and its
/// columns are still the app's own.
fn headed_reader(text: &str) -> csv::Reader<&[u8]> {
    reader_with(text, true, sniffed_delimiter(text))
}

fn written_header(text: &str) -> Option<Vec<String>> {
    let mut reader = headed_reader(text);
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
    } else if has_all(&headers, &["otpauth", "title", "url", "favorite", "archived"]) {
        // 1Password 8 writes Apple's columns and a few more.
        Some(Format::OnePasswordCsv)
    } else if has_all(&headers, &["otpauth", "title", "url"]) {
        Some(Format::AppleCsv)
    } else if has_all(&headers, &["username2", "password", "title"]) || dashlane_part(&headers).is_some() {
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
    let records: Vec<csv::StringRecord> = headed_reader(text)
        .records()
        .map(|record| record.map_err(|e| unreadable(&e)))
        .collect::<Result<_>>()?;
    let records: Vec<csv::StringRecord> = records.into_iter().filter(|r| !blank(r)).collect();
    // Buttercup writes a row for each group too, naming its parent: the rows of entries name
    // only their own group, so the paths come from these.
    let mut buttercup_groups: HashMap<String, (String, String)> = HashMap::new();
    if format == Format::ButtercupCsv {
        for row in records.iter().map(|record| Row { record, columns: &columns, names: &names }) {
            let id = row.get("!group_id");
            if id.is_empty() {
                continue;
            }
            // A group's own row says where it sits; an entry's row only names it.
            let group = (row.get("!group_name"), row.get("!group_parent"));
            if row.get("!type") == "group" {
                buttercup_groups.insert(id, group);
            } else {
                buttercup_groups.entry(id).or_insert(group);
            }
        }
    }
    let dashlane = if format == Format::DashlaneCsv { dashlane_part(&columns_of(&names)) } else { None };
    let mut out = Imported::default();
    for record in &records {
        let row = Row {
            record,
            columns: &columns,
            names: &names,
        };
        match format {
            Format::BitwardenCsv => bitwarden_row(&row, at, &mut out),
            Format::LastPassCsv => lastpass_row(&row, at, &mut out),
            Format::WmPassCsv => wmpass_row(&row, at, &mut out),
            Format::NordPassCsv => nordpass_row(&row, at, &mut out),
            Format::ZohoVaultCsv => zoho_row(&row, at, &mut out),
            Format::DashlaneCsv if let Some(part) = dashlane => dashlane_row(part, &row, at, &mut out),
            Format::ButtercupCsv => {
                if row.get("!type") == "group" {
                    continue;
                }
                let mut draft = simple_row(format, &row);
                draft.folder = buttercup_path(&buttercup_groups, &row.get("!group_id"));
                out.entries.push(draft.into_login(at, &mut out.skipped));
            }
            // Proton Pass writes its notes into the same CSV, told apart by `type`.
            Format::ChromeCsv if row.get("type") == "note" => {
                out.entries.push(note_item(&row.get("name"), &row.get("note"), Vec::new(), non_empty(row.get("vault")), at));
            }
            // Firefox's own sync account is kept among its logins; it signs in nowhere.
            Format::FirefoxCsv if row.get("url").starts_with("chrome://") => {
                out.skipped.push(format!("{}: Firefox's own account entry is left out", row.get("url")));
            }
            _ => {
                let draft = simple_row(format, &row);
                let mut entry = draft.into_login(at, &mut out.skipped);
                if format == Format::FirefoxCsv {
                    entry.entry.created_ms = row.get("timecreated").trim().parse().unwrap_or(0);
                }
                if format == Format::KeePassXcCsv {
                    entry.entry.created_ms = super::json_formats::bitwarden_time(&row.get("created"));
                }
                out.entries.push(entry);
            }
        }
    }
    Ok(out)
}

fn columns_of(names: &[String]) -> Vec<String> {
    names.iter().map(|n| n.to_lowercase()).collect()
}

/// Dashlane exports a zip of CSVs, one per kind: besides `credentials.csv`, these.
#[derive(Clone, Copy)]
enum DashlanePart {
    /// `securenotes.csv`.
    Notes,
    /// `payments.csv`: cards and bank accounts.
    Payments,
    /// `ids.csv`: passports, licences and other papers.
    Ids,
    /// `personalInfo.csv`: names, addresses, phone numbers.
    Personal,
}

fn dashlane_part(headers: &[String]) -> Option<DashlanePart> {
    if has_all(headers, &["cc_number", "account_holder", "expiration_month"]) {
        Some(DashlanePart::Payments)
    } else if has_all(headers, &["number", "issue_date", "place_of_issue"]) {
        Some(DashlanePart::Ids)
    } else if has_all(headers, &["first_name", "item_name", "address_recipient"]) {
        Some(DashlanePart::Personal)
    } else if has_all(headers, &["title", "note"])
        && headers.iter().all(|h| ["title", "note", "category"].contains(&h.as_str()))
    {
        Some(DashlanePart::Notes)
    } else {
        None
    }
}

/// One row of a Dashlane CSV other than its logins.
fn dashlane_row(part: DashlanePart, row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let folder = non_empty(row.get("category"));
    let item = match part {
        DashlanePart::Notes => note_item(&row.get("title"), &row.get("note"), Vec::new(), folder, at),
        DashlanePart::Payments if row.get("type") == "payment_card" || !row.get("cc_number").is_empty() => {
            let (month, year) = (row.get("expiration_month"), row.get("expiration_year"));
            let mut entry = base_entry(&row.get("account_name"), "", at);
            entry.kind = Kind::Card(Card {
                holder: Lww::new(row.get("account_holder"), at),
                number: Lww::new(Secret::from(row.get("cc_number")), at),
                cvv: Lww::new(Secret::from(row.get("code")), at),
                expiry: Lww::new(
                    if month.is_empty() && year.is_empty() { String::new() } else { format!("{month}/{year}") },
                    at,
                ),
                issuer: Lww::new(row.get("issuing_bank"), at),
                ..Card::default()
            });
            ImportedEntry { entry, folder: None, files: Vec::new() }
        }
        // A bank account, and anything else: every column is kept, as a note's fields.
        DashlanePart::Payments | DashlanePart::Personal => {
            let title = [row.get("account_name"), row.get("item_name"), row.get("title"), row.get("type")]
                .into_iter()
                .find(|t| !t.trim().is_empty())
                .unwrap_or_default();
            let extra = row
                .others(&["account_name", "item_name"])
                .into_iter()
                .map(|(name, value, _)| {
                    let secret = ["account_number", "routing_number", "cc_number", "code"].contains(&name.as_str());
                    (name, value, secret)
                })
                .collect();
            note_item(&title, "", extra, None, at)
        }
        DashlanePart::Ids => {
            let kind = row.get("type");
            let title = match row.get("name") {
                name if name.trim().is_empty() => kind.clone(),
                name => format!("{kind}: {name}"),
            };
            let mut entry = base_entry(&title, "", at);
            entry.kind = Kind::Document(crate::model::Document {
                label: Lww::new(kind, at),
                number: Lww::new(Secret::from(row.get("number")), at),
            });
            let extra = row.others(&["type", "number"]);
            entry.common.custom_fields = Lww::new(super::fields(extra), at);
            ImportedEntry { entry, folder: None, files: Vec::new() }
        }
    };
    out.entries.push(item);
}

/// A Buttercup group's path, from its id up through its parents. A file with no group rows
/// still names each entry's own group.
fn buttercup_path(groups: &HashMap<String, (String, String)>, start: &str) -> String {
    let mut parts = Vec::new();
    let mut id = start;
    while let Some((name, parent)) = groups.get(id) {
        // A loop in the parents would otherwise never end.
        if parts.len() > 64 {
            break;
        }
        parts.push(name.as_str());
        id = parent;
    }
    parts.reverse();
    parts.join("/")
}

fn simple_row(format: Format, row: &Row<'_>) -> Draft {
    match format {
        // Chrome's columns, which other apps write too: Proton Pass adds an email used when
        // the username is empty, a code, and the vault each login is in.
        Format::ChromeCsv => Draft {
            title: row.get("name"),
            urls: vec![row.get("url")],
            username: match row.get("username") {
                name if name.trim().is_empty() => row.get("email"),
                name => name,
            },
            password: row.get("password"),
            notes: row.get("note"),
            otp: row.get("totp"),
            folder: row.get("vault"),
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
            // KeePassXC names the root group too: its own entries say just `Root`.
            folder: match row.get("group") {
                group if group == "Root" => String::new(),
                group => group.strip_prefix("Root/").unwrap_or(&group).to_owned(),
            },
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
        Format::OnePasswordCsv => {
            let yes = |column: &str| matches!(row.get(column).trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes");
            let mut tags: Vec<String> = row
                .get("tags")
                .split([',', ';'])
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_owned)
                .collect();
            // There is no archive here: an archived item comes in tagged so, to be found and
            // cleared out, rather than left behind.
            if yes("archived") && !tags.iter().any(|tag| tag.eq_ignore_ascii_case("archived")) {
                tags.push("Archived".to_owned());
            }
            Draft {
                title: row.get("title"),
                urls: vec![row.get("url")],
                username: row.get("username"),
                password: row.get("password"),
                notes: row.get("notes"),
                otp: row.get("otpauth"),
                favourite: yes("favorite"),
                tags,
                ..Draft::default()
            }
        }
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
                "!group_parent",
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
    let mut fields = Vec::new();
    for line in row.get("fields").lines() {
        if let Some((name, value)) = line.split_once(": ") {
            fields.push((name.to_owned(), value.to_owned(), false));
        }
    }
    // An organisation's export names its collections where a person's names a folder.
    let folder = match row.get("folder") {
        folder if folder.trim().is_empty() => {
            row.get("collections").split(',').map(str::trim).find(|c| !c.is_empty()).unwrap_or_default().to_owned()
        }
        folder => folder,
    };
    if kind == "note" {
        let mut item = note_item(&row.get("name"), &row.get("notes"), fields, non_empty(folder), at);
        item.entry.common.favourite = Lww::new(row.get("favorite") == "1", at);
        out.entries.push(item);
        return;
    }
    let draft = Draft {
        title: row.get("name"),
        username: row.get("login_username"),
        password: row.get("login_password"),
        urls: row.get("login_uri").split(',').map(str::to_owned).collect(),
        notes: row.get("notes"),
        otp: row.get("login_totp"),
        folder,
        favourite: row.get("favorite") == "1",
        fields,
        ..Draft::default()
    };
    let entry = draft.into_login(at, &mut out.skipped);
    out.entries.push(entry);
}

/// LastPass's `grouping`: nested folders joined with backslashes, and `(none)` for none.
fn lastpass_folder(grouping: &str) -> String {
    match grouping.trim() {
        "(none)" => String::new(),
        _ => grouping.replace('\\', "/"),
    }
}

fn lastpass_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let url = row.get("url");
    let folder = lastpass_folder(&row.get("grouping"));
    let favourite = row.get("fav") == "1";
    // LastPass marks secure notes with this placeholder address.
    if url == "http://sn" {
        let mut item = lastpass_note(&row.get("name"), &row.get("extra"), at)
            .unwrap_or_else(|| note_item(&row.get("name"), &row.get("extra"), Vec::new(), None, at));
        item.folder = non_empty(folder);
        item.entry.common.favourite = Lww::new(favourite, at);
        out.entries.push(item);
        return;
    }
    let draft = Draft {
        title: row.get("name"),
        urls: vec![url],
        username: row.get("username"),
        password: row.get("password"),
        notes: row.get("extra"),
        otp: row.get("totp"),
        folder,
        favourite,
        ..Draft::default()
    };
    let entry = draft.into_login(at, &mut out.skipped);
    out.entries.push(entry);
}

/// A LastPass secure note of the credit card type, whose `extra` is `NoteType:Credit Card`
/// then one `Key:value` line each, with `Notes:` running to the end. `None` for any other
/// note, which stays a note as written.
fn lastpass_note(title: &str, extra: &str, at: Hlc) -> Option<ImportedEntry> {
    let mut lines = extra.lines();
    if lines.next()?.trim() != "NoteType:Credit Card" {
        return None;
    }
    let mut card = Card::default();
    let mut notes = String::new();
    let mut rest = Vec::new();
    while let Some(line) = lines.next() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key {
            "Notes" => {
                notes = std::iter::once(value).chain(lines.by_ref()).collect::<Vec<_>>().join("\n");
                break;
            }
            "Name on Card" => card.holder = Lww::new(value.to_owned(), at),
            "Number" => card.number = Lww::new(Secret::from(value.to_owned()), at),
            "Security Code" => card.cvv = Lww::new(Secret::from(value.to_owned()), at),
            "Type" => card.issuer = Lww::new(value.to_owned(), at),
            "Expiration Date" => card.expiry = Lww::new(lastpass_month_year(value), at),
            "Language" => {}
            _ if value.trim().is_empty() || value.trim() == "," => {}
            _ => rest.push((key.to_owned(), value.to_owned(), false)),
        }
    }
    let mut entry = base_entry(title, &notes, at);
    entry.kind = Kind::Card(card);
    entry.common.custom_fields = Lww::new(super::fields(rest), at);
    Some(ImportedEntry { entry, folder: None, files: Vec::new() })
}

/// LastPass writes a card's dates as `June,2027`: as `06/2027` here. Anything else is kept.
fn lastpass_month_year(value: &str) -> String {
    const MONTHS: [&str; 12] = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september", "october",
        "november", "december",
    ];
    let Some((month, year)) = value.split_once(',') else {
        return value.trim().to_owned();
    };
    let (month, year) = (month.trim(), year.trim());
    match MONTHS.iter().position(|m| m.eq_ignore_ascii_case(month)) {
        Some(index) if !year.is_empty() => format!("{:02}/{year}", index + 1),
        _ if month.is_empty() && year.is_empty() => String::new(),
        _ => value.trim().to_owned(),
    }
}

fn wmpass_row(row: &Row<'_>, at: Hlc, out: &mut Imported) {
    let extra: Vec<Field> = serde_json::from_str(&row.get("fields")).unwrap_or_default();
    let kind = row.get("wmpass_kind");
    if kind == "note" {
        // The export writes a note's body last among its fields, as `Body`: it comes back as the
        // note it was, not as a note holding its own body in a field.
        let mut fields = extra;
        let body = match fields.last() {
            Some(last) if last.name == "Body" && last.kind == FieldKind::Hidden => fields.pop().map(|f| f.value),
            _ => None,
        };
        let mut entry = base_entry(&row.get("title"), &row.get("notes"), at);
        entry.kind = Kind::Note(Note { body: Lww::new(body.unwrap_or_default(), at) });
        entry.common.custom_fields = Lww::new(fields, at);
        entry.common.tags = Lww::new(split_tags(&row.get("tags")), at);
        entry.common.favourite = Lww::new(row.get("favourite") == "1", at);
        out.entries.push(super::ImportedEntry { entry, folder: super::non_empty(row.get("folder")), files: Vec::new() });
        return;
    }
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
        entry.common.tags = Lww::new(split_tags(&row.get("tags")), at);
        entry.common.favourite = Lww::new(row.get("favourite") == "1", at);
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
    let mut matchers: Vec<SiteMatcher> = Vec::new();
    let mut plain = Vec::new();
    for token in row.get("urls").split(' ').filter(|t| !t.trim().is_empty()) {
        let matcher = match wmpass_rule(token) {
            Some(matcher) => matcher,
            None => {
                plain.push(token.to_owned());
                crate::matching::matcher_from_address(token.trim())
            }
        };
        if !matchers.contains(&matcher) {
            matchers.push(matcher);
        }
    }
    let draft = Draft {
        title: row.get("title"),
        username: row.get("username"),
        password: row.get("password"),
        urls: plain,
        notes: row.get("notes"),
        otp: row.get("otp"),
        folder: row.get("folder"),
        tags: split_tags(&row.get("tags")),
        favourite: row.get("favourite") == "1",
        ..Draft::default()
    };
    let mut item = draft.into_login(at, &mut out.skipped);
    item.entry.common.custom_fields = Lww::new(extra, at);
    if let Some(login) = item.entry.login_mut() {
        login.site_matchers = Lww::new(matchers, at);
    }
    out.entries.push(item);
}

/// The `tags` column of WM Pass CSV, comma-separated.
fn split_tags(text: &str) -> Vec<String> {
    text.split(',').filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

/// A rule WM Pass CSV writes as `wmpass:<kind>:<escaped value>`; `None` for a plain address.
/// A kind this version does not know stays a plain address, which fills nowhere.
fn wmpass_rule(token: &str) -> Option<SiteMatcher> {
    let rest = token.trim().strip_prefix("wmpass:")?;
    if rest == "never" {
        return Some(SiteMatcher::Never);
    }
    let (kind, payload) = rest.split_once(':')?;
    let payload = payload.replace("%20", " ").replace("%25", "%");
    if payload.is_empty() {
        return None;
    }
    Some(match kind {
        "url" => crate::matching::matcher_from_address(&payload),
        "domain" => SiteMatcher::Domain(payload),
        "host" => SiteMatcher::Host(payload),
        "regex" => SiteMatcher::Regex(payload),
        _ => return None,
    })
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
        let record = record.map_err(|e| unreadable(&e))?;
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
    for record in reader_with(text, has_header, sniffed_delimiter(text)).records() {
        let record = record.map_err(|e| unreadable(&e))?;
        if blank(&record) {
            continue;
        }
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
    for record in reader_with(text, false, sniffed_delimiter(text)).records().take(rows) {
        let record = record.map_err(|e| unreadable(&e))?;
        out.push(record.iter().map(str::to_owned).collect());
    }
    Ok(out)
}
