//! An unlocked vault: the document, its key and the rules for changing it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::clock::{Clock, Hlc};
use crate::crypto;
use crate::format::{
    self, FactorKind, FactorOutput, Header, KdfConfig, RootKey, SLOTS, SlotAction, Unlock, VaultKey,
};
use crate::model::{SLOT_ANCESTRY, Attachment, Entry, Folder, Id, Kind, LinkedVault, Lww, SlotInfo, VaultDoc};
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
    // `from_str_radix` alone would also take a sign, so `+f` would read as a byte.
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
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

/// Clocks further apart than this are worth a warning: an hour.
pub const CLOCK_WARNING_MS: u64 = 60 * 60 * 1000;

/// A device's clock that looks wrong, from [`Vault::clock_warning`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockWarning {
    /// This device's clock is the wrong one, behind the others.
    pub this_device: bool,
    /// Otherwise the device whose clock is ahead, by its number.
    pub device: Option<u32>,
    /// By about how much, in milliseconds.
    pub by_ms: u64,
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
    /// When each slot was last written, by place. Empty from a peer from before slots kept
    /// their own times, which is then judged by `slots_at` alone.
    pub slot_times: Vec<Hlc>,
    /// What each delete from the trash saw of the item it removed.
    pub purge_seen: BTreeMap<Id, Hlc>,
    /// When each device's name was last written.
    pub devices: BTreeMap<String, Hlc>,
    /// The latest password change lost to another made at the same time.
    pub password_dropped: Hlc,
    /// The latest such loss the owner has been told about.
    pub password_dropped_seen: Hlc,
    /// When the trash's length was last set.
    pub trash_days_at: Hlc,
    /// When the history's length was last set.
    pub history_limit_at: Hlc,
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
    /// What deletes from the trash saw, where the peer holds less.
    pub purge_seen: BTreeMap<Id, Hlc>,
    /// Device names the peer has not seen.
    pub devices: BTreeMap<String, Lww<String>>,
    /// The sender's latest lost password change.
    pub password_dropped: Hlc,
    /// The latest such loss the sender's owner was told about.
    pub password_dropped_seen: Hlc,
    /// The trash's length, when set later than the peer's.
    pub trash_days: Option<Lww<u32>>,
    /// The history's length, when set later than the peer's.
    pub history_limit: Option<Lww<u32>>,
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

/// A slot a password, with any factor it needs, was just checked against. Only
/// [`Vault::check_slot`] makes one, and [`Vault::change_password`] takes one, so a new password
/// keeps the key file or hardware key the old one needed: the factor's output is here, and was
/// shown to open the slot.
pub struct CheckedSlot {
    index: usize,
    factor: Option<(FactorKind, FactorOutput)>,
    /// The sealed record when checked, so a slot rewritten since is not changed blind.
    record: Vec<u8>,
}

impl std::fmt::Debug for CheckedSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckedSlot")
            .field("index", &self.index)
            .field("factor", &self.factor())
            .finish_non_exhaustive()
    }
}

impl CheckedSlot {
    /// Which slot.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The second factor the slot needs, if any.
    pub fn factor(&self) -> Option<&FactorKind> {
        self.factor.as_ref().map(|(kind, _)| kind)
    }
}

/// What came of checking a password against the open vault.
#[derive(Debug)]
pub enum SlotCheck {
    /// It opens this slot.
    Opens(CheckedSlot),
    /// It is right for a slot that also needs one of these, and none given opened it.
    NeedsFactor(Vec<FactorKind>),
    /// It opens nothing, or only a duress action.
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
                slot,
            } => {
                let mut vault = Vault::from_file(file, key, device)?;
                vault.note_opened_with(slot);
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
            opens: Some(true),
            at,
            line: at,
            replaced: Vec::new(),
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

    /// Which slot this password opens, so a change replaces that one and not another.
    pub fn password_slot(&self, password: &[u8], factors: &[FactorOutput]) -> Result<Option<usize>> {
        Ok(match self.check_slot(password, factors)? {
            SlotCheck::Opens(slot) => Some(slot.index),
            SlotCheck::NeedsFactor(_) | SlotCheck::NoMatch => None,
        })
    }

    /// Checks a password, with any factor outputs, against the open vault, for a change of
    /// password next: says which slot it opens and keeps the factor that slot needs.
    pub fn check_slot(&self, password: &[u8], factors: &[FactorOutput]) -> Result<SlotCheck> {
        let root = self.header.config()?.derive(password)?;
        self.check_slot_with_root(&root, factors)
    }

    fn check_slot_with_root(&self, root: &RootKey, factors: &[FactorOutput]) -> Result<SlotCheck> {
        match self.header.slot_match_with(root, factors, &self.key)? {
            Some((index, SlotAction::Open, used)) => {
                let factor = match used {
                    Some(position) => {
                        let kind = self.header.slot_factor(root, index)?.ok_or(Error::Malformed)?;
                        let output = factors.get(position).ok_or(Error::Malformed)?.clone();
                        Some((kind, output))
                    }
                    None => None,
                };
                let record = self.header.slot_record(index).ok_or(Error::Malformed)?.to_vec();
                Ok(SlotCheck::Opens(CheckedSlot { index, factor, record }))
            }
            Some(_) => Ok(SlotCheck::NoMatch),
            None => {
                let mut needed = Vec::new();
                for index in 0..SLOTS {
                    needed.extend(self.header.slot_factor(root, index)?);
                }
                Ok(if needed.is_empty() { SlotCheck::NoMatch } else { SlotCheck::NeedsFactor(needed) })
            }
        }
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
        let prior = self.get(id).ok_or_else(|| Error::Invalid("no such entry".into()))?.latest();
        // After everything the item holds, even a write from a device whose clock runs more than
        // a day ahead, or this edit would lose to the value it replaces on the next sync.
        let at = self.clock.tick_over(now_ms, prior);
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
        // Trashed again, it keeps the day it went: the trash empties on that day's clock.
        if self.get(id).is_some_and(Entry::is_trashed) {
            return Ok(());
        }
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

    /// Removes for good. Returns the blobs to delete, whose keys are now gone. A blob another
    /// item still holds, as the one kept after a merge holds the files of those merged into
    /// it, is not among them.
    ///
    /// The delete notes where the item stood in or out of the trash as this device saw it, so
    /// that an item another device restored meanwhile, not knowing of the delete, comes back
    /// whole rather than going: see [`VaultDoc::apply_tombstones`].
    pub fn purge(&mut self, id: &str, now_ms: u64) -> Vec<String> {
        let prior = self.get(id).map(Entry::latest).unwrap_or_default();
        let at = self.clock.tick_over(now_ms, prior);
        let removed = self.doc.entries.remove(id);
        if let Some(entry) = &removed {
            let seen = self.doc.purge_seen.entry(id.to_owned()).or_default();
            *seen = (*seen).max(entry.common.deleted_at_ms.at);
        }
        let blobs = removed.map(|entry| blob_names(&entry.common.attachments.v));
        let tombstone = self.doc.tombstones.entry(id.to_owned()).or_default();
        *tombstone = (*tombstone).max(at);
        let mut blobs = blobs.unwrap_or_default();
        blobs.retain(|blob| !self.holds_blob(blob));
        blobs
    }

    /// Whether any item still refers to an attachment blob.
    pub fn holds_blob(&self, blob: &str) -> bool {
        self.doc
            .entries
            .values()
            .any(|entry| entry.common.attachments.v.iter().any(|a| a.blob == blob))
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
    /// Refuses a file under a different key, and adopts each of the other copy's slot records
    /// that is newer than this one's.
    pub fn merge_file(&mut self, file: &[u8]) -> Result<u64> {
        let (header, plain) = format::open_file(file, &self.key)?;
        let other: VaultDoc = rmp_serde::from_slice(&plain).map_err(|_| Error::Encoding)?;
        observe_doc(&mut self.clock, &other);
        // Slot by slot, as the slot list merges: a record the other copy wrote later comes with
        // the list's entry for it.
        for index in self.doc.newer_slots(&other) {
            self.header.copy_slot(&header, index)?;
        }
        let generation = other.generation;
        self.doc.merge_from(&other);
        self.after_merge();
        Ok(generation)
    }

    /// What every merge ends with: a newer layout makes the vault read-only, and the history
    /// limit applies to what arrived as much as to edits made here. Without that, a revision
    /// one device had trimmed came back from every other device on the next sync, and the
    /// history of an item edited elsewhere grew without bound. Every copy trims alike, newest
    /// kept, so they still agree.
    fn after_merge(&mut self) {
        if self.doc.minor > DOC_MINOR {
            self.read_only = true;
        }
        let limit = self.doc.prefs.history_limit as usize;
        for entry in self.doc.entries.values_mut() {
            trim_history(entry, limit);
        }
    }

    /// Sets how many days the trash keeps things, for every device.
    pub fn set_trash_days(&mut self, days: u32, now_ms: u64) {
        let at = self.clock.tick_over(now_ms, self.doc.prefs.trash_days_at);
        self.doc.prefs.trash_days = days;
        self.doc.prefs.trash_days_at = at;
    }

    /// Sets how many past values each field keeps, for every device.
    pub fn set_history_limit(&mut self, limit: u32, now_ms: u64) {
        let at = self.clock.tick_over(now_ms, self.doc.prefs.history_limit_at);
        self.doc.prefs.history_limit = limit;
        self.doc.prefs.history_limit_at = at;
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
            slot_times: (0..SLOTS).map(|index| self.doc.slot_time(index)).collect(),
            purge_seen: self.doc.purge_seen.clone(),
            devices: self.doc.prefs.devices.iter().map(|(k, v)| (k.clone(), v.at)).collect(),
            password_dropped: self.doc.password_dropped,
            password_dropped_seen: self.doc.password_dropped_seen,
            trash_days_at: self.doc.prefs.trash_days_at,
            history_limit_at: self.doc.prefs.history_limit_at,
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

    /// What a peer with those fingerprints lacks. Slot records only exist in the file, so when
    /// any of this device's differ the delta also carries a sealed copy of it.
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
            // Equal times too: a shared vault's newest version can differ under one write time,
            // since a merge keeps the highest of both, and links are few.
            .filter(|link| theirs.linked.get(&link.file).is_none_or(|known| *known <= link.at))
            .cloned()
            .collect();
        let tag_icons = newer(&self.doc.prefs.tag_icons, &theirs.tag_icons);
        let app_prefs = newer(&self.doc.prefs.app, &theirs.app_prefs);
        let inboxes = newer(&self.doc.prefs.inboxes, &theirs.inboxes);
        let devices = newer(&self.doc.prefs.devices, &theirs.devices);
        let purge_seen = mine
            .purge_seen
            .iter()
            .filter(|(id, at)| theirs.purge_seen.get(*id).is_none_or(|known| known < *at))
            .map(|(id, at)| (id.clone(), *at))
            .collect();
        let prefs = &self.doc.prefs;
        let trash_days = (prefs.trash_days_at > theirs.trash_days_at).then(|| Lww::new(prefs.trash_days, prefs.trash_days_at));
        let history_limit =
            (prefs.history_limit_at > theirs.history_limit_at).then(|| Lww::new(prefs.history_limit, prefs.history_limit_at));
        // Any slot written differently here, older too: the peer then sees both of two changes
        // of one password that landed in one slot, and can tell its owner one was lost.
        let slots_differ = mine.slots_at > theirs.slots_at
            || mine.slot_times.iter().zip(&theirs.slot_times).any(|(mine, theirs)| mine != theirs);
        let file = if slots_differ {
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
            purge_seen,
            devices,
            password_dropped: mine.password_dropped,
            password_dropped_seen: mine.password_dropped_seen,
            trash_days,
            history_limit,
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
            purge_seen: delta.purge_seen,
            password_dropped: delta.password_dropped,
            password_dropped_seen: delta.password_dropped_seen,
            linked: delta.linked,
            prefs: crate::model::VaultPrefs {
                tag_icons: delta.tag_icons,
                app: delta.app_prefs,
                inboxes: delta.inboxes,
                devices: delta.devices,
                trash_days: delta.trash_days.as_ref().map_or(0, |v| v.v),
                trash_days_at: delta.trash_days.map(|v| v.at).unwrap_or_default(),
                history_limit: delta.history_limit.as_ref().map_or(0, |v| v.v),
                history_limit_at: delta.history_limit.map(|v| v.at).unwrap_or_default(),
            },
            ..VaultDoc::default()
        };
        observe_doc(&mut self.clock, &partial);
        self.doc.merge_from(&partial);
        self.after_merge();
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

    fn note_slot(&mut self, index: usize, label: &str, opens: Option<bool>, now_ms: u64) {
        let at = self.slot_tick(now_ms);
        let line = if opens.is_some() { at } else { Hlc::default() };
        let slot = SlotInfo {
            used: opens.is_some(),
            label: label.to_owned(),
            opens,
            at,
            line,
            replaced: Vec::new(),
        };
        self.set_slot(index, slot);
    }

    /// A moment for a change of slots, after every slot this copy holds was written.
    fn slot_tick(&mut self, now_ms: u64) -> Hlc {
        let prior = (0..SLOTS).map(|index| self.doc.slot_time(index)).fold(self.doc.slots_at, Hlc::max);
        self.clock.tick_over(now_ms, prior)
    }

    fn set_slot(&mut self, index: usize, slot: SlotInfo) {
        // A document without the full list, as one written without it reads, is filled out
        // rather than indexed past its end.
        if self.doc.slots.len() <= index {
            self.doc.slots.resize(SLOTS.max(index + 1), SlotInfo::default());
        }
        // The others keep the time they had from the list's, which is about to move.
        for held in &mut self.doc.slots {
            if held.at.is_zero() {
                held.at = self.doc.slots_at;
            }
        }
        self.doc.slots_at = self.doc.slots_at.max(slot.at);
        self.doc.slots[index] = slot;
        // The owner is changing the ways in: a note about a lost change has done its job.
        self.doc.password_dropped_seen = self.doc.password_dropped_seen.max(self.doc.password_dropped);
    }

    /// Where a changed password goes: a free slot, chosen by this device's number, with the old
    /// one cleared. Two devices changing one password at once then most likely write different
    /// slots, and a merge keeps both, rather than the later change silently replacing the other,
    /// which would leave someone's new password opening nothing. A full vault, or one without a
    /// full list of what each slot holds, changes it in place as before.
    fn change_target(&self, index: usize) -> usize {
        if self.doc.slots.len() < SLOTS {
            return index;
        }
        let free: Vec<usize> = (0..SLOTS).filter(|&at| at != index && !self.doc.slots[at].used).collect();
        if free.is_empty() {
            return index;
        }
        free[self.device as usize % free.len()]
    }

    /// Records that a slot written before slots kept what they do opens the vault, since
    /// its password just opened it. Not a change of its own: the clock does not move, so
    /// this rides along with the next save and gives way to any newer list of slots.
    fn note_opened_with(&mut self, index: usize) {
        if let Some(slot) = self.doc.slots.get_mut(index)
            && slot.used
            && slot.opens.is_none()
        {
            slot.opens = Some(true);
        }
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
        self.note_slot(index, label, Some(action == SlotAction::Open), now_ms);
        Ok(index)
    }

    /// Removes a way in. Refuses to remove the last one known to open the vault: a duress
    /// password left on its own would lock the owner out.
    pub fn remove_slot(&mut self, index: usize, now_ms: u64) -> Result<()> {
        let duress = self.doc.slots.get(index).is_some_and(|slot| slot.opens == Some(false));
        let others = || self.doc.slots.iter().enumerate().filter(move |(at, slot)| *at != index && slot.used);
        if !duress && !others().any(|(_, slot)| slot.opens == Some(true)) {
            return Err(Error::Invalid(if others().any(|(_, slot)| slot.opens.is_none()) {
                // Slots from before this was kept: one of them may open the vault, but a
                // duress password looks the same until it is typed.
                "no other way in is known to open the vault. If another password does, unlock with it once, then remove this one".into()
            } else {
                "a vault needs at least one way in that opens it".into()
            }));
        }
        self.header.clear_slot(index)?;
        self.note_slot(index, "", None, now_ms);
        Ok(())
    }

    /// Replaces the password in a slot just checked with [`Vault::check_slot`]. A slot that
    /// needed a key file or hardware key goes on needing the same one. Old backups still open
    /// with the old password; [`Vault::rekey`] is what cuts them off.
    pub fn change_password(&mut self, slot: &CheckedSlot, password: &[u8], now_ms: u64) -> Result<()> {
        let root = self.header.config()?.derive(password)?;
        self.change_password_with_root(slot, &root, now_ms)
    }

    fn change_password_with_root(&mut self, slot: &CheckedSlot, root: &RootKey, now_ms: u64) -> Result<()> {
        if self.header.slot_record(slot.index) != Some(slot.record.as_slice()) {
            return Err(Error::Invalid(
                "that password was changed since it was checked. Enter the current one again".into(),
            ));
        }
        let factor = slot.factor.as_ref().map(|(kind, output)| (kind, output));
        self.write_password(slot.index, root, factor, now_ms)
    }

    /// Gives a slot a password alone, whatever it needed before. Only for a vault opened
    /// without its owner's password, as emergency recovery does: the old owner's key file or
    /// hardware key is out of reach there, and the new password takes over the slot.
    pub fn replace_password(&mut self, index: usize, password: &[u8], now_ms: u64) -> Result<()> {
        let root = self.header.config()?.derive(password)?;
        self.write_password(index, &root, None, now_ms)
    }

    fn write_password(
        &mut self,
        index: usize,
        root: &RootKey,
        factor: Option<(&FactorKind, &FactorOutput)>,
        now_ms: u64,
    ) -> Result<()> {
        let old = self.doc.slots.get(index).cloned().unwrap_or_default();
        let old_at = self.doc.slot_time(index);
        let target = self.change_target(index);
        self.header.write_slot(target, root, &self.key, SlotAction::Open, factor)?;
        let at = self.slot_tick(now_ms);
        if target != index {
            self.header.clear_slot(index)?;
            self.set_slot(index, SlotInfo { at, ..SlotInfo::default() });
        }
        let mut replaced = old.replaced;
        replaced.push(old_at);
        if replaced.len() > SLOT_ANCESTRY {
            replaced.drain(..replaced.len() - SLOT_ANCESTRY);
        }
        let slot = SlotInfo {
            used: true,
            label: old.label,
            opens: Some(true),
            at,
            line: if old.line.is_zero() { old_at } else { old.line },
            replaced,
        };
        self.set_slot(target, slot);
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
        let at = self.slot_tick(now_ms);
        self.doc.slots = vec![SlotInfo { at, ..SlotInfo::default() }; SLOTS];
        self.set_slot(0, SlotInfo { used: true, label: "Master password".to_owned(), opens: Some(true), at, line: at, replaced: Vec::new() });
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

    /// This device's number, as its writes carry it.
    pub fn device(&self) -> u32 {
        self.device
    }

    /// Records what this device calls itself, for warnings on other devices that name it.
    /// Returns whether anything changed.
    pub fn set_device_name(&mut self, name: &str, now_ms: u64) -> bool {
        let name = name.trim();
        let key = self.device.to_string();
        if name.is_empty() || self.doc.prefs.devices.get(&key).is_some_and(|held| held.v == name) {
            return false;
        }
        let prior = self.doc.prefs.devices.get(&key).map(|held| held.at).unwrap_or_default();
        let at = self.clock.tick_over(now_ms, prior);
        self.doc.prefs.devices.insert(key, Lww::new(name.to_owned(), at));
        true
    }

    /// What a device calls itself, if it ever said.
    pub fn device_name(&self, device: u32) -> Option<&str> {
        self.doc.prefs.devices.get(&device.to_string()).map(|held| held.v.as_str()).filter(|name| !name.is_empty())
    }

    /// Whether some device's clock looks far enough off to tell the owner, judged against this
    /// device's wall clock `now_ms`. Only writes stamped with their writer's own clock count
    /// ([`Hlc::is_wall_time`]): a moment carried along from another device says nothing about
    /// the device that carried it. Imports, written as device zero, never count.
    ///
    /// This device's clock is the wrong one when what it wrote itself is more than a day ahead
    /// of its clock now (its clock went back), or when two or more other devices are ahead of
    /// it. One other device ahead is that device's clock. Nothing here changes how a merge
    /// decides; it is only a warning.
    pub fn clock_warning(&self, now_ms: u64) -> Option<ClockWarning> {
        let mut stamps = Vec::new();
        for entry in self.doc.entries.values() {
            entry.stamps(&mut stamps);
            stamps.extend(entry.history.iter().map(|revision| revision.at));
        }
        for folder in self.doc.folders.values() {
            stamps.extend([folder.name.at, folder.parent.at]);
        }
        stamps.extend(self.doc.tombstones.values().copied());
        stamps.extend(self.doc.linked.iter().map(|link| link.at));
        let prefs = &self.doc.prefs;
        stamps.extend(prefs.tag_icons.values().chain(prefs.app.values()).chain(prefs.devices.values()).map(|value| value.at));
        stamps.extend(prefs.inboxes.values().map(|inbox| inbox.at));
        stamps.extend([prefs.trash_days_at, prefs.history_limit_at, self.doc.slots_at]);
        stamps.extend(self.doc.slots.iter().map(|slot| slot.at));
        let mut peaks: BTreeMap<u32, u64> = BTreeMap::new();
        for at in stamps.into_iter().filter(|at| at.is_wall_time() && at.device != 0) {
            let peak = peaks.entry(at.device).or_default();
            *peak = (*peak).max(at.ms);
        }
        let lead = |ms: u64| ms.saturating_sub(now_ms);
        let own = peaks.get(&self.device).map_or(0, |ms| lead(*ms));
        let ahead: Vec<(u32, u64)> = peaks
            .iter()
            .filter(|(device, ms)| **device != self.device && lead(**ms) > CLOCK_WARNING_MS)
            .map(|(device, ms)| (*device, lead(*ms)))
            .collect();
        if own > crate::clock::MAX_AHEAD_MS {
            return Some(ClockWarning { this_device: true, device: None, by_ms: own });
        }
        match ahead.as_slice() {
            [] => None,
            [(device, by_ms)] => Some(ClockWarning { this_device: false, device: Some(*device), by_ms: *by_ms }),
            many => Some(ClockWarning {
                this_device: true,
                device: None,
                by_ms: many.iter().map(|(_, by)| *by).min().unwrap_or_default(),
            }),
        }
    }

    /// Marks a lost change of password as told, on every device.
    pub fn password_dropped_seen(&mut self) {
        self.doc.password_dropped_seen = self.doc.password_dropped_seen.max(self.doc.password_dropped);
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
    for slot in &doc.slots {
        clock.observe(slot.at);
    }
    for name in doc.prefs.devices.values() {
        clock.observe(name.at);
    }
    for entry in doc.entries.values() {
        clock.observe(entry.latest());
    }
    // Folders and links merge by their write times too: a rename here must sort after the
    // one that arrived, or it would lose to it however much later it was made.
    for folder in doc.folders.values() {
        clock.observe(folder.name.at.max(folder.parent.at));
    }
    for link in &doc.linked {
        clock.observe(link.at);
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
    clock.observe(doc.prefs.trash_days_at);
    clock.observe(doc.prefs.history_limit_at);
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
    fn the_slot_a_password_opens_is_the_one_found() {
        let mut vault = fast_vault(1);
        let config = vault.kdf_config().unwrap();
        let first = config.derive_for_tests(b"pw").unwrap();
        let second = config.derive_for_tests(b"second").unwrap();
        let added = vault
            .add_slot_with_root(&second, "Second", SlotAction::Open, None, 2000)
            .unwrap();
        assert_ne!(added, 0);
        let at = |root: &RootKey| vault.header.slot_match_at(root, &[], &vault.key).unwrap().map(|(i, _)| i);
        assert_eq!(at(&first), Some(0));
        assert_eq!(at(&second), Some(added));
        assert_eq!(at(&config.derive_for_tests(b"wrong").unwrap()), None);
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
    fn a_duress_password_is_never_left_as_the_only_way_in() {
        let config = KdfConfig::new(KdfParams::default()).unwrap();
        let root = config.derive_for_tests(b"real").unwrap();
        let mut vault = Vault::create_with_root("V", &root, &config, 1, 1).unwrap();
        let wipe = config.derive_for_tests(b"wipe").unwrap();
        let lock = config.derive_for_tests(b"lock").unwrap();
        let wipe_at = vault.add_slot_with_root(&wipe, "Wipe", SlotAction::WipeLocal, None, 2).unwrap();
        vault.add_slot_with_root(&lock, "Lock", SlotAction::HardLock, None, 3).unwrap();
        assert!(vault.remove_slot(0, 4).is_err(), "the master password is the only one that opens it");
        vault.remove_slot(wipe_at, 5).unwrap();
        let second = config.derive_for_tests(b"second").unwrap();
        vault.add_slot_with_root(&second, "Second", SlotAction::Open, None, 6).unwrap();
        vault.remove_slot(0, 7).unwrap();
        let file = vault.save().unwrap();
        assert!(matches!(set_outcome(&[&file], b"second", &[]), OpenOutcome::Opened { .. }));
    }

    #[test]
    fn slots_from_before_their_kind_was_kept_are_learned_on_opening() {
        let config = KdfConfig::new(KdfParams::default()).unwrap();
        let root = config.derive_for_tests(b"real").unwrap();
        let mut vault = Vault::create_with_root("V", &root, &config, 1, 1).unwrap();
        let wipe = config.derive_for_tests(b"wipe").unwrap();
        vault.add_slot_with_root(&wipe, "Wipe", SlotAction::WipeLocal, None, 2).unwrap();
        // As an older build wrote them: in use, with nothing said about what they do.
        for slot in &mut vault.doc.slots {
            slot.opens = None;
        }
        assert!(vault.remove_slot(0, 3).is_err(), "unknown slots might all be duress");
        assert!(vault.remove_slot(1, 3).is_err(), "nor is the master password known yet");
        let file = vault.save().unwrap();
        let OpenOutcome::Opened { vault: mut opened, .. } = set_outcome(&[&file], b"real", &[]) else {
            panic!("did not open");
        };
        assert_eq!(opened.doc.slots[0].opens, Some(true));
        assert_eq!(opened.doc.slots[1].opens, None);
        assert!(opened.remove_slot(0, 4).is_err(), "the other slot is still unknown");
        opened.remove_slot(1, 4).unwrap();
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
    fn a_new_password_keeps_the_key_file_its_slot_needed() {
        let mut vault = fast_vault(1);
        let config = vault.header.config().unwrap();
        let old = config.derive_for_tests(b"second").unwrap();
        let output = format::key_file_factor(b"file");
        let index = vault
            .add_slot_with_root(&old, "With file", SlotAction::Open, Some((&FactorKind::KeyFile, &output)), 2)
            .unwrap();

        // Without the key file the password is known to be right, but no slot is handed out.
        assert!(matches!(
            vault.check_slot_with_root(&old, &[]).unwrap(),
            SlotCheck::NeedsFactor(kinds) if kinds == vec![FactorKind::KeyFile]
        ));
        let SlotCheck::Opens(checked) = vault.check_slot_with_root(&old, std::slice::from_ref(&output)).unwrap() else {
            panic!("the password and key file open the slot");
        };
        assert_eq!(checked.index(), index);
        assert_eq!(checked.factor(), Some(&FactorKind::KeyFile));

        let new = config.derive_for_tests(b"renewed").unwrap();
        vault.change_password_with_root(&checked, &new, 3).unwrap();
        let file = vault.save().unwrap();
        assert!(
            matches!(set_outcome(&[&file], b"renewed", &[]), OpenOutcome::NeedsFactor(kinds) if kinds == vec![FactorKind::KeyFile]),
            "the new password alone does not open it"
        );
        assert!(matches!(
            set_outcome(&[&file], b"renewed", std::slice::from_ref(&output)),
            OpenOutcome::Opened { .. }
        ));
        assert!(matches!(set_outcome(&[&file], b"second", &[output]), OpenOutcome::NoMatch));
        assert!(matches!(set_outcome(&[&file], b"pw", &[]), OpenOutcome::Opened { .. }), "the first slot is untouched");

        // The check was for the slot as it was; once rewritten it is checked again.
        let again = config.derive_for_tests(b"third").unwrap();
        assert!(vault.change_password_with_root(&checked, &again, 4).is_err());
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
        // Trashing it again does not wind the trash's clock back.
        vault.trash(&gone, 4400).unwrap();
        assert_eq!(vault.get(&gone).unwrap().common.deleted_at_ms.v, Some(4000));
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
    fn purging_an_item_keeps_a_file_another_item_still_holds() {
        let mut vault = fast_vault(1);
        let at = vault.tick(2000);
        let (record, _) = Vault::seal_attachment("a.txt", "text/plain", b"hi").unwrap();
        let mut kept = login_entry("Kept", "", "", at);
        kept.common.attachments = Lww::new(vec![record.clone()], at);
        let mut merged_away = login_entry("Merged away", "", "", at);
        merged_away.common.attachments = Lww::new(vec![record.clone()], at);
        vault.add(kept, 2000).unwrap();
        let gone = vault.add(merged_away, 2000).unwrap();
        assert!(vault.purge(&gone, 3000).is_empty(), "the kept item still needs the file");
        assert!(vault.holds_blob(&record.blob));
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
        assert_eq!(unhex("+f"), None, "a sign is not a digit");
        assert_eq!(unhex("-1"), None);
    }

    #[test]
    fn a_password_change_on_a_document_without_its_slot_list_does_not_panic() {
        let mut vault = fast_vault(1);
        vault.doc.slots.clear();
        let root = vault.header.config().unwrap().derive_for_tests(b"new").unwrap();
        vault.header.write_slot(0, &root, &vault.key, SlotAction::Open, None).unwrap();
        vault.note_slot(0, "Master password", Some(true), 2000);
        assert_eq!(vault.doc.slots.len(), SLOTS);
        assert!(vault.doc.slots[0].used);
    }

    #[test]
    fn a_folder_renamed_after_a_merge_keeps_the_later_name() {
        let mut ahead = fast_vault(1);
        let folder = ahead.add_folder("Work", None, 1000).unwrap();
        let mut behind = Vault::from_file(&ahead.save().unwrap(), ahead.key().clone_key(), 2).unwrap();
        // The other device's clock runs well ahead of this one's.
        let at = ahead.tick(50_000);
        ahead.doc.folders.get_mut(&folder).unwrap().name = Lww::new("Remote".to_owned(), at);
        behind.merge_file(&ahead.save().unwrap()).unwrap();
        let at = behind.tick(2000);
        behind.doc.folders.get_mut(&folder).unwrap().name = Lww::new("Local".to_owned(), at);
        ahead.merge_file(&behind.save().unwrap()).unwrap();
        assert_eq!(ahead.doc.folders[&folder].name.v, "Local", "the rename made after seeing the other wins");
    }

    fn twin(a: &mut Vault, device: u32) -> Vault {
        Vault::from_file(&a.save().unwrap(), a.key().clone_key(), device).unwrap()
    }

    fn passkey(id: &str, counter: u32, created_ms: u64) -> crate::model::Passkey {
        crate::model::Passkey {
            credential_id: id.into(),
            rp_id: "example.com".into(),
            private_key: Secret::new(format!("key-{id}")),
            counter,
            created_ms,
            ..crate::model::Passkey::default()
        }
    }

    fn passkey_ids(vault: &Vault, id: &str) -> Vec<(String, u32)> {
        vault.get(id).unwrap().login().unwrap().passkeys.v.iter().map(|k| (k.credential_id.clone(), k.counter)).collect()
    }

    #[test]
    fn a_sign_in_on_one_device_does_not_drop_a_passkey_made_on_another() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Site", "me", "", at), 2000).unwrap();
        a.edit(&id, 2000, |e, at| e.login_mut().unwrap().passkeys = Lww::new(vec![passkey("one", 1, 2000)], at)).unwrap();
        let mut b = twin(&mut a, 2);
        // A makes a second passkey; B then signs in with the first, which moves its counter on.
        a.edit(&id, 3000, |e, at| {
            let mut keys = e.login().unwrap().passkeys.v.clone();
            keys.push(passkey("two", 0, 3000));
            e.login_mut().unwrap().passkeys = Lww::new(keys, at);
        })
        .unwrap();
        b.edit(&id, 4000, |e, at| {
            let mut keys = e.login().unwrap().passkeys.v.clone();
            keys[0].counter = 5;
            e.login_mut().unwrap().passkeys = Lww::new(keys, at);
        })
        .unwrap();
        let (file_a, file_b) = (a.save().unwrap(), b.save().unwrap());
        a.merge_file(&file_b).unwrap();
        b.merge_file(&file_a).unwrap();
        let want = vec![("one".to_owned(), 5), ("two".to_owned(), 0)];
        assert_eq!(passkey_ids(&a, &id), want);
        assert_eq!(passkey_ids(&b, &id), want);
        assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
        // A counter never goes back, whichever copy is newer.
        a.merge_file(&file_a).unwrap();
        assert_eq!(passkey_ids(&a, &id), want);
    }

    #[test]
    fn fields_and_attachments_added_on_two_devices_at_once_keep_what_lost_in_history() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Site", "me", "", at), 2000).unwrap();
        let mut b = twin(&mut a, 2);
        let field = |name: &str, value: &str| crate::model::Field { name: name.into(), value: Secret::new(value), kind: crate::model::FieldKind::Hidden };
        let attachment = |blob: &str| Attachment { blob: blob.into(), key: Secret::new(format!("{blob}-key")), name: format!("{blob}.pdf"), ..Attachment::default() };
        a.edit(&id, 3000, |e, at| {
            e.common.custom_fields = Lww::new(vec![field("PIN", "1234")], at);
            e.common.attachments = Lww::new(vec![attachment("from-a")], at);
        })
        .unwrap();
        b.edit(&id, 4000, |e, at| {
            e.common.custom_fields = Lww::new(vec![field("Door", "9999")], at);
            e.common.attachments = Lww::new(vec![attachment("from-b")], at);
        })
        .unwrap();
        let (file_a, file_b) = (a.save().unwrap(), b.save().unwrap());
        a.merge_file(&file_b).unwrap();
        b.merge_file(&file_a).unwrap();
        let entry = a.get(&id).unwrap();
        assert_eq!(entry.common.custom_fields.v[0].name, "Door");
        let pin: Vec<_> = entry.history_of("custom field PIN").map(|r| r.value.expose().to_owned()).collect();
        assert_eq!(pin, vec!["1234"], "the losing field's value survives");
        let kept: Vec<Attachment> = entry.history_of("attachment from-a.pdf").map(|r| serde_json::from_str(r.value.expose()).unwrap()).collect();
        assert_eq!(kept, vec![attachment("from-a")], "the losing attachment keeps its key");
        assert_eq!(a.doc.entries, b.doc.entries);
        // A list that only grew leaves nothing behind.
        let before = a.get(&id).unwrap().history.len();
        a.edit(&id, 5000, |e, at| {
            let mut list = e.common.custom_fields.v.clone();
            list.push(field("More", "x"));
            e.common.custom_fields = Lww::new(list, at);
        })
        .unwrap();
        b.merge_file(&a.save().unwrap()).unwrap();
        assert_eq!(b.get(&id).unwrap().history.len(), before);
    }

    #[test]
    fn trash_and_history_lengths_reach_every_device() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Old", "", "", at), 2000).unwrap();
        a.trash(&id, 2000).unwrap();
        let mut b = twin(&mut a, 2);
        let mut c = twin(&mut a, 3);
        a.set_trash_days(0, 3000);
        a.set_history_limit(1, 3000);
        // By file.
        b.merge_file(&a.save().unwrap()).unwrap();
        assert_eq!((b.doc.prefs.trash_days, b.doc.prefs.history_limit), (0, 1));
        // A device still on the default would have emptied the trash the owner asked to keep.
        assert!(b.sweep(2000 + 31 * DAY_MS).is_empty());
        assert!(b.get(&id).is_some());
        // By delta.
        let delta = a.delta_for(&c.digests().unwrap()).unwrap();
        c.apply_delta(delta).unwrap();
        assert_eq!((c.doc.prefs.trash_days, c.doc.prefs.history_limit), (0, 1));
        assert_eq!(c.state_hash().unwrap(), a.state_hash().unwrap());
        // The later choice wins.
        c.set_trash_days(7, 4000);
        a.merge_file(&c.save().unwrap()).unwrap();
        assert_eq!(a.doc.prefs.trash_days, 7);
    }

    #[test]
    fn the_history_limit_holds_across_a_merge() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Site", "me", "p0", at), 2000).unwrap();
        let mut b = twin(&mut a, 2);
        for n in 1..=4 {
            b.edit(&id, 2000 + n, |e, at| e.set_password(Secret::new(format!("p{n}")), at)).unwrap();
        }
        a.set_history_limit(2, 3000);
        a.merge_file(&b.save().unwrap()).unwrap();
        let past: Vec<_> = a.get(&id).unwrap().history_of("password").map(|r| r.value.expose().to_owned()).collect();
        assert_eq!(past, vec!["p2", "p3"]);
        // What A trimmed does not come back from B.
        b.merge_file(&a.save().unwrap()).unwrap();
        a.merge_file(&b.save().unwrap()).unwrap();
        assert_eq!(a.get(&id).unwrap().history_of("password").count(), 2);
        assert_eq!(a.doc.entries, b.doc.entries);
    }

    #[test]
    fn what_goes_into_a_folder_removed_elsewhere_meanwhile_is_let_go() {
        let mut a = fast_vault(1);
        let folder = a.add_folder("Work", None, 2000).unwrap();
        let at = a.tick(2000);
        let id = a.add(login_entry("Site", "", "", at), 2000).unwrap();
        let mut b = twin(&mut a, 2);
        // B puts an item and a new folder in it while A removes it.
        b.edit(&id, 3000, |e, at| e.common.folder = Lww::new(Some(folder.clone()), at)).unwrap();
        let inner = b.add_folder("Inner", Some(folder.clone()), 3000).unwrap();
        let at = a.tick(2500);
        a.doc.folders.remove(&folder);
        a.doc.tombstones.insert(folder.clone(), at);
        let (file_a, file_b) = (a.save().unwrap(), b.save().unwrap());
        a.merge_file(&file_b).unwrap();
        b.merge_file(&file_a).unwrap();
        for vault in [&a, &b] {
            assert_eq!(vault.get(&id).unwrap().common.folder.v, None);
            assert_eq!(vault.doc.folders[&inner].parent.v, None);
            assert!(!vault.doc.folders.contains_key(&folder));
        }
        assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
    }

    fn sync_both_ways(a: &mut Vault, b: &mut Vault, by_delta: bool) {
        if by_delta {
            let (theirs, mine) = (b.digests().unwrap(), a.digests().unwrap());
            let there = a.delta_for(&theirs).unwrap();
            let back = b.delta_for(&mine).unwrap();
            b.apply_delta(there).unwrap();
            a.apply_delta(back).unwrap();
        } else {
            let (file_a, file_b) = (a.save().unwrap(), b.save().unwrap());
            a.merge_file(&file_b).unwrap();
            b.merge_file(&file_a).unwrap();
        }
    }

    #[test]
    fn restoring_beats_emptying_the_trash_on_another_device_at_once() {
        for by_delta in [false, true] {
            let mut a = fast_vault(1);
            let at = a.tick(2000);
            let id = a.add(login_entry("Bank", "me", "kept-secret", at), 2000).unwrap();
            a.trash(&id, 3000).unwrap();
            let mut b = twin(&mut a, 2);
            // Offline from each other: A empties its trash, B takes the item back out.
            a.purge(&id, 4000);
            b.restore(&id, 4000).unwrap();
            sync_both_ways(&mut a, &mut b, by_delta);
            for vault in [&a, &b] {
                let entry = vault.get(&id).expect("the restored item survives everywhere");
                assert!(!entry.is_trashed());
                assert_eq!(entry.common.title.v, "Bank");
                assert_eq!(entry.login().unwrap().password.v.expose(), "kept-secret", "whole, secrets included");
            }
            assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
            // Again, either way, changes nothing.
            sync_both_ways(&mut a, &mut b, !by_delta);
            a.merge_file(&b.save().unwrap()).unwrap();
            assert!(a.get(&id).is_some());
            assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
        }
    }

    #[test]
    fn the_trash_emptying_itself_on_one_device_loses_to_a_restore_on_another() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Old", "me", "p", at), 2000).unwrap();
        a.trash(&id, 3000).unwrap();
        let mut b = twin(&mut a, 2);
        let mut c = twin(&mut a, 3);
        b.restore(&id, 3000 + DAY_MS).unwrap();
        // A's trash days run out while it has not heard of the restore.
        assert_eq!(a.sweep(3000 + 31 * DAY_MS), Vec::<String>::new());
        assert!(a.get(&id).is_none());
        // C hears from A first, then from B; A hears from B.
        c.merge_file(&a.save().unwrap()).unwrap();
        assert!(c.get(&id).is_none());
        c.merge_file(&b.save().unwrap()).unwrap();
        a.merge_file(&b.save().unwrap()).unwrap();
        b.merge_file(&a.save().unwrap()).unwrap();
        for vault in [&a, &b, &c] {
            assert_eq!(vault.get(&id).unwrap().login().unwrap().password.v.expose(), "p");
        }
        // Out of the trash, the next sweep leaves it be.
        assert!(a.sweep(3000 + 62 * DAY_MS).is_empty());
        assert!(a.get(&id).is_some());
        assert_eq!(a.state_hash().unwrap(), c.state_hash().unwrap());
    }

    #[test]
    fn emptied_again_after_a_restore_it_stays_gone_even_from_a_stale_copy() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Twice", "me", "p", at), 2000).unwrap();
        a.trash(&id, 3000).unwrap();
        let mut b = twin(&mut a, 2);
        a.purge(&id, 4000);
        b.restore(&id, 4000).unwrap();
        sync_both_ways(&mut a, &mut b, false);
        // A copy that holds the restored item, and never hears what comes next until the end.
        let mut stale = twin(&mut b, 3);
        a.trash(&id, 5000).unwrap();
        a.purge(&id, 6000);
        sync_both_ways(&mut a, &mut b, true);
        assert!(a.get(&id).is_none() && b.get(&id).is_none());
        stale.merge_file(&a.save().unwrap()).unwrap();
        b.merge_file(&stale.save().unwrap()).unwrap();
        assert!(stale.get(&id).is_none(), "the second delete saw the restore");
        assert!(b.get(&id).is_none());
        assert_eq!(stale.state_hash().unwrap(), a.state_hash().unwrap());
    }

    #[test]
    fn two_devices_emptying_one_trash_agree() {
        let mut a = fast_vault(1);
        let at = a.tick(2000);
        let id = a.add(login_entry("Both", "me", "p", at), 2000).unwrap();
        a.trash(&id, 3000).unwrap();
        let mut b = twin(&mut a, 2);
        a.purge(&id, 4000);
        b.purge(&id, 4500);
        sync_both_ways(&mut a, &mut b, false);
        sync_both_ways(&mut a, &mut b, true);
        assert!(a.get(&id).is_none() && b.get(&id).is_none());
        assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
    }

    #[test]
    fn a_device_far_ahead_neither_drags_another_nor_wins_its_later_edits() {
        const HOUR: u64 = 3_600_000;
        let now = 100 * DAY_MS;
        let mut honest = fast_vault(1);
        let at = honest.tick(now);
        let id = honest.add(login_entry("Site", "me", "p", at), now).unwrap();
        let mut fast = twin(&mut honest, 2);
        fast.set_device_name("Old laptop", now);
        // Its clock runs ten days fast.
        fast.edit(&id, now + 10 * DAY_MS, |e, at| e.common.title = Lww::new("From the fast one".into(), at)).unwrap();
        honest.merge_file(&fast.save().unwrap()).unwrap();
        let warning = honest.clock_warning(now + HOUR).unwrap();
        assert!(!warning.this_device);
        assert_eq!(warning.device, Some(2));
        assert_eq!(honest.device_name(2), Some("Old laptop"));
        assert!(warning.by_ms > 9 * DAY_MS);
        assert_eq!(fast.clock_warning(now + 10 * DAY_MS), None, "its own clock looks fine to it");
        // The honest device keeps its own time for new writes...
        assert_eq!(honest.tick(now + HOUR).ms, now + HOUR);
        // ...and an edit made after seeing the fast write still beats it.
        honest.edit(&id, now + HOUR, |e, at| e.common.title = Lww::new("Later, here".into(), at)).unwrap();
        fast.merge_file(&honest.save().unwrap()).unwrap();
        honest.merge_file(&fast.save().unwrap()).unwrap();
        assert_eq!(fast.get(&id).unwrap().common.title.v, "Later, here");
        assert_eq!(honest.state_hash().unwrap(), fast.state_hash().unwrap());
        // A new item made here afterwards carries this device's time, not the fast one's.
        let at = honest.tick(now + 2 * HOUR);
        assert_eq!(at.ms, now + 2 * HOUR);
    }

    #[test]
    fn a_clock_warning_says_when_this_device_is_the_one_behind() {
        let now = 100 * DAY_MS;
        let mut me = fast_vault(1);
        let at = me.tick(now);
        let id = me.add(login_entry("Site", "me", "p", at), now).unwrap();
        assert_eq!(me.clock_warning(now), None);
        let mut b = twin(&mut me, 2);
        let mut c = twin(&mut me, 3);
        b.edit(&id, now + 5 * DAY_MS, |e, at| e.common.title = Lww::new("b".into(), at)).unwrap();
        c.edit(&id, now + 5 * DAY_MS + 7, |e, at| e.common.notes = Lww::new(Secret::new("c"), at)).unwrap();
        me.merge_file(&b.save().unwrap()).unwrap();
        me.merge_file(&c.save().unwrap()).unwrap();
        // Two others agree this device is days behind.
        let warning = me.clock_warning(now).unwrap();
        assert!(warning.this_device);
        assert!(warning.by_ms >= 5 * DAY_MS);
        // A clock that went back past what this device wrote itself.
        let mut alone = fast_vault(4);
        let at = alone.tick(now);
        alone.add(login_entry("Mine", "", "", at), now).unwrap();
        assert!(alone.clock_warning(now - 3 * DAY_MS).unwrap().this_device);
        // A few minutes apart is ordinary.
        assert_eq!(alone.clock_warning(now - 60_000), None);
    }

    fn open_index(file: &[u8], password: &[u8], factors: &[FactorOutput]) -> Option<usize> {
        let mut found = None;
        let outcome = open_set_with(&[file], factors, 9, |config| config.derive_for_tests(password)).unwrap();
        if let OpenOutcome::Opened { vault, .. } = outcome {
            let root = vault.header.config().unwrap().derive_for_tests(password).unwrap();
            if let SlotCheck::Opens(slot) = vault.check_slot_with_root(&root, factors).unwrap() {
                found = Some(slot.index());
            }
        }
        found
    }

    fn change(vault: &mut Vault, old: &[u8], new: &[u8], factors: &[FactorOutput], now: u64) {
        let config = vault.header.config().unwrap();
        let SlotCheck::Opens(slot) = vault.check_slot_with_root(&config.derive_for_tests(old).unwrap(), factors).unwrap() else {
            panic!("the old password opens a slot");
        };
        vault.change_password_with_root(&slot, &config.derive_for_tests(new).unwrap(), now).unwrap();
    }

    #[test]
    fn a_password_changed_on_two_devices_at_once_opens_with_either_until_one_goes() {
        for by_delta in [false, true] {
            let mut a = fast_vault(1);
            let mut b = twin(&mut a, 2);
            change(&mut a, b"pw", b"from-a", &[], 3000);
            change(&mut b, b"pw", b"from-b", &[], 4000);
            sync_both_ways(&mut a, &mut b, by_delta);
            for vault in [&mut a, &mut b] {
                let file = vault.save().unwrap();
                assert!(open_index(&file, b"from-a", &[]).is_some(), "A's new password opens");
                assert!(open_index(&file, b"from-b", &[]).is_some(), "B's new password opens");
                assert_eq!(open_index(&file, b"pw", &[]), None, "the old one is gone");
                assert_eq!(vault.doc.password_conflict().len(), 2);
                assert!(!vault.doc.password_dropped());
            }
            assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
            assert_eq!(a.doc.slots, b.doc.slots);
            // Removing one ends it, and the removal travels.
            let gone = open_index(&a.save().unwrap(), b"from-b", &[]).unwrap();
            a.remove_slot(gone, 5000).unwrap();
            assert!(a.doc.password_conflict().is_empty());
            sync_both_ways(&mut a, &mut b, by_delta);
            assert!(b.doc.password_conflict().is_empty());
            let file = b.save().unwrap();
            assert!(open_index(&file, b"from-a", &[]).is_some());
            assert_eq!(open_index(&file, b"from-b", &[]), None);
        }
    }

    #[test]
    fn both_changes_of_a_password_that_needs_a_key_file_keep_needing_it() {
        let mut a = fast_vault(1);
        let config = a.header.config().unwrap();
        let output = format::key_file_factor(b"file");
        a.add_slot_with_root(&config.derive_for_tests(b"filed").unwrap(), "With file", SlotAction::Open, Some((&FactorKind::KeyFile, &output)), 2000)
            .unwrap();
        let mut b = twin(&mut a, 2);
        let factors = std::slice::from_ref(&output);
        change(&mut a, b"filed", b"filed-a", factors, 3000);
        change(&mut b, b"filed", b"filed-b", factors, 4000);
        sync_both_ways(&mut a, &mut b, false);
        let file = a.save().unwrap();
        for password in [&b"filed-a"[..], b"filed-b"] {
            assert!(
                matches!(set_outcome(&[&file], password, &[]), OpenOutcome::NeedsFactor(kinds) if kinds == vec![FactorKind::KeyFile]),
                "not without the key file"
            );
            assert!(open_index(&file, password, factors).is_some());
        }
        assert!(open_index(&file, b"pw", &[]).is_some(), "the master password is untouched");
        assert_eq!(a.doc.password_conflict().len(), 2);
    }

    #[test]
    fn a_full_vault_keeps_the_later_change_and_says_one_was_lost() {
        let mut a = fast_vault(1);
        let config = a.header.config().unwrap();
        for n in 1..SLOTS {
            a.add_slot_with_root(&config.derive_for_tests(format!("x{n}").as_bytes()).unwrap(), "x", SlotAction::Open, None, 2000).unwrap();
        }
        let mut b = twin(&mut a, 2);
        change(&mut a, b"pw", b"from-a", &[], 3000);
        change(&mut b, b"pw", b"from-b", &[], 4000);
        sync_both_ways(&mut a, &mut b, true);
        for vault in [&mut a, &mut b] {
            let file = vault.save().unwrap();
            assert_eq!(open_index(&file, b"from-a", &[]), None);
            assert_eq!(open_index(&file, b"from-b", &[]), Some(0), "the later change stays");
            assert!(vault.doc.password_dropped(), "and the owner is told");
            assert!(vault.doc.password_conflict().is_empty());
        }
        assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
        // Told on one device, told on all.
        a.password_dropped_seen();
        sync_both_ways(&mut a, &mut b, false);
        assert!(!b.doc.password_dropped());
    }

    #[test]
    fn changes_one_after_another_and_duress_passwords_are_no_conflict() {
        let mut a = fast_vault(1);
        let config = a.header.config().unwrap();
        a.add_slot_with_root(&config.derive_for_tests(b"duress").unwrap(), "Duress", SlotAction::WipeLocal, None, 2000).unwrap();
        let mut stale = twin(&mut a, 2);
        change(&mut a, b"pw", b"second", &[], 3000);
        change(&mut a, b"second", b"third", &[], 4000);
        change(&mut a, b"third", b"fourth", &[], 5000);
        sync_both_ways(&mut a, &mut stale, false);
        sync_both_ways(&mut a, &mut stale, true);
        for vault in [&mut a, &mut stale] {
            assert!(vault.doc.password_conflict().is_empty());
            assert!(!vault.doc.password_dropped());
            let file = vault.save().unwrap();
            assert!(open_index(&file, b"fourth", &[]).is_some());
            for old in [&b"pw"[..], b"second", b"third"] {
                assert_eq!(open_index(&file, old, &[]), None);
            }
            assert!(matches!(set_outcome(&[&file], b"duress", &[]), OpenOutcome::Duress { .. }));
        }
    }

    #[test]
    fn a_way_in_added_on_one_device_and_a_password_changed_on_another_both_stay() {
        let mut a = fast_vault(1);
        let mut b = twin(&mut a, 2);
        let config = a.header.config().unwrap();
        a.add_slot_with_root(&config.derive_for_tests(b"added").unwrap(), "Added", SlotAction::Open, None, 3000).unwrap();
        change(&mut b, b"pw", b"changed", &[], 4000);
        sync_both_ways(&mut a, &mut b, false);
        for vault in [&mut a, &mut b] {
            let file = vault.save().unwrap();
            assert!(open_index(&file, b"added", &[]).is_some());
            assert!(open_index(&file, b"changed", &[]).is_some());
            assert_eq!(open_index(&file, b"pw", &[]), None);
        }
        assert_eq!(a.doc.slots, b.doc.slots);
    }

    #[test]
    fn a_vault_from_before_slots_kept_their_times_still_merges_whole() {
        let mut a = fast_vault(1);
        let mut b = twin(&mut a, 2);
        let config = a.header.config().unwrap();
        b.add_slot_with_root(&config.derive_for_tests(b"added").unwrap(), "Added", SlotAction::Open, None, 3000).unwrap();
        // As an older build writes it: no time on any slot.
        for slot in &mut b.doc.slots {
            slot.at = Hlc::default();
            slot.line = Hlc::default();
        }
        a.merge_file(&b.save().unwrap()).unwrap();
        let file = a.save().unwrap();
        assert!(open_index(&file, b"added", &[]).is_some());
        assert!(open_index(&file, b"pw", &[]).is_some());
        assert_eq!(a.doc.slots[1].label, "Added");
    }

    /// A tiny deterministic generator, so a failing sequence can be replayed by its seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % n as u64).unwrap()
        }
    }

    fn random_edit(rng: &mut Rng, vault: &mut Vault, now: u64, step: usize, passwords: &mut Vec<String>) {
        let ids: Vec<Id> = vault.doc.entries.keys().cloned().collect();
        let folders: Vec<Id> = vault.doc.folders.keys().cloned().collect();
        let pick = |rng: &mut Rng, list: &[Id]| (!list.is_empty()).then(|| list[rng.below(list.len())].clone());
        match rng.below(16) {
            12 | 13 => {
                // A change of some password this copy holds, as two devices may make at once.
                let config = vault.header.config().unwrap();
                let start = rng.below(passwords.len());
                for n in 0..passwords.len() {
                    let old = config.derive_for_tests(passwords[(start + n) % passwords.len()].as_bytes()).unwrap();
                    if let SlotCheck::Opens(slot) = vault.check_slot_with_root(&old, &[]).unwrap() {
                        let new = format!("r{step}");
                        vault.change_password_with_root(&slot, &config.derive_for_tests(new.as_bytes()).unwrap(), now).unwrap();
                        passwords.push(new);
                        break;
                    }
                }
            }
            14 => {
                // The trash emptying itself, well after anything went in.
                vault.sweep(now + 40 * DAY_MS);
            }
            15 => {
                let used: Vec<usize> = (0..SLOTS).filter(|&i| vault.doc.slots.get(i).is_some_and(|s| s.used)).collect();
                if rng.below(2) == 0 && used.len() > 1 {
                    // Refused when it is the last way in; that is fine here.
                    let _ = vault.remove_slot(used[rng.below(used.len())], now);
                } else {
                    let config = vault.header.config().unwrap();
                    let new = format!("s{step}");
                    if vault.add_slot_with_root(&config.derive_for_tests(new.as_bytes()).unwrap(), "Added", SlotAction::Open, None, now).is_ok() {
                        passwords.push(new);
                    }
                }
            }
            0 => {
                let at = vault.tick(now);
                vault.add(login_entry(&format!("New {step}"), "u", &format!("p{step}"), at), now).unwrap();
            }
            1 | 2 => {
                if let Some(id) = pick(rng, &ids) {
                    vault.edit(&id, now, |e, at| e.common.title = Lww::new(format!("T{step}"), at)).unwrap();
                }
            }
            3 => {
                if let Some(id) = pick(rng, &ids) {
                    vault.edit(&id, now, |e, at| e.set_password(Secret::new(format!("pw{step}")), at)).unwrap();
                }
            }
            4 => {
                if let Some(id) = pick(rng, &ids) {
                    if vault.get(&id).unwrap().is_trashed() {
                        vault.restore(&id, now).unwrap();
                    } else {
                        vault.trash(&id, now).unwrap();
                    }
                }
            }
            5 => {
                if let Some(id) = pick(rng, &ids) {
                    vault.purge(&id, now);
                }
            }
            6 => {
                if let Some(id) = pick(rng, &ids) {
                    vault
                        .edit(&id, now, |e, at| {
                            if let Some(login) = e.login_mut() {
                                let mut keys = login.passkeys.v.clone();
                                if keys.is_empty() || step.is_multiple_of(2) {
                                    keys.push(passkey(&format!("k{step}"), 0, now));
                                } else {
                                    keys[0].counter += 1;
                                }
                                login.passkeys = Lww::new(keys, at);
                            }
                        })
                        .unwrap();
                }
            }
            7 => {
                if let Some(id) = pick(rng, &ids) {
                    vault
                        .edit(&id, now, |e, at| {
                            let mut list = e.common.custom_fields.v.clone();
                            if list.is_empty() || !step.is_multiple_of(3) {
                                list.push(crate::model::Field { name: format!("F{step}"), value: Secret::new(format!("v{step}")), ..Default::default() });
                            } else {
                                list.remove(0);
                            }
                            e.common.custom_fields = Lww::new(list, at);
                        })
                        .unwrap();
                }
            }
            8 => {
                let parent = if rng.below(2) == 0 { pick(rng, &folders) } else { None };
                vault.add_folder(&format!("D{step}"), parent, now).unwrap();
            }
            9 => {
                if let Some(folder) = pick(rng, &folders) {
                    let at = vault.tick(now);
                    vault.doc.folders.remove(&folder);
                    vault.doc.tombstones.insert(folder, at);
                }
            }
            10 => {
                if let (Some(id), Some(folder)) = (pick(rng, &ids), pick(rng, &folders)) {
                    vault.edit(&id, now, |e, at| e.common.folder = Lww::new(Some(folder), at)).unwrap();
                }
            }
            _ => {
                if rng.below(2) == 0 {
                    vault.set_history_limit(u32::try_from(rng.below(3)).unwrap(), now);
                } else {
                    vault.set_trash_days(u32::try_from(rng.below(40)).unwrap(), now);
                }
            }
        }
    }

    fn exchange(vaults: &mut [Vault], from: usize, to: usize, by_delta: bool) {
        if by_delta {
            let (theirs, mine) = (vaults[to].digests().unwrap(), vaults[from].digests().unwrap());
            let there = vaults[from].delta_for(&theirs).unwrap();
            let back = vaults[to].delta_for(&mine).unwrap();
            vaults[to].apply_delta(there).unwrap();
            vaults[from].apply_delta(back).unwrap();
        } else {
            let file = vaults[from].save().unwrap();
            vaults[to].merge_file(&file).unwrap();
        }
    }

    #[test]
    fn random_edits_and_syncs_in_any_order_converge_and_repeat_harmlessly() {
        for seed in 1..=25_u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut first = fast_vault(1);
            for n in 0..3 {
                let at = first.tick(1000);
                first.add(login_entry(&format!("Seed {n}"), "u", "p", at), 1000).unwrap();
            }
            first.add_folder("Home", None, 1000).unwrap();
            let mut vaults = vec![twin(&mut first, 2), twin(&mut first, 3), first];
            // Clocks that disagree: one far ahead, one behind.
            let skew = [0_u64, 90_000, 0];
            let mut now = 2000_u64;
            let mut passwords = vec!["pw".to_owned()];
            for step in 0..60 {
                now += u64::try_from(rng.below(5)).unwrap();
                let device = rng.below(3);
                let wall = now + skew[device] - u64::from(device == 2) * 500;
                if rng.below(3) == 0 {
                    let to = (device + 1 + rng.below(2)) % 3;
                    exchange(&mut vaults, device, to, rng.below(2) == 0);
                } else {
                    random_edit(&mut rng, &mut vaults[device], wall, step, &mut passwords);
                }
            }
            for round in 0..2 {
                for from in 0..3 {
                    for to in 0..3 {
                        if from != to {
                            exchange(&mut vaults, from, to, round == 1);
                        }
                    }
                }
            }
            let hash = vaults[0].state_hash().unwrap();
            for (n, vault) in vaults.iter().enumerate() {
                assert_eq!(vault.state_hash().unwrap(), hash, "seed {seed}: device {n} differs");
                assert_eq!(vault.doc.entries, vaults[0].doc.entries, "seed {seed}");
                assert_eq!(vault.doc.prefs.trash_days, vaults[0].doc.prefs.trash_days, "seed {seed}");
                // The same ways in, slot for slot, so every password opens on every device.
                let slots = |v: &Vault| {
                    (0..SLOTS)
                        .map(|i| {
                            let slot = v.doc.slots.get(i).cloned().unwrap_or_default();
                            (v.doc.slot_time(i), slot.used, slot.label, slot.line, slot.replaced, v.header.slot_record(i).map(<[u8]>::to_vec))
                        })
                        .collect::<Vec<_>>()
                };
                assert_eq!(slots(vault), slots(&vaults[0]), "seed {seed}: device {n}'s slots differ");
                assert_eq!(vault.doc.password_conflict(), vaults[0].doc.password_conflict(), "seed {seed}");
                assert_eq!(vault.doc.password_dropped(), vaults[0].doc.password_dropped(), "seed {seed}");
            }
            // Every password still held by some slot opens every copy.
            let file = vaults[0].save().unwrap();
            for vault in &mut vaults[1..] {
                let other = vault.save().unwrap();
                for password in &passwords {
                    assert_eq!(open_index(&file, password.as_bytes(), &[]), open_index(&other, password.as_bytes(), &[]), "seed {seed}: {password}");
                }
            }
            // The same file or delta again changes nothing.
            let file = vaults[1].save().unwrap();
            vaults[0].merge_file(&file).unwrap();
            vaults[0].merge_file(&file).unwrap();
            exchange(&mut vaults, 0, 2, true);
            exchange(&mut vaults, 0, 2, true);
            assert_eq!(vaults[0].state_hash().unwrap(), hash, "seed {seed}: repeating a merge changed something");
            assert_eq!(vaults[2].state_hash().unwrap(), hash, "seed {seed}");
        }
    }

    #[test]
    fn a_shared_vaults_newest_version_travels_by_delta_too() {
        let link = |version: u64, at: Hlc| LinkedVault {
            file: "shared".into(),
            key: Secret::new(format!("key-{version}")),
            at,
            shared: Some(crate::model::SharedLink { min_version: version, ..Default::default() }),
            ..LinkedVault::default()
        };
        let mut a = fast_vault(1);
        let mut b = twin(&mut a, 2);
        let at = a.tick(2000);
        a.doc.linked = vec![link(5, at)];
        let at = b.tick(3000);
        b.doc.linked = vec![link(4, at)];
        let mut c = twin(&mut b, 3);
        a.merge_file(&b.save().unwrap()).unwrap();
        assert_eq!(a.doc.linked[0].key.expose(), "key-5");
        // C holds B's link, written at the same moment as A's merged one.
        let delta = a.delta_for(&c.digests().unwrap()).unwrap();
        c.apply_delta(delta).unwrap();
        assert_eq!(c.doc.linked, a.doc.linked);
    }
}
