//! An unlocked vault: the document, its key and the rules for changing it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::clock::{Clock, Hlc};
use crate::crypto;
use crate::format::{
    self, FactorKind, FactorOutput, Header, KdfConfig, RootKey, SLOTS, SlotAction, Unlock, VaultKey,
};
use crate::model::{Attachment, Entry, Folder, Id, Kind, LinkedVault, Lww, SlotInfo, VaultDoc};
use crate::{Error, Result, Secret};

/// The newest document layout this build understands.
pub const DOC_MINOR: u32 = 0;
/// Days in the trash by default.
pub const DEFAULT_TRASH_DAYS: u32 = 30;
const DAY_MS: u64 = 86_400_000;

/// A random hex id.
pub fn new_id() -> Result<Id> {
    let bytes: [u8; 16] = crypto::random_array()?;
    Ok(hex(&bytes))
}

/// Lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Reverses [`hex`].
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

/// What a device remembers per vault, to notice a sync location replaying an old
/// copy. AEAD alone cannot catch that, since the old copy is genuine.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Freshness {
    /// As new as or newer than anything seen.
    Fresh,
    /// Older than what this device has already seen.
    Rollback {
        /// Highest generation this device saw.
        seen: u64,
        /// What the file holds.
        file: u64,
    },
}

/// Compares a file's generation with the highest this device has seen.
pub fn check_freshness(seen: u64, file: u64) -> Freshness {
    if file < seen {
        Freshness::Rollback { seen, file }
    } else {
        Freshness::Fresh
    }
}

/// Fingerprints of a vault's mergeable contents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Digests {
    /// Entries by id.
    pub entries: BTreeMap<Id, [u8; 16]>,
    /// Folders by id.
    pub folders: BTreeMap<Id, [u8; 16]>,
    /// Deleted ids.
    pub tombstones: BTreeMap<Id, Hlc>,
    /// Linked files.
    pub linked: BTreeMap<String, Hlc>,
    /// When slots last changed.
    pub slots_at: Hlc,
    /// When each tag's glyph last changed.
    pub tag_icons: BTreeMap<String, Hlc>,
    /// When each synced app setting last changed.
    pub app_prefs: BTreeMap<String, Hlc>,
    /// When each kept disposable mailbox last changed.
    pub inboxes: BTreeMap<String, Hlc>,
    /// Save counter.
    pub generation: u64,
    /// Layout version.
    pub minor: u32,
}

/// What one device sends another after comparing [`Digests`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Delta {
    /// Entries the peer lacks or holds differently.
    pub entries: Vec<Entry>,
    /// Folders likewise.
    pub folders: Vec<Folder>,
    /// Deletes the peer has not seen.
    pub tombstones: BTreeMap<Id, Hlc>,
    /// Linked files likewise.
    pub linked: Vec<LinkedVault>,
    /// Tag glyphs the peer has not seen.
    pub tag_icons: BTreeMap<String, Lww<String>>,
    /// Synced app settings the peer has not seen.
    pub app_prefs: BTreeMap<String, Lww<String>>,
    /// Kept disposable mailboxes the peer has not seen.
    pub inboxes: BTreeMap<String, Lww<Option<crate::model::Inbox>>>,
    /// Sender's save counter.
    pub generation: u64,
    /// Sender's layout version.
    pub minor: u32,
    /// The whole sealed file, when slot records changed.
    pub file: Option<Vec<u8>>,
}

fn fingerprint<T: Serialize>(value: &T) -> Result<[u8; 16]> {
    let bytes = Zeroizing::new(rmp_serde::to_vec_named(value).map_err(|_| Error::Encoding)?);
    let hash = blake3::hash(&bytes);
    let mut out = [0u8; 16];
    out.copy_from_slice(&hash.as_bytes()[..16]);
    Ok(out)
}

/// An unlocked vault.
#[derive(Debug)]
pub struct Vault {
    header: Header,
    key: VaultKey,
    /// The contents.
    pub doc: VaultDoc,
    clock: Clock,
    device: u32,
    read_only: bool,
}

/// What came of trying a password on a set of files.
#[derive(Debug)]
pub enum OpenOutcome {
    /// A vault opened.
    Opened {
        /// Which file, by position in the input.
        index: usize,
        /// The vault.
        vault: Box<Vault>,
    },
    /// A duress password that asks for wiping or locking rather than opening.
    Duress {
        /// The requested action.
        action: SlotAction,
    },
    /// The password was right for some file but a factor is missing.
    NeedsFactor(Vec<FactorKind>),
    /// Nothing opened.
    NoMatch,
}

/// Tries one password on every file in a set, as the unlock screen does. Files
/// sharing a salt share one derivation, and a real vault, decoy and chaff file get
/// the same work, so the attempt reveals nothing about which ones are real.
pub fn open_set(
    files: &[&[u8]],
    password: &[u8],
    factors: &[FactorOutput],
    device: u32,
) -> Result<OpenOutcome> {
    open_set_with(files, factors, device, |config| config.derive(password))
}

fn open_set_with(
    files: &[&[u8]],
    factors: &[FactorOutput],
    device: u32,
    derive: impl Fn(&KdfConfig) -> Result<RootKey>,
) -> Result<OpenOutcome> {
    let mut roots: Vec<(KdfConfig, RootKey)> = Vec::new();
    let mut needed = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let Ok(header) = Header::parse(file) else {
            continue;
        };
        let config = header.config()?;
        if !roots.iter().any(|(known, _)| *known == config) {
            let root = derive(&config)?;
            roots.push((config.clone(), root));
        }
        let Some((_, root)) = roots.iter().find(|(known, _)| *known == config) else {
            continue;
        };
        match header.unlock(root, factors)? {
            Unlock::Opened {
                action: SlotAction::Open,
                key,
            } => {
                let vault = Vault::from_file(file, key, device)?;
                return Ok(OpenOutcome::Opened {
                    index,
                    vault: Box::new(vault),
                });
            }
            Unlock::Opened { action, .. } => return Ok(OpenOutcome::Duress { action }),
            Unlock::NeedsFactor(kinds) => needed.extend(kinds),
            Unlock::NoMatch => {}
        }
    }
    if needed.is_empty() {
        Ok(OpenOutcome::NoMatch)
    } else {
        Ok(OpenOutcome::NeedsFactor(needed))
    }
}

impl Vault {
    /// Creates a vault with one password. The master key is new and random.
    pub fn create(
        name: &str,
        password: &[u8],
        config: &KdfConfig,
        device: u32,
        now_ms: u64,
    ) -> Result<Self> {
        let root = config.derive(password)?;
        Self::create_with_root(name, &root, config, device, now_ms)
    }

    fn create_with_root(
        name: &str,
        root: &RootKey,
        config: &KdfConfig,
        device: u32,
        now_ms: u64,
    ) -> Result<Self> {
        let mut header = Header::new(config)?;
        let key = VaultKey::generate()?;
        header.write_slot(0, root, &key, SlotAction::Open, None)?;
        let mut clock = Clock::new(device);
        let at = clock.tick(now_ms);
        let mut slots = vec![SlotInfo::default(); SLOTS];
        slots[0] = SlotInfo {
            used: true,
            label: "Master password".to_owned(),
        };
        let doc = VaultDoc {
            minor: DOC_MINOR,
            name: name.to_owned(),
            slots,
            slots_at: at,
            prefs: crate::model::VaultPrefs {
                trash_days: DEFAULT_TRASH_DAYS,
                ..crate::model::VaultPrefs::default()
            },
            ..VaultDoc::default()
        };
        Ok(Self {
            header,
            key,
            doc,
            clock,
            device,
            read_only: false,
        })
    }

    /// Opens a file with a key that a slot or hardware keystore supplied.
    pub fn from_file(file: &[u8], key: VaultKey, device: u32) -> Result<Self> {
        let (header, plain) = format::open_file(file, &key)?;
        let doc: VaultDoc = rmp_serde::from_slice(&plain).map_err(|_| Error::Encoding)?;
        let read_only = doc.minor > DOC_MINOR;
        let mut clock = Clock::new(device);
        observe_doc(&mut clock, &doc);
        Ok(Self {
            header,
            key,
            doc,
            clock,
            device,
            read_only,
        })
    }

    /// Whether this build must not save, because the file is newer than it knows.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Whether a password opens this vault now, for the actions that ask for it again:
    /// the hidden shelf, exporting, changing how the vault unlocks.
    pub fn verify_password(&self, password: &[u8], factors: &[FactorOutput]) -> Result<bool> {
        let root = self.header.config()?.derive(password)?;
        Ok(self.header.slot_match(&root, factors, &self.key)? == Some(SlotAction::Open))
    }

    /// The KDF settings this file uses.
    pub fn kdf_config(&self) -> Result<KdfConfig> {
        self.header.config()
    }

    /// The file's key, for hardware wrapping only.
    pub fn key(&self) -> &VaultKey {
        &self.key
    }

    /// Encrypts for saving and bumps the generation. Write the result atomically.
    pub fn save(&mut self) -> Result<Vec<u8>> {
        if self.read_only {
            return Err(Error::Invalid(
                "this vault was written by a newer version of WM Pass".into(),
            ));
        }
        self.doc.generation = self.doc.generation.saturating_add(1);
        let plain =
            Zeroizing::new(rmp_serde::to_vec_named(&self.doc).map_err(|_| Error::Encoding)?);
        format::seal_file(&self.header, &self.key, &plain)
    }

    /// Takes note of a moment seen in data from elsewhere, so later edits here sort after it.
    pub fn observe(&mut self, seen: Hlc) {
        self.clock.observe(seen);
    }

    /// The next moment on this device.
    pub fn tick(&mut self, now_ms: u64) -> Hlc {
        self.clock.tick(now_ms)
    }

    /// Adds an entry, assigning its id when empty, and returns the id.
    pub fn add(&mut self, mut entry: Entry, now_ms: u64) -> Result<Id> {
        if entry.id.is_empty() {
            entry.id = new_id()?;
        }
        if entry.created_ms == 0 {
            entry.created_ms = now_ms;
        }
        let id = entry.id.clone();
        self.doc.entries.insert(id.clone(), entry);
        Ok(id)
    }

    /// Reads an entry.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.doc.entries.get(id)
    }

    /// Changes an entry through a closure given the current moment.
    pub fn edit<R>(
        &mut self,
        id: &str,
        now_ms: u64,
        edit: impl FnOnce(&mut Entry, Hlc) -> R,
    ) -> Result<R> {
        let at = self.clock.tick(now_ms);
        let entry = self
            .doc
            .entries
            .get_mut(id)
            .ok_or_else(|| Error::Invalid("no such entry".into()))?;
        let result = edit(entry, at);
        let limit = self.doc.prefs.history_limit as usize;
        trim_history(entry, limit);
        Ok(result)
    }

    /// Moves to the trash. [`Vault::restore`] brings it back.
    pub fn trash(&mut self, id: &str, now_ms: u64) -> Result<()> {
        self.edit(id, now_ms, |entry, at| {
            entry.common.deleted_at_ms = Lww::new(Some(now_ms), at)
        })
    }

    /// Takes out of the trash.
    pub fn restore(&mut self, id: &str, now_ms: u64) -> Result<()> {
        self.edit(id, now_ms, |entry, at| {
            entry.common.deleted_at_ms = Lww::new(None, at)
        })
    }

    /// Removes for good. Returns the blobs to delete, whose keys are now gone.
    pub fn purge(&mut self, id: &str, now_ms: u64) -> Vec<String> {
        let at = self.clock.tick(now_ms);
        let blobs = self
            .doc
            .entries
            .remove(id)
            .map(|entry| blob_names(&entry.common.attachments.v));
        self.doc.tombstones.insert(id.to_owned(), at);
        blobs.unwrap_or_default()
    }

    /// Applies timers: self-destruct and trash expiry. Returns the blobs to delete.
    pub fn sweep(&mut self, now_ms: u64) -> Vec<String> {
        let trash_ms = u64::from(self.doc.prefs.trash_days) * DAY_MS;
        let due: Vec<Id> = self
            .doc
            .entries
            .values()
            .filter(|entry| {
                let destroyed = entry.common.destroy_at_ms.v.is_some_and(|at| at <= now_ms);
                let expired = trash_ms > 0
                    && entry
                        .common
                        .deleted_at_ms
                        .v
                        .is_some_and(|at| at.saturating_add(trash_ms) <= now_ms);
                destroyed || expired
            })
            .map(|entry| entry.id.clone())
            .collect();
        due.iter().flat_map(|id| self.purge(id, now_ms)).collect()
    }

    /// Adds a folder and returns its id.
    pub fn add_folder(&mut self, name: &str, parent: Option<Id>, now_ms: u64) -> Result<Id> {
        let at = self.clock.tick(now_ms);
        let id = new_id()?;
        let folder = Folder {
            id: id.clone(),
            name: Lww::new(name.to_owned(), at),
            parent: Lww::new(parent, at),
        };
        self.doc.folders.insert(id.clone(), folder);
        Ok(id)
    }

    /// The file name of this vault's decoy, which keeps pace with the vault's size. Derived
    /// from the key so that the vault can recognise its own decoy without anything on disk
    /// saying which file that is: a hidden vault's file must never be mistaken for it.
    pub fn chaff_file_name(&self) -> Result<String> {
        let name = crypto::hkdf(&self.key.0[..], self.header.vault_id(), "wmpass/chaff-name/v1")?;
        Ok(name.iter().take(16).map(|b| format!("{b:02x}")).collect())
    }

    /// The name this device's sync file goes under. It is derived from the vault key,
    /// so a sync location sees random names that reveal neither the vault nor the device.
    pub fn sync_file_name(&self) -> Result<String> {
        let name_key = crypto::hkdf(&self.key.0[..], self.header.vault_id(), "wmpass/sync-name/v1")?;
        let tag = crypto::hmac_sha256(&name_key[..], &self.device.to_le_bytes())?;
        Ok(hex(&tag[..16]))
    }

    /// Merges another saved copy of this vault in, returning that copy's generation.
    /// Refuses a file under a different key, and adopts the other copy's slot records
    /// if they are newer.
    pub fn merge_file(&mut self, file: &[u8]) -> Result<u64> {
        let (header, plain) = format::open_file(file, &self.key)?;
        let other: VaultDoc = rmp_serde::from_slice(&plain).map_err(|_| Error::Encoding)?;
        observe_doc(&mut self.clock, &other);
        if other.slots_at > self.doc.slots_at {
            self.header = header;
        }
        let generation = other.generation;
        self.doc.merge_from(&other);
        if self.doc.minor > DOC_MINOR {
            self.read_only = true;
        }
        Ok(generation)
    }

    /// A key for one purpose, derived from the vault key. The vault key itself never leaves.
    pub fn purpose_key(&self, purpose: &str) -> Result<crypto::Key> {
        crypto::hkdf(&self.key.0[..], self.header.vault_id(), purpose)
    }

    /// Fingerprints of everything that merges, so a sync can send only what differs.
    pub fn digests(&self) -> Result<Digests> {
        let mut digests = Digests {
            tombstones: self.doc.tombstones.clone(),
            linked: self.doc.linked.iter().map(|l| (l.file.clone(), l.at)).collect(),
            slots_at: self.doc.slots_at,
            tag_icons: self.doc.prefs.tag_icons.iter().map(|(k, v)| (k.clone(), v.at)).collect(),
            app_prefs: self.doc.prefs.app.iter().map(|(k, v)| (k.clone(), v.at)).collect(),
            inboxes: self.doc.prefs.inboxes.iter().map(|(k, v)| (k.clone(), v.at)).collect(),
            generation: self.doc.generation,
            minor: self.doc.minor,
            ..Digests::default()
        };
        for (id, entry) in &self.doc.entries {
            digests.entries.insert(id.clone(), fingerprint(entry)?);
        }
        for (id, folder) in &self.doc.folders {
            digests.folders.insert(id.clone(), fingerprint(folder)?);
        }
        Ok(digests)
    }

    /// What a peer with those fingerprints lacks. Slot records only exist whole, so when
    /// this device's are newer the delta also carries a sealed copy of the file.
    pub fn delta_for(&mut self, theirs: &Digests) -> Result<Delta> {
        let mine = self.digests()?;
        let changed = |map: &BTreeMap<Id, [u8; 16]>, id: &Id, digest: &[u8; 16]| map.get(id) != Some(digest);
        let entries = mine
            .entries
            .iter()
            .filter(|(id, digest)| changed(&theirs.entries, id, digest))
            .filter_map(|(id, _)| self.doc.entries.get(id).cloned())
            .collect();
        let folders = mine
            .folders
            .iter()
            .filter(|(id, digest)| changed(&theirs.folders, id, digest))
            .filter_map(|(id, _)| self.doc.folders.get(id).cloned())
            .collect();
        let tombstones = mine
            .tombstones
            .iter()
            .filter(|(id, at)| theirs.tombstones.get(*id).is_none_or(|known| known < *at))
            .map(|(id, at)| (id.clone(), *at))
            .collect();
        let linked = self
            .doc
            .linked
            .iter()
            .filter(|link| theirs.linked.get(&link.file).is_none_or(|known| *known < link.at))
            .cloned()
            .collect();
        let tag_icons = newer(&self.doc.prefs.tag_icons, &theirs.tag_icons);
        let app_prefs = newer(&self.doc.prefs.app, &theirs.app_prefs);
        let inboxes = newer(&self.doc.prefs.inboxes, &theirs.inboxes);
        let file = if mine.slots_at > theirs.slots_at {
            Some(self.save()?)
        } else {
            None
        };
        Ok(Delta {
            entries,
            folders,
            tombstones,
            linked,
            tag_icons,
            app_prefs,
            inboxes,
            generation: mine.generation,
            minor: mine.minor,
            file,
        })
    }

    /// Merges a peer's delta in, with the same rules as merging a whole file.
    pub fn apply_delta(&mut self, delta: Delta) -> Result<()> {
        if let Some(file) = &delta.file {
            self.merge_file(file)?;
        }
        let partial = VaultDoc {
            minor: delta.minor,
            generation: delta.generation,
            entries: delta.entries.into_iter().map(|e| (e.id.clone(), e)).collect(),
            folders: delta.folders.into_iter().map(|f| (f.id.clone(), f)).collect(),
            tombstones: delta.tombstones,
            linked: delta.linked,
            prefs: crate::model::VaultPrefs {
                tag_icons: delta.tag_icons,
                app: delta.app_prefs,
                inboxes: delta.inboxes,
                ..crate::model::VaultPrefs::default()
            },
            ..VaultDoc::default()
        };
        observe_doc(&mut self.clock, &partial);
        self.doc.merge_from(&partial);
        if self.doc.minor > DOC_MINOR {
            self.read_only = true;
        }
        Ok(())
    }

    /// One value for everything that merges. Two devices that hold the same value have
    /// converged.
    pub fn state_hash(&self) -> Result<[u8; 16]> {
        let mut digests = self.digests()?;
        digests.generation = 0;
        digests.minor = 0;
        fingerprint(&digests)
    }

    /// Attachment blobs the entries refer to.
    pub fn referenced_blobs(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .doc
            .entries
            .values()
            .flat_map(|entry| blob_names(&entry.common.attachments.v))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn free_slot(&self) -> Result<usize> {
        self.doc
            .slots
            .iter()
            .position(|slot| !slot.used)
            .ok_or(Error::NoFreeSlot)
    }

    fn note_slot(&mut self, index: usize, label: &str, used: bool, now_ms: u64) {
        let at = self.clock.tick(now_ms);
        self.doc.slots[index] = SlotInfo {
            used,
            label: label.to_owned(),
        };
        self.doc.slots_at = at;
    }

    /// Adds another way in. Pass a duress `action` for a duress password, and a
    /// factor for a slot that also needs a key file or hardware key.
    pub fn add_slot(
        &mut self,
        password: &[u8],
        label: &str,
        action: SlotAction,
        factor: Option<(&FactorKind, &FactorOutput)>,
        now_ms: u64,
    ) -> Result<usize> {
        let root = self.header.config()?.derive(password)?;
        self.add_slot_with_root(&root, label, action, factor, now_ms)
    }

    /// As [`Vault::add_slot`], with a root key from elsewhere, such as Shamir recovery.
    pub fn add_slot_with_root(
        &mut self,
        root: &RootKey,
        label: &str,
        action: SlotAction,
        factor: Option<(&FactorKind, &FactorOutput)>,
        now_ms: u64,
    ) -> Result<usize> {
        // A password already in use would make the first matching slot win, so a
        // duress password equal to the master password would never fire, and the
        // owner would not find out until the moment it mattered.
        if factor.is_none() && self.header.slot_match(root, &[], &self.key)?.is_some() {
            return Err(Error::Invalid(
                "that password is already in use on this vault".into(),
            ));
        }
        let index = self.free_slot()?;
        self.header
            .write_slot(index, root, &self.key, action, factor)?;
        self.note_slot(index, label, true, now_ms);
        Ok(index)
    }

    /// Removes a way in. Refuses to remove the last one that opens the vault.
    pub fn remove_slot(&mut self, index: usize, now_ms: u64) -> Result<()> {
        let used = self.doc.slots.iter().filter(|slot| slot.used).count();
        if used <= 1 {
            return Err(Error::Invalid("a vault needs at least one way in".into()));
        }
        self.header.clear_slot(index)?;
        self.note_slot(index, "", false, now_ms);
        Ok(())
    }

    /// Replaces the password in one slot. Old backups still open with the old
    /// password; [`Vault::rekey`] is what cuts them off.
    pub fn change_password(&mut self, index: usize, password: &[u8], now_ms: u64) -> Result<()> {
        let root = self.header.config()?.derive(password)?;
        let label = self
            .doc
            .slots
            .get(index)
            .map(|slot| slot.label.clone())
            .unwrap_or_default();
        self.header
            .write_slot(index, &root, &self.key, SlotAction::Open, None)?;
        self.note_slot(index, &label, true, now_ms);
        Ok(())
    }

    /// Generates a new master key and clears every other slot, for after a device
    /// or member is removed or a password leaked. Copies written after this cannot
    /// be read with old keys; copies already taken still can.
    pub fn rekey(&mut self, password: &[u8], config: &KdfConfig, now_ms: u64) -> Result<()> {
        let root = config.derive(password)?;
        self.rekey_with_root(&root, config, now_ms)
    }

    fn rekey_with_root(&mut self, root: &RootKey, config: &KdfConfig, now_ms: u64) -> Result<()> {
        let mut header = Header::new(config)?;
        let key = VaultKey::generate()?;
        header.write_slot(0, root, &key, SlotAction::Open, None)?;
        self.header = header;
        self.key = key;
        self.doc.slots = vec![SlotInfo::default(); SLOTS];
        self.note_slot(0, "Master password", true, now_ms);
        Ok(())
    }

    /// A vault meant for sharing. No password opens it: each member's key comes from a
    /// [`crate::sharing::Manifest`], and the root key here is thrown away at once.
    pub fn create_shared(name: &str, config: &KdfConfig, device: u32, now_ms: u64) -> Result<Self> {
        let root = RootKey::from_high_entropy(&crypto::random_array::<32>()?)?;
        Self::create_with_root(name, &root, config, device, now_ms)
    }

    /// Moves a shared vault to a new key and id, for after someone leaves.
    pub fn rekey_shared(&mut self, config: &KdfConfig, now_ms: u64) -> Result<()> {
        let root = RootKey::from_high_entropy(&crypto::random_array::<32>()?)?;
        self.rekey_with_root(&root, config, now_ms)
    }

    /// This file's random id.
    pub fn vault_id(&self) -> &[u8] {
        self.header.vault_id()
    }

    /// Encrypts an attachment. Store the bytes under the returned record's `blob` name.
    pub fn seal_attachment(name: &str, mime: &str, plain: &[u8]) -> Result<(Attachment, Vec<u8>)> {
        let key = crypto::random_key()?;
        let blob = new_id()?;
        let prefix: [u8; crypto::STREAM_PREFIX_LEN] = crypto::random_array()?;
        let mut out = prefix.to_vec();
        out.extend(crypto::stream_seal(&key, &prefix, plain, blob.as_bytes())?);
        let record = Attachment {
            blob,
            key: Secret::from(hex(&key[..])),
            name: name.to_owned(),
            mime: mime.to_owned(),
            size: plain.len() as u64,
        };
        Ok((record, out))
    }

    /// Decrypts an attachment.
    pub fn open_attachment(record: &Attachment, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let raw = Zeroizing::new(unhex(record.key.expose()).ok_or(Error::Integrity)?);
        let key: [u8; 32] = raw.as_slice().try_into().map_err(|_| Error::Integrity)?;
        let key = Zeroizing::new(key);
        let (prefix, body) = sealed
            .split_at_checked(crypto::STREAM_PREFIX_LEN)
            .ok_or(Error::Integrity)?;
        let prefix: [u8; crypto::STREAM_PREFIX_LEN] =
            prefix.try_into().map_err(|_| Error::Integrity)?;
        crypto::stream_open(&key, &prefix, body, record.blob.as_bytes())
    }

    /// Entries that are not trashed or on the hidden shelf, as lists and autofill see them.
    pub fn visible(&self) -> impl Iterator<Item = &Entry> {
        self.doc
            .entries
            .values()
            .filter(|e| !e.is_trashed() && !e.common.hidden.v)
    }
}

fn trim_history(entry: &mut Entry, limit: usize) {
    if limit == 0 {
        return;
    }
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut keep = vec![true; entry.history.len()];
    for (index, revision) in entry.history.iter().enumerate().rev() {
        let count = seen.entry(revision.field.clone()).or_default();
        *count += 1;
        if *count > limit {
            keep[index] = false;
        }
    }
    let mut flags = keep.into_iter();
    entry.history.retain(|_| flags.next().unwrap_or(true));
}

fn blob_names(attachments: &[Attachment]) -> Vec<String> {
    attachments.iter().map(|a| a.blob.clone()).collect()
}

fn observe_doc(clock: &mut Clock, doc: &VaultDoc) {
    clock.observe(doc.slots_at);
    for entry in doc.entries.values() {
        clock.observe(entry.latest());
    }
    for at in doc.tombstones.values() {
        clock.observe(*at);
    }
    for value in doc.prefs.tag_icons.values().chain(doc.prefs.app.values()) {
        clock.observe(value.at);
    }
    for inbox in doc.prefs.inboxes.values() {
        clock.observe(inbox.at);
    }
}

/// The values set one key at a time that a peer holding those write times has not seen.
fn newer<T: Clone>(mine: &BTreeMap<String, Lww<T>>, theirs: &BTreeMap<String, Hlc>) -> BTreeMap<String, Lww<T>> {
    mine.iter()
        .filter(|(key, value)| theirs.get(*key).is_none_or(|known| *known < value.at))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Builds a kind's default body.
pub fn login_entry(title: &str, username: &str, password: &str, at: Hlc) -> Entry {
    let mut entry = Entry::default();
    entry.common.title = Lww::new(title.to_owned(), at);
    entry.kind = Kind::Login(crate::model::Login {
        username: Lww::new(username.to_owned(), at),
        password: Lww::new(Secret::new(password), at),
        ..crate::model::Login::default()
    });
    entry
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::crypto::KdfParams;

    pub fn fast_vault(device: u32) -> Vault {
        let config = KdfConfig::new(KdfParams::default()).unwrap();
        let root = config.derive_for_tests(b"pw").unwrap();
        Vault::create_with_root("Test", &root, &config, device, 1000).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::fast_vault;
    use super::*;
    use crate::crypto::KdfParams;

    fn set_outcome(files: &[&[u8]], password: &[u8], factors: &[FactorOutput]) -> OpenOutcome {
        open_set_with(files, factors, 2, |config| {
            config.derive_for_tests(password)
        })
        .unwrap()
    }

    #[test]
    fn save_and_reopen_keeps_everything_and_raises_the_generation() {
        let mut vault = fast_vault(1);
        let at = vault.tick(2000);
        let id = vault
            .add(login_entry("Mail", "me", "s3cret", at), 2000)
            .unwrap();
        let first = vault.save().unwrap();
        let second = vault.save().unwrap();
        let OpenOutcome::Opened {
            vault: again,
            index,
        } = set_outcome(&[&second], b"pw", &[])
        else {
            panic!("expected to open");
        };
        assert_eq!(index, 0);
        assert_eq!(again.doc.generation, 2);
        assert_eq!(
            again.get(&id).unwrap().login().unwrap().password.v.expose(),
            "s3cret"
        );
        let OpenOutcome::Opened { vault: old, .. } = set_outcome(&[&first], b"pw", &[]) else {
            panic!("expected to open");
        };
        assert_eq!(
            check_freshness(again.doc.generation, old.doc.generation),
            Freshness::Rollback { seen: 2, file: 1 }
        );
        assert_eq!(check_freshness(2, 2), Freshness::Fresh);
    }

    #[test]
    fn unlock_screen_finds_the_right_file_among_chaff_and_decoys() {
        let config = KdfConfig::new(KdfParams::default()).unwrap();
        let real_root = config.derive_for_tests(b"real").unwrap();
        let decoy_root = config.derive_for_tests(b"decoy").unwrap();
        let mut real = Vault::create_with_root("Real", &real_root, &config, 1, 1).unwrap();
        let mut decoy = Vault::create_with_root("Decoy", &decoy_root, &config, 1, 1).unwrap();
        let (real_file, decoy_file) = (real.save().unwrap(), decoy.save().unwrap());
        let chaff = format::chaff_file(&config, 400).unwrap();
        let files: [&[u8]; 3] = [&chaff, &decoy_file, &real_file];

        let OpenOutcome::Opened { index, vault } = set_outcome(&files, b"real", &[]) else {
            panic!("expected the real vault");
        };
        assert_eq!((index, vault.doc.name.as_str()), (2, "Real"));
        let OpenOutcome::Opened { index, vault } = set_outcome(&files, b"decoy", &[]) else {
            panic!("expected the decoy");
        };
        assert_eq!((index, vault.doc.name.as_str()), (1, "Decoy"));
        assert!(vault.doc.linked.is_empty());
        assert!(matches!(
            set_outcome(&files, b"nope", &[]),
            OpenOutcome::NoMatch
        ));
    }

    #[test]
    fn duress_wipe_password_reports_the_action_without_opening() {
        let config = KdfConfig::new(KdfParams::default()).unwrap();
        let root = config.derive_for_tests(b"real").unwrap();
        let mut vault = Vault::create_with_root("V", &root, &config, 1, 1).unwrap();
        let duress = config.derive_for_tests(b"panic").unwrap();
        vault
            .add_slot_with_root(&duress, "Panic", SlotAction::WipeLocal, None, 2)
            .unwrap();
        let file = vault.save().unwrap();
        assert!(matches!(
            set_outcome(&[&file], b"panic", &[]),
            OpenOutcome::Duress {
                action: SlotAction::WipeLocal
            }
        ));
        assert!(matches!(
            set_outcome(&[&file], b"real", &[]),
            OpenOutcome::Opened { .. }
        ));
    }

    #[test]
    fn a_duress_password_equal_to_an_existing_one_is_refused() {
        let mut vault = fast_vault(1);
        let same = vault
            .header
            .config()
            .unwrap()
            .derive_for_tests(b"pw")
            .unwrap();
        let refused = vault.add_slot_with_root(&same, "Panic", SlotAction::WipeLocal, None, 2);
        assert!(refused.is_err());
    }

    #[test]
    fn missing_factor_is_reported_and_supplying_it_opens() {
        let mut vault = fast_vault(1);
        let config = vault.header.config().unwrap();
        let root = config.derive_for_tests(b"second").unwrap();
        let output = format::key_file_factor(b"file");
        vault
            .add_slot_with_root(
                &root,
                "With file",
                SlotAction::Open,
                Some((&FactorKind::KeyFile, &output)),
                2,
            )
            .unwrap();
        let file = vault.save().unwrap();
        assert!(matches!(
            set_outcome(&[&file], b"second", &[]),
            OpenOutcome::NeedsFactor(_)
        ));
        assert!(matches!(
            set_outcome(&[&file], b"second", &[output]),
            OpenOutcome::Opened { .. }
        ));
    }

    #[test]
    fn password_edits_keep_the_old_value_in_history() {
        let mut vault = fast_vault(1);
        let at = vault.tick(2000);
        let id = vault
            .add(login_entry("Mail", "me", "one", at), 2000)
            .unwrap();
        vault
            .edit(&id, 3000, |entry, at| entry.set_password("two".into(), at))
            .unwrap();
        vault
            .edit(&id, 4000, |entry, at| entry.set_password("two".into(), at))
            .unwrap();
        let entry = vault.get(&id).unwrap();
        let past: Vec<_> = entry
            .history_of("password")
            .map(|r| r.value.expose().to_owned())
            .collect();
        assert_eq!(past, vec!["one"]);
    }

    #[test]
    fn history_limit_keeps_only_the_newest() {
        let mut vault = fast_vault(1);
        vault.doc.prefs.history_limit = 2;
        let at = vault.tick(2000);
        let id = vault
            .add(login_entry("Mail", "me", "p0", at), 2000)
            .unwrap();
        for n in 1..=5 {
            vault
                .edit(&id, 2000 + n, |entry, at| {
                    entry.set_password(format!("p{n}").into(), at)
                })
                .unwrap();
        }
        let past: Vec<_> = vault
            .get(&id)
            .unwrap()
            .history_of("password")
            .map(|r| r.value.expose().to_owned())
            .collect();
        assert_eq!(past, vec!["p3", "p4"]);
    }

    #[test]
    fn trash_restore_purge_and_sweep() {
        let mut vault = fast_vault(1);
        let at = vault.tick(2000);
        let keep = vault.add(login_entry("Keep", "", "", at), 2000).unwrap();
        let gone = vault.add(login_entry("Gone", "", "", at), 2000).unwrap();
        let timed = vault.add(login_entry("Timed", "", "", at), 2000).unwrap();
        vault.trash(&keep, 3000).unwrap();
        assert!(vault.get(&keep).unwrap().is_trashed());
        assert_eq!(vault.visible().count(), 2);
        vault.restore(&keep, 3500).unwrap();
        assert!(!vault.get(&keep).unwrap().is_trashed());

        let (record, _) = Vault::seal_attachment("a.txt", "text/plain", b"hi").unwrap();
        let blob = record.blob.clone();
        vault
            .edit(&timed, 3600, |entry, at| {
                entry.common.destroy_at_ms = Lww::new(Some(5000), at);
                entry.common.attachments = Lww::new(vec![record], at);
            })
            .unwrap();
        vault.trash(&gone, 4000).unwrap();
        assert!(vault.sweep(4500).is_empty());
        assert_eq!(vault.sweep(5000), vec![blob]);
        assert!(vault.get(&timed).is_none());
        assert!(vault.doc.tombstones.contains_key(&timed));
        assert!(vault.get(&gone).is_some());
        vault.sweep(4000 + 31 * DAY_MS);
        assert!(vault.get(&gone).is_none());
        assert!(vault.get(&keep).is_some());
    }

    #[test]
    fn hidden_entries_leave_every_list() {
        let mut vault = fast_vault(1);
        let at = vault.tick(2000);
        let id = vault.add(login_entry("Secret", "", "", at), 2000).unwrap();
        assert_eq!(vault.visible().count(), 1);
        vault
            .edit(&id, 2100, |entry, at| {
                entry.common.hidden = Lww::new(true, at)
            })
            .unwrap();
        assert_eq!(vault.visible().count(), 0);
        assert!(vault.get(&id).is_some());
    }

    #[test]
    fn attachments_round_trip_and_reject_tampering() {
        let plain = vec![42u8; 200_000];
        let (record, mut sealed) =
            Vault::seal_attachment("doc.pdf", "application/pdf", &plain).unwrap();
        assert_eq!(
            Vault::open_attachment(&record, &sealed).unwrap().as_slice(),
            plain.as_slice()
        );
        sealed[40] ^= 1;
        assert_eq!(
            Vault::open_attachment(&record, &sealed).err(),
            Some(Error::Integrity)
        );
    }

    #[test]
    fn two_devices_keep_both_field_edits_and_lose_no_password() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Mail", "me", "base", at), 2000).unwrap();
        let shared = a.save().unwrap();
        let mut b = Vault::from_file(&shared, a.key().clone_key(), 2).unwrap();

        a.edit(&id, 3000, |entry, at| {
            entry.common.title = Lww::new("Mail A".into(), at)
        })
        .unwrap();
        a.edit(&id, 3000, |entry, at| {
            entry.set_password("from-a".into(), at)
        })
        .unwrap();
        b.edit(&id, 4000, |entry, at| {
            if let Some(login) = entry.login_mut() {
                login.username = Lww::new("me@b".into(), at);
            }
        })
        .unwrap();
        b.edit(&id, 4000, |entry, at| {
            entry.set_password("from-b".into(), at)
        })
        .unwrap();

        let (file_a, file_b) = (a.save().unwrap(), b.save().unwrap());
        a.merge_file(&file_b).unwrap();
        b.merge_file(&file_a).unwrap();

        let entry = a.get(&id).unwrap();
        assert_eq!(entry.common.title.v, "Mail A");
        let login = entry.login().unwrap();
        assert_eq!(login.username.v, "me@b");
        assert_eq!(login.password.v.expose(), "from-b");
        let past: Vec<_> = entry
            .history_of("password")
            .map(|r| r.value.expose().to_owned())
            .collect();
        assert!(past.contains(&"base".to_owned()));
        assert!(
            past.contains(&"from-a".to_owned()),
            "the losing password must survive"
        );
        assert_eq!(a.doc.entries, b.doc.entries, "both devices must converge");
    }

    #[test]
    fn delete_beats_an_older_edit_but_not_a_newer_one() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("X", "", "", at), 2000).unwrap();
        let shared = a.save().unwrap();
        let mut b = Vault::from_file(&shared, a.key().clone_key(), 2).unwrap();
        a.edit(&id, 3000, |entry, at| {
            entry.common.title = Lww::new("older".into(), at)
        })
        .unwrap();
        b.purge(&id, 4000);
        let file_b = b.save().unwrap();
        a.merge_file(&file_b).unwrap();
        assert!(a.get(&id).is_none());

        let mut c = fast_vault(1);
        let at = c.tick(2000);
        let id = c.add(login_entry("X", "", "", at), 2000).unwrap();
        let shared = c.save().unwrap();
        let mut d = Vault::from_file(&shared, c.key().clone_key(), 2).unwrap();
        d.purge(&id, 3000);
        c.edit(&id, 4000, |entry, at| {
            entry.common.title = Lww::new("newer".into(), at)
        })
        .unwrap();
        let file_d = d.save().unwrap();
        c.merge_file(&file_d).unwrap();
        assert_eq!(c.get(&id).unwrap().common.title.v, "newer");
    }

    #[test]
    fn a_stale_edit_after_a_delete_brings_back_only_what_it_wrote() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a
            .add(login_entry("Bank", "me", "secret-before-delete", at), 2000)
            .unwrap();
        let shared = a.save().unwrap();
        let mut b = Vault::from_file(&shared, a.key().clone_key(), 2).unwrap();
        a.purge(&id, 3000);
        b.edit(&id, 4000, |entry, at| {
            entry.common.title = Lww::new("Renamed".into(), at)
        })
        .unwrap();
        let file_b = b.save().unwrap();
        a.merge_file(&file_b).unwrap();
        let entry = a.get(&id).unwrap();
        assert_eq!(entry.common.title.v, "Renamed");
        assert!(
            entry.login().unwrap().password.v.is_empty(),
            "the deleted password must stay deleted"
        );
    }

    #[test]
    fn sync_names_differ_by_device_and_vault_and_stay_stable() {
        let a = fast_vault(1);
        let b = Vault::from_file(&fast_vault_file(&a), a.key().clone_key(), 2).unwrap();
        let again = Vault::from_file(&fast_vault_file(&a), a.key().clone_key(), 2).unwrap();
        assert_ne!(a.sync_file_name().unwrap(), b.sync_file_name().unwrap());
        assert_eq!(b.sync_file_name().unwrap(), again.sync_file_name().unwrap());
        assert_ne!(a.sync_file_name().unwrap(), fast_vault(1).sync_file_name().unwrap());
    }

    fn fast_vault_file(vault: &Vault) -> Vec<u8> {
        let plain = rmp_serde::to_vec_named(&vault.doc).unwrap();
        format::seal_file(&vault.header, &vault.key, &plain).unwrap()
    }

    #[test]
    fn slot_change_travels_with_a_merge() {
        let mut a = fast_vault(1);
        let shared = a.save().unwrap();
        let mut b = Vault::from_file(&shared, a.key().clone_key(), 2).unwrap();
        let config = a.header.config().unwrap();
        let root = config.derive_for_tests(b"added").unwrap();
        a.add_slot_with_root(&root, "Added", SlotAction::Open, None, 5000)
            .unwrap();
        let file_a = a.save().unwrap();
        b.merge_file(&file_a).unwrap();
        let merged = b.save().unwrap();
        assert!(matches!(
            set_outcome(&[&merged], b"added", &[]),
            OpenOutcome::Opened { .. }
        ));
    }

    #[test]
    fn rekey_shuts_out_the_old_key_and_other_slots() {
        let mut vault = fast_vault(1);
        let config = vault.header.config().unwrap();
        let other = config.derive_for_tests(b"other").unwrap();
        vault
            .add_slot_with_root(&other, "Other", SlotAction::Open, None, 2)
            .unwrap();
        let before = vault.save().unwrap();
        let old_key = vault.key().clone_key();
        let new_root = config.derive_for_tests(b"new").unwrap();
        vault.rekey_with_root(&new_root, &config, 3).unwrap();
        let after = vault.save().unwrap();
        assert!(matches!(
            set_outcome(&[&after], b"other", &[]),
            OpenOutcome::NoMatch
        ));
        assert!(matches!(
            set_outcome(&[&after], b"new", &[]),
            OpenOutcome::Opened { .. }
        ));
        assert!(format::open_file(&after, &old_key).is_err());
        assert!(
            vault.merge_file(&before).is_err(),
            "an old copy under the old key must not merge"
        );
    }

    #[test]
    fn last_slot_cannot_be_removed_and_full_vaults_say_so() {
        let mut vault = fast_vault(1);
        assert!(vault.remove_slot(0, 2).is_err());
        let config = vault.header.config().unwrap();
        let root = |n: usize| config.derive_for_tests(format!("x{n}").as_bytes()).unwrap();
        for n in 1..SLOTS {
            vault
                .add_slot_with_root(&root(n), "x", SlotAction::Open, None, 2)
                .unwrap();
        }
        assert_eq!(
            vault
                .add_slot_with_root(&root(100), "x", SlotAction::Open, None, 2)
                .err(),
            Some(Error::NoFreeSlot)
        );
        vault.remove_slot(0, 3).unwrap();
        assert_eq!(
            vault
                .add_slot_with_root(&root(101), "x", SlotAction::Open, None, 4)
                .unwrap(),
            0
        );
    }

    #[test]
    fn newer_layout_opens_read_only_and_refuses_to_save() {
        let mut vault = fast_vault(1);
        vault.doc.minor = DOC_MINOR + 1;
        let file = vault.save().unwrap();
        let mut again = Vault::from_file(&file, vault.key().clone_key(), 2).unwrap();
        assert!(again.is_read_only());
        assert!(again.save().is_err());
    }

    #[test]
    fn no_secret_appears_in_the_file_or_in_debug_output() {
        let mut vault = fast_vault(1);
        let at = vault.tick(2000);
        vault
            .add(
                login_entry("TitleMarker", "UserMarker", "PasswordMarker", at),
                2000,
            )
            .unwrap();
        let file = vault.save().unwrap();
        for marker in [&b"TitleMarker"[..], b"UserMarker", b"PasswordMarker"] {
            assert!(!file.windows(marker.len()).any(|window| window == marker));
        }
        assert!(!format!("{vault:?}").contains("PasswordMarker"));
    }

    #[test]
    fn app_settings_follow_the_vault_by_file_and_by_delta() {
        let mut a = fast_vault(1);
        let shared = a.save().unwrap();
        let mut b = Vault::from_file(&shared, a.key().clone_key(), 2).unwrap();
        let mut c = Vault::from_file(&shared, a.key().clone_key(), 3).unwrap();

        let at = a.tick(2000);
        a.doc.prefs.app.insert("layout".into(), Lww::new("\"grid\"".into(), at));
        let at = b.tick(3000);
        b.doc.prefs.app.insert("layout".into(), Lww::new("\"compact\"".into(), at));
        b.doc.prefs.app.insert("gen_length".into(), Lww::new("32".into(), at));
        let at = b.tick(3000);
        b.doc.prefs.tag_icons.insert("work".into(), Lww::new("work".into(), at));

        // By whole file: the later write wins, whichever way round.
        a.merge_file(&b.save().unwrap()).unwrap();
        assert_eq!(a.doc.prefs.app["layout"].v, "\"compact\"");
        assert_eq!(a.doc.prefs.app["gen_length"].v, "32");

        // By delta, as the local network sync sends it, tag glyphs included.
        let delta = a.delta_for(&c.digests().unwrap()).unwrap();
        assert_eq!(delta.app_prefs.len(), 2);
        assert_eq!(delta.tag_icons.len(), 1);
        c.apply_delta(delta).unwrap();
        assert_eq!(c.doc.prefs.app, a.doc.prefs.app);
        assert_eq!(c.doc.prefs.tag_icons, a.doc.prefs.tag_icons);
        assert_eq!(c.state_hash().unwrap(), a.state_hash().unwrap());
        assert!(a.delta_for(&c.digests().unwrap()).unwrap().app_prefs.is_empty());

        // A setting written after the merge outranks what came in.
        let at = c.tick(2500);
        assert!(at > a.doc.prefs.app["layout"].at);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(unhex(&hex(&[0, 255, 16])).unwrap(), vec![0, 255, 16]);
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }
}
