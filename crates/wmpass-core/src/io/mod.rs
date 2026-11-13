//! Moving data in and out. Importers turn other apps' files into entries; exports
//! write them back out. Plain exports are unprotected by nature, and the screens in
//! front of them say so.

mod authenticators;
mod csv_formats;
pub mod export;
mod json_formats;
pub mod kdbx;
mod onepux;
mod safeincloud;
mod text_formats;
mod xml_formats;

use std::collections::{BTreeMap, HashSet};

use crate::clock::Hlc;
use crate::health::entry_domains;
use crate::model::{Entry, Field, FieldKind, Kind, Login, Lww, Otp, Revision, SiteMatcher};
use crate::otp::OtpConfig;
use crate::vault::Vault;
use crate::{Error, Result, Secret};

/// The most a compressed member of an export may grow to when inflated.
pub(crate) const MAX_INFLATED: u64 = 512 * 1024 * 1024;

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
    OnePasswordCsv,
    SafeInCloudXml,
    SafeInCloudDb,
    RoboFormCsv,
    WmPassCsv,
    KeePassXml,
    KasperskyTxt,
    ZohoVaultCsv,
    StickyPasswordXml,
    AvastCsv,
    AvastJson,
    AviraCsv,
    PsonoJson,
    ButtercupCsv,
    MSecureCsv,
    PasswordSafeXml,
    Aegis,
    TwoFas,
    AndOtp,
    FreeOtpPlus,
    Raivo,
    Stratum,
    ProtonAuthenticator,
    LastPassAuthenticator,
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
            Self::OnePasswordCsv => "1Password CSV",
            Self::SafeInCloudXml => "SafeInCloud XML",
            Self::SafeInCloudDb => "SafeInCloud database",
            Self::RoboFormCsv => "RoboForm CSV",
            Self::WmPassCsv => "WM Pass CSV",
            Self::KeePassXml => "KeePass XML",
            Self::KasperskyTxt => "Kaspersky Password Manager",
            Self::ZohoVaultCsv => "Zoho Vault CSV",
            Self::StickyPasswordXml => "Sticky Password XML",
            Self::AvastCsv => "Avast Passwords CSV",
            Self::AvastJson => "Avast Passwords JSON",
            Self::AviraCsv => "Avira Password Manager CSV",
            Self::PsonoJson => "Psono JSON",
            Self::ButtercupCsv => "Buttercup CSV",
            Self::MSecureCsv => "mSecure CSV",
            Self::PasswordSafeXml => "Password Safe XML",
            Self::Aegis => "Aegis",
            Self::TwoFas => "2FAS",
            Self::AndOtp => "andOTP",
            Self::FreeOtpPlus => "FreeOTP+",
            Self::Raivo => "Raivo",
            Self::Stratum => "Authenticator Pro (Stratum)",
            Self::ProtonAuthenticator => "Proton Authenticator",
            Self::LastPassAuthenticator => "LastPass Authenticator",
            Self::OtpAuthList => "List of otpauth links",
        }
    }

    /// Every format, for looking one up by its name.
    pub const ALL: [Self; 39] = [
        Self::Kdbx,
        Self::BitwardenJson,
        Self::BitwardenCsv,
        Self::OnePux,
        Self::LastPassCsv,
        Self::ChromeCsv,
        Self::FirefoxCsv,
        Self::KeePassXcCsv,
        Self::DashlaneCsv,
        Self::EnpassJson,
        Self::NordPassCsv,
        Self::KeeperJson,
        Self::ProtonPass,
        Self::AppleCsv,
        Self::OnePasswordCsv,
        Self::SafeInCloudXml,
        Self::SafeInCloudDb,
        Self::RoboFormCsv,
        Self::WmPassCsv,
        Self::KeePassXml,
        Self::KasperskyTxt,
        Self::ZohoVaultCsv,
        Self::StickyPasswordXml,
        Self::AvastCsv,
        Self::AvastJson,
        Self::AviraCsv,
        Self::PsonoJson,
        Self::ButtercupCsv,
        Self::MSecureCsv,
        Self::PasswordSafeXml,
        Self::Aegis,
        Self::TwoFas,
        Self::AndOtp,
        Self::FreeOtpPlus,
        Self::Raivo,
        Self::Stratum,
        Self::ProtonAuthenticator,
        Self::LastPassAuthenticator,
        Self::OtpAuthList,
    ];
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
    pub fn into_login(mut self, at: Hlc, skipped: &mut Vec<String>) -> ImportedEntry {
        let mut matchers: Vec<SiteMatcher> = Vec::new();
        // LastPass writes `http://` for a login with no address: that is no address, and kept
        // it would read as a site on plain http.
        let bare_scheme = |u: &str| u.split_once("://").is_some_and(|(_, rest)| rest.trim_matches('/').is_empty());
        for url in self.urls.iter().map(|u| u.trim()).filter(|u| !u.is_empty() && !bare_scheme(u)) {
            let matcher = crate::matching::matcher_from_address(url);
            if !matchers.contains(&matcher) {
                matchers.push(matcher);
            }
        }
        // Browsers leave the name empty on some rows, Chrome's app logins among them.
        if self.title.trim().is_empty() {
            self.title = matchers
                .iter()
                .find_map(|m| match m {
                    SiteMatcher::Url(url) => crate::matching::host_of(url),
                    SiteMatcher::AndroidApp { package, .. } => Some(package.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| self.username.clone());
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

/// A note with fields, for items that carry no login.
pub(crate) fn note_item(
    title: &str,
    body: &str,
    extra: Vec<(String, String, bool)>,
    folder: Option<String>,
    at: Hlc,
) -> ImportedEntry {
    let mut entry = base_entry(title, "", at);
    entry.kind = crate::model::Kind::Note(crate::model::Note {
        body: Lww::new(Secret::from(body.to_owned()), at),
    });
    entry.common.custom_fields = Lww::new(fields(extra), at);
    ImportedEntry {
        entry,
        folder,
        files: Vec::new(),
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

/// A one-time code item, when its settings can make codes: a seed that is base32 and not empty,
/// a sensible number of digits and a period. Otherwise `None`, with the reason in `skipped`,
/// rather than an item that only ever shows an error.
pub(crate) fn checked_code(config: OtpConfig, at: Hlc, skipped: &mut Vec<String>) -> Option<ImportedEntry> {
    let name = match (config.issuer.trim(), config.account.trim()) {
        ("", "") => "A code",
        ("", account) => account,
        (issuer, _) => issuer,
    }
    .to_owned();
    let problem = match crate::otp::base32_decode(config.secret.expose()) {
        Err(_) => Some("its setup key is not base32 (the letters A to Z and digits 2 to 7)"),
        Ok(seed) if seed.is_empty() => Some("it has no setup key"),
        Ok(_) if !(1..=10).contains(&config.digits) && config.kind != crate::otp::OtpKind::Steam => {
            Some("its number of digits is not one codes are made with")
        }
        Ok(_) if config.kind == (crate::otp::OtpKind::Totp { period: 0 }) => Some("its period is zero"),
        Ok(_) => None,
    };
    match problem {
        Some(reason) => {
            skipped.push(format!("{name}: left out, as {reason}"));
            None
        }
        None => Some(otp_entry(config, at)),
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
    let lower = text.get(..10).unwrap_or(text).to_ascii_lowercase();
    if lower.starts_with("otpauth://") || lower.starts_with("steam://") {
        // Zoho Vault writes links with no label, `otpauth://totp?secret=…`; the
        // entry's title stands in for it.
        let labelled = ["totp", "hotp"].iter().find_map(|kind| {
            let rest = text.strip_prefix(&format!("otpauth://{kind}?"))?;
            let label = crate::otp::percent_encode(if title.is_empty() { "code" } else { title });
            Some(format!("otpauth://{kind}/{label}?{rest}"))
        });
        let text = labelled.as_deref().unwrap_or(text);
        return match OtpConfig::from_uri(text) {
            Ok(config) => Some(config),
            Err(_) => {
                skipped.push(format!("{title}: its one-time code link could not be read"));
                None
            }
        };
    }
    if crate::otp::base32_decode(text).is_ok_and(|bytes| !bytes.is_empty()) {
        let mut config = OtpConfig::totp(crate::otp::normalize_secret(text));
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
    if safeincloud::is_database(data) {
        return Some(Format::SafeInCloudDb);
    }
    let wide = utf16_text(data);
    let text = match &wide {
        Some(text) => text.as_str(),
        None => std::str::from_utf8(data).ok()?,
    };
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.starts_with('<') {
        return xml_kind(text);
    }
    if text.starts_with('{') || text.starts_with('[') {
        return json_formats::detect(text);
    }
    // A link read from a QR code in its alphanumeric mode comes in capitals.
    if text.lines().next()?.trim_start().get(..7).is_some_and(|head| head.eq_ignore_ascii_case("otpauth")) {
        return Some(Format::OtpAuthList);
    }
    if text_formats::is_kaspersky(text) {
        return Some(Format::KasperskyTxt);
    }
    csv_formats::detect(text)
}

/// Text saved as UTF-16 with its byte-order mark, as Excel's "Unicode Text" and some Windows
/// apps write it. Everything else is read as UTF-8.
fn utf16_text(data: &[u8]) -> Option<zeroize::Zeroizing<String>> {
    let little = match data {
        [0xff, 0xfe, ..] => true,
        [0xfe, 0xff, ..] => false,
        _ => return None,
    };
    let (pairs, rest) = data[2..].as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let units = pairs.iter().map(|pair| if little { u16::from_le_bytes(*pair) } else { u16::from_be_bytes(*pair) });
    let mut text = zeroize::Zeroizing::new(String::with_capacity(pairs.len()));
    for unit in char::decode_utf16(units) {
        text.push(unit.ok()?);
    }
    Some(text)
}

/// Whether a file opens only with its own password: a KeePass or SafeInCloud database,
/// or a password-protected Proton Authenticator export.
pub fn needs_password(data: &[u8]) -> bool {
    match detect(data) {
        Some(Format::Kdbx | Format::SafeInCloudDb) => true,
        Some(Format::ProtonAuthenticator) => std::str::from_utf8(data)
            .ok()
            .and_then(|text| {
                serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')).ok()
            })
            .is_some_and(|root| root.get("entries").is_none() && root.get("content").is_some()),
        _ => false,
    }
}

/// Which XML export this is, by its root element.
fn xml_kind(text: &str) -> Option<Format> {
    let head: String = text.chars().take(4096).collect();
    if head.contains("<KeePassFile") {
        Some(Format::KeePassXml)
    } else if head.contains("<passwordsafe") {
        Some(Format::PasswordSafeXml)
    } else if head.contains("<database") {
        Some(Format::SafeInCloudXml)
    } else if text.contains("<Logins") && text.contains("<Accounts") {
        Some(Format::StickyPasswordXml)
    } else {
        None
    }
}

/// Reads a file. `at` stamps everything, so imported data loses to newer edits.
pub fn import(format: Format, data: &[u8], password: Option<&str>, at: Hlc) -> Result<Imported> {
    import_with_key_file(format, data, password, None, at)
}

/// Like [`import`], with a KeePass key file. A KeePass database with a key file may
/// have no password at all, so then `password` may be empty or absent.
pub fn import_with_key_file(
    format: Format,
    data: &[u8],
    password: Option<&str>,
    key_file: Option<&[u8]>,
    at: Hlc,
) -> Result<Imported> {
    import_with_keys(format, data, password, key_file, None, at)
}

/// Like [`import_with_key_file`], with a YubiKey's answer to [`kdbx::challenge`] for a
/// KeePass database that one is part of the key to.
pub fn import_with_keys(
    format: Format,
    data: &[u8],
    password: Option<&str>,
    key_file: Option<&[u8]>,
    response: Option<&[u8]>,
    at: Hlc,
) -> Result<Imported> {
    if response.is_some() && format != Format::Kdbx {
        return Err(Error::Import(
            "only KeePass databases are opened with a security key".into(),
        ));
    }
    match format {
        Format::Kdbx => {
            let password = match (password, key_file.or(response)) {
                (Some(password), _) => password,
                (None, Some(_)) => "",
                (None, None) => {
                    return Err(Error::Import("this database needs its password".into()));
                }
            };
            let credentials = kdbx::Credentials {
                password,
                key_file,
                response,
            };
            kdbx::import_with(data, &credentials, at)
        }
        Format::SafeInCloudDb => safeincloud::database(data, password, at),
        Format::OnePux => onepux::import_1pux(data, at),
        Format::ProtonPass if data.starts_with(b"PK\x03\x04") => {
            onepux::import_proton_zip(data, at)
        }
        Format::DashlaneCsv if data.starts_with(b"PK\x03\x04") => onepux::import_dashlane_zip(data, at),
        _ => {
            let wide = utf16_text(data);
            let text = match &wide {
                Some(text) => text.as_str(),
                None => std::str::from_utf8(data).map_err(|_| {
                    Error::Import(
                        "this file is not UTF-8 text. Save it again as UTF-8 (in Excel, as CSV UTF-8) and import that"
                            .into(),
                    )
                })?,
            };
            let text = text.trim_start_matches('\u{feff}');
            match format {
                Format::BitwardenJson => json_formats::bitwarden(text, at),
                Format::EnpassJson => json_formats::enpass(text, at),
                Format::KeeperJson => json_formats::keeper(text, at),
                Format::ProtonPass => json_formats::proton(text, at),
                Format::SafeInCloudXml => safeincloud::xml_export(text, at),
                Format::KeePassXml => xml_formats::keepass(text, at),
                Format::StickyPasswordXml => xml_formats::sticky_password(text, at),
                Format::PasswordSafeXml => xml_formats::password_safe(text, at),
                Format::KasperskyTxt => text_formats::kaspersky(text, at),
                Format::AvastJson => json_formats::avast(text, at),
                Format::PsonoJson => json_formats::psono(text, at),
                Format::Stratum => authenticators::stratum(text, at),
                Format::ProtonAuthenticator => authenticators::proton(text, password, at),
                Format::LastPassAuthenticator => authenticators::lastpass(text, at),
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

pub(crate) fn identity_key(entry: &Entry) -> Option<(String, String, [u8; 32])> {
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

/// A one-time code item's seed and settings, as an import compares them: a second item that
/// makes the same codes is the same item. The counter of a HOTP code is left out, since it moves.
fn code_key(entry: &Entry) -> Option<[u8; 32]> {
    let Kind::Otp(otp) = &entry.kind else {
        return None;
    };
    let config = otp.config.v.as_ref()?;
    let seed = zeroize::Zeroizing::new(crate::otp::normalize_secret(config.secret.expose()));
    if seed.is_empty() {
        return None;
    }
    let kind = match config.kind {
        crate::otp::OtpKind::Totp { period } => format!("totp {period}"),
        crate::otp::OtpKind::Hotp { .. } => "hotp".to_owned(),
        crate::otp::OtpKind::Steam => "steam".to_owned(),
    };
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.update(format!("/{kind}/{:?}/{}", config.algorithm, config.digits).as_bytes());
    Some(*hasher.finalize().as_bytes())
}

/// Adds imported items to a vault, creating folders by path and leaving out
/// logins and one-time codes identical to one already there.
pub fn apply(vault: &mut Vault, imported: Imported, now_ms: u64) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();
    // A copy in the trash is no reason to leave one out: the person would be told it is already
    // there and find it nowhere.
    let live = || vault.doc.entries.values().filter(|entry| !entry.is_trashed());
    let mut seen: HashSet<_> = live().filter_map(identity_key).collect();
    // An authenticator's export read in a second time would otherwise double every code.
    let mut codes: HashSet<[u8; 32]> = live().filter_map(code_key).collect();
    let mut folders: BTreeMap<String, String> = BTreeMap::new();
    for item in imported.entries {
        let mut entry = item.entry;
        let repeated = match (identity_key(&entry), code_key(&entry)) {
            (Some(key), _) => !seen.insert(key),
            (None, Some(code)) => !codes.insert(code),
            (None, None) => false,
        };
        if repeated {
            report.duplicates += 1;
            continue;
        }
        if let Some(path) = item.folder
            && let Some(id) = folder_for_path(vault, &mut folders, &path, now_ms)?
        {
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

/// The folder for a path such as `Work/Mail`, one level per part, as KeePass groups and
/// Bitwarden's nested folders come: a folder already there under the same parent and name is
/// used, and the rest are made. `None` for a path with no name in it.
fn folder_for_path(
    vault: &mut Vault,
    made: &mut BTreeMap<String, String>,
    path: &str,
    now_ms: u64,
) -> Result<Option<String>> {
    let mut parent: Option<String> = None;
    let mut walked = String::new();
    for name in path.split('/').map(str::trim).filter(|name| !name.is_empty()) {
        walked.push('/');
        walked.push_str(name);
        let id = match made.get(&walked) {
            Some(id) => id.clone(),
            None => {
                let existing = vault
                    .doc
                    .folders
                    .values()
                    .filter(|f| f.name.v == name && f.parent.v == parent)
                    .map(|f| f.id.clone())
                    .min();
                let id = match existing {
                    Some(id) => id,
                    None => vault.add_folder(name, parent.clone(), now_ms)?,
                };
                made.insert(walked.clone(), id.clone());
                id
            }
        };
        parent = Some(id);
    }
    Ok(parent)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_kdbx;
#[cfg(test)]
mod tests_kdbx_yubikey;
#[cfg(test)]
mod tests_more;
#[cfg(test)]
mod tests_audit;
