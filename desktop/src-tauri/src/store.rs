//! Files in the app's own folder. The page names them by relative path, and every path is checked
//! here, so nothing the page says can reach outside this folder. All of it is ciphertext, apart
//! from settings that hold nothing secret.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

/// The same moment everywhere, so file times do not say which vault file changed.
const COVER_TIME_MS: u64 = 1_600_000_000_000;

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}

#[derive(Serialize)]
pub struct Listed {
    pub name: String,
    pub size: u64,
}

fn segment_is_fine(part: &str) -> bool {
    !part.is_empty() && part != "." && part != ".." && part.len() <= 128 && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Store {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        restrict(&root);
        Ok(Self { root })
    }


    /// A path inside the folder, or a refusal.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf, String> {
        let mut path = self.root.clone();
        for part in relative.split('/') {
            if !segment_is_fine(part) {
                return Err("that path is not allowed".to_owned());
            }
            path.push(part);
        }
        Ok(path)
    }

    pub fn list(&self, folder: &str) -> Result<Vec<Listed>, String> {
        let path = self.resolve(folder)?;
        let Ok(entries) = fs::read_dir(path) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
            if name.ends_with(".part") {
                continue;
            }
            let size = entry.metadata().map_or(0, |m| if m.is_file() { m.len() } else { 0 });
            out.push(Listed { name, size });
        }
        Ok(out)
    }

    pub fn read(&self, relative: &str) -> Result<Vec<u8>, String> {
        fs::read(self.resolve(relative)?).map_err(|_| "that file is gone".to_owned())
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) -> Result<(), String> {
        write_atomically(&self.resolve(relative)?, bytes)
    }

    pub fn remove(&self, relative: &str) -> Result<(), String> {
        match fs::remove_file(self.resolve(relative)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    pub fn remove_folder(&self, relative: &str) -> Result<(), String> {
        match fs::remove_dir_all(self.resolve(relative)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    pub fn cover(&self, relative: &str) -> Result<(), String> {
        let file = fs::File::options().write(true).open(self.resolve(relative)?).map_err(|e| e.to_string())?;
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_millis(COVER_TIME_MS)).map_err(|e| e.to_string())
    }
}

/// Written beside the target and moved over it, so a crash leaves the old file or the new one.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
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

/// Other users on this computer have no business here, even for ciphertext.
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside() {
        let store = Store { root: PathBuf::from("/tmp/x") };
        assert!(store.resolve("vaults/abc123").is_ok());
        assert!(store.resolve("blobs/abc/def.bin").is_ok());
        for bad in ["../x", "vaults/../../x", "/etc/passwd", "vaults//x", "", "a/./b", "vaults\\x", "vaults/x y"] {
            assert!(store.resolve(bad).is_err(), "{bad}");
        }
    }
}
