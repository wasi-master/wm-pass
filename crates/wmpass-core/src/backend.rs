//! One interface over what WM Pass can be a client of: its own vault files, KeePass
//! databases opened in place, and (in `wmpass-bw`) Bitwarden-compatible servers.

use crate::clock::Hlc;
use crate::io::kdbx::InPlace;
use crate::model::{Entry, Id};
use crate::vault::Vault;
use crate::{Result, Secret};

/// Which kind of account this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// A `.wmvault` file.
    WmVault,
    /// A KeePass database, edited in place.
    Kdbx,
    /// A Bitwarden-compatible server.
    Bitwarden,
}

/// What every account type offers the app.
pub trait Backend: std::fmt::Debug {
    /// Which kind this is.
    fn kind(&self) -> BackendKind;
    /// Everything not deleted.
    fn entries(&self, now_ms: u64) -> Vec<Entry>;
    /// Adds an entry when its id is new or empty, else updates it. Returns the id.
    fn upsert(&mut self, entry: Entry, now_ms: u64) -> Result<Id>;
    /// Deletes. Backends with a trash use it.
    fn remove(&mut self, id: &str, now_ms: u64) -> Result<()>;
    /// Bytes to store. Write them atomically.
    fn save(&mut self) -> Result<Vec<u8>>;
}

impl Backend for Vault {
    fn kind(&self) -> BackendKind {
        BackendKind::WmVault
    }

    fn entries(&self, _now_ms: u64) -> Vec<Entry> {
        self.doc.entries.values().filter(|entry| !entry.is_trashed()).cloned().collect()
    }

    fn upsert(&mut self, entry: Entry, now_ms: u64) -> Result<Id> {
        if !entry.id.is_empty() && self.get(&entry.id).is_some() {
            let id = entry.id.clone();
            self.edit(&id, now_ms, |existing, _| existing.merge_from(&entry))?;
            return Ok(id);
        }
        self.add(entry, now_ms)
    }

    fn remove(&mut self, id: &str, now_ms: u64) -> Result<()> {
        self.trash(id, now_ms)
    }

    fn save(&mut self) -> Result<Vec<u8>> {
        Vault::save(self)
    }
}

/// A KeePass database opened in place, with the credentials to save it again.
#[derive(Debug)]
pub struct KdbxBackend {
    database: InPlace,
    password: Secret,
    key_file: Option<Vec<u8>>,
}

impl KdbxBackend {
    /// Opens a database.
    pub fn open(data: &[u8], password: Secret, key_file: Option<Vec<u8>>) -> Result<Self> {
        let database = InPlace::open(data, password.expose(), key_file.as_deref())?;
        Ok(Self { database, password, key_file })
    }

    /// The database, for operations only it has.
    pub fn database(&self) -> &InPlace {
        &self.database
    }
}

impl Backend for KdbxBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Kdbx
    }

    fn entries(&self, now_ms: u64) -> Vec<Entry> {
        self.database.entries(Hlc { ms: now_ms, ..Hlc::default() })
    }

    fn upsert(&mut self, entry: Entry, _now_ms: u64) -> Result<Id> {
        self.database.upsert(&entry)
    }

    fn remove(&mut self, id: &str, _now_ms: u64) -> Result<()> {
        self.database.remove(id)
    }

    fn save(&mut self) -> Result<Vec<u8>> {
        self.database.save(self.password.expose(), self.key_file.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Lww, SiteMatcher};
    use crate::vault::login_entry;
    use crate::vault::test_support::fast_vault;

    fn exercise(backend: &mut dyn Backend, reopen: impl Fn(&[u8]) -> Box<dyn Backend>) {
        let at = Hlc { ms: 5000, ..Hlc::default() };
        let mut entry = login_entry("Mail", "me", "first", at);
        entry.login_mut().unwrap().site_matchers = Lww::new(vec![SiteMatcher::Url("https://m.example".into())], at);
        let id = backend.upsert(entry, 5000).unwrap();
        let mut stored = backend.entries(6000).into_iter().find(|e| e.id == id).unwrap();
        stored.set_password("second".into(), Hlc { ms: 7000, ..Hlc::default() });
        assert_eq!(backend.upsert(stored, 7000).unwrap(), id);

        let bytes = backend.save().unwrap();
        let again = reopen(&bytes);
        let back = again.entries(8000).into_iter().find(|e| e.id == id).unwrap();
        let login = back.login().unwrap();
        assert_eq!(login.password.v.expose(), "second");
        assert_eq!(login.username.v, "me");
        assert_eq!(login.site_matchers.v.len(), 1);

        backend.remove(&id, 9000).unwrap();
        assert!(backend.entries(9500).iter().all(|e| e.id != id));
    }

    #[test]
    fn vault_backend() {
        let mut vault = fast_vault(1);
        let key = crate::format::VaultKey::from_bytes(*vault.key().expose_for_hardware_wrap());
        exercise(&mut vault, move |bytes| {
            let key = crate::format::VaultKey::from_bytes(*key.expose_for_hardware_wrap());
            Box::new(Vault::from_file(bytes, key, 2).unwrap())
        });
    }

    #[test]
    fn kdbx_backend() {
        let mut database = keepass::Database::new();
        database.config.kdf_config = keepass::config::KdfConfig::Argon2id {
            iterations: 1,
            memory: 1024 * 1024,
            parallelism: 1,
            version: argon2_for_keepass::Version::Version13,
        };
        let mut bytes = Vec::new();
        database.save(&mut bytes, keepass::DatabaseKey::new().with_password("pw")).unwrap();
        let mut backend = KdbxBackend::open(&bytes, "pw".into(), None).unwrap();
        exercise(&mut backend, |saved| Box::new(KdbxBackend::open(saved, "pw".into(), None).unwrap()));
    }
}
