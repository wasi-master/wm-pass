//! Between Bitwarden ciphers and WM Pass entries.
//!
//! Bitwarden has five types and WM Pass twelve. Kinds without a home travel as secure notes
//! with labelled custom fields, so they stay readable in official clients, and a hidden
//! `wmpass:kind` field brings them back here. What Bitwarden has no place for goes in a
//! hidden `wmpass:meta` field. A cipher being updated starts from the server's copy, so
//! fields this client does not know about survive.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use wmpass_core::clock::Hlc;
use wmpass_core::model::{
    AliasRef, ApiKey, Card, Database, Document, Entry, Field, FieldKind, Identity, Kind, Licence, Login, Lww, Note, Otp, Passkey,
    Recovery, RecoveryForm, Reminders, Revision, SiteMatcher, SshKey, Wifi, WifiSecurity,
};
use wmpass_core::otp::OtpConfig;
use wmpass_core::Secret;

use crate::api::{prop, text};
use crate::crypto::SymKey;
use crate::{BwError, Result};

const KIND_FIELD: &str = "wmpass:kind";
const META_FIELD: &str = "wmpass:meta";

pub use wmpass_core::clock::{format_date, parse_date};

/// What travels in the meta field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Meta {
    icon: Option<String>,
    color: Option<String>,
    pinned: bool,
    tags: Vec<String>,
    links: Vec<String>,
    reminders: Reminders,
    destroy_at_ms: Option<u64>,
    hidden: bool,
    alias: Option<AliasRef>,
    questions: Vec<Field>,
    matchers: Vec<SiteMatcher>,
}

fn is_empty_meta(meta: &Meta) -> bool {
    *meta == Meta::default()
}

/// Decrypts one optional field.
fn dec(value: &Value, name: &str, key: &SymKey) -> Result<Option<Secret>> {
    match text(value, name) {
        Some(sealed) if !sealed.is_empty() => Ok(Some(key.decrypt_text(sealed)?)),
        _ => Ok(None),
    }
}

fn dec_text(value: &Value, name: &str, key: &SymKey) -> Result<String> {
    Ok(dec(value, name, key)?.map(|s| s.expose().to_owned()).unwrap_or_default())
}

fn dec_secret(value: &Value, name: &str, key: &SymKey) -> Result<Secret> {
    Ok(dec(value, name, key)?.unwrap_or_default())
}

fn enc(plain: &str, key: &SymKey) -> Result<Value> {
    if plain.is_empty() { Ok(Value::Null) } else { Ok(Value::String(key.encrypt(plain.as_bytes())?)) }
}

/// Picks the key for a cipher: its own if it has one, else its organisation's or the account's.
pub fn cipher_key(cipher: &Value, account: &SymKey, organisation: Option<&SymKey>) -> Result<SymKey> {
    let outer = organisation.unwrap_or(account);
    match text(cipher, "key") {
        Some(wrapped) if !wrapped.is_empty() => outer.unwrap_key(wrapped),
        _ => Ok(outer.clone()),
    }
}

fn read_fields(cipher: &Value, key: &SymKey) -> Result<(Vec<Field>, Option<String>, Meta)> {
    let mut fields = Vec::new();
    let (mut kind_name, mut meta) = (None, Meta::default());
    for field in prop(cipher, "fields").and_then(Value::as_array).into_iter().flatten() {
        let name = dec_text(field, "name", key)?;
        let value = dec_secret(field, "value", key)?;
        match name.as_str() {
            KIND_FIELD => kind_name = Some(value.expose().to_owned()),
            META_FIELD => meta = serde_json::from_str(value.expose()).unwrap_or_default(),
            _ => {
                let kind = match prop(field, "type").and_then(Value::as_u64) {
                    Some(1) => FieldKind::Hidden,
                    Some(2) => FieldKind::Toggle,
                    _ => FieldKind::Text,
                };
                fields.push(Field { name, value, kind });
            }
        }
    }
    Ok((fields, kind_name, meta))
}

fn take(fields: &mut Vec<Field>, label: &str) -> Secret {
    match fields.iter().position(|field| field.name == label) {
        Some(index) => fields.remove(index).value,
        None => Secret::default(),
    }
}

fn take_text(fields: &mut Vec<Field>, label: &str) -> String {
    take(fields, label).expose().to_owned()
}

fn read_login(cipher: &Value, key: &SymKey, at: Hlc, meta: &Meta) -> Result<Login> {
    let body = prop(cipher, "login").cloned().unwrap_or(Value::Null);
    let mut matchers = Vec::new();
    for uri in prop(&body, "uris").and_then(Value::as_array).into_iter().flatten() {
        let Some(address) = dec(uri, "uri", key)? else { continue };
        let address = address.expose().to_owned();
        matchers.push(match prop(uri, "match").and_then(Value::as_u64) {
            Some(0) => SiteMatcher::Domain(address),
            Some(1) => SiteMatcher::Host(address),
            Some(4) => SiteMatcher::Regex(address),
            Some(5) => SiteMatcher::Never,
            _ => match address.strip_prefix("androidapp://") {
                Some(package) => SiteMatcher::AndroidApp { package: package.to_owned(), cert_sha256: None },
                None => SiteMatcher::Url(address),
            },
        });
    }
    if !meta.matchers.is_empty() {
        matchers = meta.matchers.clone();
    }
    let otp = dec(&body, "totp", key)?.and_then(|uri| read_otp(uri.expose()));
    let mut passkeys = Vec::new();
    for credential in prop(&body, "fido2Credentials").and_then(Value::as_array).into_iter().flatten() {
        passkeys.push(Passkey {
            credential_id: dec_text(credential, "credentialId", key)?,
            rp_id: dec_text(credential, "rpId", key)?,
            rp_name: dec_text(credential, "rpName", key)?,
            user_handle: dec_text(credential, "userHandle", key)?,
            user_name: dec_text(credential, "userName", key)?,
            private_key: dec_secret(credential, "keyValue", key)?,
            algorithm: -7,
            counter: dec_text(credential, "counter", key)?.parse().unwrap_or(0),
            created_ms: text(credential, "creationDate").and_then(parse_date).unwrap_or(0),
        });
    }
    Ok(Login {
        site_matchers: Lww::new(matchers, at),
        username: Lww::new(dec_text(&body, "username", key)?, at),
        password: Lww::new(dec_secret(&body, "password", key)?, at),
        otp: Lww::new(otp, at),
        passkeys: Lww::new(passkeys, at),
        alias: Lww::new(meta.alias.clone(), at),
        questions: Lww::new(meta.questions.clone(), at),
    })
}

/// Bitwarden accepts a bare secret as well as a URI.
fn read_otp(text: &str) -> Option<OtpConfig> {
    if text.starts_with("otpauth://") || text.starts_with("steam://") {
        // `from_uri` reads `steam://` as a Steam code; a `totp` link would show six digits.
        OtpConfig::from_uri(text).ok()
    } else {
        Some(OtpConfig::totp(text))
    }
}

fn read_extra_kind(name: &str, fields: &mut Vec<Field>, at: Hlc) -> Option<Kind> {
    macro_rules! l {
        ($v:expr) => {
            Lww::new($v, at)
        };
    }
    Some(match name {
        "document" => Kind::Document(Document { label: l!(take_text(fields, "Type")), number: l!(take(fields, "Number")) }),
        "api" => Kind::ApiKey(ApiKey {
            provider: l!(take_text(fields, "Provider")),
            key: l!(take(fields, "Key")),
            secret: l!(take(fields, "Secret")),
            scopes: l!(take_text(fields, "Scopes")),
            expires_ms: l!(take_text(fields, "Expires").parse().ok()),
        }),
        "database" => Kind::Database(Database {
            engine: l!(take_text(fields, "Engine")),
            host: l!(take_text(fields, "Host")),
            port: l!(take_text(fields, "Port").parse().ok()),
            name: l!(take_text(fields, "Database")),
            user: l!(take_text(fields, "User")),
            password: l!(take(fields, "Password")),
            tls: l!(take_text(fields, "TLS") == "true"),
            url: l!(take(fields, "URL")),
        }),
        "licence" => Kind::Licence(Licence {
            product: l!(take_text(fields, "Product")),
            key: l!(take(fields, "Licence key")),
            email: l!(take_text(fields, "Email")),
            order_id: l!(take_text(fields, "Order")),
            expires_ms: l!(take_text(fields, "Expires").parse().ok()),
        }),
        "recovery" => Kind::Recovery(Recovery {
            form: l!(if take_text(fields, "Form") == "codes" { RecoveryForm::Codes } else { RecoveryForm::Phrase }),
            phrase: l!(take(fields, "Phrase")),
            linked_entry: l!(Some(take_text(fields, "Linked")).filter(|s| !s.is_empty())),
        }),
        "wifi" => Kind::Wifi(Wifi {
            ssid: l!(take_text(fields, "Network")),
            security: l!(match take_text(fields, "Security").as_str() {
                "open" => WifiSecurity::Open,
                "wep" => WifiSecurity::Wep,
                "sae" => WifiSecurity::Sae,
                "eap" => WifiSecurity::Eap,
                _ => WifiSecurity::Wpa,
            }),
            password: l!(take(fields, "Password")),
            hidden_ssid: l!(take_text(fields, "Hidden") == "true"),
        }),
        "note" => return None,
        _ => return None,
    })
}

/// Decrypts a cipher into an entry.
pub fn entry_from_cipher(cipher: &Value, account: &SymKey, organisation: Option<&SymKey>) -> Result<Entry> {
    let key = cipher_key(cipher, account, organisation)?;
    let key = &key;
    let revision = text(cipher, "revisionDate").and_then(parse_date).unwrap_or(0);
    let at = Hlc { ms: revision, counter: 0, device: 0 };
    let (mut fields, kind_name, meta) = read_fields(cipher, key)?;
    let notes = dec_secret(cipher, "notes", key)?;
    let kind = match prop(cipher, "type").and_then(Value::as_u64) {
        Some(1) if kind_name.as_deref() == Some("otp") => {
            let login = read_login(cipher, key, at, &meta)?;
            Kind::Otp(Otp { config: login.otp })
        }
        Some(1) => Kind::Login(read_login(cipher, key, at, &meta)?),
        Some(2) => {
            let extra = kind_name.as_deref().and_then(|name| read_extra_kind(name, &mut fields, at));
            extra.unwrap_or_else(|| Kind::Note(Note { body: Lww::new(Secret::default(), at) }))
        }
        Some(3) => {
            let body = prop(cipher, "card").cloned().unwrap_or(Value::Null);
            let month = dec_text(&body, "expMonth", key)?;
            let year = dec_text(&body, "expYear", key)?;
            let expiry = if month.is_empty() && year.is_empty() { String::new() } else { format!("{month}/{year}") };
            Kind::Card(Card {
                holder: Lww::new(dec_text(&body, "cardholderName", key)?, at),
                number: Lww::new(dec_secret(&body, "number", key)?, at),
                expiry: Lww::new(expiry, at),
                cvv: Lww::new(dec_secret(&body, "code", key)?, at),
                pin: Lww::new(take(&mut fields, "PIN"), at),
                issuer: Lww::new(dec_text(&body, "brand", key)?, at),
                billing: Lww::new(take_text(&mut fields, "Billing"), at),
            })
        }
        Some(4) => {
            let body = prop(cipher, "identity").cloned().unwrap_or(Value::Null);
            let join = |names: &[&str]| -> Result<String> {
                let mut parts = Vec::new();
                for name in names {
                    let part = dec_text(&body, name, key)?;
                    if !part.is_empty() {
                        parts.push(part);
                    }
                }
                Ok(parts.join(" "))
            };
            let mut documents = Vec::new();
            for (label, name) in [("SSN", "ssn"), ("Passport", "passportNumber"), ("Licence", "licenseNumber")] {
                if let Some(value) = dec(&body, name, key)? {
                    documents.push(Field { name: label.into(), value, kind: FieldKind::Hidden });
                }
            }
            Kind::Identity(Identity {
                full_name: Lww::new(join(&["title", "firstName", "middleName", "lastName"])?, at),
                address: Lww::new(join(&["address1", "address2", "address3", "city", "state", "postalCode", "country"])?, at),
                phone: Lww::new(dec_text(&body, "phone", key)?, at),
                email: Lww::new(dec_text(&body, "email", key)?, at),
                birth_date: Lww::new(take(&mut fields, "Birth date"), at),
                document_numbers: Lww::new(documents, at),
            })
        }
        Some(5) => {
            let body = prop(cipher, "sshKey").cloned().unwrap_or(Value::Null);
            Kind::SshKey(SshKey {
                private_key: Lww::new(dec_secret(&body, "privateKey", key)?, at),
                public_key: Lww::new(dec_text(&body, "publicKey", key)?, at),
                fingerprint: Lww::new(dec_text(&body, "keyFingerprint", key)?, at),
                passphrase: Lww::new(take(&mut fields, "Passphrase"), at),
                agent_allowed: Lww::new(take_text(&mut fields, "Agent") == "true", at),
            })
        }
        _ => return Err(BwError::Unsupported("this item's type is not known")),
    };
    // A plain secure note keeps its body in `notes`, so nothing else holds it.
    let (kind, notes) = match kind {
        Kind::Note(_) => (Kind::Note(Note { body: Lww::new(notes, at) }), Secret::default()),
        other => (other, notes),
    };
    let mut entry = Entry { id: text(cipher, "id").unwrap_or_default().to_owned(), kind, ..Entry::default() };
    entry.created_ms = text(cipher, "creationDate").and_then(parse_date).unwrap_or(revision);
    let common = &mut entry.common;
    common.title = Lww::new(dec_text(cipher, "name", key)?, at);
    common.notes = Lww::new(notes, at);
    common.favourite = Lww::new(prop(cipher, "favorite").and_then(Value::as_bool).unwrap_or(false), at);
    common.folder = Lww::new(text(cipher, "folderId").map(str::to_owned), at);
    common.custom_fields = Lww::new(fields, at);
    common.deleted_at_ms = Lww::new(text(cipher, "deletedDate").and_then(parse_date), at);
    common.icon = Lww::new(meta.icon, at);
    common.color = Lww::new(meta.color, at);
    common.pinned = Lww::new(meta.pinned, at);
    common.tags = Lww::new(meta.tags, at);
    common.links = Lww::new(meta.links, at);
    common.reminders = Lww::new(meta.reminders, at);
    common.destroy_at_ms = Lww::new(meta.destroy_at_ms, at);
    common.hidden = Lww::new(meta.hidden, at);
    let history = prop(cipher, "passwordHistory").and_then(Value::as_array);
    for past in history.into_iter().flatten() {
        let moment = text(past, "lastUsedDate").and_then(parse_date).unwrap_or(0);
        entry.history.push(Revision {
            field: "password".into(),
            value: dec_secret(past, "password", key)?,
            at: Hlc { ms: moment, counter: 0, device: 0 },
        });
    }
    Ok(entry)
}

fn push_field(out: &mut Vec<Value>, name: &str, value: &str, hidden: bool, key: &SymKey) -> Result<()> {
    if !value.is_empty() {
        out.push(json!({"type": u32::from(hidden), "name": enc(name, key)?, "value": enc(value, key)?}));
    }
    Ok(())
}

fn login_json(login: &Login, base: Option<&Value>, key: &SymKey) -> Result<Value> {
    let mut object = base.and_then(Value::as_object).cloned().unwrap_or_default();
    object.insert("username".into(), enc(&login.username.v, key)?);
    object.insert("password".into(), enc(login.password.v.expose(), key)?);
    let totp = match &login.otp.v {
        // Bitwarden reads a Steam code only in its own `steam://` form.
        Some(config) if matches!(config.kind, wmpass_core::otp::OtpKind::Steam) => {
            enc(&format!("steam://{}", config.secret.expose()), key)?
        }
        Some(config) => enc(config.to_uri().expose(), key)?,
        None => Value::Null,
    };
    object.insert("totp".into(), totp);
    let mut uris = Vec::new();
    for matcher in &login.site_matchers.v {
        let (address, mode) = match matcher {
            SiteMatcher::Domain(a) => (a.clone(), Value::from(0)),
            SiteMatcher::Host(a) => (a.clone(), Value::from(1)),
            SiteMatcher::Url(a) => (a.clone(), Value::Null),
            SiteMatcher::AndroidApp { package, .. } => (format!("androidapp://{package}"), Value::Null),
            SiteMatcher::Regex(a) => (a.clone(), Value::from(4)),
            SiteMatcher::Never => continue,
        };
        uris.push(json!({"uri": enc(&address, key)?, "match": mode}));
    }
    object.insert("uris".into(), Value::Array(uris));
    let mut credentials = Vec::new();
    for passkey in &login.passkeys.v {
        credentials.push(json!({
            "credentialId": enc(&passkey.credential_id, key)?,
            "keyType": enc("public-key", key)?,
            "keyAlgorithm": enc("ECDSA", key)?,
            "keyCurve": enc("P-256", key)?,
            "keyValue": enc(passkey.private_key.expose(), key)?,
            "rpId": enc(&passkey.rp_id, key)?,
            "rpName": enc(&passkey.rp_name, key)?,
            "userHandle": enc(&passkey.user_handle, key)?,
            "userName": enc(&passkey.user_name, key)?,
            "counter": enc(&passkey.counter.to_string(), key)?,
            "discoverable": enc("true", key)?,
            "creationDate": format_date(passkey.created_ms),
        }));
    }
    object.insert("fido2Credentials".into(), Value::Array(credentials));
    Ok(Value::Object(object))
}

/// Encrypts an entry into a cipher. Pass the server's current copy when updating.
pub fn cipher_from_entry(entry: &Entry, server: Option<&Value>, key: &SymKey) -> Result<Value> {
    let mut cipher: Map<String, Value> = server.and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(known) = server.and_then(|s| text(s, "revisionDate")) {
        cipher.insert("lastKnownRevisionDate".into(), Value::String(known.to_owned()));
    }
    // This client wraps nothing per cipher, so a per-cipher key must not linger over fields it re-encrypted.
    cipher.remove("key");
    let common = &entry.common;
    let mut fields = Vec::new();
    for field in &common.custom_fields.v {
        let mode = match field.kind {
            FieldKind::Hidden => 1,
            FieldKind::Toggle => 2,
            _ => 0,
        };
        fields.push(json!({"type": mode, "name": enc(&field.name, key)?, "value": enc(field.value.expose(), key)?}));
    }
    let mut meta = Meta {
        icon: common.icon.v.clone(),
        color: common.color.v.clone(),
        pinned: common.pinned.v,
        tags: common.tags.v.clone(),
        links: common.links.v.clone(),
        reminders: common.reminders.v.clone(),
        destroy_at_ms: common.destroy_at_ms.v,
        hidden: common.hidden.v,
        ..Meta::default()
    };
    let mut notes = common.notes.v.expose().to_owned();
    let set_kind = |fields: &mut Vec<Value>, name: &str| push_field(fields, KIND_FIELD, name, true, key);
    let bw_type = match &entry.kind {
        Kind::Login(login) => {
            meta.alias = login.alias.v.clone();
            meta.questions = login.questions.v.clone();
            let lossless = login.site_matchers.v.iter().all(|m| !matches!(m, SiteMatcher::Never | SiteMatcher::AndroidApp { cert_sha256: Some(_), .. }));
            if !lossless {
                meta.matchers = login.site_matchers.v.clone();
            }
            cipher.insert("login".into(), login_json(login, server.and_then(|s| prop(s, "login")), key)?);
            1
        }
        Kind::Otp(otp) => {
            set_kind(&mut fields, "otp")?;
            let as_login = Login { otp: otp.config.clone(), ..Login::default() };
            cipher.insert("login".into(), login_json(&as_login, None, key)?);
            1
        }
        Kind::Note(note) => {
            notes = note.body.v.expose().to_owned();
            cipher.insert("secureNote".into(), json!({"type": 0}));
            2
        }
        Kind::Card(card) => {
            // `12/2030` goes as written; `2030-12` or `1230`, as imports leave them, would put
            // the whole date in the month.
            let (month, year) = match card.expiry.v.split_once('/') {
                Some((month, year)) => (month.trim().to_owned(), year.trim().to_owned()),
                None => wmpass_core::io::export::expiry_parts(&card.expiry.v)
                    .unwrap_or_else(|| (card.expiry.v.clone(), String::new())),
            };
            cipher.insert(
                "card".into(),
                json!({
                    "cardholderName": enc(&card.holder.v, key)?,
                    "brand": enc(&card.issuer.v, key)?,
                    "number": enc(card.number.v.expose(), key)?,
                    "expMonth": enc(&month, key)?,
                    "expYear": enc(&year, key)?,
                    "code": enc(card.cvv.v.expose(), key)?,
                }),
            );
            push_field(&mut fields, "PIN", card.pin.v.expose(), true, key)?;
            push_field(&mut fields, "Billing", &card.billing.v, false, key)?;
            3
        }
        Kind::Identity(identity) => {
            let mut body = server.and_then(|s| prop(s, "identity")).and_then(Value::as_object).cloned().unwrap_or_default();
            // One name field here, several there. If it did not change, the parts stay as they are.
            let unchanged = |value: &str, names: &[&str]| -> bool {
                let joined: Vec<String> = names
                    .iter()
                    .filter_map(|name| {
                        let holder = Value::Object(body.clone());
                        dec_text(&holder, name, key).ok().filter(|t| !t.is_empty())
                    })
                    .collect();
                joined.join(" ") == value && !joined.is_empty()
            };
            let name_same = unchanged(&identity.full_name.v, &["title", "firstName", "middleName", "lastName"]);
            let address_same = unchanged(&identity.address.v, &["address1", "address2", "address3", "city", "state", "postalCode", "country"]);
            if !name_same {
                for part in ["title", "middleName", "lastName"] {
                    body.remove(part);
                }
                body.insert("firstName".into(), enc(&identity.full_name.v, key)?);
            }
            if !address_same {
                for part in ["address2", "address3", "city", "state", "postalCode", "country"] {
                    body.remove(part);
                }
                body.insert("address1".into(), enc(&identity.address.v, key)?);
            }
            body.insert("phone".into(), enc(&identity.phone.v, key)?);
            body.insert("email".into(), enc(&identity.email.v, key)?);
            for (label, part) in [("SSN", "ssn"), ("Passport", "passportNumber"), ("Licence", "licenseNumber")] {
                let found = identity.document_numbers.v.iter().find(|d| d.name == label);
                body.insert(part.into(), enc(found.map_or("", |d| d.value.expose()), key)?);
            }
            for other in identity.document_numbers.v.iter().filter(|d| !["SSN", "Passport", "Licence"].contains(&d.name.as_str())) {
                push_field(&mut fields, &other.name, other.value.expose(), true, key)?;
            }
            push_field(&mut fields, "Birth date", identity.birth_date.v.expose(), true, key)?;
            cipher.insert("identity".into(), Value::Object(body));
            4
        }
        Kind::SshKey(ssh) => {
            cipher.insert(
                "sshKey".into(),
                json!({
                    "privateKey": enc(ssh.private_key.v.expose(), key)?,
                    "publicKey": enc(&ssh.public_key.v, key)?,
                    "keyFingerprint": enc(&ssh.fingerprint.v, key)?,
                }),
            );
            push_field(&mut fields, "Passphrase", ssh.passphrase.v.expose(), true, key)?;
            if ssh.agent_allowed.v {
                push_field(&mut fields, "Agent", "true", false, key)?;
            }
            5
        }
        Kind::Document(doc) => {
            set_kind(&mut fields, "document")?;
            push_field(&mut fields, "Type", &doc.label.v, false, key)?;
            push_field(&mut fields, "Number", doc.number.v.expose(), true, key)?;
            cipher.insert("secureNote".into(), json!({"type": 0}));
            2
        }
        Kind::ApiKey(api) => {
            set_kind(&mut fields, "api")?;
            push_field(&mut fields, "Provider", &api.provider.v, false, key)?;
            push_field(&mut fields, "Key", api.key.v.expose(), true, key)?;
            push_field(&mut fields, "Secret", api.secret.v.expose(), true, key)?;
            push_field(&mut fields, "Scopes", &api.scopes.v, false, key)?;
            if let Some(expires) = api.expires_ms.v {
                push_field(&mut fields, "Expires", &expires.to_string(), false, key)?;
            }
            cipher.insert("secureNote".into(), json!({"type": 0}));
            2
        }
        Kind::Database(db) => {
            set_kind(&mut fields, "database")?;
            push_field(&mut fields, "Engine", &db.engine.v, false, key)?;
            push_field(&mut fields, "Host", &db.host.v, false, key)?;
            if let Some(port) = db.port.v {
                push_field(&mut fields, "Port", &port.to_string(), false, key)?;
            }
            push_field(&mut fields, "Database", &db.name.v, false, key)?;
            push_field(&mut fields, "User", &db.user.v, false, key)?;
            push_field(&mut fields, "Password", db.password.v.expose(), true, key)?;
            if db.tls.v {
                push_field(&mut fields, "TLS", "true", false, key)?;
            }
            push_field(&mut fields, "URL", db.url.v.expose(), true, key)?;
            cipher.insert("secureNote".into(), json!({"type": 0}));
            2
        }
        Kind::Licence(licence) => {
            set_kind(&mut fields, "licence")?;
            push_field(&mut fields, "Product", &licence.product.v, false, key)?;
            push_field(&mut fields, "Licence key", licence.key.v.expose(), true, key)?;
            push_field(&mut fields, "Email", &licence.email.v, false, key)?;
            push_field(&mut fields, "Order", &licence.order_id.v, false, key)?;
            if let Some(expires) = licence.expires_ms.v {
                push_field(&mut fields, "Expires", &expires.to_string(), false, key)?;
            }
            cipher.insert("secureNote".into(), json!({"type": 0}));
            2
        }
        Kind::Recovery(recovery) => {
            set_kind(&mut fields, "recovery")?;
            let form = if recovery.form.v == RecoveryForm::Codes { "codes" } else { "phrase" };
            push_field(&mut fields, "Form", form, false, key)?;
            push_field(&mut fields, "Phrase", recovery.phrase.v.expose(), true, key)?;
            push_field(&mut fields, "Linked", recovery.linked_entry.v.as_deref().unwrap_or(""), false, key)?;
            cipher.insert("secureNote".into(), json!({"type": 0}));
            2
        }
        Kind::Wifi(wifi) => {
            set_kind(&mut fields, "wifi")?;
            let security = match wifi.security.v {
                WifiSecurity::Open => "open",
                WifiSecurity::Wep => "wep",
                WifiSecurity::Wpa => "wpa",
                WifiSecurity::Sae => "sae",
                WifiSecurity::Eap => "eap",
            };
            push_field(&mut fields, "Network", &wifi.ssid.v, false, key)?;
            push_field(&mut fields, "Security", security, false, key)?;
            push_field(&mut fields, "Password", wifi.password.v.expose(), true, key)?;
            if wifi.hidden_ssid.v {
                push_field(&mut fields, "Hidden", "true", false, key)?;
            }
            cipher.insert("secureNote".into(), json!({"type": 0}));
            2
        }
    };
    if !is_empty_meta(&meta) {
        let json = serde_json::to_string(&meta).map_err(|_| BwError::BadAnswer)?;
        push_field(&mut fields, META_FIELD, &json, true, key)?;
    }
    cipher.insert("type".into(), Value::from(bw_type));
    cipher.insert("name".into(), enc(&common.title.v, key)?);
    cipher.insert("notes".into(), enc(&notes, key)?);
    cipher.insert("favorite".into(), Value::Bool(common.favourite.v));
    cipher.insert("folderId".into(), common.folder.v.clone().map_or(Value::Null, Value::String));
    cipher.insert("fields".into(), Value::Array(fields));
    let mut history = Vec::new();
    for past in entry.history_of("password") {
        history.push(json!({"password": enc(past.value.expose(), key)?, "lastUsedDate": format_date(past.at.ms)}));
    }
    cipher.insert("passwordHistory".into(), Value::Array(history));
    cipher.entry("reprompt").or_insert(Value::from(0));
    Ok(Value::Object(cipher))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Hlc = Hlc { ms: 1_700_000_000_000, counter: 0, device: 0 };

    #[test]
    fn a_steam_secret_reads_as_a_steam_code() {
        let otp = read_otp("steam://JBSWY3DPEHPK3PXP").unwrap();
        assert!(matches!(otp.kind, wmpass_core::otp::OtpKind::Steam));
        assert_eq!(otp.digits, 5);
        let key = SymKey::generate().unwrap();
        let login = Login { otp: Lww::new(Some(otp), AT), ..Login::default() };
        let sent = login_json(&login, None, &key).unwrap();
        let back = dec_text(&sent, "totp", &key).unwrap();
        assert_eq!(back, "steam://JBSWY3DPEHPK3PXP");
    }

    fn stamped(mut cipher: Value) -> Value {
        cipher["revisionDate"] = Value::String(format_date(AT.ms));
        cipher["id"] = Value::String("id-1".into());
        cipher
    }

    fn round_trip(entry: &Entry) -> Entry {
        let key = SymKey::generate().unwrap();
        let cipher = stamped(cipher_from_entry(entry, None, &key).unwrap());
        entry_from_cipher(&cipher, &key, None).unwrap()
    }

    fn base(kind: Kind) -> Entry {
        let mut entry = Entry { id: "id-1".into(), kind, created_ms: AT.ms, ..Entry::default() };
        entry.common.title = Lww::new("Title".into(), AT);
        entry
    }

    fn l<T>(value: T) -> Lww<T> {
        Lww::new(value, AT)
    }

    #[test]
    fn dates_round_trip() {
        for ms in [0, 1_700_000_000_123, 1_709_210_096_000] {
            assert_eq!(parse_date(&format_date(ms)), Some(ms));
        }
        assert_eq!(parse_date("2026-11-02T10:00:00Z"), Some(1_793_613_600_000));
        assert_eq!(parse_date("2026-11-02T10:00:00.5Z"), Some(1_793_613_600_500));
        assert_eq!(parse_date("2026-11-02T10:00:00.1234567Z"), Some(1_793_613_600_123));
        assert_eq!(parse_date("nonsense"), None);
    }

    #[test]
    fn a_login_survives_with_everything_it_had() {
        let login = Login {
            site_matchers: l(vec![
                SiteMatcher::Url("https://example.com/login".into()),
                SiteMatcher::Domain("example.org".into()),
                SiteMatcher::AndroidApp { package: "com.example".into(), cert_sha256: None },
            ]),
            username: l("me".into()),
            password: l("pw".into()),
            otp: l(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP"))),
            passkeys: l(vec![Passkey {
                credential_id: "cred".into(),
                rp_id: "example.com".into(),
                rp_name: "Example".into(),
                user_handle: "handle".into(),
                user_name: "me".into(),
                private_key: "pkcs8".into(),
                algorithm: -7,
                counter: 4,
                created_ms: 1_700_000_000_000,
            }]),
            alias: l(Some(AliasRef { provider: "simplelogin".into(), remote_id: "1".into(), address: "a@b.c".into() })),
            questions: l(vec![Field { name: "Pet".into(), value: "Rex".into(), kind: FieldKind::Hidden }]),
        };
        let mut entry = base(Kind::Login(login.clone()));
        entry.common.notes = l("a note".into());
        entry.common.favourite = l(true);
        entry.common.tags = l(vec!["work".into()]);
        entry.common.color = l(Some("#fff".into()));
        entry.common.custom_fields = l(vec![
            Field { name: "PINish".into(), value: "1".into(), kind: FieldKind::Hidden },
            Field { name: "On".into(), value: "true".into(), kind: FieldKind::Toggle },
        ]);
        entry.history.push(Revision { field: "password".into(), value: "older".into(), at: Hlc { ms: 5000, counter: 0, device: 0 } });
        let got = round_trip(&entry);
        assert_eq!(got.login().unwrap(), &login);
        assert_eq!(got.common.notes.v.expose(), "a note");
        assert!(got.common.favourite.v);
        assert_eq!(got.common.tags.v, vec!["work".to_owned()]);
        assert_eq!(got.common.color.v.as_deref(), Some("#fff"));
        assert_eq!(got.common.custom_fields.v, entry.common.custom_fields.v);
        assert_eq!(got.history, entry.history);
    }

    #[test]
    fn every_kind_comes_back_as_itself() {
        let kinds = vec![
            Kind::Note(Note { body: l("body".into()) }),
            Kind::Card(Card {
                holder: l("A B".into()),
                number: l("4111".into()),
                expiry: l("12/2030".into()),
                cvv: l("123".into()),
                pin: l("9999".into()),
                issuer: l("Visa".into()),
                billing: l("Street".into()),
            }),
            Kind::Identity(Identity {
                full_name: l("Ada Lovelace".into()),
                address: l("1 Road".into()),
                phone: l("123".into()),
                email: l("a@b.c".into()),
                birth_date: l("1815-12-10".into()),
                document_numbers: l(vec![Field { name: "Passport".into(), value: "P1".into(), kind: FieldKind::Hidden }]),
            }),
            Kind::SshKey(SshKey {
                private_key: l("PRIV".into()),
                public_key: l("PUB".into()),
                fingerprint: l("FP".into()),
                passphrase: l("pp".into()),
                agent_allowed: l(true),
            }),
            Kind::Document(Document { label: l("Passport".into()), number: l("N1".into()) }),
            Kind::ApiKey(ApiKey {
                provider: l("Acme".into()),
                key: l("k".into()),
                secret: l("s".into()),
                scopes: l("read".into()),
                expires_ms: l(Some(123)),
            }),
            Kind::Database(Database {
                engine: l("postgres".into()),
                host: l("db".into()),
                port: l(Some(5432)),
                name: l("app".into()),
                user: l("u".into()),
                password: l("p".into()),
                tls: l(true),
                url: l("postgres://db".into()),
            }),
            Kind::Licence(Licence {
                product: l("Tool".into()),
                key: l("LIC".into()),
                email: l("a@b.c".into()),
                order_id: l("7".into()),
                expires_ms: l(None),
            }),
            Kind::Recovery(Recovery { form: l(RecoveryForm::Codes), phrase: l("a b c".into()), linked_entry: l(Some("id-2".into())) }),
            Kind::Wifi(Wifi { ssid: l("Home".into()), security: l(WifiSecurity::Sae), password: l("wifipw".into()), hidden_ssid: l(true) }),
            Kind::Otp(Otp { config: l(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP"))) }),
        ];
        for kind in kinds {
            let entry = base(kind.clone());
            let got = round_trip(&entry);
            assert_eq!(got.kind, kind, "{}", kind.name());
            assert!(got.common.custom_fields.v.is_empty(), "{} left fields behind", kind.name());
        }
    }

    #[test]
    fn an_expiry_written_year_first_goes_as_month_and_year() {
        let card = |expiry: &str| base(Kind::Card(Card { expiry: l(expiry.into()), ..Card::default() }));
        for (saved, sent) in [("2030-12", "12/2030"), ("1230", "12/2030"), ("12 / 2030", "12/2030")] {
            let Kind::Card(back) = round_trip(&card(saved)).kind else { panic!("a card") };
            assert_eq!(back.expiry.v, sent, "{saved}");
        }
    }

    #[test]
    fn nothing_in_a_cipher_is_readable_without_the_key() {
        let login = Login { username: l("visible-user".into()), password: l("visible-pass".into()), ..Login::default() };
        let mut entry = base(Kind::Login(login));
        entry.common.title = l("visible-title".into());
        entry.common.tags = l(vec!["visible-tag".into()]);
        let key = SymKey::generate().unwrap();
        let json = cipher_from_entry(&entry, None, &key).unwrap().to_string();
        assert!(!json.contains("visible"), "{json}");
    }

    #[test]
    fn updates_keep_what_the_mapping_does_not_understand() {
        let key = SymKey::generate().unwrap();
        let server = json!({
            "id": "x",
            "revisionDate": "2026-01-01T00:00:00.000Z",
            "organizationId": "org",
            "collectionIds": ["c"],
            "someNewFeature": {"a": 1},
            "type": 1,
            "name": key.encrypt(b"Old").unwrap(),
            "login": {"username": key.encrypt(b"u").unwrap(), "autofillOnPageLoad": true},
        });
        let mut entry = entry_from_cipher(&server, &key, None).unwrap();
        entry.common.title = l("New".into());
        let out = cipher_from_entry(&entry, Some(&server), &key).unwrap();
        assert_eq!(out["someNewFeature"]["a"], 1);
        assert_eq!(out["organizationId"], "org");
        assert_eq!(out["login"]["autofillOnPageLoad"], true);
        assert_eq!(out["lastKnownRevisionDate"], "2026-01-01T00:00:00.000Z");
    }

    #[test]
    fn identity_parts_stay_split_unless_the_name_changed() {
        let key = SymKey::generate().unwrap();
        let server = json!({
            "id": "x",
            "type": 4,
            "name": key.encrypt(b"Me").unwrap(),
            "identity": {"firstName": key.encrypt(b"Ada").unwrap(), "lastName": key.encrypt(b"Lovelace").unwrap()},
        });
        let mut entry = entry_from_cipher(&server, &key, None).unwrap();
        let same = cipher_from_entry(&entry, Some(&server), &key).unwrap();
        assert_eq!(same["identity"]["lastName"], server["identity"]["lastName"]);
        if let Kind::Identity(identity) = &mut entry.kind {
            identity.full_name = l("Grace Hopper".into());
        }
        let changed = cipher_from_entry(&entry, Some(&server), &key).unwrap();
        assert!(changed["identity"].get("lastName").is_none());
        let back = entry_from_cipher(&changed, &key, None).unwrap();
        let Kind::Identity(identity) = &back.kind else { panic!() };
        assert_eq!(identity.full_name.v, "Grace Hopper");
    }

    #[test]
    fn per_cipher_keys_and_organisation_keys_open() {
        let (account, organisation) = (SymKey::generate().unwrap(), SymKey::generate().unwrap());
        let item = SymKey::generate().unwrap();
        let cipher = json!({
            "id": "x",
            "type": 2,
            "key": organisation.encrypt(&item.to_bytes()).unwrap(),
            "name": item.encrypt(b"Shared").unwrap(),
            "notes": item.encrypt(b"body").unwrap(),
        });
        let entry = entry_from_cipher(&cipher, &account, Some(&organisation)).unwrap();
        assert_eq!(entry.common.title.v, "Shared");
        let Kind::Note(note) = &entry.kind else { panic!() };
        assert_eq!(note.body.v.expose(), "body");
        assert!(entry_from_cipher(&cipher, &account, None).is_err());
    }
}
