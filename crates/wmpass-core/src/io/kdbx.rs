//! KeePass databases, both ways. Reading and writing go through the `keepass` crate,
//! whose KDBX 4 writer is labelled experimental. So every write here is read back and
//! compared before it is handed over, and a mismatch is an error rather than a bad file.
//!
//! Reading covers every format KeePass apps have written since 2006: KeePass 1's KDB,
//! KDBX 3.0 and 3.1, and KDBX 4.0 and 4.1, with any of their ciphers and key derivations,
//! and any mix of a password, a key file and a YubiKey (KeePassXC's "YubiKey
//! challenge-response"). Writing is always KDBX 4.
//!
//! A YubiKey takes part through its answer rather than the device: [`challenge`] reads
//! what the file asks of it, the app's own hardware layer gets the answer (over USB or
//! NFC, wherever the key is), and the answer comes back in [`Credentials`]. A database
//! opened in place keeps that same challenge when it is saved, so the answer stays good
//! and saving needs no second touch.

use std::collections::HashMap;

use keepass::config::{DatabaseConfig, DatabaseVersion, KdfConfig};
use keepass::db::{GroupId, GroupRef, Times, Value, fields as kp};
use keepass::error::{DatabaseKeyError, DatabaseOpenError};
use keepass::{Database, DatabaseKey};
use sha2::{Digest, Sha256};

use super::{Imported, ImportedEntry, base_entry};
use crate::clock::Hlc;
use crate::model::{Entry, Kind, Login, Lww, Revision, SiteMatcher};
use crate::otp::{OtpAlgorithm, OtpConfig, OtpKind, base32_decode, base32_encode};
use crate::{Error, Result, Secret};

/// Fields with a place of their own on a login. A one-time code is read separately,
/// and kept as a field when it cannot be.
const STANDARD: [&str; 5] = [kp::TITLE, kp::USERNAME, kp::PASSWORD, kp::URL, kp::NOTES];

/// Every field a KeePass app keeps a one-time code in.
const OTP_FIELDS: [&str; 15] = [
    kp::OTP,
    "TimeOtp-Secret-Base32",
    "TimeOtp-Secret-Hex",
    "TimeOtp-Secret-Base64",
    "TimeOtp-Secret",
    "TimeOtp-Length",
    "TimeOtp-Period",
    "TimeOtp-Algorithm",
    "HmacOtp-Secret-Base32",
    "HmacOtp-Secret-Hex",
    "HmacOtp-Secret-Base64",
    "HmacOtp-Secret",
    "HmacOtp-Counter",
    "TOTP Seed",
    "TOTP Settings",
];

/// How the typed password becomes part of the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Typed {
    /// Hashed as UTF-8, as KeePass 2, KeePassXC and every KDBX app do.
    Utf8,
    /// Left out. An empty field means "no password" in KeePassXC and KeePassDX, but an
    /// empty password in KeePass 2, and the two make different keys.
    Absent,
    /// Hashed as Windows-1252, as KeePass 1 did for passwords outside ASCII.
    Ansi,
}

/// Windows-1252 bytes for a password, when every character has one.
fn windows_1252(text: &str) -> Option<Vec<u8>> {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    text.chars()
        .map(|ch| match u32::from(ch) {
            0..=0x7f | 0xa0..=0xff => u8::try_from(u32::from(ch)).ok(),
            _ => HIGH
                .iter()
                .position(|&c| c == ch)
                .and_then(|i| u8::try_from(0x80 + i).ok()),
        })
        .collect()
}

/// Everything a KeePass database can be locked with, in any mix.
#[derive(Clone, Copy, Default)]
pub struct Credentials<'a> {
    /// The password, empty for none.
    pub password: &'a str,
    /// The key file's bytes, as they are on disk.
    pub key_file: Option<&'a [u8]>,
    /// A YubiKey's HMAC-SHA1 answer to this file's [`challenge`].
    pub response: Option<&'a [u8]>,
}

impl std::fmt::Debug for Credentials<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("password", &!self.password.is_empty())
            .field("key_file", &self.key_file.is_some())
            .field("response", &self.response.is_some())
            .finish()
    }
}

/// One field of a KDBX outer header.
fn header_field(data: &[u8], wanted: u8) -> Option<&[u8]> {
    let major = u16::from_le_bytes(data.get(10..12)?.try_into().ok()?);
    let mut at = 12;
    loop {
        let id = *data.get(at)?;
        let (size, start) = if major >= 4 {
            let size = u32::from_le_bytes(data.get(at + 1..at + 5)?.try_into().ok()?);
            (usize::try_from(size).ok()?, at + 5)
        } else {
            let size = u16::from_le_bytes(data.get(at + 1..at + 3)?.try_into().ok()?);
            (usize::from(size), at + 3)
        };
        let value = data.get(start..start.checked_add(size)?)?;
        if id == wanted {
            return Some(value);
        }
        if id == 0 {
            return None;
        }
        at = start + size;
    }
}

/// The seed in a KDBX 4 header's key derivation settings, a variant dictionary.
fn kdf_seed(parameters: &[u8]) -> Option<&[u8]> {
    let mut at = 2;
    loop {
        let kind = *parameters.get(at)?;
        if kind == 0 {
            return None;
        }
        let length = |from: usize| -> Option<usize> {
            usize::try_from(u32::from_le_bytes(
                parameters.get(from..from + 4)?.try_into().ok()?,
            ))
            .ok()
        };
        let name_length = length(at + 1)?;
        let name = parameters.get(at + 5..at + 5 + name_length)?;
        let value_at = at + 5 + name_length;
        let value_length = length(value_at)?;
        let value = parameters.get(value_at + 4..value_at + 4 + value_length)?;
        if name == b"S" {
            return Some(value);
        }
        at = value_at + 4 + value_length;
    }
}

/// What a YubiKey is asked for this file, as KeePassXC asks it: the key derivation's
/// seed in KDBX 4, the master seed in KDBX 3. Hand it to the key's HMAC-SHA1 slot and
/// pass the answer as [`Credentials::response`]. Every save writes a new seed, so ask
/// again for each version of the file.
pub fn challenge(data: &[u8]) -> Result<[u8; 32]> {
    let unsupported = || Error::Import("this database cannot be opened with a security key".into());
    let found = match DatabaseVersion::parse(data) {
        Ok(DatabaseVersion::KDB3(_)) => header_field(data, 4),
        Ok(DatabaseVersion::KDB4(_)) => header_field(data, 11).and_then(kdf_seed),
        Ok(_) => return Err(unsupported()),
        Err(_) => return Err(Error::Import("this is not a KeePass database".into())),
    };
    found
        .and_then(|seed| seed.try_into().ok())
        .ok_or_else(unsupported)
}

fn key(credentials: &Credentials<'_>, typed: Typed) -> Result<DatabaseKey> {
    let Credentials {
        password,
        key_file,
        response,
    } = *credentials;
    let mut built = DatabaseKey::new();
    match typed {
        Typed::Utf8 => built = built.with_password(password),
        Typed::Absent => {}
        Typed::Ansi => {
            // A 32-byte key file is taken as is, so it stands in for the hash of the
            // password's bytes, which is all the key ever holds of a password.
            let bytes = windows_1252(password)
                .ok_or_else(|| Error::Import("the password could not be encoded".into()))?;
            let hashed = Sha256::digest(&bytes).to_vec();
            built = built
                .with_keyfile(&mut hashed.as_slice())
                .map_err(|_| Error::Import("the key file could not be read".into()))?;
        }
    }
    if let Some(mut bytes) = key_file {
        built = built
            .with_keyfile(&mut bytes)
            .map_err(|_| Error::Import("the key file could not be read".into()))?;
    }
    if let Some(response) = response {
        built = built.with_challenge_response_answer(response);
    }
    Ok(built)
}

/// The keys to try, most likely first. Each try runs the database's key derivation, so
/// a second one is only made where apps genuinely disagree.
fn attempts(credentials: &Credentials<'_>, kdb: bool) -> Vec<Typed> {
    let Credentials {
        password, key_file, ..
    } = *credentials;
    if password.is_empty() {
        if key_file.is_some() || credentials.response.is_some() {
            vec![Typed::Absent, Typed::Utf8]
        } else {
            vec![Typed::Utf8]
        }
    } else if kdb && key_file.is_none() && !password.is_ascii() && windows_1252(password).is_some()
    {
        vec![Typed::Utf8, Typed::Ansi]
    } else {
        vec![Typed::Utf8]
    }
}

/// Whether the file is KeePass 1's format.
fn is_kdb(data: &[u8]) -> bool {
    matches!(DatabaseVersion::parse(data), Ok(DatabaseVersion::KDB(_)))
}

/// Opens a database with whichever form of the key it was made with.
fn open(data: &[u8], credentials: &Credentials<'_>) -> Result<(Database, Typed)> {
    let version = DatabaseVersion::parse(data)
        .map_err(|_| Error::Import("this is not a KeePass database".into()))?;
    if matches!(version, DatabaseVersion::KDB2(_)) {
        return Err(Error::Import(
            "this database is from a test release of KeePass 2 that came before 2.0. \
             Open it in KeePass 2 and save it, then try again"
                .into(),
        ));
    }
    let kdb = matches!(version, DatabaseVersion::KDB(_));
    if kdb && credentials.response.is_some() {
        return Err(Error::Import(
            "KeePass 1 databases cannot be locked with a security key. Open it without one".into(),
        ));
    }
    for typed in attempts(credentials, kdb) {
        match Database::parse(data, key(credentials, typed)?) {
            Ok(database) => return Ok((database, typed)),
            // KDBX 3 decrypts before it can check the key, so a wrong one usually shows
            // as bad padding.
            Err(
                DatabaseOpenError::Key(DatabaseKeyError::IncorrectKey)
                | DatabaseOpenError::Cryptography(_),
            ) => {}
            Err(DatabaseOpenError::Key(_)) => {
                return Err(Error::Import("the key file could not be read".into()));
            }
            // KeePass 1 checks the key only after decrypting everything, so a wrong one
            // can surface as a broken file.
            Err(_) if kdb => {}
            Err(_) => {
                return Err(Error::Import(
                    "this KeePass database could not be read. It may be damaged or cut short"
                        .into(),
                ));
            }
        }
    }
    Err(Error::Import(
        match (
            credentials.key_file.is_some(),
            credentials.response.is_some(),
        ) {
            (false, false) => "wrong password, or this database also needs its key file",
            (true, false) => "wrong password or key file",
            (false, true) => {
                "wrong password or security key, or this database also needs its key file. \
                 Check that the key's slot is the one the database was set up with"
            }
            (true, true) => {
                "wrong password, key file or security key. Check that the key's slot is the \
                 one the database was set up with"
            }
        }
        .into(),
    ))
}

fn hex(text: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !clean.len().is_multiple_of(2) {
        return None;
    }
    clean
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

/// A seed in the encoding a field names, as base32.
fn seed(text: &str, encoding: &str) -> Option<String> {
    let bytes = match encoding {
        "hex" => hex(text)?,
        "base64" => super::xml_formats::base64_body(text)?,
        "utf8" | "text" => text.as_bytes().to_vec(),
        _ => base32_decode(text).ok()?.to_vec(),
    };
    (!bytes.is_empty()).then(|| base32_encode(&bytes))
}

fn hash(name: &str) -> OtpAlgorithm {
    let name = name.to_ascii_lowercase();
    if name.contains("256") {
        OtpAlgorithm::Sha256
    } else if name.contains("512") {
        OtpAlgorithm::Sha512
    } else {
        OtpAlgorithm::Sha1
    }
}

/// Whether two configurations make the same codes, whatever they are labelled.
fn same_codes(a: &OtpConfig, b: &OtpConfig) -> bool {
    let seed = |c: &OtpConfig| base32_decode(c.secret.expose()).map(|s| s.to_vec()).ok();
    seed(a) == seed(b) && a.kind == b.kind && a.digits == b.digits && a.algorithm == b.algorithm
}

/// KeePass 2.47's own `TimeOtp-*` and `HmacOtp-*` fields, and KeeTrayTOTP's
/// `TOTP Seed` with `TOTP Settings`. `Err` when they are there but unreadable.
fn native_otp(
    get: &dyn Fn(&str) -> Option<String>,
    title: &str,
) -> Option<std::result::Result<(OtpConfig, Vec<&'static str>), ()>> {
    let mut config = OtpConfig::totp(String::new());
    config.issuer = title.to_owned();
    let number = |key: &str, fallback: u64| {
        get(key)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(fallback)
    };
    for (prefix, hotp) in [("TimeOtp", false), ("HmacOtp", true)] {
        let found = [
            ("-Secret-Base32", "base32"),
            ("-Secret-Hex", "hex"),
            ("-Secret-Base64", "base64"),
            ("-Secret", "utf8"),
        ]
        .into_iter()
        .find_map(|(suffix, encoding)| get(&format!("{prefix}{suffix}")).map(|v| (v, encoding)));
        let Some((value, encoding)) = found else {
            continue;
        };
        let Some(secret) = seed(&value, encoding) else {
            return Some(Err(()));
        };
        config.secret = Secret::from(secret);
        if hotp {
            config.kind = OtpKind::Hotp {
                counter: number("HmacOtp-Counter", 0),
            };
        } else {
            config.digits = u32::try_from(number("TimeOtp-Length", 6)).unwrap_or(6);
            config.kind = OtpKind::Totp {
                period: u32::try_from(number("TimeOtp-Period", 30)).unwrap_or(30),
            };
            config.algorithm = hash(&get("TimeOtp-Algorithm").unwrap_or_default());
        }
        let used = OTP_FIELDS
            .iter()
            .copied()
            .filter(|name| name.starts_with(prefix))
            .collect();
        return Some(Ok((config, used)));
    }
    let value = get("TOTP Seed")?;
    let Some(secret) = seed(&value, "base32") else {
        return Some(Err(()));
    };
    config.secret = Secret::from(secret);
    // "30;6", or "30;S" for Steam.
    let settings = get("TOTP Settings").unwrap_or_default();
    let (period, size) = settings.split_once(';').unwrap_or(("30", "6"));
    config.kind = if size.trim().eq_ignore_ascii_case("s") {
        OtpKind::Steam
    } else {
        config.digits = size.trim().parse().unwrap_or(6);
        OtpKind::Totp {
            period: period.trim().parse().unwrap_or(30),
        }
    };
    Some(Ok((config, vec!["TOTP Seed", "TOTP Settings"])))
}

/// The `otp` field: an `otpauth://` link (KeePassXC, KeePassDX) or KeeOtp's
/// `key=…&step=…`.
fn otp_link(value: &str, title: &str) -> Option<OtpConfig> {
    let value = value.trim();
    if value.get(..10).is_some_and(|head| head.eq_ignore_ascii_case("otpauth://")) {
        return OtpConfig::from_uri(value).ok();
    }
    if !value.contains("key=") {
        return None;
    }
    let params: Vec<(&str, &str)> = value
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect();
    let param = |name: &str| {
        params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| crate::otp::percent_decode(v))
    };
    let encoding = param("encoding").unwrap_or_default().to_ascii_lowercase();
    let secret = param("key").and_then(|k| seed(&k, &encoding))?;
    let mut config = OtpConfig::totp(secret);
    config.issuer = title.to_owned();
    config.digits = param("size").and_then(|v| v.parse().ok()).unwrap_or(6);
    config.algorithm = hash(&param("otpHashMode").unwrap_or_default());
    config.kind = if param("type").is_some_and(|t| t.eq_ignore_ascii_case("hotp")) {
        OtpKind::Hotp {
            counter: param("counter").and_then(|v| v.parse().ok()).unwrap_or(0),
        }
    } else {
        OtpKind::Totp {
            period: param("step").and_then(|v| v.parse().ok()).unwrap_or(30),
        }
    };
    Some(config)
}

/// An entry's one-time code, in each form KeePass apps keep one: an `otp` link
/// (KeePassXC, KeePassDX), KeeOtp's `key=…&step=…` in that same field, KeePass 2.47's
/// own `TimeOtp-*` and `HmacOtp-*` fields, and KeeTrayTOTP's `TOTP Seed` with
/// `TOTP Settings`. Also returns the fields it read, which the caller then leaves out
/// of the custom fields; fields that hold the same code in another app's form count as
/// read too. A value it cannot read is left for the caller to keep as a field, so
/// nothing is lost.
pub(super) fn otp_from_fields(
    get: &dyn Fn(&str) -> Option<String>,
    title: &str,
    skipped: &mut Vec<String>,
) -> (Option<OtpConfig>, Vec<&'static str>) {
    let unreadable = |skipped: &mut Vec<String>| {
        skipped.push(format!(
            "{title}: its one-time code could not be read, so it is kept as a field"
        ));
    };
    let native = native_otp(get, title);
    if let Some(value) = get(kp::OTP) {
        if let Some(config) = otp_link(&value, title) {
            let mut used = vec![kp::OTP];
            if let Some(Ok((other, names))) = &native
                && same_codes(&config, other)
            {
                used.extend(names);
            }
            return (Some(config), used);
        }
        return match native {
            Some(Ok((config, used))) => (Some(config), used),
            _ => {
                unreadable(skipped);
                (None, Vec::new())
            }
        };
    }
    match native {
        Some(Ok((config, used))) => (Some(config), used),
        Some(Err(())) => {
            unreadable(skipped);
            (None, Vec::new())
        }
        None => (None, Vec::new()),
    }
}

/// A code as fields: the `otp` link KeePassXC, KeePassDX and Strongbox read, and
/// KeePass 2's own fields, which is all KeePass 2 reads.
fn otp_fields(otp: &OtpConfig) -> Vec<(&'static str, String)> {
    let mut out = vec![(kp::OTP, otp.to_uri().expose().to_owned())];
    let seed = base32_decode(otp.secret.expose())
        .map(|bytes| base32_encode(&bytes))
        .unwrap_or_default();
    if seed.is_empty() {
        return out;
    }
    match otp.kind {
        OtpKind::Totp { period } => {
            out.push(("TimeOtp-Secret-Base32", seed));
            if otp.digits != 6 {
                out.push(("TimeOtp-Length", otp.digits.to_string()));
            }
            if period != 30 {
                out.push(("TimeOtp-Period", period.to_string()));
            }
            match otp.algorithm {
                OtpAlgorithm::Sha1 => {}
                OtpAlgorithm::Sha256 => out.push(("TimeOtp-Algorithm", "HMAC-SHA-256".into())),
                OtpAlgorithm::Sha512 => out.push(("TimeOtp-Algorithm", "HMAC-SHA-512".into())),
            }
        }
        OtpKind::Hotp { counter } => {
            out.push(("HmacOtp-Secret-Base32", seed));
            out.push(("HmacOtp-Counter", counter.to_string()));
        }
        // KeePass 2 has no Steam codes.
        OtpKind::Steam => {}
    }
    out
}

fn set_otp(target: &mut keepass::db::Entry, otp: Option<&OtpConfig>) {
    for name in OTP_FIELDS {
        target.fields.remove(name);
    }
    if let Some(otp) = otp {
        for (name, value) in otp_fields(otp) {
            let protected = !name.ends_with("Length")
                && !name.ends_with("Period")
                && !name.ends_with("Algorithm")
                && !name.ends_with("Counter");
            target.set(
                name,
                if protected {
                    Value::protected(value)
                } else {
                    Value::unprotected(value)
                },
            );
        }
    }
}

/// Follows KeePass field references such as `{REF:P@I:46C9B1FF…}`, which stand for a
/// field of another entry. Shared passwords are often kept this way, and the code
/// itself must never be filled into a site.
fn resolve(database: &Database, value: &str, depth: u8) -> String {
    const OPEN: &str = "{REF:";
    if depth > 8 || !value.to_ascii_uppercase().contains(OPEN) {
        return value.to_owned();
    }
    let upper = value.to_ascii_uppercase();
    let mut out = String::new();
    let mut at = 0;
    while let Some(found) = upper[at..].find(OPEN) {
        let start = at + found;
        out.push_str(&value[at..start]);
        let Some(length) = value[start..].find('}') else {
            at = start;
            break;
        };
        let end = start + length;
        match referenced(database, &value[start + OPEN.len()..end]) {
            Some(text) => out.push_str(&resolve(database, &text, depth + 1)),
            None => out.push_str(&value[start..=end]),
        }
        at = end + 1;
    }
    out.push_str(&value[at..]);
    out
}

/// One reference's target: `P@I:<uuid>`, `U@T:<title text>` and the like.
fn referenced(database: &Database, token: &str) -> Option<String> {
    let mut chars = token.chars();
    let wanted = chars.next()?.to_ascii_uppercase();
    if chars.next()? != '@' {
        return None;
    }
    let within = chars.next()?.to_ascii_uppercase();
    let text = chars.as_str().strip_prefix(':')?;
    let field = |code: char| match code {
        'T' => Some(kp::TITLE),
        'U' => Some(kp::USERNAME),
        'P' => Some(kp::PASSWORD),
        'A' => Some(kp::URL),
        'N' => Some(kp::NOTES),
        _ => None,
    };
    let lower = text.to_lowercase();
    let wanted_id = text.replace(['-', '{', '}'], "");
    let found = database.iter_all_entries().find(|entry| match within {
        'I' => entry
            .id()
            .uuid()
            .simple()
            .to_string()
            .eq_ignore_ascii_case(wanted_id.trim()),
        'O' => entry
            .fields
            .iter()
            .filter(|(name, _)| !STANDARD.contains(&name.as_str()))
            .any(|(_, value)| value.get().to_lowercase().contains(&lower)),
        code => field(code)
            .and_then(|name| entry.get(name))
            .is_some_and(|value| value.to_lowercase().contains(&lower)),
    })?;
    if wanted == 'I' {
        return Some(found.id().uuid().simple().to_string().to_ascii_uppercase());
    }
    Some(found.get(field(wanted)?).unwrap_or_default().to_owned())
}

/// KeePass 1 keeps its own settings in hidden entries like this one.
fn is_meta_stream(entry: &keepass::db::Entry) -> bool {
    entry.get(kp::TITLE) == Some("Meta-Info")
        && entry.get(kp::USERNAME) == Some("SYSTEM")
        && entry.get(kp::URL) == Some("$")
}

fn walk(
    database: &Database,
    group: GroupRef<'_>,
    path: &str,
    at: Hlc,
    skip: &[GroupId],
    out: &mut Imported,
) {
    if skip.contains(&group.id()) {
        return;
    }
    for item in group.entries() {
        if is_meta_stream(&item) {
            continue;
        }
        let read = |name: &str| item.get(name).map(|value| resolve(database, value, 0));
        let get = |name: &str| read(name).unwrap_or_default();
        let title = get(kp::TITLE);
        let mut entry = base_entry(&title, &get(kp::NOTES), at);
        let mut matchers = Vec::new();
        let url = get(kp::URL);
        if !url.trim().is_empty() {
            matchers.push(crate::matching::matcher_from_address(&url));
        }
        let (otp, used) = otp_from_fields(&read, &title, &mut out.skipped);
        let all: Vec<(String, String)> =
            item.fields.iter().map(|(name, value)| (name.clone(), resolve(database, value.get(), 0))).collect();
        let (urls, passkey, read_too) = keepassxc_extras(&all, &get(kp::USERNAME));
        for url in &urls {
            let matcher = crate::matching::matcher_from_address(url);
            if !matchers.contains(&matcher) {
                matchers.push(matcher);
            }
        }
        entry.kind = Kind::Login(Login {
            site_matchers: Lww::new(matchers, at),
            username: Lww::new(get(kp::USERNAME), at),
            password: Lww::new(Secret::from(get(kp::PASSWORD)), at),
            otp: Lww::new(otp, at),
            passkeys: Lww::new(passkey.into_iter().collect(), at),
            ..Login::default()
        });
        entry.common.tags = Lww::new(item.tags.clone(), at);
        entry.created_ms = item
            .times
            .creation
            .map_or(0, |time| u64::try_from(time.and_utc().timestamp_millis()).unwrap_or(0));
        let mut custom = Vec::new();
        for (name, value) in &item.fields {
            let read_as_code = used.contains(&name.as_str()) || read_too.contains(name);
            if !STANDARD.contains(&name.as_str()) && !read_as_code {
                custom.push((
                    name.clone(),
                    resolve(database, value.get(), 0),
                    value.is_protected(),
                ));
            }
        }
        custom.sort();
        entry.common.custom_fields = Lww::new(super::fields(custom), at);
        if let Some(history) = &item.history {
            for old in history.get_entries() {
                let value = old.get_password().unwrap_or_default();
                if !value.is_empty() {
                    let ms = old.times.last_modification.map_or(0, |time| {
                        u64::try_from(time.and_utc().timestamp_millis()).unwrap_or(0)
                    });
                    let revision = Revision {
                        field: "password".to_owned(),
                        value: Secret::new(value),
                        at: Hlc {
                            ms,
                            ..Hlc::default()
                        },
                    };
                    entry.history.push(revision);
                }
            }
            entry.history.sort_by_key(|r| r.at);
            // KeePass keeps a copy of the whole entry on every edit, so most copies hold the
            // password unchanged: only a change of password is an earlier value.
            entry.history.dedup_by(|later, earlier| later.value == earlier.value);
            let current = entry.login().map(|login| login.password.v.clone());
            if entry.history.last().map(|r| &r.value) == current.as_ref() {
                entry.history.pop();
            }
        }
        // KeePass 1 writes an empty, nameless attachment on every entry, and keeps its
        // own settings in a "bin-stream" one.
        let files = item
            .attachments_named()
            .filter(|(name, attachment)| !name.is_empty() || !attachment.data.get().is_empty())
            .map(|(name, attachment)| (name.to_owned(), attachment.data.get().clone()))
            .collect();
        out.entries.push(ImportedEntry {
            entry,
            folder: super::non_empty(path.to_owned()),
            files,
        });
    }
    for child in group.groups() {
        let name = child.name.clone();
        let child_path = if path.is_empty() {
            name
        } else {
            format!("{path}/{name}")
        };
        walk(database, child, &child_path, at, skip, out);
    }
}

/// Groups whose entries are not live: the recycle bin, and KeePass 1's "Backup" group,
/// which holds old copies of edited entries.
fn not_live(database: &Database, out: &mut Imported) -> Vec<GroupId> {
    let mut skip: Vec<GroupId> = database.recycle_bin().map(|g| g.id()).into_iter().collect();
    if matches!(database.config.version, DatabaseVersion::KDB(_))
        && let Some(backup) = database.root().groups().find(|g| g.name == "Backup")
    {
        let count = backup.entries().count();
        if count > 0 {
            out.skipped.push(format!(
                "{count} old copies in KeePass 1's Backup group were left out"
            ));
        }
        skip.push(backup.id());
    }
    skip
}

/// Reads a database.
pub fn import(data: &[u8], password: &str, key_file: Option<&[u8]>, at: Hlc) -> Result<Imported> {
    let credentials = Credentials {
        password,
        key_file,
        response: None,
    };
    import_with(data, &credentials, at)
}

/// Reads a database locked with any mix of a password, a key file and a YubiKey.
pub fn import_with(data: &[u8], credentials: &Credentials<'_>, at: Hlc) -> Result<Imported> {
    let (database, _) = open(data, credentials)?;
    let mut out = Imported::default();
    let skip = not_live(&database, &mut out);
    walk(&database, database.root(), "", at, &skip, &mut out);
    Ok(out)
}

/// Field names a KeePass entry uses for itself, which a field of the person's own must not
/// take: it would be written over, or write over the title or password.
fn reserved(name: &str) -> bool {
    STANDARD.contains(&name)
        || OTP_FIELDS.contains(&name)
        || name == KIND_FIELD
        || name.starts_with(EXTRA_URL)
        || name.starts_with(PASSKEY)
}

/// A name for a field that no other field on the entry has, `Name (2)` when taken.
fn unique_name(taken: &mut std::collections::HashSet<String>, name: &str) -> String {
    let name = if name.trim().is_empty() { "Field" } else { name };
    let mut candidate = name.to_owned();
    let mut n = 2;
    while reserved(&candidate) || taken.contains(&candidate) {
        candidate = format!("{name} ({n})");
        n += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

fn set_common(target: &mut keepass::db::EntryMut<'_>, entry: &Entry, taken: &mut std::collections::HashSet<String>) {
    target.set_unprotected(kp::TITLE, entry.common.title.v.as_str());
    let notes = entry.common.notes.v.expose();
    if !notes.is_empty() {
        target.set_unprotected(kp::NOTES, notes);
    }
    target.tags.clone_from(&entry.common.tags.v);
    for field in &entry.common.custom_fields.v {
        let name = unique_name(taken, &field.name);
        if field.kind == crate::model::FieldKind::Hidden {
            target.set_protected(name, field.value.expose());
        } else {
            target.set_unprotected(name, field.value.expose());
        }
    }
}

/// A login's earlier passwords as KeePass keeps them: whole past versions of the entry in its
/// `<History>`, oldest first, each with the password it then had and when that was written.
fn set_history(target: &mut keepass::db::EntryMut<'_>, entry: &Entry) {
    if entry.login().is_none() {
        return;
    }
    let mut old: Vec<&Revision> = entry.history_of("password").collect();
    if old.is_empty() {
        return;
    }
    old.sort_by_key(|r| r.at);
    let base: keepass::db::Entry = (**target).clone();
    let mut history = keepass::db::History::default();
    // Each version added goes in front, so the newest goes in first.
    for revision in old.iter().rev() {
        let mut version = base.clone();
        version.set_protected(kp::PASSWORD, revision.value.expose());
        if let Some(time) = i64::try_from(revision.at.ms)
            .ok()
            .filter(|ms| *ms > 0)
            .and_then(chrono::DateTime::from_timestamp_millis)
        {
            version.times.last_modification = Some(time.naive_utc());
        }
        history.add_entry(version);
    }
    target.history = Some(history);
}

/// The field naming what kind of WM Pass entry a non-login was.
const KIND_FIELD: &str = "WM Pass kind";
/// KeePassXC and KeePass2Android keep more addresses in `KP2A_URL`, `KP2A_URL_1`, and so on.
const EXTRA_URL: &str = "KP2A_URL";
/// KeePassXC keeps a passkey in fields named so.
const PASSKEY: &str = "KPEX_PASSKEY_";

/// A login's addresses as KeePass writes them: an Android app as KeePass2Android names one.
fn addresses(login: &Login) -> Vec<String> {
    login
        .site_matchers
        .v
        .iter()
        .filter_map(|m| match m {
            SiteMatcher::Url(url) | SiteMatcher::Domain(url) | SiteMatcher::Host(url) => Some(url.clone()),
            SiteMatcher::AndroidApp { package, .. } => Some(format!("androidapp://{package}")),
            _ => None,
        })
        .collect()
}

/// A passkey as KeePassXC keeps one, its key as PEM. KeePassXC holds one per entry.
fn passkey_fields(passkey: &crate::model::Passkey) -> Option<Vec<(String, String, bool)>> {
    use base64::Engine;
    let der = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(passkey.private_key.expose()).ok()?;
    let body = base64::engine::general_purpose::STANDARD.encode(&der);
    let lines: Vec<&str> = body.as_bytes().chunks(64).filter_map(|c| std::str::from_utf8(c).ok()).collect();
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----", lines.join("\n"));
    Some(vec![
        (format!("{PASSKEY}CREDENTIAL_ID"), passkey.credential_id.clone(), true),
        (format!("{PASSKEY}PRIVATE_KEY_PEM"), pem, true),
        (format!("{PASSKEY}RELYING_PARTY"), passkey.rp_id.clone(), false),
        (format!("{PASSKEY}USERNAME"), passkey.user_name.clone(), false),
        (format!("{PASSKEY}USER_HANDLE"), passkey.user_handle.clone(), true),
    ])
}

/// What KeePassXC keeps in fields of its own on an entry: more addresses, and a passkey.
/// Also returns the names of the fields read, which are then no custom fields.
pub(super) fn keepassxc_extras(
    fields: &[(String, String)],
    username: &str,
) -> (Vec<String>, Option<crate::model::Passkey>, Vec<String>) {
    let mut extra: Vec<&(String, String)> = fields.iter().filter(|(name, _)| name.starts_with(EXTRA_URL)).collect();
    // `KP2A_URL` before `KP2A_URL_1`, `KP2A_URL_2` before `KP2A_URL_10`.
    extra.sort_by_key(|(name, _)| {
        let n: u64 = name.trim_start_matches(EXTRA_URL).trim_start_matches('_').parse().unwrap_or(0);
        (n, name.clone())
    });
    let mut used: Vec<String> = extra.iter().map(|(name, _)| name.clone()).collect();
    let urls = extra.into_iter().map(|(_, value)| value.clone()).filter(|v| !v.trim().is_empty()).collect();
    let get = |suffix: &str| fields.iter().find(|(name, _)| *name == format!("{PASSKEY}{suffix}")).map(|(_, v)| v.clone());
    let passkey = get("PRIVATE_KEY_PEM").and_then(|pem| {
        use base64::Engine;
        let body: String = pem.lines().filter(|line| !line.trim_start().starts_with("-----")).collect();
        let der = super::xml_formats::base64_body(&body)?;
        // The key's own type, by the object identifier in it: EC (P-256), Ed25519 or RSA.
        let has = |oid: &[u8]| der.windows(oid.len()).any(|w| w == oid);
        let algorithm = if has(&[0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01]) {
            -7
        } else if has(&[0x06, 0x03, 0x2b, 0x65, 0x70]) {
            -8
        } else {
            -257
        };
        Some(crate::model::Passkey {
            credential_id: get("CREDENTIAL_ID").unwrap_or_default(),
            rp_id: get("RELYING_PARTY").unwrap_or_default(),
            rp_name: get("RELYING_PARTY").unwrap_or_default(),
            user_handle: get("USER_HANDLE").unwrap_or_default(),
            user_name: get("USERNAME").unwrap_or_else(|| username.to_owned()),
            private_key: Secret::from(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(der)),
            algorithm,
            ..crate::model::Passkey::default()
        })
    });
    if passkey.is_some() {
        used.extend(fields.iter().filter(|(name, _)| name.starts_with(PASSKEY)).map(|(name, _)| name.clone()));
    }
    (urls, passkey, used)
}

fn first_url(login: &Login) -> Option<&str> {
    login.site_matchers.v.iter().find_map(|m| match m {
        SiteMatcher::Url(url) | SiteMatcher::Domain(url) | SiteMatcher::Host(url) => {
            Some(url.as_str())
        }
        _ => None,
    })
}

/// The group for a `A/B/C` folder path, made as needed.
fn group_for(database: &mut Database, made: &mut HashMap<String, GroupId>, path: &str) -> GroupId {
    let mut parent = database.root().id();
    let mut so_far = String::new();
    for part in path.split('/').map(str::trim).filter(|p| !p.is_empty()) {
        if !so_far.is_empty() {
            so_far.push('/');
        }
        so_far.push_str(part);
        parent = match made.get(&so_far) {
            Some(id) => *id,
            None => {
                let Some(mut group) = database.group_mut(parent) else {
                    break;
                };
                let mut child = group.add_group();
                child.name = part.to_owned();
                let id = child.id();
                made.insert(so_far.clone(), id);
                id
            }
        };
    }
    parent
}

/// Writes entries as a KDBX 4 database under a password, then reads the result back
/// and refuses to return it unless every login survived intact.
pub fn export(entries: &[&Entry], password: &str) -> Result<Vec<u8>> {
    let placed: Vec<(&Entry, String)> = entries.iter().map(|e| (*e, String::new())).collect();
    export_in_folders(&placed, password)
}

/// Like [`export`], with each entry in a folder path such as `Work/Mail`, which
/// becomes nested KeePass groups.
pub fn export_in_folders(entries: &[(&Entry, String)], password: &str) -> Result<Vec<u8>> {
    if password.is_empty() {
        return Err(Error::Invalid("the export needs a password".into()));
    }
    let mut database = Database::with_config(strong_config());
    let mut made = HashMap::new();
    for (entry, folder) in entries {
        let group = group_for(&mut database, &mut made, folder);
        let mut parent = database.group_mut(group).ok_or(Error::Integrity)?;
        let mut target = parent.add_entry();
        let mut taken = std::collections::HashSet::new();
        match &entry.kind {
            Kind::Login(login) => {
                target.set_unprotected(kp::USERNAME, login.username.v.as_str());
                target.set_protected(kp::PASSWORD, login.password.v.expose());
                let mut urls = addresses(login).into_iter();
                if let Some(url) = urls.next() {
                    target.set_unprotected(kp::URL, url);
                }
                for (n, url) in urls.enumerate() {
                    target.set_unprotected(format!("{EXTRA_URL}_{}", n + 1), url);
                }
                if let Some(fields) = login.passkeys.v.first().and_then(passkey_fields) {
                    for (name, value, hidden) in fields {
                        if hidden {
                            target.set_protected(name, value);
                        } else {
                            target.set_unprotected(name, value);
                        }
                    }
                }
                set_otp(&mut target, login.otp.v.as_ref());
            }
            other => {
                for (name, value, hidden) in super::export::kind_fields(other) {
                    let name = unique_name(&mut taken, name);
                    if hidden {
                        target.set_protected(name, value.expose());
                    } else {
                        target.set_unprotected(name, value.expose());
                    }
                }
                target.set_unprotected(KIND_FIELD, other.name());
            }
        }
        set_common(&mut target, entry, &mut taken);
        set_history(&mut target, entry);
    }
    let mut out = Vec::new();
    database
        .save(&mut out, key(&by_password(password), Typed::Utf8)?)
        .map_err(|_| Error::Encoding)?;
    verify(&out, password, entries)?;
    Ok(out)
}

fn by_password(password: &str) -> Credentials<'_> {
    Credentials {
        password,
        ..Credentials::default()
    }
}

fn strong_config() -> DatabaseConfig {
    let mut config = DatabaseConfig::default();
    config.kdf_config = KdfConfig::Argon2id {
        iterations: 3,
        memory: 128 * 1024 * 1024,
        parallelism: 4,
        version: argon2_for_keepass::Version::Version13,
    };
    config
}

fn verify(bytes: &[u8], password: &str, entries: &[(&Entry, String)]) -> Result<()> {
    let back = Database::parse(bytes, key(&by_password(password), Typed::Utf8)?)
        .map_err(|_| Error::Integrity)?;
    let mut found: Vec<(String, String, String)> = back
        .iter_all_entries()
        .map(|e| {
            (
                e.get(kp::TITLE).unwrap_or_default().to_owned(),
                e.get(kp::USERNAME).unwrap_or_default().to_owned(),
                e.get(kp::PASSWORD).unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let mut wanted: Vec<(String, String, String)> = entries
        .iter()
        .map(|(entry, _)| match entry.login() {
            Some(login) => (
                entry.common.title.v.clone(),
                login.username.v.clone(),
                login.password.v.expose().to_owned(),
            ),
            None => (entry.common.title.v.clone(), String::new(), String::new()),
        })
        .collect();
    found.sort();
    wanted.sort();
    if found == wanted {
        Ok(())
    } else {
        Err(Error::Integrity)
    }
}

/// Whether two versions of an entry hold the same, as far as a file can tell.
fn same_entry(a: &keepass::db::EntryRef<'_>, b: &keepass::db::EntryRef<'_>) -> bool {
    let (x, y): (&keepass::db::Entry, &keepass::db::Entry) = (a, b);
    let files = |e: &keepass::db::EntryRef<'_>| -> Vec<(String, Vec<u8>)> {
        let mut out: Vec<(String, Vec<u8>)> = e
            .attachments_named()
            .map(|(name, file)| (name.to_owned(), file.data.get().clone()))
            .collect();
        out.sort();
        out
    };
    let versions =
        |e: &keepass::db::EntryRef<'_>| e.history.as_ref().map_or(0, |h| h.get_entries().len());
    x.fields == y.fields
        && x.tags == y.tags
        && x.times == y.times
        && x.autotype == y.autotype
        && x.custom_data == y.custom_data
        && x.foreground_color == y.foreground_color
        && x.background_color == y.background_color
        && x.override_url == y.override_url
        && x.quality_check == y.quality_check
        && a.icon() == b.icon()
        && files(a) == files(b)
        && versions(a) == versions(b)
        && (0..versions(a)).all(|i| match (a.historical(i), b.historical(i)) {
            (Some(p), Some(q)) => same_entry(&p, &q),
            _ => false,
        })
}

/// Whether a database read back from its file holds everything the one in memory does.
/// The `keepass` crate keeps a parent group on each past version of an entry, which a
/// file does not store, so after a move the two differ there and nowhere else.
fn same_database(back: &Database, mine: &Database) -> bool {
    let icons = |d: &Database| {
        let mut out: Vec<(String, Vec<u8>)> = d
            .iter_all_custom_icons()
            .map(|icon| (icon.id().uuid().to_string(), icon.data.clone()))
            .collect();
        out.sort();
        out
    };
    back.config == mine.config
        && back.meta == mine.meta
        && back.deleted_objects == mine.deleted_objects
        && back.root().id() == mine.root().id()
        && icons(back) == icons(mine)
        && back.iter_all_groups().count() == mine.iter_all_groups().count()
        && mine
            .iter_all_groups()
            .all(|g| back.group(g.id()).is_some_and(|b| *b == *g))
        && back.iter_all_entries().count() == mine.iter_all_entries().count()
        && mine.iter_all_entries().all(|e| {
            back.entry(e.id()).is_some_and(|b| {
                b.parent().id() == e.parent().id()
                    && b.previous_parent().map(|g| g.id()) == e.previous_parent().map(|g| g.id())
                    && same_entry(&b, &e)
            })
        })
}

/// Opens a `.kdbx` in place. Edits go back into the same database, so everything
/// WM Pass does not model (groups, icons, auto-type, custom data, field references,
/// other fields) is kept.
pub struct InPlace {
    database: Database,
    typed: Typed,
    upgraded: bool,
    /// What the YubiKey answered, when one is part of the key. Saves keep it as the KDF
    /// seed, so the answer the app holds stays the right one.
    challenge: Option<[u8; 32]>,
}

impl std::fmt::Debug for InPlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InPlace(***)")
    }
}

/// One entry as the in-place mode shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InPlaceEntry {
    /// The database's id for it.
    pub id: String,
    /// Group path.
    pub group: String,
    /// Title.
    pub title: String,
    /// Username.
    pub username: String,
    /// Address.
    pub url: String,
}

/// What a login looks like through WM Pass, read from one KeePass entry.
struct Shown {
    title: String,
    username: String,
    password: String,
    url: String,
    notes: String,
    otp: Option<OtpConfig>,
}

impl InPlace {
    /// Opens a database.
    pub fn open(data: &[u8], password: &str, key_file: Option<&[u8]>) -> Result<Self> {
        let credentials = Credentials {
            password,
            key_file,
            response: None,
        };
        Self::open_with(data, &credentials)
    }

    /// Opens a database locked with any mix of a password, a key file and a YubiKey.
    pub fn open_with(data: &[u8], credentials: &Credentials<'_>) -> Result<Self> {
        if is_kdb(data) {
            // Saving would put KDBX 4 into a file KeePass 1 then cannot open.
            return Err(Error::Import(
                "this is a KeePass 1 database (.kdb), which can be imported but not edited \
                 in place. Import it, or convert it to .kdbx in KeePassXC or KeePass 2 first"
                    .into(),
            ));
        }
        let (mut database, typed) = open(data, credentials)?;
        let challenge = credentials.response.map(|_| challenge(data)).transpose()?;
        // KDBX 3 can be read but not written. The next save moves the file to KDBX 4
        // with Argon2id, which is the stronger format, and the app says so first.
        let upgraded = !matches!(database.config.version, DatabaseVersion::KDB4(_));
        if upgraded {
            database.config = strong_config();
        } else {
            // The writer only produces the current KDBX 4 minor version. KeePassXC
            // writes 4.0 when it can, and every 4.x reader opens the newer minor.
            database.config.version = DatabaseConfig::default().version;
        }
        Ok(Self {
            database,
            typed,
            upgraded,
            challenge,
        })
    }

    /// Whether saving will change this file from an older format to KDBX 4. Older
    /// KeePass apps may not open it afterwards, so ask before the first save.
    pub fn will_upgrade(&self) -> bool {
        self.upgraded
    }

    fn resolved(&self, value: &str) -> String {
        resolve(&self.database, value, 0)
    }

    /// Lists every entry outside the recycle bin.
    pub fn list(&self) -> Vec<InPlaceEntry> {
        fn visit(
            owner: &InPlace,
            group: GroupRef<'_>,
            path: &str,
            recycle: Option<GroupId>,
            out: &mut Vec<InPlaceEntry>,
        ) {
            if recycle == Some(group.id()) {
                return;
            }
            for entry in group.entries() {
                let get = |name: &str| owner.resolved(entry.get(name).unwrap_or_default());
                out.push(InPlaceEntry {
                    id: entry.id().uuid().to_string(),
                    group: path.to_owned(),
                    title: get(kp::TITLE),
                    username: get(kp::USERNAME),
                    url: get(kp::URL),
                });
            }
            for child in group.groups() {
                let name = child.name.clone();
                let next = if path.is_empty() {
                    name
                } else {
                    format!("{path}/{name}")
                };
                visit(owner, child, &next, recycle, out);
            }
        }
        let mut out = Vec::new();
        let recycle = self.database.recycle_bin().map(|group| group.id());
        visit(self, self.database.root(), "", recycle, &mut out);
        out
    }

    fn find_id(&self, id: &str) -> Option<keepass::db::EntryId> {
        self.database
            .iter_all_entries()
            .find(|e| e.id().uuid().to_string() == id)
            .map(|e| e.id())
    }

    fn shown(&self, item: &keepass::db::Entry) -> Shown {
        let read = |name: &str| item.get(name).map(|value| self.resolved(value));
        let get = |name: &str| read(name).unwrap_or_default();
        let title = get(kp::TITLE);
        let (otp, _) = otp_from_fields(&read, &title, &mut Vec::new());
        Shown {
            username: get(kp::USERNAME),
            password: get(kp::PASSWORD),
            url: get(kp::URL),
            notes: get(kp::NOTES),
            title,
            otp,
        }
    }

    /// Reads an entry's password, with any field reference followed.
    pub fn password(&self, id: &str) -> Option<Secret> {
        let found = self.find_id(id)?;
        let entry = self.database.entry(found)?;
        entry
            .get(kp::PASSWORD)
            .map(|p| Secret::new(self.resolved(p)))
    }

    /// Changes a password, keeping the previous version in the entry's own history
    /// so KeePassXC sees it the usual way.
    pub fn set_password(&mut self, id: &str, password: &Secret) -> Result<()> {
        let found = self
            .find_id(id)
            .ok_or_else(|| Error::Invalid("no such entry".into()))?;
        let mut entry = self
            .database
            .entry_mut(found)
            .ok_or_else(|| Error::Invalid("no such entry".into()))?;
        entry.edit_tracking(|tracked| tracked.set_protected(kp::PASSWORD, password.expose()));
        Ok(())
    }

    /// Every login as a WM Pass entry, stamped `at`. Ids are the database's own.
    pub fn entries(&self, at: Hlc) -> Vec<Entry> {
        let live: std::collections::HashSet<String> =
            self.list().into_iter().map(|item| item.id).collect();
        self.database
            .iter_all_entries()
            .filter(|item| live.contains(&item.id().uuid().to_string()))
            .map(|item| {
                let shown = self.shown(&item);
                let mut entry = base_entry(&shown.title, &shown.notes, at);
                entry.id = item.id().uuid().to_string();
                let matchers = if shown.url.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![SiteMatcher::Url(shown.url)]
                };
                entry.kind = Kind::Login(Login {
                    site_matchers: Lww::new(matchers, at),
                    username: Lww::new(shown.username, at),
                    password: Lww::new(Secret::from(shown.password), at),
                    otp: Lww::new(shown.otp, at),
                    ..Login::default()
                });
                entry.common.tags = Lww::new(item.tags.clone(), at);
                entry
            })
            .collect()
    }

    /// Writes a login's standard fields, creating it at the top level if it is new,
    /// and returns its id. Only fields that changed are written, so a field reference
    /// stays a reference unless its value was edited, and a save with no change adds
    /// nothing to the entry's history. Anything else on the entry is left alone.
    pub fn upsert(&mut self, entry: &Entry) -> Result<String> {
        let login = entry
            .login()
            .ok_or_else(|| Error::Invalid("a KeePass database holds logins here".into()))?;
        let wanted = Shown {
            title: entry.common.title.v.clone(),
            username: login.username.v.clone(),
            password: login.password.v.expose().to_owned(),
            url: first_url(login).unwrap_or_default().to_owned(),
            notes: entry.common.notes.v.expose().to_owned(),
            otp: login.otp.v.clone(),
        };
        let tags = entry.common.tags.v.clone();
        let existing = self.find_id(&entry.id);
        let (current, current_tags) = match existing.and_then(|id| self.database.entry(id)) {
            Some(item) => (Some(self.shown(&item)), item.tags.clone()),
            None => (None, Vec::new()),
        };
        let mut changes: Vec<(&'static str, String, bool)> = Vec::new();
        let mut change = |name, new: &String, old: Option<&String>, protected| {
            if old != Some(new) {
                changes.push((name, new.clone(), protected));
            }
        };
        let old = current.as_ref();
        change(kp::TITLE, &wanted.title, old.map(|c| &c.title), false);
        change(
            kp::USERNAME,
            &wanted.username,
            old.map(|c| &c.username),
            false,
        );
        change(
            kp::PASSWORD,
            &wanted.password,
            old.map(|c| &c.password),
            true,
        );
        change(kp::URL, &wanted.url, old.map(|c| &c.url), false);
        change(kp::NOTES, &wanted.notes, old.map(|c| &c.notes), false);
        let otp_changed = match (old.and_then(|c| c.otp.as_ref()), wanted.otp.as_ref()) {
            (Some(a), Some(b)) => !same_codes(a, b),
            (None, None) => false,
            _ => true,
        };
        let tags_changed = existing.is_some() && tags != current_tags;
        let apply = |target: &mut keepass::db::Entry| {
            for (name, value, protected) in &changes {
                let value = if *protected {
                    Value::protected(value.clone())
                } else {
                    Value::unprotected(value.clone())
                };
                target.set(*name, value);
            }
            if otp_changed {
                set_otp(target, wanted.otp.as_ref());
            }
            if tags_changed || existing.is_none() {
                target.tags.clone_from(&tags);
            }
            target.times.last_modification = Some(Times::now());
        };
        match existing {
            Some(id) => {
                if changes.is_empty() && !otp_changed && !tags_changed {
                    return Ok(id.uuid().to_string());
                }
                let mut target = self.database.entry_mut(id).ok_or(Error::Integrity)?;
                target.edit_tracking(|tracked| apply(tracked));
                Ok(id.uuid().to_string())
            }
            None => {
                let mut root = self.database.root_mut();
                let mut target = root.add_entry();
                apply(&mut target);
                Ok(target.id().uuid().to_string())
            }
        }
    }

    /// Deletes an entry as KeePass does: into the recycle bin, made if the database
    /// has none yet, unless the database turned its recycle bin off. Then, or when the
    /// entry is already in the bin, it goes for good, recorded so other copies learn of
    /// it when they merge.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let found = self
            .find_id(id)
            .ok_or_else(|| Error::Invalid("no such entry".into()))?;
        let in_bin = self.list().iter().all(|item| item.id != id);
        if self.database.meta.recyclebin_enabled != Some(false) && !in_bin {
            let bin = self.recycle_bin();
            // A move, as KeePass makes it: no history version, the place it came from kept.
            let mut target = self.database.entry_mut(found).ok_or(Error::Integrity)?;
            target.move_to(bin).map_err(|_| Error::Integrity)?;
            target.times.location_changed = Some(Times::now());
            self.database.meta.recyclebin_changed = Some(Times::now());
            return Ok(());
        }
        self.database
            .entry_mut(found)
            .ok_or(Error::Integrity)?
            .track_changes()
            .remove();
        Ok(())
    }

    /// The recycle bin, made at the top level as KeePass makes it.
    fn recycle_bin(&mut self) -> GroupId {
        if let Some(bin) = self.database.recycle_bin() {
            return bin.id();
        }
        let mut root = self.database.root_mut();
        let mut bin = root.add_group();
        bin.name = "Recycle Bin".to_owned();
        bin.set_icon_builtin(43);
        let id = bin.id();
        self.database.meta.recyclebin_uuid = Some(id.uuid());
        self.database.meta.recyclebin_enabled = Some(true);
        id
    }

    /// Writes the database back, reads it back and compares it. Save the result
    /// atomically and keep the previous file as a `.bak`.
    pub fn save(&self, password: &str, key_file: Option<&[u8]>) -> Result<Vec<u8>> {
        let credentials = Credentials {
            password,
            key_file,
            response: None,
        };
        self.save_with(&credentials)
    }

    /// Like [`InPlace::save`], with the credentials it was opened with. With a YubiKey,
    /// the file keeps the challenge it was opened with (a KDBX 3 file's master seed
    /// becomes its KDBX 4 KDF seed), so the same answer opens it in KeePassXC and here.
    /// Each save still gets a new master seed, so its encryption keys are new.
    pub fn save_with(&self, credentials: &Credentials<'_>) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let built = key(credentials, self.typed)?;
        match (self.challenge, credentials.response) {
            (Some(seed), Some(_)) => self.database.save_with_kdf_seed(&mut out, built, &seed),
            (None, None) => self.database.save(&mut out, built),
            _ => {
                return Err(Error::Invalid(
                    "save with the security key's answer the database was opened with".into(),
                ));
            }
        }
        .map_err(|_| Error::Encoding)?;
        let back =
            Database::parse(&out, key(credentials, self.typed)?).map_err(|_| Error::Integrity)?;
        if back != self.database && !same_database(&back, &self.database) {
            return Err(Error::Integrity);
        }
        Ok(out)
    }
}
