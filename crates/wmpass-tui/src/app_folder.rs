//! The desktop app's own folder, so the terminal app and `wmpass` open the same vault as the WM
//! Pass app on this computer instead of a copy of their own.
//!
//! The app keeps its vaults as files with random names in `vaults/`, beside decoys of the same
//! size, with every file's time set to one moment, so the folder never says which file is a
//! vault or which one changed. Attachments live in `blobs/<vault file>/`. Its door state, a
//! count of wrong passwords and when the next try is allowed, is in `settings/door.json`. All
//! of this is kept the way the app keeps it, so either one can write the folder at any time.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// The desktop app's identifier, which names its folder.
pub const APP_ID: &str = "com.wasimaster.wmpass.desktop";

/// The time every file in `vaults/` carries.
const COVER_TIME_MS: u64 = 1_600_000_000_000;

/// Where the desktop app keeps its data on this computer: `WMPASS_APP_DIR`, then the place
/// Tauri's `app_data_dir` uses on each system.
pub fn default_root() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    if let Some(dir) = std::env::var_os("WMPASS_APP_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share")))
    };
    base.map(|b| b.join(APP_ID))
}

/// Whether `path` is an app folder rather than a vault file.
pub fn is_app_folder(path: &Path) -> bool {
    path.join("vaults").is_dir()
}

/// The app's folder, if the app has a vault in it.
pub fn with_vaults() -> Option<PathBuf> {
    default_root().filter(|root| names(root).is_ok_and(|n| !n.is_empty()))
}

/// The file names in `vaults/`, in the order the app tries them.
pub fn names(root: &Path) -> std::io::Result<Vec<String>> {
    let mut out: Vec<String> = fs::read_dir(root.join("vaults"))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.') && !n.ends_with(".part"))
        .collect();
    out.sort();
    Ok(out)
}

/// Every file in `vaults/`, vaults and decoys alike, with its name.
pub fn files(root: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    names(root)
        .map_err(|e| format!("could not read {}: {e}", root.display()))?
        .into_iter()
        .map(|name| fs::read(vault_path(root, &name)).map(|bytes| (name, bytes)).map_err(|e| e.to_string()))
        .collect()
}

pub fn vault_path(root: &Path, name: &str) -> PathBuf {
    root.join("vaults").join(name)
}

/// The app folder a vault file is in, if it is in one.
pub fn root_of(vault: &Path) -> Option<PathBuf> {
    let parent = vault.parent()?;
    (parent.file_name()? == "vaults" && vault.extension().is_none()).then(|| parent.parent().map(Path::to_owned)).flatten()
}

/// A new random file name, as the app makes them.
pub fn new_name() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "no randomness available".to_owned())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes beside the target and moves over it, readable by this user only.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("that path is not allowed")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("that path is not allowed")?;
    let temporary = parent.join(format!("{name}.part"));
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    restrict(&temporary);
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temporary, path).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

/// After a vault of `saved_len` bytes is written: its partner decoy is brought to the same size,
/// made by `chaff`, and every file's time is set back to the one moment.
pub fn cover_tracks(root: &Path, partner: &str, saved_len: usize, chaff: impl FnOnce(usize) -> Result<Vec<u8>, String>) -> Result<(), String> {
    let path = vault_path(root, partner);
    if fs::metadata(&path).map(|m| m.len()).ok() != u64::try_from(saved_len).ok() {
        write(&path, &chaff(saved_len)?)?;
    }
    cover_times(root)
}

/// Sets every file in `vaults/` to the one moment.
pub fn cover_times(root: &Path) -> Result<(), String> {
    let when = SystemTime::UNIX_EPOCH + Duration::from_millis(COVER_TIME_MS);
    for name in names(root).map_err(|e| e.to_string())? {
        let file = fs::File::options().write(true).open(vault_path(root, &name)).map_err(|e| e.to_string())?;
        file.set_modified(when).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The app's count of wrong passwords, shared so no front end gets more tries than another.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorState {
    pub device: u32,
    pub fails: u32,
    /// When typing a password is allowed again, in ms since 1970, or 0.
    pub retry_at: u64,
    /// Vault file name to the newest generation seen, for noticing a rolled-back copy.
    pub seen: serde_json::Map<String, serde_json::Value>,
}

fn door_path(root: &Path) -> PathBuf {
    root.join("settings").join("door.json")
}

impl DoorState {
    pub fn load(root: &Path) -> Self {
        fs::read(door_path(root)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, root: &Path) {
        if let Ok(text) = serde_json::to_vec(self) {
            let _ = write(&door_path(root), &text);
        }
    }

    /// When the next try is allowed, if that is still ahead.
    pub fn waiting(&self, now_ms: u64) -> Option<u64> {
        (self.retry_at > now_ms).then_some(self.retry_at)
    }

    /// A wrong password: five free tries, then thirty seconds doubling up to fifteen minutes,
    /// as the app counts them.
    pub fn failed(&mut self, now_ms: u64) {
        self.fails += 1;
        let wait = if self.fails <= 5 { 0 } else { (30_000u64 << (self.fails - 6).min(5)).min(900_000) };
        self.retry_at = if wait == 0 { 0 } else { now_ms + wait };
    }

    pub fn opened(&mut self) {
        self.fails = 0;
        self.retry_at = 0;
    }

    /// A duress password set to lock: every password refused for an hour.
    pub fn hard_lock(&mut self, now_ms: u64) {
        self.retry_at = now_ms + 3_600_000;
    }
}

/// A duress password set to wipe: the vaults, their files and linked vaults go, as in the app.
pub fn wipe(root: &Path) {
    for part in ["vaults", "blobs", "linked"] {
        let _ = fs::remove_dir_all(root.join(part));
    }
    let door = DoorState::load(root);
    DoorState { device: door.device, ..DoorState::default() }.save(root);
}

/// The app's own timers, for using its vault the way it is set to: auto-lock seconds, clipboard
/// seconds and reveal seconds, where it has them. These stay on each device, so they are not in
/// the vault; on one computer the two apps share them.
pub fn app_timers(root: &Path) -> (Option<u64>, Option<u64>, Option<u64>) {
    let prefs: serde_json::Value = fs::read(root.join("settings").join("prefs.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let get = |key: &str| prefs.get(key).and_then(serde_json::Value::as_u64);
    (get("lock_after"), get("clipboard_seconds"), get("reveal_seconds"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_matches_the_app() {
        let mut door = DoorState::default();
        for _ in 0..5 {
            door.failed(1000);
            assert_eq!(door.waiting(1000), None);
        }
        door.failed(1000);
        assert_eq!(door.waiting(1000), Some(31_000));
        door.failed(1000);
        assert_eq!(door.retry_at, 61_000);
        for _ in 0..10 {
            door.failed(1000);
        }
        assert_eq!(door.retry_at, 901_000);
        door.opened();
        assert_eq!(door.waiting(1000), None);
    }

    #[test]
    fn a_vault_in_the_folder_knows_its_folder() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path();
        fs::create_dir_all(root.join("vaults")).expect("mkdir");
        assert!(is_app_folder(root));
        let file = vault_path(root, "0123abcd");
        assert_eq!(root_of(&file).as_deref(), Some(root));
        assert_eq!(root_of(Path::new("/x/Personal.wmvault")), None);
        write(&file, b"vault").expect("write");
        write(&vault_path(root, "ffff"), b"decoy").expect("write");
        cover_tracks(root, "ffff", 5, |_| Ok(b"never".to_vec())).expect("cover");
        assert_eq!(fs::read(vault_path(root, "ffff")).expect("read"), b"decoy", "a decoy of the right size stays");
        cover_tracks(root, "eeee", 5, |n| Ok(vec![7; n])).expect("cover");
        assert_eq!(fs::read(vault_path(root, "eeee")).expect("read"), vec![7; 5]);
        let times: Vec<SystemTime> = names(root).expect("names").iter().map(|n| fs::metadata(vault_path(root, n)).and_then(|m| m.modified()).expect("time")).collect();
        assert!(times.windows(2).all(|w| w[0] == w[1]));
    }
}
