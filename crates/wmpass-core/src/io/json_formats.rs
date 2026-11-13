//! JSON exports from password managers.

use serde_json::Value;

use super::{
    Draft, Format, Imported, ImportedEntry, base_entry, fields, non_empty, note_item, otp_entry,
};
use crate::clock::Hlc;
use crate::model::{Card, Identity, Kind, Lww, Note, Passkey, SshKey};
use crate::otp::{OtpAlgorithm, OtpConfig, OtpKind};
use crate::{Error, Result, Secret};

pub(super) fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub(super) fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

pub(super) fn parse(json: &str) -> Result<Value> {
    serde_json::from_str(json).map_err(|_| Error::Import("the file is not valid JSON".into()))
}

pub(super) fn detect(json: &str) -> Option<Format> {
    let root: Value = serde_json::from_str(json).ok()?;
    if let Some(items) = root.as_array() {
        let first = items.first()?;
        if first.get("kind").is_some() && first.get("timer").is_some() {
            return Some(Format::Raivo);
        }
        return first.get("secret").is_some().then_some(Format::AndOtp);
    }
    let first_of = |key: &str| list(&root, key).first().cloned().unwrap_or(Value::Null);
    if root.get("db").is_some() && root.get("header").is_some() {
        Some(Format::Aegis)
    } else if root.get("Authenticators").is_some() {
        Some(Format::Stratum)
    } else if root.get("version").is_some()
        && (root.get("entries").is_some()
            || root.get("salt").is_some() && root.get("content").is_some())
    {
        Some(Format::ProtonAuthenticator)
    } else if first_of("accounts").get("issuerName").is_some()
        || first_of("accounts").get("secret").is_some()
            && first_of("accounts").get("timeStep").is_some()
    {
        Some(Format::LastPassAuthenticator)
    } else if root.get("logins").is_some()
        && (first_of("logins").get("pwd").is_some()
            || root.get("cards").is_some()
            || root.get("notes").is_some())
    {
        Some(Format::AvastJson)
    } else if is_psono(&root) {
        Some(Format::PsonoJson)
    } else if root.get("services").is_some() {
        Some(Format::TwoFas)
    } else if root.get("tokenOrder").is_some() || root.get("tokens").is_some() {
        Some(Format::FreeOtpPlus)
    } else if root.get("vaults").is_some() {
        Some(Format::ProtonPass)
    } else if root.get("records").is_some() {
        Some(Format::KeeperJson)
    } else if root.get("encrypted").is_some()
        || root.get("folders").is_some()
            && root.get("items").is_some()
            && list(&root, "items")
                .first()
                .is_some_and(|item| item.get("type").is_some_and(Value::is_number))
    {
        Some(Format::BitwardenJson)
    } else if root.get("items").is_some() {
        Some(Format::EnpassJson)
    } else {
        None
    }
}

pub(super) fn bitwarden(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    if root.get("encrypted").and_then(Value::as_bool) == Some(true) {
        return Err(Error::Import(
            "this Bitwarden export is encrypted. Export it again as unencrypted JSON".into(),
        ));
    }
    let mut out = Imported::default();
    let folder_names: std::collections::HashMap<String, String> = list(&root, "folders")
        .iter()
        .map(|f| (text(f, "id"), text(f, "name")))
        .collect();
    // An organisation's export has collections where a person's has folders.
    let collection_names: std::collections::HashMap<String, String> = list(&root, "collections")
        .iter()
        .map(|c| (text(c, "id"), text(c, "name")))
        .collect();
    for item in list(&root, "items") {
        let folder = item
            .get("folderId")
            .and_then(Value::as_str)
            .and_then(|id| folder_names.get(id).cloned())
            .or_else(|| {
                list(item, "collectionIds")
                    .first()
                    .and_then(Value::as_str)
                    .and_then(|id| collection_names.get(id).cloned())
            });
        let custom: Vec<(String, String, bool)> = list(item, "fields")
            .iter()
            .map(|f| {
                (
                    text(f, "name"),
                    text(f, "value"),
                    f.get("type").and_then(Value::as_i64) == Some(1),
                )
            })
            .collect();
        let favourite = item
            .get("favorite")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (title, notes) = (text(item, "name"), text(item, "notes"));
        let created = bitwarden_time(&text(item, "creationDate"));
        let mut custom = custom;
        if item.get("type").and_then(Value::as_i64) == Some(4) {
            // An identity's parts with no place of their own here are kept as fields.
            let i = item.get("identity").cloned().unwrap_or(Value::Null);
            let parts = [("Title", "title"), ("Company", "company"), ("Username", "username")]
                .map(|(name, key)| (name.to_owned(), text(&i, key), false));
            custom.splice(0..0, parts);
        }
        match item.get("type").and_then(Value::as_i64) {
            Some(1) => {
                let login = item.get("login").cloned().unwrap_or(Value::Null);
                // A login with nothing but a code, as Bitwarden Authenticator writes every one
                // and as a one-time code item goes out to Bitwarden, is a one-time code item.
                let only_a_code = ["username", "password"].iter().all(|key| text(&login, key).is_empty())
                    && list(&login, "uris").iter().all(|u| text(u, "uri").trim().is_empty())
                    && list(&login, "fido2Credentials").is_empty()
                    && list(item, "passwordHistory").is_empty();
                if only_a_code
                    && let Some(config) =
                        // An unreadable code is reported once, by the login it then stays.
                        super::parse_otp(&text(&login, "totp"), &title, "", &mut Vec::new())
                {
                    let mut imported = otp_entry(config, at);
                    if !title.trim().is_empty() {
                        imported.entry.common.title = Lww::new(title, at);
                    }
                    imported.entry.common.notes = Lww::new(Secret::from(notes), at);
                    imported.entry.common.favourite = Lww::new(favourite, at);
                    imported.entry.common.custom_fields = Lww::new(fields(custom), at);
                    imported.folder = folder;
                    imported.entry.created_ms = created;
                    out.entries.push(imported);
                    continue;
                }
                let urls = list(&login, "uris")
                    .iter()
                    .map(|u| text(u, "uri"))
                    .collect();
                let history = list(item, "passwordHistory")
                    .iter()
                    .map(|h| (text(h, "password"), bitwarden_time(&text(h, "lastUsedDate"))))
                    .collect();
                let draft = Draft {
                    title,
                    username: text(&login, "username"),
                    password: text(&login, "password"),
                    urls,
                    notes,
                    otp: text(&login, "totp"),
                    favourite,
                    fields: custom,
                    history,
                    ..Draft::default()
                };
                let mut imported = draft.into_login(at, &mut out.skipped);
                imported.folder = folder;
                if let Some(matchers) = bitwarden_matchers(list(&login, "uris"))
                    && let Some(body) = imported.entry.login_mut()
                {
                    body.site_matchers = Lww::new(matchers, at);
                }
                let passkeys: Vec<Passkey> = list(&login, "fido2Credentials")
                    .iter()
                    .map(|p| Passkey {
                        credential_id: bitwarden_credential_id(&text(p, "credentialId")),
                        rp_id: text(p, "rpId"),
                        rp_name: text(p, "rpName"),
                        user_handle: url_safe(&text(p, "userHandle")),
                        user_name: text(p, "userName"),
                        private_key: Secret::from(url_safe(&text(p, "keyValue"))),
                        algorithm: -7,
                        counter: text(p, "counter").parse().unwrap_or(0),
                        created_ms: bitwarden_time(&text(p, "creationDate")),
                    })
                    .collect();
                if let Some(body) = imported.entry.login_mut() {
                    body.passkeys = Lww::new(passkeys, at);
                }
                imported.entry.created_ms = created;
                out.entries.push(imported);
            }
            other => {
                let kind = match other {
                    Some(2) => Kind::Note(Note::default()),
                    Some(3) => {
                        let c = item.get("card").cloned().unwrap_or(Value::Null);
                        Kind::Card(Card {
                            holder: Lww::new(text(&c, "cardholderName"), at),
                            number: Lww::new(Secret::from(text(&c, "number")), at),
                            expiry: Lww::new(
                                match (text(&c, "expMonth"), text(&c, "expYear")) {
                                    (month, year) if month.is_empty() && year.is_empty() => String::new(),
                                    (month, year) => format!("{month}/{year}"),
                                },
                                at,
                            ),
                            cvv: Lww::new(Secret::from(text(&c, "code")), at),
                            issuer: Lww::new(text(&c, "brand"), at),
                            ..Card::default()
                        })
                    }
                    Some(4) => {
                        let i = item.get("identity").cloned().unwrap_or(Value::Null);
                        let name = [
                            text(&i, "firstName"),
                            text(&i, "middleName"),
                            text(&i, "lastName"),
                        ]
                        .into_iter()
                        .filter(|p| !p.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ");
                        let address = [
                            text(&i, "address1"),
                            text(&i, "address2"),
                            text(&i, "city"),
                            text(&i, "state"),
                            text(&i, "postalCode"),
                            text(&i, "country"),
                        ]
                        .into_iter()
                        .filter(|p| !p.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ");
                        let numbers = vec![
                            ("SSN".to_owned(), text(&i, "ssn"), true),
                            ("Passport".to_owned(), text(&i, "passportNumber"), true),
                            ("Licence".to_owned(), text(&i, "licenseNumber"), true),
                        ];
                        Kind::Identity(Identity {
                            full_name: Lww::new(name, at),
                            address: Lww::new(address, at),
                            phone: Lww::new(text(&i, "phone"), at),
                            email: Lww::new(text(&i, "email"), at),
                            document_numbers: Lww::new(fields(numbers), at),
                            ..Identity::default()
                        })
                    }
                    Some(5) => {
                        let k = item.get("sshKey").cloned().unwrap_or(Value::Null);
                        Kind::SshKey(SshKey {
                            private_key: Lww::new(Secret::from(text(&k, "privateKey")), at),
                            public_key: Lww::new(text(&k, "publicKey"), at),
                            fingerprint: Lww::new(text(&k, "keyFingerprint"), at),
                            ..SshKey::default()
                        })
                    }
                    _ => {
                        out.skipped
                            .push(format!("{title}: unknown Bitwarden item type"));
                        continue;
                    }
                };
                let mut entry = if matches!(kind, Kind::Note(_)) {
                    let mut entry = base_entry(&title, "", at);
                    entry.kind = Kind::Note(Note {
                        body: Lww::new(Secret::from(notes), at),
                    });
                    entry
                } else {
                    let mut entry = base_entry(&title, &notes, at);
                    entry.kind = kind;
                    entry
                };
                entry.common.favourite = Lww::new(favourite, at);
                entry.common.custom_fields = Lww::new(fields(custom), at);
                entry.created_ms = created;
                out.entries.push(ImportedEntry {
                    entry,
                    folder,
                    files: Vec::new(),
                });
            }
        }
    }
    Ok(out)
}

/// A time in whole seconds since 1970, as a number or a string of digits, in milliseconds;
/// 0 when there is none.
pub(super) fn seconds(value: Option<&Value>) -> u64 {
    value
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
        .map_or(0, |s: u64| s.saturating_mul(1000))
}

/// Base64 in its URL-safe form without padding, as passkeys are kept here.
fn url_safe(text: &str) -> String {
    text.trim().trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// Bitwarden writes a passkey's credential id as a GUID, `b9d84a0c-…`, standing for its 16
/// bytes, or as `b64.` and the bytes in base64url when they are not a GUID's. Here it is the
/// bytes in base64url, which is what a site asks for.
pub(super) fn bitwarden_credential_id(id: &str) -> String {
    use base64::Engine;
    let id = id.trim();
    if let Some(rest) = id.strip_prefix("b64.") {
        return url_safe(rest);
    }
    let digits: String = id.chars().filter(|c| *c != '-').collect();
    if id.len() == 36 && digits.len() == 32 && digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes: Vec<u8> =
            (0..16).filter_map(|i| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).ok()).collect();
        return base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    }
    url_safe(id)
}

/// A Bitwarden date, `2024-04-08T13:25:14.123Z`, in milliseconds; 0 when missing or unreadable.
pub(super) fn bitwarden_time(text: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .and_then(|d| u64::try_from(d.timestamp_millis()).ok())
        .unwrap_or(0)
}

/// Bitwarden's match setting on each address, when any has one: base domain, host, a regular
/// expression, or never. Starts-with and exact read as a plain address, as does no setting.
/// `None` when no address has one, so plain addresses go the usual way.
fn bitwarden_matchers(uris: &[Value]) -> Option<Vec<crate::model::SiteMatcher>> {
    use crate::model::SiteMatcher;
    if uris.iter().all(|u| u.get("match").and_then(Value::as_i64).is_none()) {
        return None;
    }
    let mut out: Vec<SiteMatcher> = Vec::new();
    for u in uris {
        let uri = text(u, "uri");
        if uri.trim().is_empty() {
            continue;
        }
        let matcher = match u.get("match").and_then(Value::as_i64) {
            Some(0) => SiteMatcher::Domain(uri),
            Some(1) => SiteMatcher::Host(uri),
            Some(4) => SiteMatcher::Regex(uri),
            // Never filled there: here that keeps the whole login from being offered, which
            // offers less, never more.
            Some(5) => {
                if !out.contains(&SiteMatcher::Never) {
                    out.push(SiteMatcher::Never);
                }
                crate::matching::matcher_from_address(&uri)
            }
            _ => crate::matching::matcher_from_address(&uri),
        };
        if !out.contains(&matcher) {
            out.push(matcher);
        }
    }
    Some(out)
}

pub(super) fn enpass(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    // Folders nest by `parent_uuid`.
    let folder_names: std::collections::HashMap<String, (String, String)> = list(&root, "folders")
        .iter()
        .map(|f| (text(f, "uuid"), (text(f, "title"), text(f, "parent_uuid"))))
        .collect();
    let folder_path = |start: &str| {
        let mut parts = Vec::new();
        let mut id = start.to_owned();
        while let Some((name, parent)) = folder_names.get(&id) {
            if parts.len() > 64 {
                break;
            }
            parts.push(name.clone());
            id.clone_from(parent);
        }
        parts.reverse();
        parts.join("/")
    };
    for item in list(&root, "items") {
        if item.get("trashed").and_then(Value::as_i64).unwrap_or(0) != 0 {
            continue;
        }
        let title = text(item, "title");
        let folder = list(item, "folders").first().and_then(Value::as_str).map(folder_path).unwrap_or_default();
        // Attachments come inside the file, as base64.
        let mut files = Vec::new();
        for file in list(item, "attachments") {
            let name = text(file, "name");
            match super::xml_formats::base64_body(&text(file, "data")) {
                Some(bytes) if !bytes.is_empty() => files.push((name, bytes)),
                _ => out.skipped.push(format!("{title}: the attachment {name} was not in the file")),
            }
        }
        let live: Vec<&Value> = list(item, "fields")
            .iter()
            .filter(|f| f.get("deleted").and_then(Value::as_i64).unwrap_or(0) == 0)
            .filter(|f| !text(f, "value").is_empty())
            .collect();
        let hidden = |field: &Value| {
            field.get("sensitive").and_then(Value::as_i64).unwrap_or(0) != 0
                || text(field, "type") == "pin"
        };
        let favourite = item.get("favorite").and_then(Value::as_i64).unwrap_or(0) != 0;
        let created = seconds(item.get("createdAt"));
        match text(item, "category").as_str() {
            "note" => {
                let extra = live
                    .iter()
                    .map(|f| (text(f, "label"), text(f, "value"), hidden(f)))
                    .collect();
                let mut entry =
                    note_item(&title, &text(item, "note"), extra, non_empty(folder), at);
                entry.entry.common.favourite = Lww::new(favourite, at);
                entry.entry.created_ms = created;
                entry.files = files;
                out.entries.push(entry);
                continue;
            }
            "creditcard" => {
                let mut payment = Card::default();
                let mut extra = Vec::new();
                for field in &live {
                    let value = text(field, "value");
                    match text(field, "type").as_str() {
                        "ccName" if payment.holder.v.is_empty() => {
                            payment.holder = Lww::new(value, at)
                        }
                        "ccNumber" if payment.number.v.is_empty() => {
                            payment.number = Lww::new(Secret::from(value), at);
                        }
                        "ccCvc" if payment.cvv.v.is_empty() => {
                            payment.cvv = Lww::new(Secret::from(value), at)
                        }
                        "ccPin" if payment.pin.v.is_empty() => {
                            payment.pin = Lww::new(Secret::from(value), at)
                        }
                        "ccExpiry" if payment.expiry.v.is_empty() => {
                            payment.expiry = Lww::new(value, at)
                        }
                        "ccType" if payment.issuer.v.is_empty() => {
                            payment.issuer = Lww::new(value, at)
                        }
                        _ => extra.push((text(field, "label"), value, hidden(field))),
                    }
                }
                let mut entry = base_entry(&title, &text(item, "note"), at);
                entry.kind = Kind::Card(payment);
                entry.common.favourite = Lww::new(favourite, at);
                entry.common.custom_fields = Lww::new(fields(extra), at);
                entry.created_ms = created;
                out.entries.push(ImportedEntry {
                    entry,
                    folder: non_empty(folder),
                    files,
                });
                continue;
            }
            _ => {}
        }
        let mut draft = Draft {
            title,
            notes: text(item, "note"),
            folder,
            favourite,
            ..Draft::default()
        };
        // The first of each kind fills the login; any more are kept as fields.
        for field in live {
            let value = text(field, "value");
            match text(field, "type").as_str() {
                "username" if draft.username.is_empty() => draft.username = value,
                "email" if draft.username.is_empty() => draft.username = value,
                "password" if draft.password.is_empty() => draft.password = value,
                "url" => draft.urls.push(value),
                ".Android#" => draft.urls.push(if value.contains("://") {
                    value
                } else {
                    format!("androidapp://{value}")
                }),
                "totp" if draft.otp.is_empty() => draft.otp = value,
                _ => draft
                    .fields
                    .push((text(field, "label"), value, hidden(field))),
            }
        }
        let mut entry = draft.into_login(at, &mut out.skipped);
        entry.files = files;
        entry.entry.created_ms = created;
        out.entries.push(entry);
    }
    Ok(out)
}

pub(super) fn keeper(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    for record in list(&root, "records") {
        let mut draft = Draft {
            title: text(record, "title"),
            username: text(record, "login"),
            password: text(record, "password"),
            urls: vec![text(record, "login_url")],
            notes: text(record, "notes"),
            ..Draft::default()
        };
        if let Some(custom) = record.get("custom_fields").and_then(Value::as_object) {
            for (name, value) in custom {
                // Typed fields are named `$type:label`, or `$type` alone; their values may be
                // objects, such as a phone number with its kind or a name in its parts.
                let (kind, label) = match name.strip_prefix('$') {
                    Some(rest) => rest.split_once(':').unwrap_or((rest, rest)),
                    None => ("", name.as_str()),
                };
                let value = keeper_value(value);
                if (kind == "oneTimeCode" || name == "TFC:Keeper") && draft.otp.is_empty() {
                    draft.otp = value;
                } else {
                    let hidden = matches!(kind, "secret" | "pinCode" | "password" | "oneTimeCode");
                    draft.fields.push((label.to_owned(), value, hidden));
                }
            }
        }
        // Keeper joins nested folders with backslashes, and a record in a shared folder names
        // the shared folder and the folder inside it apart.
        draft.folder = list(record, "folders")
            .first()
            .map(|f| {
                [text(f, "shared_folder"), text(f, "folder")]
                    .into_iter()
                    .filter(|part| !part.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("/")
                    .replace('\\', "/")
            })
            .unwrap_or_default();
        let entry = draft.into_login(at, &mut out.skipped);
        out.entries.push(entry);
    }
    Ok(out)
}

/// A Keeper field's value as text: objects and lists have their parts joined.
fn keeper_value(value: &Value) -> String {
    let parts = |values: Vec<&Value>| {
        values.into_iter().map(keeper_value).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(", ")
    };
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(list) => parts(list.iter().collect()),
        Value::Object(map) => parts(map.values().collect()),
        Value::Null => String::new(),
    }
}

pub(super) fn proton(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    if root.get("encrypted").and_then(Value::as_bool) == Some(true) {
        return Err(Error::Import(
            "this Proton Pass export is encrypted. Export it again without a passphrase".into(),
        ));
    }
    let mut out = Imported::default();
    let Some(vaults) = root.get("vaults").and_then(Value::as_object) else {
        return Err(Error::Import("no vaults in the file".into()));
    };
    for vault in vaults.values() {
        let folder = non_empty(text(vault, "name"));
        for item in list(vault, "items") {
            if item.get("state").and_then(Value::as_i64) == Some(2) {
                continue;
            }
            let data = item.get("data").cloned().unwrap_or(Value::Null);
            let meta = data.get("metadata").cloned().unwrap_or(Value::Null);
            let content = data.get("content").cloned().unwrap_or(Value::Null);
            let (title, note) = (text(&meta, "name"), text(&meta, "note"));
            let mut extra: Vec<(String, String, bool)> =
                list(&data, "extraFields").iter().map(proton_field).collect();
            // Newer kinds keep their own fields in named sections.
            for section in list(&content, "sections").iter().chain(list(&content, "extraSections")) {
                extra.extend(list(section, "sectionFields").iter().map(proton_field));
            }
            let pinned = item.get("pinned").and_then(Value::as_bool).unwrap_or(false);
            let created = seconds(item.get("createTime"));
            let mut entry = match text(&data, "type").as_str() {
                "login" => {
                    let (name, email) = (text(&content, "itemUsername"), text(&content, "itemEmail"));
                    // A login may have both: the username signs in, and the email is kept.
                    let username = if name.is_empty() {
                        email
                    } else {
                        if !email.is_empty() {
                            extra.insert(0, ("Email".to_owned(), email, false));
                        }
                        name
                    };
                    if !list(&content, "passkeys").is_empty() {
                        out.skipped.push(format!(
                            "{title}: its passkeys are kept in a form only Proton Pass reads, so they stay there"
                        ));
                    }
                    let draft = Draft {
                        title,
                        username,
                        password: text(&content, "password"),
                        urls: list(&content, "urls")
                            .iter()
                            .filter_map(|u| u.as_str().map(str::to_owned))
                            .collect(),
                        notes: note,
                        otp: text(&content, "totpUri"),
                        fields: extra,
                        folder: folder.clone().unwrap_or_default(),
                        favourite: pinned,
                        ..Draft::default()
                    };
                    let mut entry = draft.into_login(at, &mut out.skipped);
                    entry.entry.created_ms = created;
                    out.entries.push(entry);
                    continue;
                }
                "note" | "custom" => note_item(&title, &note, extra, folder.clone(), at),
                "creditCard" => {
                    let mut entry = base_entry(&title, &note, at);
                    // Proton writes the expiry as `2027-12`.
                    let expiry = text(&content, "expirationDate");
                    let expiry = match expiry.split_once('-') {
                        Some((year, month)) if year.len() == 4 => format!("{month}/{year}"),
                        _ => expiry,
                    };
                    entry.kind = Kind::Card(Card {
                        holder: Lww::new(text(&content, "cardholderName"), at),
                        number: Lww::new(Secret::from(text(&content, "number")), at),
                        cvv: Lww::new(Secret::from(text(&content, "verificationNumber")), at),
                        expiry: Lww::new(expiry, at),
                        pin: Lww::new(Secret::from(text(&content, "pin")), at),
                        ..Card::default()
                    });
                    entry.common.custom_fields = Lww::new(fields(extra), at);
                    ImportedEntry { entry, folder: folder.clone(), files: Vec::new() }
                }
                "identity" => proton_identity(&title, &note, &content, extra, folder.clone(), at),
                "sshKey" => {
                    let mut entry = base_entry(&title, &note, at);
                    entry.kind = Kind::SshKey(SshKey {
                        private_key: Lww::new(Secret::from(text(&content, "privateKey")), at),
                        public_key: Lww::new(text(&content, "publicKey"), at),
                        ..SshKey::default()
                    });
                    entry.common.custom_fields = Lww::new(fields(extra), at);
                    ImportedEntry { entry, folder: folder.clone(), files: Vec::new() }
                }
                "wifi" => {
                    use crate::model::WifiSecurity;
                    // Only a security written by name is read; a number, or a name not known
                    // here, is kept as written. Zero is protobuf's "unspecified": nothing.
                    let raw = text(&content, "security");
                    let security = match raw.trim().to_ascii_uppercase().as_str() {
                        "WPA" | "WPA2" => Some(WifiSecurity::Wpa),
                        "WPA3" => Some(WifiSecurity::Sae),
                        "WEP" => Some(WifiSecurity::Wep),
                        "NONE" | "OPEN" => Some(WifiSecurity::Open),
                        _ => None,
                    };
                    if security.is_none() && !raw.trim().is_empty() && raw.trim() != "0" {
                        extra.insert(0, ("Security".to_owned(), raw, false));
                    }
                    let mut entry = base_entry(&title, &note, at);
                    entry.kind = Kind::Wifi(crate::model::Wifi {
                        ssid: Lww::new(text(&content, "ssid"), at),
                        password: Lww::new(Secret::from(text(&content, "password")), at),
                        security: Lww::new(security.unwrap_or_default(), at),
                        ..crate::model::Wifi::default()
                    });
                    entry.common.custom_fields = Lww::new(fields(extra), at);
                    ImportedEntry { entry, folder: folder.clone(), files: Vec::new() }
                }
                other => {
                    out.skipped.push(format!("{title}: Proton Pass {other} items are not brought across"));
                    continue;
                }
            };
            entry.entry.common.favourite = Lww::new(pinned, at);
            entry.entry.created_ms = created;
            out.entries.push(entry);
        }
    }
    Ok(out)
}

/// One of Proton Pass's own fields: its name, its value, and whether it is hidden.
fn proton_field(field: &Value) -> (String, String, bool) {
    let value = field.get("data").map_or_else(String::new, |d| text(d, "content"));
    let kind = text(field, "type");
    (text(field, "fieldName"), value, kind == "hidden" || kind == "totp")
}

/// A Proton Pass identity. The parts with a place of their own fill it; every other part,
/// work and contact details among them, is kept as a field under its name.
fn proton_identity(
    title: &str,
    note: &str,
    content: &Value,
    mut extra: Vec<(String, String, bool)>,
    folder: Option<String>,
    at: Hlc,
) -> ImportedEntry {
    let joined = |keys: &[&str], with: &str| {
        keys.iter().map(|key| text(content, key)).filter(|p| !p.trim().is_empty()).collect::<Vec<_>>().join(with)
    };
    let name = match text(content, "fullName") {
        name if name.trim().is_empty() => joined(&["firstName", "middleName", "lastName"], " "),
        name => name,
    };
    let address = joined(
        &["streetAddress", "floor", "city", "county", "stateOrProvince", "zipOrPostalCode", "countryOrRegion"],
        ", ",
    );
    let numbers = vec![
        ("SSN".to_owned(), text(content, "socialSecurityNumber"), true),
        ("Passport".to_owned(), text(content, "passportNumber"), true),
        ("Licence".to_owned(), text(content, "licenseNumber"), true),
    ];
    const MAPPED: [&str; 17] = [
        "fullName", "firstName", "middleName", "lastName", "streetAddress", "floor", "city", "county",
        "stateOrProvince", "zipOrPostalCode", "countryOrRegion", "phoneNumber", "email", "birthdate",
        "socialSecurityNumber", "passportNumber", "licenseNumber",
    ];
    let mut rest = Vec::new();
    for (key, value) in content.as_object().into_iter().flatten() {
        if MAPPED.contains(&key.as_str()) {
            continue;
        }
        match value {
            Value::String(s) if !s.trim().is_empty() => rest.push((words(key), s.clone(), false)),
            // `extraPersonalDetails` and the like hold fields of the person's own.
            Value::Array(list) if key.starts_with("extra") && key != "extraSections" => {
                rest.extend(list.iter().map(proton_field));
            }
            _ => {}
        }
    }
    extra.splice(0..0, rest);
    let mut entry = base_entry(title, note, at);
    entry.kind = Kind::Identity(Identity {
        full_name: Lww::new(name, at),
        address: Lww::new(address, at),
        phone: Lww::new(text(content, "phoneNumber"), at),
        email: Lww::new(text(content, "email"), at),
        birth_date: Lww::new(Secret::from(text(content, "birthdate")), at),
        document_numbers: Lww::new(fields(numbers), at),
    });
    entry.common.custom_fields = Lww::new(fields(extra), at);
    ImportedEntry { entry, folder, files: Vec::new() }
}

/// `jobTitle` as `Job title`.
fn words(key: &str) -> String {
    let mut out = String::new();
    for (i, ch) in key.chars().enumerate() {
        if i == 0 {
            out.extend(ch.to_uppercase());
        } else if ch.is_uppercase() {
            out.push(' ');
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Avast Passwords' JSON: `logins`, `notes` and `cards`.
pub(super) fn avast(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    for login in list(&root, "logins") {
        let draft = Draft {
            title: text(login, "custName"),
            urls: vec![text(login, "url")],
            username: text(login, "loginName"),
            password: text(login, "pwd"),
            notes: text(login, "note"),
            ..Draft::default()
        };
        out.entries.push(draft.into_login(at, &mut out.skipped));
    }
    for note in list(&root, "notes") {
        out.entries.push(note_item(
            &text(note, "label"),
            &text(note, "text"),
            Vec::new(),
            None,
            at,
        ));
    }
    for card in list(&root, "cards") {
        let expiry = card.get("expirationDate").cloned().unwrap_or(Value::Null);
        let expiry = match (text(&expiry, "month"), text(&expiry, "year")) {
            (month, year) if month.is_empty() && year.is_empty() => String::new(),
            (month, year) => format!("{month}/{year}"),
        };
        let mut entry = base_entry(&text(card, "custName"), &text(card, "note"), at);
        entry.kind = Kind::Card(Card {
            holder: Lww::new(text(card, "holderName"), at),
            number: Lww::new(Secret::from(text(card, "cardNumber")), at),
            cvv: Lww::new(Secret::from(text(card, "cvv")), at),
            expiry: Lww::new(expiry, at),
            ..Card::default()
        });
        out.entries.push(ImportedEntry {
            entry,
            folder: None,
            files: Vec::new(),
        });
    }
    Ok(out)
}

const PSONO_TYPES: [&str; 9] = [
    "website_password",
    "application_password",
    "bookmark",
    "note",
    "totp",
    "environment_variables",
    "mail_gpg_own_key",
    "credit_card",
    "ssh_own_key",
];

/// Psono's items name every field after the item's type: `website_password_title`.
fn is_psono(root: &Value) -> bool {
    let mut stack = vec![root];
    let mut seen = 0;
    while let Some(node) = stack.pop() {
        seen += 1;
        if seen > 10_000 {
            return false;
        }
        for item in list(node, "items") {
            let kind = text(item, "type");
            if PSONO_TYPES.contains(&kind.as_str()) && item.get(format!("{kind}_title")).is_some() {
                return true;
            }
        }
        stack.extend(list(node, "folders"));
    }
    false
}

/// Psono's JSON: items at the top and in nested folders, one shape per type.
pub(super) fn psono(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    let mut stack: Vec<(&Value, String, usize)> = vec![(&root, String::new(), 0)];
    while let Some((node, path, depth)) = stack.pop() {
        if depth > 64 {
            return Err(Error::Import("the folders are nested too deeply".into()));
        }
        for item in list(node, "items") {
            psono_item(item, &path, at, &mut out);
        }
        for folder in list(node, "folders") {
            let name = text(folder, "name");
            let child = if path.is_empty() {
                name
            } else {
                format!("{path}/{name}")
            };
            stack.push((folder, child, depth + 1));
        }
    }
    Ok(out)
}

fn psono_item(item: &Value, folder: &str, at: Hlc, out: &mut Imported) {
    let kind = text(item, "type");
    let get = |field: &str| text(item, &format!("{kind}_{field}"));
    let title = match get("title") {
        title if title.is_empty() => text(item, "name"),
        title => title,
    };
    let folder = non_empty(folder.to_owned());
    match kind.as_str() {
        "website_password" | "application_password" | "bookmark" => {
            let draft = Draft {
                title,
                urls: vec![get("url")],
                username: get("username"),
                password: get("password"),
                notes: get("notes"),
                otp: get("totp_code"),
                folder: folder.unwrap_or_default(),
                ..Draft::default()
            };
            out.entries.push(draft.into_login(at, &mut out.skipped));
        }
        "totp" => {
            let algorithm = match get("algorithm").to_ascii_uppercase().as_str() {
                "SHA256" => OtpAlgorithm::Sha256,
                "SHA512" => OtpAlgorithm::Sha512,
                _ => OtpAlgorithm::Sha1,
            };
            let number = |field: &str, fallback: u32| get(field).parse().unwrap_or(fallback);
            let config = OtpConfig {
                secret: Secret::from(get("code").replace(' ', "").to_uppercase()),
                issuer: title,
                account: String::new(),
                algorithm,
                digits: number("digits", 6),
                kind: OtpKind::Totp {
                    period: number("period", 30),
                },
            };
            let Some(mut entry) = super::checked_code(config, at, &mut out.skipped) else {
                return;
            };
            entry.entry.common.notes = Lww::new(Secret::from(get("notes")), at);
            entry.folder = folder;
            out.entries.push(entry);
        }
        "note" => out
            .entries
            .push(note_item(&title, &get("notes"), Vec::new(), folder, at)),
        "environment_variables" => {
            let extra = list(item, "environment_variables_variables")
                .iter()
                .map(|pair| (text(pair, "key"), text(pair, "value"), true))
                .collect();
            out.entries
                .push(note_item(&title, &get("notes"), extra, folder, at));
        }
        "mail_gpg_own_key" => {
            let extra = vec![
                ("Email".to_owned(), get("email"), false),
                ("Name".to_owned(), get("name"), false),
                ("Public key".to_owned(), get("public"), false),
                ("Private key".to_owned(), get("private"), true),
            ];
            out.entries.push(note_item(&title, "", extra, folder, at));
        }
        "credit_card" => {
            let mut entry = base_entry(&title, &get("notes"), at);
            entry.kind = Kind::Card(Card {
                holder: Lww::new(get("name"), at),
                number: Lww::new(Secret::from(get("number")), at),
                expiry: Lww::new(get("valid_through"), at),
                cvv: Lww::new(Secret::from(get("cvc")), at),
                pin: Lww::new(Secret::from(get("pin")), at),
                ..Card::default()
            });
            out.entries.push(ImportedEntry { entry, folder, files: Vec::new() });
        }
        "ssh_own_key" => {
            let mut entry = base_entry(&title, &get("notes"), at);
            entry.kind = Kind::SshKey(SshKey {
                private_key: Lww::new(Secret::from(get("private")), at),
                public_key: Lww::new(get("public"), at),
                ..SshKey::default()
            });
            out.entries.push(ImportedEntry { entry, folder, files: Vec::new() });
        }
        other => out.skipped.push(format!(
            "{title}: Psono {other} items are not brought across"
        )),
    }
}
