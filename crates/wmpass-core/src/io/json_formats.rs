//! JSON exports from password managers.

use serde_json::Value;

use super::{Draft, Format, Imported, ImportedEntry, base_entry, fields, non_empty};
use crate::clock::Hlc;
use crate::model::{Card, Identity, Kind, Lww, Note, Passkey, SshKey};
use crate::{Error, Result, Secret};

pub(super) fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub(super) fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
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
    if root.get("db").is_some() && root.get("header").is_some() {
        Some(Format::Aegis)
    } else if root.get("services").is_some() {
        Some(Format::TwoFas)
    } else if root.get("tokenOrder").is_some() || root.get("tokens").is_some() {
        Some(Format::FreeOtpPlus)
    } else if root.get("vaults").is_some() {
        Some(Format::ProtonPass)
    } else if root.get("records").is_some() {
        Some(Format::KeeperJson)
    } else if root.get("encrypted").is_some() || root.get("folders").is_some() && root.get("items").is_some() && list(&root, "items").first().is_some_and(|item| item.get("type").is_some_and(Value::is_number)) {
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
        return Err(Error::Import("this Bitwarden export is encrypted. Export it again as unencrypted JSON".into()));
    }
    let mut out = Imported::default();
    let folder_names: std::collections::HashMap<String, String> =
        list(&root, "folders").iter().map(|f| (text(f, "id"), text(f, "name"))).collect();
    for item in list(&root, "items") {
        let folder = item.get("folderId").and_then(Value::as_str).and_then(|id| folder_names.get(id).cloned());
        let custom: Vec<(String, String, bool)> = list(item, "fields")
            .iter()
            .map(|f| (text(f, "name"), text(f, "value"), f.get("type").and_then(Value::as_i64) == Some(1)))
            .collect();
        let favourite = item.get("favorite").and_then(Value::as_bool).unwrap_or(false);
        let (title, notes) = (text(item, "name"), text(item, "notes"));
        match item.get("type").and_then(Value::as_i64) {
            Some(1) => {
                let login = item.get("login").cloned().unwrap_or(Value::Null);
                let urls = list(&login, "uris").iter().map(|u| text(u, "uri")).collect();
                let history = list(item, "passwordHistory")
                    .iter()
                    .map(|h| (text(h, "password"), 0))
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
                let passkeys: Vec<Passkey> = list(&login, "fido2Credentials")
                    .iter()
                    .map(|p| Passkey {
                        credential_id: text(p, "credentialId"),
                        rp_id: text(p, "rpId"),
                        rp_name: text(p, "rpName"),
                        user_handle: text(p, "userHandle"),
                        user_name: text(p, "userName"),
                        private_key: Secret::from(text(p, "keyValue")),
                        algorithm: -7,
                        counter: text(p, "counter").parse().unwrap_or(0),
                        created_ms: 0,
                    })
                    .collect();
                if let Some(body) = imported.entry.login_mut() {
                    body.passkeys = Lww::new(passkeys, at);
                }
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
                            expiry: Lww::new(format!("{}/{}", text(&c, "expMonth"), text(&c, "expYear")), at),
                            cvv: Lww::new(Secret::from(text(&c, "code")), at),
                            issuer: Lww::new(text(&c, "brand"), at),
                            ..Card::default()
                        })
                    }
                    Some(4) => {
                        let i = item.get("identity").cloned().unwrap_or(Value::Null);
                        let name = [text(&i, "firstName"), text(&i, "middleName"), text(&i, "lastName")]
                            .into_iter()
                            .filter(|p| !p.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let address = [text(&i, "address1"), text(&i, "address2"), text(&i, "city"), text(&i, "state"), text(&i, "postalCode"), text(&i, "country")]
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
                        out.skipped.push(format!("{title}: unknown Bitwarden item type"));
                        continue;
                    }
                };
                let mut entry = if matches!(kind, Kind::Note(_)) {
                    let mut entry = base_entry(&title, "", at);
                    entry.kind = Kind::Note(Note { body: Lww::new(Secret::from(notes), at) });
                    entry
                } else {
                    let mut entry = base_entry(&title, &notes, at);
                    entry.kind = kind;
                    entry
                };
                entry.common.favourite = Lww::new(favourite, at);
                entry.common.custom_fields = Lww::new(fields(custom), at);
                out.entries.push(ImportedEntry { entry, folder, files: Vec::new() });
            }
        }
    }
    Ok(out)
}

pub(super) fn enpass(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    let folder_names: std::collections::HashMap<String, String> =
        list(&root, "folders").iter().map(|f| (text(f, "uuid"), text(f, "title"))).collect();
    for item in list(&root, "items") {
        if item.get("trashed").and_then(Value::as_i64).unwrap_or(0) != 0 {
            continue;
        }
        let mut draft = Draft { title: text(item, "title"), notes: text(item, "note"), ..Draft::default() };
        for field in list(item, "fields") {
            let value = text(field, "value");
            match text(field, "type").as_str() {
                "username" => draft.username = value,
                "email" if draft.username.is_empty() => draft.username = value,
                "password" => draft.password = value,
                "url" => draft.urls.push(value),
                "totp" => draft.otp = value,
                kind => {
                    let hidden = field.get("sensitive").and_then(Value::as_i64).unwrap_or(0) != 0 || kind == "pin";
                    draft.fields.push((text(field, "label"), value, hidden));
                }
            }
        }
        if let Some(id) = list(item, "folders").first().and_then(Value::as_str)
            && let Some(name) = folder_names.get(id)
        {
            draft.folder.clone_from(name);
        }
        let entry = draft.into_login(at, &mut out.skipped);
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
                let value = value.as_str().unwrap_or_default().to_owned();
                if name == "$oneTimeCode" || name == "TFC:Keeper" {
                    draft.otp = value;
                } else {
                    draft.fields.push((name.trim_start_matches('$').to_owned(), value, false));
                }
            }
        }
        draft.folder = list(record, "folders").first().map(|f| text(f, "folder")).unwrap_or_default();
        let entry = draft.into_login(at, &mut out.skipped);
        out.entries.push(entry);
    }
    Ok(out)
}

pub(super) fn proton(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    if root.get("encrypted").and_then(Value::as_bool) == Some(true) {
        return Err(Error::Import("this Proton Pass export is encrypted. Export it again without a passphrase".into()));
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
            let extra: Vec<(String, String, bool)> = list(&data, "extraFields")
                .iter()
                .map(|f| {
                    let value = f.get("data").map_or_else(String::new, |d| text(d, "content"));
                    (text(f, "fieldName"), value, text(f, "type") == "hidden")
                })
                .collect();
            match text(&data, "type").as_str() {
                "login" => {
                    let username = {
                        let name = text(&content, "itemUsername");
                        if name.is_empty() { text(&content, "itemEmail") } else { name }
                    };
                    let draft = Draft {
                        title,
                        username,
                        password: text(&content, "password"),
                        urls: list(&content, "urls").iter().filter_map(|u| u.as_str().map(str::to_owned)).collect(),
                        notes: note,
                        otp: text(&content, "totpUri"),
                        fields: extra,
                        folder: folder.clone().unwrap_or_default(),
                        ..Draft::default()
                    };
                    let entry = draft.into_login(at, &mut out.skipped);
                    out.entries.push(entry);
                }
                "note" => {
                    let mut entry = base_entry(&title, "", at);
                    entry.kind = Kind::Note(Note { body: Lww::new(Secret::from(note), at) });
                    entry.common.custom_fields = Lww::new(fields(extra), at);
                    out.entries.push(ImportedEntry { entry, folder: folder.clone(), files: Vec::new() });
                }
                "creditCard" => {
                    let mut entry = base_entry(&title, &note, at);
                    entry.kind = Kind::Card(Card {
                        holder: Lww::new(text(&content, "cardholderName"), at),
                        number: Lww::new(Secret::from(text(&content, "number")), at),
                        cvv: Lww::new(Secret::from(text(&content, "verificationNumber")), at),
                        expiry: Lww::new(text(&content, "expirationDate"), at),
                        pin: Lww::new(Secret::from(text(&content, "pin")), at),
                        ..Card::default()
                    });
                    out.entries.push(ImportedEntry { entry, folder: folder.clone(), files: Vec::new() });
                }
                other => out.skipped.push(format!("{title}: Proton Pass {other} items are not brought across")),
            }
        }
    }
    Ok(out)
}
