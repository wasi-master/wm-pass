//! Authenticator app exports, and Google Authenticator's migration link.

use base64::Engine;
use serde_json::Value;

use super::json_formats::{list, parse, text};
use zeroize::Zeroizing;

use super::{Imported, otp_entry};
use crate::clock::Hlc;
use crate::model::Lww;
use crate::otp::{OtpAlgorithm, OtpConfig, OtpKind, base32_encode, percent_decode};
use crate::{Error, Result, Secret};

fn algorithm(name: &str) -> OtpAlgorithm {
    match name.to_ascii_uppercase().replace('-', "").as_str() {
        "SHA256" | "HMACSHA256" => OtpAlgorithm::Sha256,
        "SHA512" | "HMACSHA512" => OtpAlgorithm::Sha512,
        _ => OtpAlgorithm::Sha1,
    }
}

fn number(value: &Value, key: &str, fallback: u64) -> u64 {
    value
        .get(key)
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(fallback)
}

fn kind_of(name: &str, period: u64, counter: u64) -> Option<OtpKind> {
    match name.to_ascii_lowercase().as_str() {
        "totp" => Some(OtpKind::Totp {
            period: u32::try_from(period).unwrap_or(30),
        }),
        "hotp" => Some(OtpKind::Hotp { counter }),
        "steam" => Some(OtpKind::Steam),
        _ => None,
    }
}

pub(super) fn aegis(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    if root.get("db").is_some_and(Value::is_string) {
        return Err(Error::Import(
            "this Aegis export is encrypted. Export it again without encryption".into(),
        ));
    }
    let db = root
        .get("db")
        .ok_or_else(|| Error::Import("no entries in the file".into()))?;
    let mut out = Imported::default();
    for entry in list(db, "entries") {
        let info = entry.get("info").cloned().unwrap_or(Value::Null);
        let name = text(entry, "name");
        let Some(kind) = kind_of(
            &text(entry, "type"),
            number(&info, "period", 30),
            number(&info, "counter", 0),
        ) else {
            out.skipped.push(format!(
                "{name}: Aegis {} codes are not supported",
                text(entry, "type")
            ));
            continue;
        };
        out.entries.push(otp_entry(
            OtpConfig {
                secret: Secret::from(text(&info, "secret")),
                issuer: text(entry, "issuer"),
                account: name,
                algorithm: algorithm(&text(&info, "algo")),
                digits: u32::try_from(number(&info, "digits", 6)).unwrap_or(6),
                kind,
            },
            at,
        ));
    }
    Ok(out)
}

pub(super) fn two_fas(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    if root.get("servicesEncrypted").is_some_and(|v| v.is_string()) {
        return Err(Error::Import(
            "this 2FAS export is encrypted. Export it again without a password".into(),
        ));
    }
    let mut out = Imported::default();
    for service in list(&root, "services") {
        let otp = service.get("otp").cloned().unwrap_or(Value::Null);
        let name = text(service, "name");
        let token_type = text(&otp, "tokenType");
        let token_type = if token_type.is_empty() {
            "TOTP".to_owned()
        } else {
            token_type
        };
        let Some(kind) = kind_of(
            &token_type,
            number(&otp, "period", 30),
            number(&otp, "counter", 0),
        ) else {
            out.skipped
                .push(format!("{name}: 2FAS {token_type} codes are not supported"));
            continue;
        };
        let issuer = text(&otp, "issuer");
        out.entries.push(otp_entry(
            OtpConfig {
                secret: Secret::from(text(service, "secret")),
                issuer: if issuer.is_empty() { name } else { issuer },
                account: text(&otp, "account"),
                algorithm: algorithm(&text(&otp, "algorithm")),
                digits: u32::try_from(number(&otp, "digits", 6)).unwrap_or(6),
                kind,
            },
            at,
        ));
    }
    Ok(out)
}

pub(super) fn and_otp(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    for item in root.as_array().map_or(&[][..], Vec::as_slice) {
        let label = text(item, "label");
        let Some(kind) = kind_of(
            &text(item, "type"),
            number(item, "period", 30),
            number(item, "counter", 0),
        ) else {
            out.skipped.push(format!(
                "{label}: andOTP {} codes are not supported",
                text(item, "type")
            ));
            continue;
        };
        out.entries.push(otp_entry(
            OtpConfig {
                secret: Secret::from(text(item, "secret")),
                issuer: text(item, "issuer"),
                account: label,
                algorithm: algorithm(&text(item, "algorithm")),
                digits: u32::try_from(number(item, "digits", 6)).unwrap_or(6),
                kind,
            },
            at,
        ));
    }
    Ok(out)
}

pub(super) fn free_otp_plus(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    for token in list(&root, "tokens") {
        // Seeds are stored as signed bytes.
        let seed: Vec<u8> = list(token, "secret")
            .iter()
            .filter_map(Value::as_i64)
            .map(|b| b as u8)
            .collect();
        let label = text(token, "label");
        let Some(kind) = kind_of(
            &text(token, "type"),
            number(token, "period", 30),
            number(token, "counter", 0),
        ) else {
            out.skipped.push(format!(
                "{label}: FreeOTP+ {} codes are not supported",
                text(token, "type")
            ));
            continue;
        };
        out.entries.push(otp_entry(
            OtpConfig {
                secret: Secret::from(base32_encode(&seed)),
                issuer: text(token, "issuerExt"),
                account: label,
                algorithm: algorithm(&text(token, "algo")),
                digits: u32::try_from(number(token, "digits", 6)).unwrap_or(6),
                kind,
            },
            at,
        ));
    }
    Ok(out)
}

pub(super) fn raivo(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    for item in root.as_array().map_or(&[][..], Vec::as_slice) {
        let account = text(item, "account");
        let Some(kind) = kind_of(
            &text(item, "kind"),
            number(item, "timer", 30),
            number(item, "counter", 0),
        ) else {
            out.skipped.push(format!(
                "{account}: Raivo {} codes are not supported",
                text(item, "kind")
            ));
            continue;
        };
        out.entries.push(otp_entry(
            OtpConfig {
                secret: Secret::from(text(item, "secret")),
                issuer: text(item, "issuer"),
                account,
                algorithm: algorithm(&text(item, "algorithm")),
                digits: u32::try_from(number(item, "digits", 6)).unwrap_or(6),
                kind,
            },
            at,
        ));
    }
    Ok(out)
}

/// Authenticator Pro, now Stratum: an unencrypted backup. Types are numbered (1 HOTP,
/// 2 TOTP, 3 mOTP, 4 Steam, 5 Yandex) and so are hashes (0 SHA-1, 1 SHA-256,
/// 2 SHA-512). Categories become folders.
pub(super) fn stratum(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let categories: std::collections::HashMap<String, String> = list(&root, "Categories")
        .iter()
        .map(|c| (text(c, "Id"), text(c, "Name")))
        .collect();
    let mut folder_of: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for link in list(&root, "AuthenticatorCategories") {
        if let Some(name) = categories.get(&text(link, "CategoryId")) {
            folder_of
                .entry(text(link, "AuthenticatorSecret"))
                .or_insert_with(|| name.clone());
        }
    }
    let mut out = Imported::default();
    for item in list(&root, "Authenticators") {
        let issuer = text(item, "Issuer");
        let kind = match number(item, "Type", 2) {
            1 => OtpKind::Hotp {
                counter: number(item, "Counter", 0),
            },
            2 => OtpKind::Totp {
                period: u32::try_from(number(item, "Period", 30)).unwrap_or(30),
            },
            4 => OtpKind::Steam,
            other => {
                let name = if other == 3 { "mOTP" } else { "Yandex" };
                out.skipped
                    .push(format!("{issuer}: {name} codes are not supported"));
                continue;
            }
        };
        let secret = text(item, "Secret");
        let mut entry = otp_entry(
            OtpConfig {
                secret: Secret::from(secret.clone()),
                issuer,
                account: text(item, "Username"),
                algorithm: match number(item, "Algorithm", 0) {
                    1 => OtpAlgorithm::Sha256,
                    2 => OtpAlgorithm::Sha512,
                    _ => OtpAlgorithm::Sha1,
                },
                digits: u32::try_from(number(item, "Digits", 6)).unwrap_or(6),
                kind,
            },
            at,
        );
        entry.folder = folder_of.get(&secret).cloned();
        out.entries.push(entry);
    }
    Ok(out)
}

const PROTON_AAD: &[u8] = b"proton.authenticator.export.v1";

/// Proton Authenticator: `{"version":1,"entries":[{"content":{"uri",…},"note"}]}`, or
/// the same JSON sealed under a password: Argon2id (19 MiB, 2 passes, 1 lane) over the
/// password and salt gives an AES-256-GCM key, the content is a 12-byte nonce then the
/// ciphertext, and the associated data names the format.
pub(super) fn proton(json: &str, password: Option<&str>, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    if root.get("entries").is_none() && root.get("content").is_some() {
        let password = password.filter(|p| !p.is_empty()).ok_or_else(|| {
            Error::Import("this Proton Authenticator export is protected by a password".into())
        })?;
        let plain = open_proton(&root, password)?;
        let inner = std::str::from_utf8(&plain)
            .map_err(|_| Error::Import("the export could not be read".into()))?;
        return proton(inner, None, at);
    }
    let mut out = Imported::default();
    for entry in list(&root, "entries") {
        let content = entry.get("content").cloned().unwrap_or(Value::Null);
        let uri = text(&content, "uri");
        let name = text(&content, "name");
        match OtpConfig::from_uri(&uri) {
            Ok(mut config) => {
                if config.issuer.is_empty() && config.account.is_empty() {
                    config.issuer = name;
                }
                let mut item = otp_entry(config, at);
                item.entry.common.notes = Lww::new(Secret::from(text(entry, "note")), at);
                out.entries.push(item);
            }
            Err(_) => out.skipped.push(format!(
                "{name}: its {} code could not be read",
                text(&content, "entry_type")
            )),
        }
    }
    Ok(out)
}

fn open_proton(root: &Value, password: &str) -> Result<Zeroizing<Vec<u8>>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let bad = || Error::Import("the export could not be read".into());
    let b64 = |key: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(text(root, key))
            .map_err(|_| bad())
    };
    let salt = b64("salt")?;
    let sealed = b64("content")?;
    if salt.len() != 16 || sealed.len() <= 12 {
        return Err(bad());
    }
    let params = argon2::Params::new(19 * 1024, 2, 1, Some(32)).map_err(|_| bad())?;
    let mut key = Zeroizing::new([0u8; 32]);
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &salt, &mut key[..])
        .map_err(|_| bad())?;
    let (nonce, body) = sealed.split_at(12);
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key[..]).map_err(|_| bad())?;
    let nonce = aes_gcm::Nonce::try_from(nonce).map_err(|_| bad())?;
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: body,
                aad: PROTON_AAD,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Import("that password does not open this export".into()))
}

/// LastPass Authenticator's JSON: `{"accounts":[{"issuerName","userName","secret",…}]}`.
pub(super) fn lastpass(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    let mut out = Imported::default();
    for account in list(&root, "accounts") {
        let issuer = match text(account, "issuerName") {
            name if name.is_empty() => text(account, "originalIssuerName"),
            name => name,
        };
        let user = match text(account, "userName") {
            name if name.is_empty() => text(account, "originalUserName"),
            name => name,
        };
        out.entries.push(otp_entry(
            OtpConfig {
                secret: Secret::from(text(account, "secret").replace(' ', "").to_uppercase()),
                issuer,
                account: user,
                algorithm: algorithm(&text(account, "algorithm")),
                digits: u32::try_from(number(account, "digits", 6)).unwrap_or(6),
                kind: OtpKind::Totp {
                    period: u32::try_from(number(account, "timeStep", 30)).unwrap_or(30),
                },
            },
            at,
        ));
    }
    Ok(out)
}

/// One link per line, as Ente Auth and others export.
pub(super) fn uri_list(text_body: &str, at: Hlc) -> Result<Imported> {
    let mut out = Imported::default();
    for line in text_body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with("otpauth-migration://") {
            for config in google_migration(line)? {
                out.entries.push(otp_entry(config, at));
            }
            continue;
        }
        match OtpConfig::from_uri(line) {
            Ok(config) => out.entries.push(otp_entry(config, at)),
            Err(_) => out
                .skipped
                .push("a line that was not a one-time code link".to_owned()),
        }
    }
    Ok(out)
}

fn varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// One protobuf field: its number and its value as bytes or an integer.
enum Pb<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

fn fields(bytes: &[u8]) -> Option<Vec<(u64, Pb<'_>)>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let tag = varint(bytes, &mut at)?;
        match tag & 7 {
            0 => out.push((tag >> 3, Pb::Int(varint(bytes, &mut at)?))),
            2 => {
                let len = usize::try_from(varint(bytes, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                out.push((tag >> 3, Pb::Bytes(bytes.get(at..end)?)));
                at = end;
            }
            1 => at = at.checked_add(8)?,
            5 => at = at.checked_add(4)?,
            _ => return None,
        }
    }
    Some(out)
}

/// Decodes Google Authenticator's `otpauth-migration://offline?data=…` export.
pub fn google_migration(link: &str) -> Result<Vec<OtpConfig>> {
    let bad = || Error::Import("that migration code could not be read".to_string());
    let (_, query) = link.split_once('?').ok_or_else(bad)?;
    let data = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("data="))
        .ok_or_else(bad)?;
    let data = percent_decode(&data.replace('+', "%2B")).replace(' ', "+");
    let payload = base64::engine::general_purpose::STANDARD
        .decode(
            data.trim_end_matches('=').to_owned()
                + &"=".repeat((4 - data.trim_end_matches('=').len() % 4) % 4),
        )
        .map_err(|_| bad())?;
    let mut out = Vec::new();
    for (number, value) in fields(&payload).ok_or_else(bad)? {
        let (1, Pb::Bytes(item)) = (number, value) else {
            continue;
        };
        let mut config = OtpConfig {
            digits: 6,
            ..OtpConfig::default()
        };
        let mut counter = 0;
        let mut hotp = false;
        for (field, value) in fields(item).ok_or_else(bad)? {
            match (field, value) {
                (1, Pb::Bytes(seed)) => config.secret = Secret::from(base32_encode(seed)),
                (2, Pb::Bytes(name)) => config.account = String::from_utf8_lossy(name).into_owned(),
                (3, Pb::Bytes(issuer)) => {
                    config.issuer = String::from_utf8_lossy(issuer).into_owned()
                }
                (4, Pb::Int(algo)) => {
                    config.algorithm = match algo {
                        2 => OtpAlgorithm::Sha256,
                        3 => OtpAlgorithm::Sha512,
                        _ => OtpAlgorithm::Sha1,
                    };
                }
                (5, Pb::Int(digits)) => config.digits = if digits == 2 { 8 } else { 6 },
                (6, Pb::Int(kind)) => hotp = kind == 1,
                (7, Pb::Int(value)) => counter = value,
                _ => {}
            }
        }
        if hotp {
            config.kind = OtpKind::Hotp { counter };
        }
        out.push(config);
    }
    Ok(out)
}
