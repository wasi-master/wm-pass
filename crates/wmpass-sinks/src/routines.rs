//! Backup and sync over any destination.
//!
//! The order in [`backup`] is the safety property: write, read back and compare, and only
//! then delete older copies. A failed run can leave one file too many, and never one too few.
//!
//! Sync gives each device its own file. Devices only ever overwrite their own, so two
//! syncing at once cannot lose each other's work, and any store can be used with no
//! locking. Everything merges field by field.

use std::collections::BTreeMap;

use wmpass_core::vault::{Freshness, Vault, check_freshness};

use crate::{Fault, Result, Sink, Stored};

/// Marks backups, so rotation never touches anything else in a folder.
const BACKUP_PREFIX: &str = "wmpass-backup-";
const BACKUP_SUFFIX: &str = ".wmvault";

/// The name of a backup taken at a moment by a device.
pub fn backup_name(now_ms: u64, device: u32) -> String {
    format!("{BACKUP_PREFIX}{now_ms:016}-{device:08x}{BACKUP_SUFFIX}")
}

fn parse_backup(name: &str) -> Option<(u64, u32)> {
    let middle = name.strip_prefix(BACKUP_PREFIX)?.strip_suffix(BACKUP_SUFFIX)?;
    let (stamp, device) = middle.split_once('-')?;
    Some((stamp.parse().ok()?, u32::from_str_radix(device, 16).ok()?))
}

/// What a backup run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupReport {
    /// The new file.
    pub stored: Stored,
    /// Older copies deleted.
    pub rotated: usize,
}

/// Writes a backup, verifies it, then removes this device's oldest beyond `keep`. Other
/// devices' backups are never counted or removed.
pub async fn backup<S: Sink>(sink: &S, bytes: &[u8], now_ms: u64, device: u32, keep: usize) -> Result<BackupReport> {
    sink.readiness().await?;
    let name = backup_name(now_ms, device);
    let stored = sink.write(&name, bytes).await?;
    if sink.read(&stored).await? != bytes {
        return Err(Fault::NotVerified);
    }
    let mut mine: Vec<(u64, Stored)> = sink
        .list()
        .await?
        .into_iter()
        .filter_map(|file| {
            let (stamp, owner) = parse_backup(&file.name)?;
            (owner == device).then_some((stamp, file))
        })
        .collect();
    // The new one must be visible before anything older may go.
    if !mine.iter().any(|(_, file)| file.name == name) {
        return Err(Fault::NotVerified);
    }
    mine.sort_by_key(|(stamp, _)| std::cmp::Reverse(*stamp));
    let mut rotated = 0;
    for (_, old) in mine.iter().skip(keep.max(1)) {
        sink.delete(old).await?;
        rotated += 1;
    }
    Ok(BackupReport { stored, rotated })
}

/// Backups at a destination from every device, newest first, for the restore screen.
pub async fn list_backups<S: Sink>(sink: &S) -> Result<Vec<(u64, u32, Stored)>> {
    let mut out: Vec<(u64, u32, Stored)> = sink
        .list()
        .await?
        .into_iter()
        .filter_map(|file| parse_backup(&file.name).map(|(stamp, device)| (stamp, device, file)))
        .collect();
    out.sort_by_key(|(stamp, _, _)| std::cmp::Reverse(*stamp));
    Ok(out)
}

/// What a device remembers between syncs: the generation last seen per file.
pub type SeenFiles = BTreeMap<String, u64>;

/// What a sync did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Other devices' files merged in.
    pub merged: usize,
    /// Files that are not this vault, such as another vault or chaff. Left alone.
    pub skipped: usize,
    /// Files that went backwards since last seen, which a location replaying an old copy
    /// would cause. Merging old data is harmless, but the owner should hear about it.
    pub rollbacks: Vec<String>,
}

/// One sync: merge every other device's file, then write and verify this device's.
pub async fn sync<S: Sink>(sink: &S, vault: &mut Vault, seen: &mut SeenFiles) -> Result<SyncReport> {
    sink.readiness().await?;
    let mine = vault.sync_file_name().map_err(|_| Fault::Io)?;
    let mut report = SyncReport::default();
    for file in sink.list().await? {
        if file.name == mine || parse_backup(&file.name).is_some() {
            continue;
        }
        let bytes = sink.read(&file).await?;
        match vault.merge_file(&bytes) {
            Ok(generation) => {
                let last = seen.get(&file.name).copied().unwrap_or(0);
                if let Freshness::Rollback { .. } = check_freshness(last, generation) {
                    report.rollbacks.push(file.name.clone());
                }
                seen.insert(file.name.clone(), last.max(generation));
                report.merged += 1;
            }
            Err(_) => report.skipped += 1,
        }
    }
    let bytes = vault.save().map_err(|_| Fault::Io)?;
    let stored = sink.write(&mine, &bytes).await?;
    if sink.read(&stored).await? != bytes {
        return Err(Fault::NotVerified);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::Folder;
    use crate::http::block_on;
    use std::cell::RefCell;
    use wmpass_core::crypto::KdfParams;
    use wmpass_core::format::{KdfConfig, VaultKey, chaff_file};
    use wmpass_core::model::Lww;
    use wmpass_core::vault::login_entry;

    fn config() -> KdfConfig {
        KdfConfig::new(KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }).unwrap()
    }

    fn pair() -> (Vault, Vault) {
        let mut a = Vault::create("V", b"pw", &config(), 1, 1000).unwrap();
        let file = a.save().unwrap();
        let key = VaultKey::from_bytes(*a.key().expose_for_hardware_wrap());
        let b = Vault::from_file(&file, key, 2).unwrap();
        (a, b)
    }

    #[test]
    fn two_devices_converge_through_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Folder::new(dir.path());
        let (mut a, mut b) = pair();
        let (mut seen_a, mut seen_b) = (SeenFiles::new(), SeenFiles::new());
        let at = a.tick(2000);
        let id_a = a.add(login_entry("From A", "u", "pa", at), 2000).unwrap();
        block_on(sync(&sink, &mut a, &mut seen_a)).unwrap();
        let at = b.tick(3000);
        let id_b = b.add(login_entry("From B", "u", "pb", at), 3000).unwrap();
        let report = block_on(sync(&sink, &mut b, &mut seen_b)).unwrap();
        assert_eq!(report.merged, 1);
        assert!(b.get(&id_a).is_some());
        b.edit(&id_a, 4000, |entry, at| entry.common.title = Lww::new("Edited on B".into(), at)).unwrap();
        block_on(sync(&sink, &mut b, &mut seen_b)).unwrap();
        block_on(sync(&sink, &mut a, &mut seen_a)).unwrap();
        assert!(a.get(&id_b).is_some());
        assert_eq!(a.get(&id_a).unwrap().common.title.v, "Edited on B");
        assert_eq!(a.doc.entries, b.doc.entries);
        assert_eq!(block_on(sink.list()).unwrap().len(), 2, "one file per device");
    }

    #[test]
    fn other_vaults_and_chaff_in_the_folder_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Folder::new(dir.path());
        let (mut a, _) = pair();
        let mut other = Vault::create("Other", b"other", &config(), 9, 1).unwrap();
        block_on(sink.write("someothervault", &other.save().unwrap())).unwrap();
        block_on(sink.write("chaff", &chaff_file(&config(), 500).unwrap())).unwrap();
        block_on(sink.write("rubbish", b"not a vault")).unwrap();
        let report = block_on(sync(&sink, &mut a, &mut SeenFiles::new())).unwrap();
        assert_eq!((report.merged, report.skipped), (0, 3));
        assert_eq!(block_on(sink.list()).unwrap().len(), 4);
    }

    #[test]
    fn a_replayed_old_copy_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Folder::new(dir.path());
        let (mut a, mut b) = pair();
        let (mut seen_a, mut seen_b) = (SeenFiles::new(), SeenFiles::new());
        block_on(sync(&sink, &mut b, &mut seen_b)).unwrap();
        let name = b.sync_file_name().unwrap();
        let old = std::fs::read(dir.path().join(&name)).unwrap();
        block_on(sync(&sink, &mut b, &mut seen_b)).unwrap();
        assert!(block_on(sync(&sink, &mut a, &mut seen_a)).unwrap().rollbacks.is_empty());
        std::fs::write(dir.path().join(&name), old).unwrap();
        assert_eq!(block_on(sync(&sink, &mut a, &mut seen_a)).unwrap().rollbacks, vec![name]);
    }

    #[test]
    fn sync_names_reveal_nothing() {
        let (a, _) = pair();
        let name = a.sync_file_name().unwrap();
        assert_eq!(name.len(), 32);
        assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn backup_rotates_only_this_devices_copies() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Folder::new(dir.path());
        block_on(sink.write(&backup_name(1, 99), b"another device")).unwrap();
        block_on(sink.write("unrelated.txt", b"keep me")).unwrap();
        for now in 10..15 {
            block_on(backup(&sink, b"data", now, 1, 3)).unwrap();
        }
        let listed = block_on(list_backups(&sink)).unwrap();
        let mine: Vec<u64> = listed.iter().filter(|(_, d, _)| *d == 1).map(|(s, _, _)| *s).collect();
        assert_eq!(mine, vec![14, 13, 12]);
        assert!(listed.iter().any(|(_, d, _)| *d == 99));
        assert!(dir.path().join("unrelated.txt").exists());
    }

    /// Silently corrupts writes, and fails if rotation runs afterwards.
    struct Corrupting {
        inner: Folder,
        deletes: RefCell<usize>,
    }

    impl Sink for Corrupting {
        async fn readiness(&self) -> Result<()> {
            self.inner.readiness().await
        }
        async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
            let mut wrong = bytes.to_vec();
            wrong.truncate(bytes.len() / 2);
            self.inner.write(name, &wrong).await
        }
        async fn list(&self) -> Result<Vec<Stored>> {
            self.inner.list().await
        }
        async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
            self.inner.read(file).await
        }
        async fn delete(&self, file: &Stored) -> Result<()> {
            *self.deletes.borrow_mut() += 1;
            self.inner.delete(file).await
        }
    }

    #[test]
    fn a_bad_write_never_rotates_good_backups_away() {
        let dir = tempfile::tempdir().unwrap();
        let good = Folder::new(dir.path());
        block_on(backup(&good, b"good-copy", 1, 1, 1)).unwrap();
        let bad = Corrupting { inner: Folder::new(dir.path()), deletes: RefCell::new(0) };
        assert_eq!(block_on(backup(&bad, b"new-data", 2, 1, 1)).err(), Some(Fault::NotVerified));
        assert_eq!(*bad.deletes.borrow(), 0);
        let survivor = Stored { id: backup_name(1, 1), name: backup_name(1, 1), size: 0 };
        assert_eq!(block_on(good.read(&survivor)).unwrap(), b"good-copy");
    }
}
