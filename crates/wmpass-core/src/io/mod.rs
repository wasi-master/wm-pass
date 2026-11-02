//! Moving data in and out. Importers turn other apps' files into entries; exports
//! write them back out. Plain exports are unprotected by nature, and the screens in
//! front of them say so.

mod authenticators;
mod csv_formats;
pub mod export;
mod json_formats;
pub mod kdbx;
mod onepux;
mod xml_formats;

use std::collections::{BTreeMap, HashSet};

use crate::clock::Hlc;
use crate::health::entry_domains;
use crate::model::{Entry, Field, FieldKind, Kind, Login, Lww, Otp, Revision, SiteMatcher};
use crate::otp::OtpConfig;
use crate::vault::Vault;
use crate::{Error, Result, Secret};

/// Every format that can be imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Format {
    Kdbx,
    BitwardenJson,
    BitwardenCsv,
    OnePux,
    LastPassCsv,
    ChromeCsv,
    FirefoxCsv,
    KeePassXcCsv,
    DashlaneCsv,
    EnpassJson,
    NordPassCsv,
    KeeperJson,
    ProtonPass,
    AppleCsv,
    SafeInCloudXml,
    RoboFormCsv,
    WmPassCsv,
    Aegis,
    TwoFas,
    AndOtp,
    FreeOtpPlus,
    Raivo,
    OtpAuthList,
}

impl Format {
    /// Name for screens.
    pub fn name(self) -> &'static str {
        match self {
            Self::Kdbx => "KeePass database",
            Self::BitwardenJson => "Bitwarden JSON",
            Self::BitwardenCsv => "Bitwarden CSV",
            Self::OnePux => "1Password (1PUX)",
            Self::LastPassCsv => "LastPass CSV",
            Self::ChromeCsv => "Chrome CSV",
            Self::FirefoxCsv => "Firefox CSV",
            Self::KeePassXcCsv => "KeePassXC CSV",
            Self::DashlaneCsv => "Dashlane CSV",
            Self::EnpassJson => "Enpass JSON",
            Self::NordPassCsv => "NordPass CSV",
            Self::KeeperJson => "Keeper JSON",
            Self::ProtonPass => "Proton Pass",
            Self::AppleCsv => "Apple Passwords CSV",
            Self::SafeInCloudXml => "SafeInCloud XML",
            Self::RoboFormCsv => "RoboForm CSV",
            Self::WmPassCsv => "WM Pass CSV",
            Self::Aegis => "Aegis",
            Self::TwoFas => "2FAS",
            Self::AndOtp => "andOTP",
            Self::FreeOtpPlus => "FreeOTP+",
            Self::Raivo => "Raivo",
            Self::OtpAuthList => "List of otpauth links",
        }
    }
}

/// One imported item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedEntry {
    /// The entry, id empty.
    pub entry: Entry,
    /// Folder path as the source named it.
    pub folder: Option<String>,
    /// Attachments to seal: file name and bytes.
    pub files: Vec<(String, Vec<u8>)>,
}

/// A whole imported file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    /// The items.
    pub entries: Vec<ImportedEntry>,
    /// Things that could not be brought across, in words, so nothing is lost silently.
    pub skipped: Vec<String>,
}

/// What [`apply`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// New entries.
    pub added: usize,
    /// Left out because an identical login already exists.
    pub duplicates: usize,
    /// Entries with attachments whose blobs to write: blob name and sealed bytes.
    pub blobs: Vec<(String, Vec<u8>)>,
}

/// Fields common to importers.
#[derive(Default)]
pub(crate) struct Draft {
    pub title: String,
    pub username: String,
    pub password: String,
    pub urls: Vec<String>,
    pub notes: String,
    pub otp: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub fields: Vec<(String, String, bool)>,
    pub favourite: bool,
    pub history: Vec<(String, u64)>,
}

impl Draft {
    pub fn into_login(self, at: Hlc, skipped: &mut Vec<String>) -> ImportedEntry {
        let mut matchers: Vec<SiteMatcher> = Vec::new();
        for url in self.urls.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
            if url.starts_with("androidapp://") {
                matchers.push(SiteMatcher::AndroidApp {
                    package: url.trim_start_matches("androidapp://").to_owned(),
                    cert_sha256: None,
                });
            } else {
                matchers.push(SiteMatcher::Url(url.to_owned()));
            }
        }
        let otp = parse_otp(&self.otp, &self.title, &self.username, skipped);
        let login = Login {
            site_matchers: Lww::new(matchers, at),
            username: Lww::new(self.username, at),
            password: Lww::new(Secret::from(self.password), at),
            otp: Lww::new(otp, at),
            ..Login::default()
        };
        let mut entry = base_entry(&self.title, &self.notes, at);
        entry.kind = Kind::Login(login);
        entry.common.tags = Lww::new(self.tags, at);
        entry.common.favourite = Lww::new(self.favourite, at);
        entry.common.custom_fields = Lww::new(fields(self.fields), at);
        entry.history = self
            .history
            .into_iter()
            .map(|(value, ms)| Revision {
                field: "password".to_owned(),
                value: Secret::from(value),
                at: Hlc {
                    ms,
                    ..Hlc::default()
                },
            })
            .collect();
        ImportedEntry {
            entry,
            folder: non_empty(self.folder),
            files: Vec::new(),
        }
    }
}

pub(crate) fn non_empty(text: String) -> Option<String> {
    (!text.trim().is_empty()).then_some(text)
}

pub(crate) fn fields(list: Vec<(String, String, bool)>) -> Vec<Field> {
    list.into_iter()
        .filter(|(_, value, _)| !value.is_empty())
        .map(|(name, value, hidden)| Field {
            name,
            value: Secret::from(value),
            kind: if hidden {
                FieldKind::Hidden
            } else {
                FieldKind::Text
            },
        })
        .collect()
}

pub(crate) fn base_entry(title: &str, notes: &str, at: Hlc) -> Entry {
    let mut entry = Entry::default();
    entry.common.title = Lww::new(title.to_owned(), at);
    entry.common.notes = Lww::new(Secret::new(notes), at);
    entry
}

pub(crate) fn otp_entry(config: OtpConfig, at: Hlc) -> ImportedEntry {
    let title = if config.issuer.is_empty() {
        config.account.clone()
    } else {
        config.issuer.clone()
    };
    let mut entry = base_entry(&title, "", at);
    entry.kind = Kind::Otp(Otp {
        config: Lww::new(Some(config), at),
    });
    ImportedEntry {
        entry,
        ..ImportedEntry::default()
    }
}

/// Reads a code from a link or a bare base32 seed.
pub(crate) fn parse_otp(
    text: &str,
    title: &str,
    account: &str,
    skipped: &mut Vec<String>,
) -> Option<OtpConfig> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.starts_with("otpauth://") || text.starts_with("steam://") {
        return match OtpConfig::from_uri(text) {
            Ok(config) => Some(config),
            Err(_) => {
                skipped.push(format!("{title}: its one-time code link could not be read"));
                None
            }
        };
    }
    if crate::otp::base32_decode(text).is_ok_and(|bytes| !bytes.is_empty()) {
        let mut config = OtpConfig::totp(text.replace([' ', '-'], "").to_uppercase());
        config.issuer = title.to_owned();
        config.account = account.to_owned();
        return Some(config);
    }
    skipped.push(format!("{title}: its one-time code could not be read"));
    None
}

/// Guesses a file's format from its contents.
pub fn detect(data: &[u8]) -> Option<Format> {
    if data.starts_with(&[0x03, 0xd9, 0xa2, 0x9a]) {
        return Some(Format::Kdbx);
    }
    if data.starts_with(b"PK\x03\x04") {
        return onepux::detect_zip(data);
    }
    let text = std::str::from_utf8(data)
        .ok()?
        .trim_start_matches('\u{feff}')
        .trim();
    if text.starts_with('<') {
        return text.contains("<database").then_some(Format::SafeInCloudXml);
    }
    if text.starts_with('{') || text.starts_with('[') {
        return json_formats::detect(text);
    }
    if text.lines().next()?.trim_start().starts_with("otpauth") {
        return Some(Format::OtpAuthList);
    }
    csv_formats::detect(text)
}

/// Reads a file. `at` stamps everything, so imported data loses to newer edits.
pub fn import(format: Format, data: &[u8], password: Option<&str>, at: Hlc) -> Result<Imported> {
    match format {
        Format::Kdbx => {
            let password =
                password.ok_or_else(|| Error::Import("this database needs its password".into()))?;
            kdbx::import(data, password, None, at)
        }
        Format::OnePux => onepux::import_1pux(data, at),
        Format::ProtonPass if data.starts_with(b"PK\x03\x04") => {
            onepux::import_proton_zip(data, at)
        }
        _ => {
            let text = std::str::from_utf8(data)
                .map_err(|_| Error::Import("the file is not text".into()))?
                .trim_start_matches('\u{feff}');
            match format {
                Format::BitwardenJson => json_formats::bitwarden(text, at),
                Format::EnpassJson => json_formats::enpass(text, at),
                Format::KeeperJson => json_formats::keeper(text, at),
                Format::ProtonPass => json_formats::proton(text, at),
                Format::SafeInCloudXml => xml_formats::safeincloud(text, at),
                Format::Aegis => authenticators::aegis(text, at),
                Format::TwoFas => authenticators::two_fas(text, at),
                Format::AndOtp => authenticators::and_otp(text, at),
                Format::FreeOtpPlus => authenticators::free_otp_plus(text, at),
                Format::Raivo => authenticators::raivo(text, at),
                Format::OtpAuthList => authenticators::uri_list(text, at),
                _ => csv_formats::import(format, text, at),
            }
        }
    }
}

/// Maps arbitrary CSV columns chosen in the import screen onto login fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnMap {
    /// Title column.
    pub title: Option<usize>,
    /// Username column.
    pub username: Option<usize>,
    /// Password column.
    pub password: Option<usize>,
    /// Address column.
    pub url: Option<usize>,
    /// Notes column.
    pub notes: Option<usize>,
    /// One-time code column.
    pub otp: Option<usize>,
    /// Folder column.
    pub folder: Option<usize>,
}

/// Imports any CSV through a mapping. The first row is the header when `has_header`.
pub fn import_mapped_csv(
    text: &str,
    map: &ColumnMap,
    has_header: bool,
    at: Hlc,
) -> Result<Imported> {
    csv_formats::mapped(text, map, has_header, at)
}

/// The first rows of a CSV, for the mapping screen.
pub fn preview_csv(text: &str, rows: usize) -> Result<Vec<Vec<String>>> {
    csv_formats::preview(text, rows)
}

fn identity_key(entry: &Entry) -> Option<(String, String, [u8; 32])> {
    let login = entry.login()?;
    if login.password.v.is_empty() {
        return None;
    }
    let site = entry_domains(entry)
        .first()
        .cloned()
        .unwrap_or_else(|| entry.common.title.v.to_lowercase());
    Some((
        site,
        login.username.v.to_lowercase(),
        *blake3::hash(login.password.v.expose().as_bytes()).as_bytes(),
    ))
}

/// Adds imported items to a vault, creating folders by path and leaving out
/// logins identical to one already there.
pub fn apply(vault: &mut Vault, imported: Imported, now_ms: u64) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();
    let mut seen: HashSet<_> = vault
        .doc
        .entries
        .values()
        .filter_map(identity_key)
        .collect();
    let mut folders: BTreeMap<String, String> = vault
        .doc
        .folders
        .values()
        .map(|f| (f.name.v.clone(), f.id.clone()))
        .collect();
    for item in imported.entries {
        let mut entry = item.entry;
        if let Some(key) = identity_key(&entry)
            && !seen.insert(key)
        {
            report.duplicates += 1;
            continue;
        }
        if let Some(path) = item.folder {
            let id = match folders.get(&path) {
                Some(id) => id.clone(),
                None => {
                    let id = vault.add_folder(&path, None, now_ms)?;
                    folders.insert(path, id.clone());
                    id
                }
            };
            let at = entry.common.title.at;
            entry.common.folder = Lww::new(Some(id), at);
        }
        for (name, bytes) in item.files {
            let (record, sealed) =
                Vault::seal_attachment(&name, "application/octet-stream", &bytes)?;
            report.blobs.push((record.blob.clone(), sealed));
            let at = entry.common.title.at;
            let mut list = entry.common.attachments.v.clone();
            list.push(record);
            entry.common.attachments = Lww::new(list, at);
        }
        vault.add(entry, now_ms)?;
        report.added += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
