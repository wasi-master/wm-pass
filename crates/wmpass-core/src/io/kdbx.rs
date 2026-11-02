//! KeePass databases, both ways. Reading and writing go through the `keepass` crate,
//! whose KDBX 4 writer is labelled experimental. So every write here is read back and
//! compared before it is handed over, and a mismatch is an error rather than a bad file.

use keepass::config::{DatabaseConfig, KdfConfig};
use keepass::db::{GroupRef, Value, fields as kp};
use keepass::{Database, DatabaseKey};

use super::{Imported, ImportedEntry, base_entry};
use crate::clock::Hlc;
use crate::model::{Entry, Kind, Login, Lww, Revision, SiteMatcher};
use crate::otp::OtpConfig;
use crate::{Error, Result, Secret};

const STANDARD: [&str; 6] = [
    kp::TITLE,
    kp::USERNAME,
    kp::PASSWORD,
    kp::URL,
    kp::NOTES,
    kp::OTP,
];

fn key(password: &str, key_file: Option<&[u8]>) -> Result<DatabaseKey> {
    let mut built = DatabaseKey::new().with_password(password);
    if let Some(mut bytes) = key_file {
        built = built
            .with_keyfile(&mut bytes)
            .map_err(|_| Error::Import("the key file could not be read".into()))?;
    }
    Ok(built)
}

fn walk(
    group: GroupRef<'_>,
    path: &str,
    at: Hlc,
    recycle: Option<keepass::db::GroupId>,
    out: &mut Imported,
) {
    if recycle == Some(group.id()) {
        return;
    }
    for item in group.entries() {
        let get = |name: &str| item.get(name).unwrap_or_default().to_owned();
        let title = get(kp::TITLE);
        let mut entry = base_entry(&title, &get(kp::NOTES), at);
        let mut matchers = Vec::new();
        let url = get(kp::URL);
        if !url.trim().is_empty() {
            matchers.push(SiteMatcher::Url(url));
        }
        let otp = item
            .get(kp::OTP)
            .and_then(|uri| match OtpConfig::from_uri(uri) {
                Ok(config) => Some(config),
                Err(_) => {
                    out.skipped
                        .push(format!("{title}: its one-time code could not be read"));
                    None
                }
            });
        entry.kind = Kind::Login(Login {
            site_matchers: Lww::new(matchers, at),
            username: Lww::new(get(kp::USERNAME), at),
            password: Lww::new(Secret::from(get(kp::PASSWORD)), at),
            otp: Lww::new(otp, at),
            ..Login::default()
        });
        entry.common.tags = Lww::new(item.tags.clone(), at);
        let mut custom = Vec::new();
        for (name, value) in &item.fields {
            if !STANDARD.contains(&name.as_str()) {
                custom.push((name.clone(), value.get().clone(), value.is_protected()));
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
        }
        let files = item
            .attachments_named()
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
        walk(child, &child_path, at, recycle, out);
    }
}

/// Reads a database.
pub fn import(data: &[u8], password: &str, key_file: Option<&[u8]>, at: Hlc) -> Result<Imported> {
    let database = Database::parse(data, key(password, key_file)?).map_err(|_| {
        Error::Import("wrong password or key file, or not a readable KeePass database".into())
    })?;
    let mut out = Imported::default();
    let recycle = database.recycle_bin().map(|group| group.id());
    walk(database.root(), "", at, recycle, &mut out);
    Ok(out)
}

fn set_common(target: &mut keepass::db::EntryMut<'_>, entry: &Entry) {
    target.set_unprotected(kp::TITLE, entry.common.title.v.as_str());
    let notes = entry.common.notes.v.expose();
    if !notes.is_empty() {
        target.set_unprotected(kp::NOTES, notes);
    }
    target.tags.clone_from(&entry.common.tags.v);
    for field in &entry.common.custom_fields.v {
        target.set_protected(field.name.as_str(), field.value.expose());
    }
}

/// Writes entries as a KDBX 4 database under a password, then reads the result back
/// and refuses to return it unless every login survived intact.
pub fn export(entries: &[&Entry], password: &str) -> Result<Vec<u8>> {
    if password.is_empty() {
        return Err(Error::Invalid("the export needs a password".into()));
    }
    let mut database = Database::with_config(strong_config());
    for entry in entries {
        let mut root = database.root_mut();
        let mut target = root.add_entry();
        set_common(&mut target, entry);
        match &entry.kind {
            Kind::Login(login) => {
                target.set_unprotected(kp::USERNAME, login.username.v.as_str());
                target.set_protected(kp::PASSWORD, login.password.v.expose());
                let url = login.site_matchers.v.iter().find_map(|m| match m {
                    SiteMatcher::Url(url) | SiteMatcher::Domain(url) | SiteMatcher::Host(url) => {
                        Some(url.as_str())
                    }
                    _ => None,
                });
                if let Some(url) = url {
                    target.set_unprotected(kp::URL, url);
                }
                if let Some(otp) = &login.otp.v {
                    target.set(kp::OTP, Value::protected(otp.to_uri().expose()));
                }
            }
            other => {
                for (name, value, hidden) in super::export::kind_fields(other) {
                    if hidden {
                        target.set_protected(name, value.expose());
                    } else {
                        target.set_unprotected(name, value.expose());
                    }
                }
                target.set_unprotected("WM Pass kind", other.name());
            }
        }
    }
    let mut out = Vec::new();
    database
        .save(&mut out, key(password, None)?)
        .map_err(|_| Error::Encoding)?;
    verify(&out, password, entries)?;
    Ok(out)
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

fn verify(bytes: &[u8], password: &str, entries: &[&Entry]) -> Result<()> {
    let back = Database::parse(bytes, key(password, None)?).map_err(|_| Error::Integrity)?;
    let mut found: Vec<(String, String, String)> = back
        .root()
        .entries()
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
        .map(|entry| match entry.login() {
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

/// Opens a `.kdbx` in place. Edits go back into the same database, so everything
/// WM Pass does not model (groups, icons, auto-type, custom data) is kept.
pub struct InPlace {
    database: Database,
    upgraded: bool,
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

impl InPlace {
    /// Opens a database.
    pub fn open(data: &[u8], password: &str, key_file: Option<&[u8]>) -> Result<Self> {
        let mut database = Database::parse(data, key(password, key_file)?).map_err(|_| {
            Error::Import("wrong password or key file, or not a readable KeePass database".into())
        })?;
        // KDBX 3 can be read but not written. The next save moves the file to KDBX 4
        // with Argon2id, which is the stronger format, and the app says so first.
        let upgraded = !matches!(
            database.config.version,
            keepass::config::DatabaseVersion::KDB4(_)
        );
        if upgraded {
            database.config = strong_config();
        } else {
            // The writer only produces the current KDBX 4 minor version. KeePassXC
            // writes 4.0 when it can, and every 4.x reader opens the newer minor.
            database.config.version = DatabaseConfig::default().version;
        }
        Ok(Self { database, upgraded })
    }

    /// Whether saving will change this file from an older format to KDBX 4. Older
    /// KeePass apps may not open it afterwards, so ask before the first save.
    pub fn will_upgrade(&self) -> bool {
        self.upgraded
    }

    /// Lists every entry outside the recycle bin.
    pub fn list(&self) -> Vec<InPlaceEntry> {
        fn visit(
            group: GroupRef<'_>,
            path: &str,
            recycle: Option<keepass::db::GroupId>,
            out: &mut Vec<InPlaceEntry>,
        ) {
            if recycle == Some(group.id()) {
                return;
            }
            for entry in group.entries() {
                out.push(InPlaceEntry {
                    id: entry.id().uuid().to_string(),
                    group: path.to_owned(),
                    title: entry.get(kp::TITLE).unwrap_or_default().to_owned(),
                    username: entry.get(kp::USERNAME).unwrap_or_default().to_owned(),
                    url: entry.get(kp::URL).unwrap_or_default().to_owned(),
                });
            }
            for child in group.groups() {
                let name = child.name.clone();
                let next = if path.is_empty() {
                    name
                } else {
                    format!("{path}/{name}")
                };
                visit(child, &next, recycle, out);
            }
        }
        let mut out = Vec::new();
        let recycle = self.database.recycle_bin().map(|group| group.id());
        visit(self.database.root(), "", recycle, &mut out);
        out
    }

    fn find_id(&self, id: &str) -> Option<keepass::db::EntryId> {
        self.database
            .iter_all_entries()
            .find(|e| e.id().uuid().to_string() == id)
            .map(|e| e.id())
    }

    /// Reads an entry's password.
    pub fn password(&self, id: &str) -> Option<Secret> {
        let found = self.find_id(id)?;
        self.database
            .entry(found)?
            .get(kp::PASSWORD)
            .map(Secret::new)
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
                let get = |name: &str| item.get(name).unwrap_or_default().to_owned();
                let mut entry = base_entry(&get(kp::TITLE), &get(kp::NOTES), at);
                entry.id = item.id().uuid().to_string();
                let url = get(kp::URL);
                let matchers = if url.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![SiteMatcher::Url(url)]
                };
                let otp = item
                    .get(kp::OTP)
                    .and_then(|uri| OtpConfig::from_uri(uri).ok());
                entry.kind = Kind::Login(Login {
                    site_matchers: Lww::new(matchers, at),
                    username: Lww::new(get(kp::USERNAME), at),
                    password: Lww::new(Secret::from(get(kp::PASSWORD)), at),
                    otp: Lww::new(otp, at),
                    ..Login::default()
                });
                entry.common.tags = Lww::new(item.tags.clone(), at);
                entry
            })
            .collect()
    }

    /// Writes a login's standard fields, creating it at the top level if it is new,
    /// and returns its id. Anything else on an existing entry is left alone.
    pub fn upsert(&mut self, entry: &Entry) -> Result<String> {
        let login = entry
            .login()
            .ok_or_else(|| Error::Invalid("a KeePass database holds logins here".into()))?;
        let url = login.site_matchers.v.iter().find_map(|m| match m {
            SiteMatcher::Url(u) | SiteMatcher::Domain(u) | SiteMatcher::Host(u) => Some(u.clone()),
            _ => None,
        });
        let existing = self.find_id(&entry.id);
        let mut root = self.database.root_mut();
        let id = match existing {
            Some(id) => id,
            None => root.add_entry().id(),
        };
        let mut target = self.database.entry_mut(id).ok_or(Error::Integrity)?;
        target.edit_tracking(|tracked| {
            tracked.set_unprotected(kp::TITLE, entry.common.title.v.as_str());
            tracked.set_unprotected(kp::USERNAME, login.username.v.as_str());
            tracked.set_protected(kp::PASSWORD, login.password.v.expose());
            tracked.set_unprotected(kp::NOTES, entry.common.notes.v.expose());
            if let Some(url) = &url {
                tracked.set_unprotected(kp::URL, url.as_str());
            }
            if let Some(otp) = &login.otp.v {
                tracked.set(kp::OTP, Value::protected(otp.to_uri().expose()));
            }
        });
        Ok(id.uuid().to_string())
    }

    /// Deletes an entry for good. KeePass records this so other copies learn of it.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let found = self
            .find_id(id)
            .ok_or_else(|| Error::Invalid("no such entry".into()))?;
        self.database
            .entry_mut(found)
            .ok_or(Error::Integrity)?
            .remove();
        Ok(())
    }

    /// Writes the database back, reads it back and compares it. Save the result
    /// atomically and keep the previous file as a `.bak`.
    pub fn save(&self, password: &str, key_file: Option<&[u8]>) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.database
            .save(&mut out, key(password, key_file)?)
            .map_err(|_| Error::Encoding)?;
        let back = Database::parse(&out, key(password, key_file)?).map_err(|_| Error::Integrity)?;
        if back != self.database {
            return Err(Error::Integrity);
        }
        Ok(out)
    }
}
