//! Files on disk: the vault itself, and the little the terminal app remembers between runs.
//!
//! Nothing written here holds a secret. The preferences file keeps paths, a theme and two
//! timers; the generation file keeps vault ids and revision numbers, shared with `wmpass`
//! so either one notices a copy rolled back behind the other's back.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use wmpass_core::vault::{Freshness, check_freshness, hex};

/// Where state lives: `WMPASS_STATE_DIR`, then the platform's usual place.
pub fn state_dir() -> Option<PathBuf> {
    // Tests never touch the real state of whoever runs them.
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("wmpass-tui-test-{}", std::process::id())));
    }
    if let Some(dir) = std::env::var_os("WMPASS_STATE_DIR") {
        return Some(PathBuf::from(dir));
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))?;
    Some(base.join("wmpass"))
}

/// What the terminal app remembers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Prefs {
    /// Vault files opened before, most recent first.
    pub recent: Vec<PathBuf>,
    /// Theme name.
    pub theme: String,
    /// `nerd` for Nerd Font icons.
    pub icons: String,
    /// Lock after this many minutes without a key press. Zero never locks.
    pub lock_minutes: u32,
    /// Clear a copied secret after this many seconds. Zero leaves it. Stored under the apps' own
    /// name: the 30 seconds older versions saved, with no way to change it, give way to 60.
    #[serde(rename = "clipboard_seconds")]
    pub clip_seconds: u32,
    /// Hide a revealed secret after this many seconds.
    pub reveal_seconds: u32,
    /// This device's number, which breaks ties when two copies merge.
    pub device: u32,
    /// Whether the theme, icons and reveal time follow the vault to other devices.
    pub sync_settings: bool,
    /// Keys from those that stay on this device, such as `tui_theme`.
    pub sync_skip: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            theme: String::new(),
            icons: String::new(),
            lock_minutes: 5,
            clip_seconds: 60,
            reveal_seconds: 20,
            device: 0,
            sync_settings: true,
            sync_skip: Vec::new(),
        }
    }
}

const RECENT_KEPT: usize = 8;

impl Prefs {
    fn path() -> Option<PathBuf> {
        state_dir().map(|dir| dir.join("tui.json"))
    }

    /// Reads the preferences, or the defaults when there are none.
    pub fn load() -> Self {
        let mut prefs: Self = Self::path()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if prefs.device == 0 {
            let mut bytes = [0u8; 4];
            let _ = getrandom::fill(&mut bytes);
            prefs.device = u32::from_le_bytes(bytes).max(1);
            prefs.save();
        }
        prefs
    }

    /// Writes the preferences. A failure only costs remembering them.
    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_vec_pretty(self) {
            let _ = fs::write(path, text);
        }
    }

    /// Moves a path to the top of the recent list.
    pub fn remember(&mut self, path: &Path) {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(RECENT_KEPT);
        self.save();
    }
}

fn seen_file() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("seen.json"))
}

fn seen_generations() -> HashMap<String, u64> {
    seen_file()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn vault_id(file: &[u8]) -> Option<String> {
    wmpass_core::format::Header::parse(file)
        .ok()
        .map(|header| hex(header.vault_id()))
}

/// The highest revision of this vault seen on this device.
pub fn seen_generation(file: &[u8]) -> u64 {
    vault_id(file)
        .and_then(|id| seen_generations().get(&id).copied())
        .unwrap_or(0)
}

/// Whether this copy is older than one seen here before.
pub fn rolled_back(file: &[u8], generation: u64) -> Option<(u64, u64)> {
    match check_freshness(seen_generation(file), generation) {
        Freshness::Rollback { seen, file } => Some((seen, file)),
        _ => None,
    }
}

/// Records the revision just read or written.
pub fn remember_generation(file: &[u8], generation: u64) {
    let (Some(path), Some(id)) = (seen_file(), vault_id(file)) else {
        return;
    };
    let mut seen = seen_generations();
    let slot = seen.entry(id).or_default();
    *slot = (*slot).max(generation);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_vec(&seen) {
        let _ = fs::write(path, text);
    }
}

/// Reads a file, with its name in any error.
pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes through a temporary file and a rename, keeping the old file as `.bak`, so a crash
/// at any moment leaves a readable vault.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    restrict(&temp);
    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak");
        fs::copy(path, PathBuf::from(backup)).map_err(|e| format!("could not keep a backup: {e}"))?;
    }
    fs::rename(&temp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes a new file nobody else may read. Refuses to replace one.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    restrict(path);
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

/// Where a vault's attachments live: beside it, as the command line keeps them, or under
/// `blobs/` for a vault in the desktop app's folder.
pub fn blob_dir(vault: &Path) -> PathBuf {
    match (crate::app_folder::root_of(vault), vault.file_name()) {
        (Some(root), Some(name)) => root.join("blobs").join(name),
        _ => vault.with_extension("blobs"),
    }
}

/// A file's size and time, to notice another program writing it.
pub fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Expands a leading `~`, which a shell would have done for an argument.
pub fn expand(text: &str) -> PathBuf {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\"))
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_paths_stay_unique_and_few() {
        let mut prefs = Prefs::default();
        for n in 0..12 {
            let path = PathBuf::from(format!("/v{n}"));
            prefs.recent.retain(|p| *p != path);
            prefs.recent.insert(0, path);
            prefs.recent.truncate(RECENT_KEPT);
        }
        assert_eq!(prefs.recent.len(), RECENT_KEPT);
        assert_eq!(prefs.recent[0], PathBuf::from("/v11"));
    }

    #[test]
    fn the_clipboard_waits_a_minute_as_in_the_apps() {
        assert_eq!(Prefs::default().clip_seconds, 60);
        // The 30 seconds older versions saved, under the old key, give way to the new default.
        let old: Prefs = serde_json::from_str(r#"{"clip_seconds": 30, "lock_minutes": 9}"#).expect("old prefs");
        assert_eq!((old.clip_seconds, old.lock_minutes), (60, 9));
        let new: Prefs = serde_json::from_str(r#"{"clipboard_seconds": 15}"#).expect("new prefs");
        assert_eq!(new.clip_seconds, 15);
    }

    #[test]
    fn a_tilde_means_home() {
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(expand("~/a.wmvault"), PathBuf::from(home).join("a.wmvault"));
        }
        assert_eq!(expand(" /x/y "), PathBuf::from("/x/y"));
    }

    #[test]
    fn writing_keeps_the_last_copy() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("v.wmvault");
        write_atomic(&path, b"one").expect("first");
        write_atomic(&path, b"two").expect("second");
        assert_eq!(fs::read(&path).expect("read"), b"two");
        assert_eq!(fs::read(dir.path().join("v.wmvault.bak")).expect("bak"), b"one");
        assert!(write_new(&path, b"x").is_err(), "never replaces a file");
    }
}
