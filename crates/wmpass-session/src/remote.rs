//! Accounts that live somewhere else: a KeePass file opened in place, or a Bitwarden account.
//!
//! The shells only know one thing, an open vault behind a handle. So an account is shown through
//! a mirror: an ordinary vault, in memory and never saved, holding the account's entries under
//! their own ids. Every screen and rule then works unchanged. Saving walks the mirror for what
//! changed and hands that to the account, which is the only copy that lasts.

use std::collections::BTreeMap;

use wmpass_core::backend::{Backend, KdbxBackend};
use wmpass_core::clock::Hlc;
use wmpass_core::vault::Vault;
use wmpass_core::{Error, Result};

/// The account behind a mirror.
#[derive(Debug)]
pub(crate) enum Account {
    Kdbx(Box<KdbxBackend>),
    #[cfg(feature = "net")]
    Bitwarden(Box<wmpass_bw::account::Account>),
}

impl Account {
    fn backend(&mut self) -> &mut dyn Backend {
        match self {
            Self::Kdbx(backend) => backend.as_mut(),
            #[cfg(feature = "net")]
            Self::Bitwarden(account) => account.as_mut(),
        }
    }
}

/// An account and what the mirror last showed of it.
#[derive(Debug)]
pub(crate) struct Remote {
    pub account: Account,
    seen: BTreeMap<String, Hlc>,
}

impl Remote {
    pub fn new(account: Account) -> Self {
        Self { account, seen: BTreeMap::new() }
    }

    /// Makes the mirror show what the account holds.
    pub fn refresh(&mut self, vault: &mut Vault, now_ms: u64) {
        let entries = self.account.backend().entries(now_ms);
        let mut seen = BTreeMap::new();
        for entry in &entries {
            seen.insert(entry.id.clone(), entry.latest());
        }
        vault.doc.entries.retain(|id, _| seen.contains_key(id));
        for entry in entries {
            vault.doc.entries.insert(entry.id.clone(), entry);
        }
        self.seen = seen;
    }

    /// Passes what changed in the mirror on to the account.
    pub fn collect(&mut self, vault: &mut Vault, now_ms: u64) -> Result<()> {
        let mut gone: Vec<String> = self.seen.keys().filter(|id| !vault.doc.entries.contains_key(*id)).cloned().collect();
        let mut renamed = Vec::new();
        for entry in vault.doc.entries.values() {
            if entry.is_trashed() {
                if self.seen.contains_key(&entry.id) {
                    gone.push(entry.id.clone());
                }
                continue;
            }
            let latest = entry.latest();
            if self.seen.get(&entry.id) == Some(&latest) {
                continue;
            }
            let id = self.account.backend().upsert(entry.clone(), now_ms)?;
            if id != entry.id {
                renamed.push((entry.id.clone(), id.clone()));
            }
            self.seen.insert(id, latest);
        }
        for (from, to) in renamed {
            let mut entry = vault.doc.entries.remove(&from).ok_or(Error::Encoding)?;
            entry.id.clone_from(&to);
            vault.doc.entries.insert(to, entry);
        }
        for id in gone {
            self.account.backend().remove(&id, now_ms)?;
            self.seen.remove(&id);
        }
        Ok(())
    }

    /// The bytes that keep the account: the KeePass file, or the sealed offline copy.
    pub fn save(&mut self) -> Result<Vec<u8>> {
        self.account.backend().save()
    }
}
