//! Taking in another vault's items, for when two vaults made apart become one. Sync merges
//! copies of the same vault; this is for two different vaults, under different keys, where
//! one is kept and the other's items move into it.

use std::collections::BTreeMap;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::model::{Entry, Folder, Id, Lww};
use crate::vault::Vault;

/// Everything of one vault that moves into another: its items and the folders they sit in.
/// Settings, slots and the trash stay behind, since the kept vault has its own.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Gift {
    /// Items not in the trash.
    pub entries: Vec<Entry>,
    /// Every folder, so nested ones keep their place.
    pub folders: Vec<Folder>,
}

impl Gift {
    /// What `vault` has to give.
    pub fn of(vault: &Vault) -> Self {
        Self {
            entries: vault.doc.entries.values().filter(|e| !e.is_trashed()).cloned().collect(),
            folders: vault.doc.folders.values().cloned().collect(),
        }
    }

    /// Attachment blobs the items refer to.
    pub fn blobs(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries
            .iter()
            .flat_map(|e| e.common.attachments.v.iter().map(|a| a.blob.clone()))
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// What taking a gift did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbsorbReport {
    /// Items new to this vault.
    pub added: usize,
    /// Items this vault already had, merged into the copy here field by field.
    pub matched: usize,
}

/// Adds a gift's items to `vault`. An item counts as one this vault already has when it has
/// the same id, or is a login for the same site and username with the same password; those
/// are merged, so newer values win and older ones go to history, and nothing is lost.
/// Folders are matched by name under the same parent, and made where missing.
pub fn absorb(vault: &mut Vault, gift: Gift, now_ms: u64) -> Result<AbsorbReport> {
    let mut report = AbsorbReport::default();
    let folders = map_folders(vault, &gift.folders, now_ms)?;
    let mut known: HashMap<_, Id> = vault
        .doc
        .entries
        .values()
        .filter(|e| !e.is_trashed())
        .filter_map(|e| crate::io::identity_key(e).map(|key| (key, e.id.clone())))
        .collect();
    for mut entry in gift.entries {
        observe(vault, &entry);
        let folder = entry.common.folder.v.as_ref().and_then(|id| folders.get(id)).cloned();
        entry.common.folder = Lww::new(folder, entry.common.folder.at);
        let same_id = vault.get(&entry.id).is_some_and(|e| !e.is_trashed());
        let same = if same_id {
            Some(entry.id.clone())
        } else {
            crate::io::identity_key(&entry).and_then(|key| known.get(&key).cloned())
        };
        match same {
            Some(id) => {
                entry.id.clone_from(&id);
                // A copy of the same item merges. A different item for the same login is
                // folded in as clearing a duplicate does, so a field both wrote at the same
                // moment, as two imports on one device can, does not drop the other's value.
                vault.edit(&id, now_ms, |mine, at| {
                    if same_id {
                        mine.merge_from(&entry);
                    } else {
                        mine.absorb_item(&entry, at);
                    }
                })?;
                report.matched += 1;
            }
            None => {
                // An id this vault has in its trash would bring that item back as this one, and
                // one it deleted for good would be dropped again by the next merge.
                if vault.get(&entry.id).is_some() || vault.doc.tombstones.contains_key(&entry.id) {
                    entry.id = String::new();
                }
                let key = crate::io::identity_key(&entry);
                let id = vault.add(entry, now_ms)?;
                if let Some(key) = key {
                    known.insert(key, id);
                }
                report.added += 1;
            }
        }
    }
    Ok(report)
}

/// So edits made here afterwards sort after the newest of what arrived.
fn observe(vault: &mut Vault, entry: &Entry) {
    vault.observe(entry.latest());
}

/// Their folder ids to this vault's, making folders that are missing.
fn map_folders(vault: &mut Vault, theirs: &[Folder], now_ms: u64) -> Result<BTreeMap<Id, Id>> {
    let by_id: BTreeMap<&str, &Folder> = theirs.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut out = BTreeMap::new();
    for folder in theirs {
        map_one(vault, &by_id, folder, &mut out, 0, now_ms)?;
    }
    Ok(out)
}

fn map_one(
    vault: &mut Vault,
    by_id: &BTreeMap<&str, &Folder>,
    folder: &Folder,
    out: &mut BTreeMap<Id, Id>,
    depth: usize,
    now_ms: u64,
) -> Result<Option<Id>> {
    if let Some(id) = out.get(&folder.id) {
        return Ok(Some(id.clone()));
    }
    // A loop in their parents ends at the top rather than running forever.
    let parent = match folder.parent.v.as_deref().and_then(|p| by_id.get(p)) {
        Some(parent) if depth < 32 => map_one(vault, by_id, parent, out, depth + 1, now_ms)?,
        _ => None,
    };
    let existing = vault
        .doc
        .folders
        .values()
        .find(|f| f.name.v == folder.name.v && f.parent.v == parent)
        .map(|f| f.id.clone());
    let id = match existing {
        Some(id) => id,
        None => vault.add_folder(&folder.name.v, parent, now_ms)?,
    };
    out.insert(folder.id.clone(), id.clone());
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Hlc;
    use crate::vault::login_entry;
    use crate::vault::test_support::fast_vault;

    #[test]
    fn new_items_arrive_and_the_same_login_is_merged_not_doubled() {
        let mut kept = fast_vault(1);
        let mut other = fast_vault(2);
        let at = kept.tick(1000);
        kept.add(login_entry("GitHub", "me", "pw1", at), 1000).unwrap();
        let at = other.tick(2000);
        other.add(login_entry("GitHub", "me", "pw1", at), 2000).unwrap();
        other.add(login_entry("Only there", "u", "p", at), 2000).unwrap();
        let report = absorb(&mut kept, Gift::of(&other), 3000).unwrap();
        assert_eq!(report, AbsorbReport { added: 1, matched: 1 });
        assert_eq!(kept.doc.entries.len(), 2);
    }

    #[test]
    fn the_same_login_written_at_the_same_moment_keeps_what_only_the_other_held() {
        let mut kept = fast_vault(1);
        let mut other = fast_vault(1);
        let at = Hlc { ms: 1000, counter: 0, device: 1 };
        kept.add(login_entry("GitHub", "me", "pw1", at), 1000).unwrap();
        let mut theirs = login_entry("GitHub", "me", "pw1", at);
        theirs.common.notes = Lww::new(crate::Secret::new("recovery codes"), at);
        other.add(theirs, 1000).unwrap();
        let report = absorb(&mut kept, Gift::of(&other), 3000).unwrap();
        assert_eq!(report.matched, 1);
        let merged = kept.doc.entries.values().next().unwrap();
        assert_eq!(merged.common.notes.v.expose(), "recovery codes");
    }

    #[test]
    fn folders_keep_their_nesting_and_existing_ones_are_reused() {
        let mut kept = fast_vault(1);
        let work = kept.add_folder("Work", None, 1000).unwrap();
        let mut other = fast_vault(2);
        let their_work = other.add_folder("Work", None, 1000).unwrap();
        let deep = other.add_folder("Servers", Some(their_work), 1000).unwrap();
        let at = other.tick(2000);
        let mut entry = login_entry("Box", "root", "x", at);
        entry.common.folder = Lww::new(Some(deep), at);
        let id = other.add(entry, 2000).unwrap();
        absorb(&mut kept, Gift::of(&other), 3000).unwrap();
        let folder = kept.get(&id).unwrap().common.folder.v.clone().unwrap();
        let servers = &kept.doc.folders[&folder];
        assert_eq!(servers.name.v, "Servers");
        assert_eq!(servers.parent.v.as_deref(), Some(work.as_str()));
        assert_eq!(kept.doc.folders.len(), 2);
    }

    #[test]
    fn an_item_whose_id_was_deleted_here_for_good_survives_the_next_merge() {
        let mut kept = fast_vault(1);
        let at = kept.tick(1000);
        let id = kept.add(login_entry("Gone", "u", "p", at), 1000).unwrap();
        let mut other = fast_vault(2);
        let mut copy = login_entry("Kept there", "v", "q", Hlc { ms: 500, counter: 0, device: 2 });
        copy.id.clone_from(&id);
        other.add(copy, 500).unwrap();
        kept.purge(&id, 4000);
        let report = absorb(&mut kept, Gift::of(&other), 5000).unwrap();
        assert_eq!(report.added, 1);
        let again = kept.doc.clone();
        kept.doc.merge_from(&again);
        assert_eq!(kept.doc.entries.len(), 1, "the arrival must not fall to the old delete");
    }

    #[test]
    fn the_trash_stays_behind() {
        let mut kept = fast_vault(1);
        let mut other = fast_vault(2);
        let at = other.tick(2000);
        let id = other.add(login_entry("Old", "u", "p", at), 2000).unwrap();
        other.trash(&id, 2100).unwrap();
        let report = absorb(&mut kept, Gift::of(&other), 3000).unwrap();
        assert_eq!(report.added, 0);
        assert!(kept.doc.entries.is_empty());
    }
}
