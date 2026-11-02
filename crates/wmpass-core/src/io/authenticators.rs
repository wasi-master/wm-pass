//! Authenticator app exports, and Google Authenticator's migration link.

use base64::Engine;
use serde_json::Value;

use super::json_formats::{list, parse, text};
use super::{Imported, otp_entry};
use crate::clock::Hlc;
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
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(fallback)
}

fn kind_of(name: &str, period: u64, counter: u64) -> Option<OtpKind> {
    match name.to_ascii_lowercase().as_str() {
        "totp" => Some(OtpKind::Totp { period: u32::try_from(period).unwrap_or(30) }),
        "hotp" => Some(OtpKind::Hotp { counter }),
        "steam" => Some(OtpKind::Steam),
        _ => None,
    }
}

pub(super) fn aegis(json: &str, at: Hlc) -> Result<Imported> {
    let root = parse(json)?;
    if root.get("db").is_some_and(Value::is_string) {
        return Err(Error::Import("this Aegis export is encrypted. Export it again without encryption".into()));
    }
    let db = root.get("db").ok_or_else(|| Error::Import("no entries in the file".into()))?;
    let mut out = Imported::default();
    for entry in list(db, "entries") {
        let info = entry.get("info").cloned().unwrap_or(Value::Null);
        let name = text(entry, "name");
        let Some(kind) = kind_of(&text(entry, "type"), number(&info, "period", 30), number(&info, "counter", 0)) else {
            out.skipped.push(format!("{name}: Aegis {} codes are not supported", text(entry, "type")));
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
        return Err(Error::Import("this 2FAS export is encrypted. Export it again without a password".into()));
    }
    let mut out = Imported::default();
    for service in list(&root, "services") {
        let otp = service.get("otp").cloned().unwrap_or(Value::Null);
        let name = text(service, "name");
        let token_type = text(&otp, "tokenType");
        let token_type = if token_type.is_empty() { "TOTP".to_owned() } else { token_type };
        let Some(kind) = kind_of(&token_type, number(&otp, "period", 30), number(&otp, "counter", 0)) else {
            out.skipped.push(format!("{name}: 2FAS {token_type} codes are not supported"));
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
        let Some(kind) = kind_of(&text(item, "type"), number(item, "period", 30), number(item, "counter", 0)) else {
            out.skipped.push(format!("{label}: andOTP {} codes are not supported", text(item, "type")));
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
        let seed: Vec<u8> = list(token, "secret").iter().filter_map(Value::as_i64).map(|b| b as u8).collect();
        let label = text(token, "label");
        let Some(kind) = kind_of(&text(token, "type"), number(token, "period", 30), number(token, "counter", 0)) else {
            out.skipped.push(format!("{label}: FreeOTP+ {} codes are not supported", text(token, "type")));
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
        let Some(kind) = kind_of(&text(item, "kind"), number(item, "timer", 30), number(item, "counter", 0)) else {
            out.skipped.push(format!("{account}: Raivo {} codes are not supported", text(item, "kind")));
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
            Err(_) => out.skipped.push("a line that was not a one-time code link".to_owned()),
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
    let data = query.split('&').find_map(|pair| pair.strip_prefix("data=")).ok_or_else(bad)?;
    let data = percent_decode(&data.replace('+', "%2B")).replace(' ', "+");
    let payload = base64::engine::general_purpose::STANDARD.decode(data.trim_end_matches('=').to_owned() + &"=".repeat((4 - data.trim_end_matches('=').len() % 4) % 4)).map_err(|_| bad())?;
    let mut out = Vec::new();
    for (number, value) in fields(&payload).ok_or_else(bad)? {
        let (1, Pb::Bytes(item)) = (number, value) else { continue };
        let mut config = OtpConfig { digits: 6, ..OtpConfig::default() };
        let mut counter = 0;
        let mut hotp = false;
        for (field, value) in fields(item).ok_or_else(bad)? {
            match (field, value) {
                (1, Pb::Bytes(seed)) => config.secret = Secret::from(base32_encode(seed)),
                (2, Pb::Bytes(name)) => config.account = String::from_utf8_lossy(name).into_owned(),
                (3, Pb::Bytes(issuer)) => config.issuer = String::from_utf8_lossy(issuer).into_owned(),
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
