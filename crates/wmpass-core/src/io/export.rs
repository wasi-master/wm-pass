//! Writing vault contents out. A plain export holds every secret in the clear.

use serde_json::{Value, json};

use crate::model::{Entry, Kind, SiteMatcher};
use crate::vault::Vault;
use crate::{Error, Result, Secret};

/// Every type-specific field of a non-login kind: name, value, whether it is hidden.
pub fn kind_fields(kind: &Kind) -> Vec<(&'static str, Secret, bool)> {
    let shown = |name, value: &str| (name, Secret::new(value), false);
    let hidden = |name, value: &Secret| (name, value.clone(), true);
    let list: Vec<(&'static str, Secret, bool)> = match kind {
        Kind::Login(_) => Vec::new(),
        Kind::Card(c) => vec![
            shown("Cardholder", &c.holder.v),
            hidden("Number", &c.number.v),
            shown("Expiry", &c.expiry.v),
            hidden("CVV", &c.cvv.v),
            hidden("PIN", &c.pin.v),
            shown("Issuer", &c.issuer.v),
            shown("Billing address", &c.billing.v),
        ],
        Kind::Identity(i) => vec![
            shown("Full name", &i.full_name.v),
            shown("Address", &i.address.v),
            shown("Phone", &i.phone.v),
            shown("Email", &i.email.v),
            hidden("Birth date", &i.birth_date.v),
        ],
        Kind::Note(n) => vec![hidden("Body", &n.body.v)],
        Kind::Document(d) => vec![shown("Label", &d.label.v), hidden("Number", &d.number.v)],
        Kind::SshKey(s) => vec![
            hidden("Private key", &s.private_key.v),
            shown("Public key", &s.public_key.v),
            shown("Fingerprint", &s.fingerprint.v),
            hidden("Passphrase", &s.passphrase.v),
        ],
        Kind::ApiKey(a) => vec![
            shown("Provider", &a.provider.v),
            hidden("Key", &a.key.v),
            hidden("Secret", &a.secret.v),
            shown("Scopes", &a.scopes.v),
        ],
        Kind::Database(d) => vec![
            shown("Engine", &d.engine.v),
            shown("Host", &d.host.v),
            shown(
                "Port",
                &d.port.v.map_or_else(String::new, |p| p.to_string()),
            ),
            shown("Database", &d.name.v),
            shown("User", &d.user.v),
            hidden("Password", &d.password.v),
            hidden("URL", &d.url.v),
        ],
        Kind::Licence(l) => vec![
            shown("Product", &l.product.v),
            hidden("Key", &l.key.v),
            shown("Email", &l.email.v),
            shown("Order", &l.order_id.v),
        ],
        Kind::Recovery(r) => vec![hidden("Phrase", &r.phrase.v)],
        Kind::Wifi(w) => vec![shown("SSID", &w.ssid.v), hidden("Password", &w.password.v)],
        Kind::Otp(o) => o
            .config
            .v
            .iter()
            .map(|c| hidden("otpauth", &c.to_uri()))
            .collect(),
    };
    list.into_iter()
        .filter(|(_, value, _)| !value.is_empty())
        .collect()
}

fn urls(entry: &Entry) -> Vec<String> {
    entry.login().map_or_else(Vec::new, |login| {
        login
            .site_matchers
            .v
            .iter()
            .filter_map(|m| match m {
                SiteMatcher::Url(u) | SiteMatcher::Domain(u) | SiteMatcher::Host(u) => {
                    Some(u.clone())
                }
                SiteMatcher::AndroidApp { package, .. } => Some(format!("androidapp://{package}")),
                _ => None,
            })
            .collect()
    })
}

/// A login's matching rules for WM Pass CSV, space-separated. A plain address is a plain
/// address, as every version reads it. A rule an address alone cannot say is written
/// `wmpass:host:…`, `wmpass:domain:…`, `wmpass:regex:…` or `wmpass:never`, with `%` and
/// spaces escaped; an older version reads those as an address with no host, which fills
/// nowhere, so it never offers a login more widely than meant. On a never-fill login every
/// rule takes that form, so an older version leaves it unfilled too.
fn csv_addresses(entry: &Entry) -> String {
    let Some(login) = entry.login() else {
        return String::new();
    };
    let matchers = &login.site_matchers.v;
    let never = matchers.iter().any(|m| matches!(m, SiteMatcher::Never));
    let tagged = |kind: &str, payload: &str| format!("wmpass:{kind}:{}", payload.replace('%', "%25").replace(' ', "%20"));
    let mut out: Vec<String> = matchers
        .iter()
        .filter_map(|m| {
            let plain = match m {
                SiteMatcher::Url(u) => u.clone(),
                SiteMatcher::AndroidApp { package, cert_sha256 } => match cert_sha256.as_deref().and_then(crate::vault::unhex) {
                    Some(cert) => {
                        use base64::Engine;
                        format!("android://{}@{package}", base64::engine::general_purpose::STANDARD.encode(cert))
                    }
                    None => format!("androidapp://{package}"),
                },
                // A domain reads back as a plain address, which matches the same registrable
                // domain, and other apps can still use it: only the rules a plain address would
                // widen are tagged.
                SiteMatcher::Domain(u) if never => return Some(tagged("domain", u)),
                SiteMatcher::Domain(u) => u.clone(),
                SiteMatcher::Host(u) => return Some(tagged("host", u)),
                SiteMatcher::Regex(pattern) => return Some(tagged("regex", pattern)),
                SiteMatcher::Never => return None,
            };
            Some(if never { tagged("url", &plain) } else { plain })
        })
        .collect();
    if never {
        out.push("wmpass:never".to_owned());
    }
    out.join(" ")
}

/// An entry's folder with its parents, as `Parent/Child`. Empty when it has none.
pub fn folder_path(vault: &Vault, entry: &Entry) -> String {
    path_of_folder(vault, entry.common.folder.v.as_deref())
}

/// A folder with its parents, as `Parent/Child`: how Bitwarden names a folder inside another,
/// and how every importer here reads one back.
fn path_of_folder(vault: &Vault, folder: Option<&str>) -> String {
    let mut parts = Vec::new();
    let mut next = folder.map(str::to_owned);
    while let Some(id) = next {
        let Some(folder) = vault.doc.folders.get(&id) else {
            break;
        };
        // A loop in the parents would otherwise never end.
        if parts.len() > 64 {
            break;
        }
        parts.push(folder.name.v.clone());
        next = folder.parent.v.clone();
    }
    parts.reverse();
    parts.join("/")
}

fn exportable(vault: &Vault) -> impl Iterator<Item = &Entry> {
    vault.doc.entries.values().filter(|e| !e.is_trashed())
}

/// WM Pass CSV. Other kinds keep their fields in the `fields` column.
pub fn to_csv(vault: &Vault) -> Result<Secret> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    let header = [
        "wmpass_kind",
        "folder",
        "title",
        "username",
        "password",
        "urls",
        "otp",
        "notes",
        "tags",
        "fields",
        "favourite",
    ];
    writer.write_record(header).map_err(|_| Error::Encoding)?;
    for entry in exportable(vault) {
        let (username, password, otp) = entry.login().map_or_else(
            || (String::new(), Secret::default(), Secret::default()),
            |login| {
                let otp = login
                    .otp
                    .v
                    .as_ref()
                    .map_or_else(Secret::default, crate::otp::OtpConfig::to_uri);
                (login.username.v.clone(), login.password.v.clone(), otp)
            },
        );
        let mut custom: Vec<Value> = entry
            .common
            .custom_fields
            .v
            .iter()
            .map(|f| serde_json::to_value(f).unwrap_or(Value::Null))
            .collect();
        for (name, value, hidden) in kind_fields(&entry.kind) {
            custom.push(json!({"name": name, "value": value.expose(), "kind": if hidden { "Hidden" } else { "Text" }}));
        }
        writer
            .write_record([
                entry.kind.name(),
                folder_path(vault, entry).as_str(),
                entry.common.title.v.as_str(),
                username.as_str(),
                password.expose(),
                csv_addresses(entry).as_str(),
                otp.expose(),
                entry.common.notes.v.expose(),
                entry.common.tags.v.join(",").as_str(),
                Value::Array(custom).to_string().as_str(),
                if entry.common.favourite.v { "1" } else { "" },
            ])
            .map_err(|_| Error::Encoding)?;
    }
    let bytes = writer.into_inner().map_err(|_| Error::Encoding)?;
    String::from_utf8(bytes)
        .map(Secret::from)
        .map_err(|_| Error::Encoding)
}

/// Bitwarden's unencrypted JSON, which most other managers can also read. Kinds it
/// lacks become secure notes with their values as custom fields.
pub fn to_bitwarden_json(vault: &Vault) -> Result<Secret> {
    let mut items = Vec::new();
    for entry in exportable(vault) {
        let mut fields: Vec<Value> = entry
            .common
            .custom_fields
            .v
            .iter()
            .map(|f| {
                let kind = u8::from(matches!(f.kind, crate::model::FieldKind::Hidden));
                json!({"name": f.name, "value": f.value.expose(), "type": kind})
            })
            .collect();
        let mut item = json!({
            "id": entry.id,
            "name": entry.common.title.v,
            "notes": entry.common.notes.v.expose(),
            "favorite": entry.common.favourite.v,
            // A folder another device removed is no folder: the id would name nothing in `folders`.
            "folderId": entry.common.folder.v.as_ref().filter(|id| vault.doc.folders.contains_key(*id)),
        });
        let hidden_field = |name: &str, value: &str| json!({"name": name, "value": value, "type": 1});
        let shown_field = |name: &str, value: &str| json!({"name": name, "value": value, "type": 0});
        match &entry.kind {
            Kind::Login(login) => {
                let uris = bitwarden_uris(&login.site_matchers.v);
                // Security questions have no place in Bitwarden: each is kept as a hidden field.
                for question in &login.questions.v {
                    fields.push(hidden_field(&question.name, question.value.expose()));
                }
                // Bitwarden keeps only ES256 passkeys, and reads a credential id that is not a
                // GUID after `b64.`.
                let passkeys: Vec<Value> = login
                    .passkeys
                    .v
                    .iter()
                    .filter(|p| p.algorithm == -7)
                    .map(|p| {
                        json!({
                            "credentialId": format!("b64.{}", p.credential_id),
                            "keyType": "public-key",
                            "keyAlgorithm": "ECDSA",
                            "keyCurve": "P-256",
                            "keyValue": p.private_key.expose(),
                            "rpId": p.rp_id,
                            "rpName": p.rp_name,
                            "userHandle": p.user_handle,
                            "userName": p.user_name,
                            "userDisplayName": p.user_name,
                            "counter": p.counter.to_string(),
                            "discoverable": "true",
                            "creationDate": bitwarden_time(p.created_ms),
                        })
                    })
                    .collect();
                let history: Vec<Value> = entry
                    .history_of("password")
                    .map(|r| json!({"password": r.value.expose(), "lastUsedDate": bitwarden_time(r.at.ms)}))
                    .collect();
                item["type"] = json!(1);
                item["login"] = json!({
                    "username": login.username.v,
                    "password": login.password.v.expose(),
                    // Bitwarden reads a Steam code only in its own `steam://` form.
                    "totp": login.otp.v.as_ref().map(bitwarden_totp),
                    "uris": uris,
                    "fido2Credentials": passkeys,
                });
                item["passwordHistory"] = json!(history);
            }
            Kind::Note(note) => {
                item["type"] = json!(2);
                item["secureNote"] = json!({"type": 0});
                // A note's body is Bitwarden's notes; notes of its own come after it rather
                // than being written over.
                let body = note.body.v.expose();
                let notes = entry.common.notes.v.expose();
                item["notes"] = json!(match (body.is_empty(), notes.is_empty()) {
                    (_, true) => body.to_owned(),
                    (true, false) => notes.to_owned(),
                    (false, false) => format!("{body}\n\n{notes}"),
                });
            }
            // A one-time code is a login with only a code in Bitwarden, the one place it makes codes.
            Kind::Otp(otp) => {
                item["type"] = json!(1);
                item["login"] = json!({
                    "username": null,
                    "password": null,
                    "totp": otp.config.v.as_ref().map(bitwarden_totp),
                    "uris": [],
                });
            }
            Kind::Identity(identity) => {
                // Bitwarden splits a name into parts: the last word is the last name, which
                // joins back to the same full name.
                let full = identity.full_name.v.trim();
                let (first, last) = full.rsplit_once(' ').unwrap_or((full, ""));
                let number = |label: &str| {
                    identity
                        .document_numbers
                        .v
                        .iter()
                        .find(|f| f.name.eq_ignore_ascii_case(label))
                        .map(|f| f.value.expose().to_owned())
                };
                let (ssn, passport, licence) = (number("SSN"), number("Passport"), number("Licence").or_else(|| number("License")));
                for field in &identity.document_numbers.v {
                    if !["ssn", "passport", "licence", "license"].contains(&field.name.to_lowercase().as_str()) {
                        fields.push(hidden_field(&field.name, field.value.expose()));
                    }
                }
                if !identity.birth_date.v.is_empty() {
                    fields.push(hidden_field("Birth date", identity.birth_date.v.expose()));
                }
                item["type"] = json!(4);
                item["identity"] = json!({
                    "title": null,
                    "firstName": first,
                    "middleName": null,
                    "lastName": last,
                    "address1": identity.address.v,
                    "address2": null,
                    "address3": null,
                    "city": null,
                    "state": null,
                    "postalCode": null,
                    "country": null,
                    "company": null,
                    "email": identity.email.v,
                    "phone": identity.phone.v,
                    "ssn": ssn,
                    "username": null,
                    "passportNumber": passport,
                    "licenseNumber": licence,
                });
            }
            Kind::Card(card) => {
                let (month, year) = match expiry_parts(&card.expiry.v) {
                    Some(parts) => parts,
                    None => {
                        // An expiry in a form of its own is kept word for word, not dropped.
                        if !card.expiry.v.trim().is_empty() {
                            fields.push(json!({"name": "Expiry", "value": card.expiry.v, "type": 0}));
                        }
                        (String::new(), String::new())
                    }
                };
                // PIN and billing address have no place on a Bitwarden card.
                if !card.pin.v.is_empty() {
                    fields.push(hidden_field("PIN", card.pin.v.expose()));
                }
                if !card.billing.v.trim().is_empty() {
                    fields.push(shown_field("Billing address", &card.billing.v));
                }
                item["type"] = json!(3);
                item["card"] = json!({
                    "cardholderName": card.holder.v,
                    "brand": card.issuer.v,
                    "number": card.number.v.expose(),
                    "expMonth": month,
                    "expYear": year,
                    "code": card.cvv.v.expose(),
                });
            }
            Kind::SshKey(key) => {
                // Bitwarden keeps keys unlocked; a passphrase is kept beside it.
                if !key.passphrase.v.is_empty() {
                    fields.push(hidden_field("Passphrase", key.passphrase.v.expose()));
                }
                item["type"] = json!(5);
                item["sshKey"] = json!({
                    "privateKey": key.private_key.v.expose(),
                    "publicKey": key.public_key.v,
                    "keyFingerprint": key.fingerprint.v,
                });
            }
            other => {
                item["type"] = json!(2);
                item["secureNote"] = json!({"type": 0});
                for (name, value, hidden) in kind_fields(other) {
                    fields.push(
                        json!({"name": name, "value": value.expose(), "type": u8::from(hidden)}),
                    );
                }
            }
        }
        item["fields"] = Value::Array(fields);
        items.push(item);
    }
    let folders: Vec<Value> = vault
        .doc
        .folders
        .values()
        .map(|f| json!({"id": f.id, "name": path_of_folder(vault, Some(&f.id))}))
        .collect();
    let root = json!({"encrypted": false, "folders": folders, "items": items});
    serde_json::to_string_pretty(&root)
        .map(Secret::from)
        .map_err(|_| Error::Encoding)
}

/// A code as Bitwarden reads one: its own `steam://` form for Steam, with the seed as written
/// (no spaces, upper case), and a link otherwise.
fn bitwarden_totp(otp: &crate::otp::OtpConfig) -> String {
    match otp.kind {
        crate::otp::OtpKind::Steam => format!("steam://{}", crate::otp::normalize_secret(otp.secret.expose())),
        _ => otp.to_uri().expose().to_owned(),
    }
}

/// A moment as Bitwarden writes dates, `2024-04-08T13:25:14.123Z`. Bitwarden wants one on
/// every old password, so an unknown time is written as the epoch it is stored as.
fn bitwarden_time(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A login's addresses as Bitwarden's `uris`, each with Bitwarden's own match setting, so a
/// login kept to one host, or never filled, stays that way in Bitwarden and back here.
fn bitwarden_uris(matchers: &[SiteMatcher]) -> Vec<Value> {
    let never = matchers.iter().any(|m| matches!(m, SiteMatcher::Never));
    matchers
        .iter()
        .filter_map(|m| {
            let (uri, mode) = match m {
                SiteMatcher::Url(u) => (u.clone(), Value::Null),
                SiteMatcher::Domain(u) => (u.clone(), json!(0)),
                SiteMatcher::Host(u) => (u.clone(), json!(1)),
                SiteMatcher::Regex(pattern) => (pattern.clone(), json!(4)),
                SiteMatcher::AndroidApp { package, .. } => (format!("androidapp://{package}"), Value::Null),
                SiteMatcher::Never => return None,
            };
            Some(json!({"uri": uri, "match": if never { json!(5) } else { mode }}))
        })
        .collect()
}

/// A card's expiry as month and four-digit year, from the forms imports leave it in:
/// `12/27`, `12/2027`, `12-27`, `2027-12`, `1227`, or 1Password's `202712`.
pub fn expiry_parts(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    // `12 / 27`, spaced as some apps write it, is the same date: a space parts the two only
    // when nothing else does.
    let (month, year) = match text.split_once(['/', '-', '.']).or_else(|| text.split_once(' ')) {
        Some((a, b)) if digits(a.trim()) && digits(b.trim()) => {
            let (a, b) = (a.trim(), b.trim());
            if a.len() == 4 { (b, a) } else { (a, b) }
        }
        Some(_) => return None,
        None if digits(text) && text.len() == 4 => (&text[..2], &text[2..]),
        None if digits(text) && text.len() == 6 && (text.starts_with("19") || text.starts_with("20")) => {
            (&text[4..], &text[..4])
        }
        None if digits(text) && text.len() == 6 => (&text[..2], &text[2..]),
        None => return None,
    };
    let number: u32 = month.parse().ok()?;
    if !(1..=12).contains(&number) || !matches!(year.len(), 2 | 4) {
        return None;
    }
    let year = if year.len() == 2 { format!("20{year}") } else { year.to_owned() };
    Some((number.to_string(), year))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A page to print and lock away. Values are escaped, so a hostile title cannot
/// run anything in whatever renders it.
pub fn to_print_html(vault: &Vault) -> Secret {
    let mut body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{name}</title>\
<style>body{{font:12pt serif;margin:2cm}}h2{{font-size:13pt;margin:1.2em 0 .2em}}\
table{{border-collapse:collapse;width:100%}}td{{border-top:1px solid #999;padding:2pt 6pt;vertical-align:top}}\
td:first-child{{width:9em;color:#333}}code{{font:11pt monospace;word-break:break-all}}\
.warn{{border:2px solid #000;padding:8pt}}</style></head><body><h1>{name}</h1>\
<p class=\"warn\">Anyone holding this page can read everything on it. Keep it locked away.</p>",
        name = escape(&vault.doc.name),
    );
    for entry in exportable(vault) {
        body.push_str(&format!(
            "<h2>{}</h2><table>",
            escape(&entry.common.title.v)
        ));
        let mut row = |name: &str, value: &str| {
            if !value.is_empty() {
                body.push_str(&format!(
                    "<tr><td>{}</td><td><code>{}</code></td></tr>",
                    escape(name),
                    escape(value)
                ));
            }
        };
        if let Some(login) = entry.login() {
            row("Username", &login.username.v);
            row("Password", login.password.v.expose());
            row("Address", &urls(entry).join(" "));
            if let Some(otp) = &login.otp.v {
                row("One-time code", otp.to_uri().expose());
            }
        }
        for (name, value, _) in kind_fields(&entry.kind) {
            row(name, value.expose());
        }
        for field in &entry.common.custom_fields.v {
            row(&field.name, field.value.expose());
        }
        row("Notes", entry.common.notes.v.expose());
        body.push_str("</table>");
    }
    body.push_str("</body></html>");
    Secret::from(body)
}
