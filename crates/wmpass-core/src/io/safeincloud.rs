//! SafeInCloud: its XML export, and its own database file (`SafeInCloud.db`, the file
//! its apps sync to a cloud drive and write as a backup), which its Android app offers
//! where it offers no export.
//!
//! The database layout, from MIT-licensed readers of it (mxschmitt/golang-safe-in-cloud):
//! a little-endian `0x0505` magic, a version byte, then byte-length-prefixed arrays:
//! salt, IV, a second salt, and a key block. PBKDF2-HMAC-SHA1 over the password and
//! salt, 10,000 rounds, gives an AES-256 key that opens the key block in CBC mode; the
//! block holds an inner IV and key, which open the rest of the file, also AES-256-CBC
//! with PKCS#7 padding. The plaintext is zlib-compressed XML, the same as the export.

use std::collections::HashMap;
use std::io::Read;

use aes::cipher::block_padding::{NoPadding, Pkcs7};
use aes::cipher::{BlockModeDecrypt, KeyIvInit};
use zeroize::Zeroizing;

use super::xml_formats::{Node, base64_body, tree};
use super::{Draft, Imported, ImportedEntry, MAX_INFLATED, base_entry, fields};
use crate::clock::Hlc;
use crate::model::{Card, Kind, Lww, Note};
use crate::{Error, Result, Secret};

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

const ROUNDS: u32 = 10_000;

/// Whether bytes look like a SafeInCloud database.
pub(super) fn is_database(data: &[u8]) -> bool {
    data.len() > 4 && data.starts_with(&[0x05, 0x05])
}

fn wrong_password() -> Error {
    Error::Import("that password does not open this SafeInCloud database".into())
}

fn array<'a>(data: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let len = usize::from(*data.get(*at)?);
    let start = *at + 1;
    let end = start.checked_add(len)?;
    let slice = data.get(start..end)?;
    *at = end;
    Some(slice)
}

/// Decrypts a database to its XML.
pub(super) fn decrypt(data: &[u8], password: &str) -> Result<Zeroizing<String>> {
    let malformed = || Error::Import("this SafeInCloud database could not be read".into());
    if !is_database(data) {
        return Err(malformed());
    }
    // Magic (2 bytes) and version (1 byte).
    let mut at = 3;
    let salt = array(data, &mut at).ok_or_else(malformed)?;
    let iv = array(data, &mut at).ok_or_else(malformed)?;
    let _check_salt = array(data, &mut at).ok_or_else(malformed)?;
    let block = array(data, &mut at).ok_or_else(malformed)?;
    let body = &data[at..];
    if iv.len() != 16
        || block.is_empty()
        || !block.len().is_multiple_of(16)
        || !body.len().is_multiple_of(16)
    {
        return Err(malformed());
    }

    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), salt, ROUNDS, &mut key[..]);
    let mut inner = Zeroizing::new(block.to_vec());
    Aes256CbcDec::new_from_slices(&key[..], iv)
        .map_err(|_| malformed())?
        .decrypt_padded::<NoPadding>(&mut inner)
        .map_err(|_| wrong_password())?;
    // A wrong password leaves noise here, which almost never parses as a 16-byte IV
    // followed by a 32-byte key; the padding and zlib checks below catch the rest.
    let mut inner_at = 0;
    let inner_iv = array(&inner, &mut inner_at).ok_or_else(wrong_password)?;
    let inner_key = array(&inner, &mut inner_at).ok_or_else(wrong_password)?;
    if inner_iv.len() != 16 || inner_key.len() != 32 {
        return Err(wrong_password());
    }

    let mut plain = Zeroizing::new(body.to_vec());
    let compressed = Aes256CbcDec::new_from_slices(inner_key, inner_iv)
        .map_err(|_| malformed())?
        .decrypt_padded::<Pkcs7>(&mut plain)
        .map_err(|_| wrong_password())?;
    let mut xml = Zeroizing::new(Vec::new());
    flate2::read::ZlibDecoder::new(compressed)
        .take(MAX_INFLATED)
        .read_to_end(&mut xml)
        .map_err(|_| wrong_password())?;
    let text = String::from_utf8(std::mem::take(&mut *xml)).map_err(|_| malformed())?;
    Ok(Zeroizing::new(text))
}

/// Opens a database with its password and reads it like the export.
pub(super) fn database(data: &[u8], password: Option<&str>, at: Hlc) -> Result<Imported> {
    let password = password
        .filter(|p| !p.is_empty())
        .ok_or_else(|| Error::Import("this SafeInCloud database needs its password".into()))?;
    let xml = decrypt(data, password)?;
    xml_export(&xml, at)
}

/// The XML export, `<database><label …/><card …><field …>…</field></card></database>`.
///
/// Labels become tags, a star marks a favourite, note cards become notes, cards with a
/// card number filled for autofill become payment cards, and attachments come along.
/// A card may hold more than one login or password: the first of each fills the
/// login, and the rest are kept as fields rather than dropped.
pub(super) fn xml_export(xml: &str, at: Hlc) -> Result<Imported> {
    let root = tree(xml)?;
    if root.name != "database" {
        return Err(Error::Import("this is not a SafeInCloud export".into()));
    }
    let labels: HashMap<&str, &str> = root
        .all("label")
        .map(|label| (label.attr("id"), label.attr("name")))
        .collect();
    let mut out = Imported::default();
    for card in root.all("card") {
        if card.attr("template") == "true" || card.attr("deleted") == "true" {
            continue;
        }
        let mut item = read_card(card, at, &mut out.skipped);
        let tags: Vec<String> = card
            .all("label_id")
            .filter_map(|id| labels.get(id.text.trim()))
            .map(|name| (*name).to_owned())
            .collect();
        if !tags.is_empty() {
            item.entry.common.tags = Lww::new(tags, at);
        }
        item.entry.common.favourite = Lww::new(card.attr("star") == "true", at);
        for (index, file) in card.all("file").chain(card.all("image")).enumerate() {
            let name = match file.attr("name") {
                "" => format!("image {}.jpg", index + 1),
                name => name.to_owned(),
            };
            match base64_body(&file.text) {
                Some(bytes) => item.files.push((name, bytes)),
                None => out.skipped.push(format!(
                    "{}: the attachment {name} could not be read",
                    item.entry.common.title.v
                )),
            }
        }
        out.entries.push(item);
    }
    Ok(out)
}

fn read_card(card: &Node, at: Hlc, skipped: &mut Vec<String>) -> ImportedEntry {
    let title = card.attr("title").to_owned();
    let mut notes: Vec<String> = card
        .all("notes")
        .map(|n| n.text.clone())
        .filter(|n| !n.trim().is_empty())
        .collect();
    let filled = |f: &&Node| !f.text.trim().is_empty();
    if card.attr("type") == "note" {
        let extra = card
            .all("field")
            .filter(filled)
            .map(|f| (f.attr("name").to_owned(), f.text.clone(), hidden(f)))
            .collect();
        let mut entry = base_entry(&title, "", at);
        entry.kind = Kind::Note(Note {
            body: Lww::new(Secret::from(notes.join("\n")), at),
        });
        entry.common.custom_fields = Lww::new(fields(extra), at);
        return ImportedEntry {
            entry,
            folder: None,
            files: Vec::new(),
        };
    }
    let is_card = card
        .all("field")
        .any(|f| f.attr("autofill") == "cc-number" && !f.text.trim().is_empty());
    if is_card {
        let mut payment = Card::default();
        let mut extra = Vec::new();
        for field in card.all("field").filter(filled) {
            let value = field.text.clone();
            match (field.attr("autofill"), field.attr("type")) {
                ("cc-number", _) => payment.number = Lww::new(Secret::from(value), at),
                ("cc-name", _) => payment.holder = Lww::new(value, at),
                ("cc-exp", _) => payment.expiry = Lww::new(value, at),
                ("cc-csc", _) => payment.cvv = Lww::new(Secret::from(value), at),
                (_, "pin") if payment.pin.v.is_empty() => {
                    payment.pin = Lww::new(Secret::from(value), at);
                }
                (_, "notes") => notes.push(value),
                _ => extra.push((field.attr("name").to_owned(), value, hidden(field))),
            }
        }
        let mut entry = base_entry(&title, &notes.join("\n"), at);
        entry.kind = Kind::Card(payment);
        entry.common.custom_fields = Lww::new(fields(extra), at);
        return ImportedEntry {
            entry,
            folder: None,
            files: Vec::new(),
        };
    }
    let mut draft = Draft {
        title,
        ..Draft::default()
    };
    for field in card.all("field").filter(filled) {
        let value = field.text.clone();
        let name = field.attr("name").to_owned();
        match field.attr("type") {
            "login" if draft.username.is_empty() => draft.username = value,
            "password" if draft.password.is_empty() => draft.password = value,
            "website" | "weblogin" => draft.urls.push(value),
            "one_time_password" if draft.otp.is_empty() => draft.otp = value,
            "notes" => notes.push(value),
            _ => draft.fields.push((name, value, hidden(field))),
        }
    }
    draft.notes = notes.join("\n");
    draft.into_login(at, skipped)
}

fn hidden(field: &Node) -> bool {
    matches!(
        field.attr("type"),
        "password" | "secret" | "pin" | "one_time_password"
    )
}
