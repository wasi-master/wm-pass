//! Accounts that live somewhere else: a KeePass file opened in place, or a Bitwarden account.
//!
//! The shells only know one thing, an open vault behind a handle. So an account is shown through
//! a mirror: an ordinary vault, in memory and never saved, holding the account's entries under
//! their own ids. Every screen and rule then works unchanged. Saving walks the mirror for what
//! changed and hands that to the account, which is the only copy that lasts.

use std::collections::BTreeMap;

use wmpass_core::backend::{Backend, KdbxBackend};
use wmpass_core::clock::Hlc;
#[cfg(any(test, feature = "online"))]
use wmpass_core::model::Entry;
use wmpass_core::vault::Vault;
use wmpass_core::{Error, Result};

/// The account behind a mirror.
#[derive(Debug)]
pub(crate) enum Account {
    Kdbx(Box<KdbxBackend>),
    #[cfg(feature = "online")]
    Bitwarden(Box<wmpass_bw::account::Account>),
}

impl Account {
    fn backend(&mut self) -> &mut dyn Backend {
        match self {
            Self::Kdbx(backend) => backend.as_mut(),
            #[cfg(feature = "online")]
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

#[cfg(any(test, feature = "online"))]
/// Each item's latest write in the mirror, taken as the account leaves for the server.
pub(crate) fn snapshot(vault: &Vault) -> BTreeMap<String, Hlc> {
    vault.doc.entries.iter().map(|(id, entry)| (id.clone(), entry.latest())).collect()
}

#[cfg(any(test, feature = "online"))]
/// What was done in the mirror while the account was away at the server: items changed or added
/// since `away`, and the ids of items deleted since.
pub(crate) type Meanwhile = (Vec<Entry>, Vec<String>);

#[cfg(any(test, feature = "online"))]
/// See [`Meanwhile`].
pub(crate) fn meanwhile(vault: &Vault, away: &BTreeMap<String, Hlc>) -> Meanwhile {
    let changed = vault.doc.entries.values().filter(|entry| away.get(&entry.id) != Some(&entry.latest())).cloned().collect();
    let deleted = away.keys().filter(|id| !vault.doc.entries.contains_key(*id)).cloned().collect();
    (changed, deleted)
}

#[cfg(any(test, feature = "online"))]
/// Puts back, over what the account brought, the edits made while it was away. A refresh alone
/// shows only the account, which never had them, so they would be gone without a word. Each
/// differs from what the account holds, so the next save or sync passes it on.
pub(crate) fn put_back(vault: &mut Vault, (changed, deleted): Meanwhile) {
    for entry in changed {
        vault.observe(entry.latest());
        match vault.doc.entries.get_mut(&entry.id) {
            Some(fresh) => fresh.merge_from(&entry),
            None => {
                vault.doc.entries.insert(entry.id.clone(), entry);
            }
        }
    }
    for id in deleted {
        vault.doc.entries.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wmpass_core::crypto::KdfParams;
    use wmpass_core::format::KdfConfig;
    use wmpass_core::model::Lww;
    use wmpass_core::vault::login_entry;

    #[test]
    fn edits_made_while_the_account_was_at_the_server_survive_its_return() {
        let config = KdfConfig::new(KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }).unwrap();
        let mut mirror = Vault::create("Account", b"throwaway", &config, 1, 1000).unwrap();
        let mut ids = Vec::new();
        for title in ["Kept", "Edited", "Deleted"] {
            let at = mirror.tick(1000);
            let mut entry = login_entry(title, "u", "p", at);
            entry.id = title.to_lowercase();
            ids.push(mirror.add(entry, 1000).unwrap());
        }
        // What the account holds as it leaves, and what it brings back: the same three.
        let account: Vec<Entry> = mirror.doc.entries.values().cloned().collect();
        let away = snapshot(&mirror);
        // Meanwhile, here: one edited, one deleted, one added.
        mirror.edit("edited", 2000, |e, at| e.common.title = Lww::new("Edited here".into(), at)).unwrap();
        mirror.doc.entries.remove("deleted");
        let at = mirror.tick(2000);
        let added = mirror.add(login_entry("Added", "u", "p", at), 2000).unwrap();
        let during = meanwhile(&mirror, &away);
        // The refresh shows the account as it came back.
        mirror.doc.entries = account.into_iter().map(|e| (e.id.clone(), e)).collect();
        put_back(&mut mirror, during);
        assert_eq!(mirror.get("edited").unwrap().common.title.v, "Edited here");
        assert!(mirror.get("deleted").is_none());
        assert!(mirror.get(&added).is_some());
        assert_eq!(mirror.get("kept").unwrap().common.title.v, "Kept");
        // And each still differs from what the account last showed, so it is passed on next.
        let after = snapshot(&mirror);
        assert_ne!(after.get("edited"), away.get("edited"));
    }
}
