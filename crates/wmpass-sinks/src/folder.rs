//! A local folder, which also covers anything another app syncs: Syncthing, Dropbox's
//! desktop client, a USB stick.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Fault, Result, Sink, Stored};

/// A folder on this device.
#[derive(Clone, Debug)]
pub struct Folder {
    path: PathBuf,
}

impl Folder {
    /// A folder, created when first written to.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self { path: path.as_ref().to_owned() }
    }

    fn checked(&self, name: &str) -> Result<PathBuf> {
        // A name read from elsewhere must not walk out of the folder.
        if name.is_empty() || name.contains(['/', '\\']) || name == ".." || name == "." {
            return Err(Fault::NotConfigured);
        }
        Ok(self.path.join(name))
    }
}

fn fault(error: &std::io::Error) -> Fault {
    match error.kind() {
        std::io::ErrorKind::NotFound => Fault::TargetMissing,
        std::io::ErrorKind::PermissionDenied => Fault::PermissionLost,
        std::io::ErrorKind::StorageFull => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

impl Sink for Folder {
    async fn readiness(&self) -> Result<()> {
        fs::create_dir_all(&self.path).map_err(|e| fault(&e))
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let target = self.checked(name)?;
        fs::create_dir_all(&self.path).map_err(|e| fault(&e))?;
        let staged = self.path.join(format!(".{name}.upload"));
        // On disk before the rename, so a power cut cannot leave the real name pointing at
        // a file whose contents never landed.
        let written = fs::File::create(&staged).and_then(|mut file| {
            std::io::Write::write_all(&mut file, bytes)?;
            file.sync_all()
        });
        if let Err(error) = written {
            let _ = fs::remove_file(&staged);
            return Err(fault(&error));
        }
        fs::rename(&staged, &target).map_err(|e| fault(&e))?;
        Ok(Stored { id: name.to_owned(), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let reader = match fs::read_dir(&self.path) {
            Ok(reader) => reader,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(fault(&error)),
        };
        let mut out = Vec::new();
        for item in reader {
            let item = item.map_err(|e| fault(&e))?;
            let Ok(name) = item.file_name().into_string() else { continue };
            // Hidden names are uploads under way, here or by the app that syncs the folder,
            // and can be renamed away at any moment.
            if name.starts_with('.') {
                continue;
            }
            let meta = match item.metadata() {
                Ok(meta) => meta,
                // Gone since the folder was read, by another device's write: not a missing folder.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(fault(&error)),
            };
            if meta.is_file() {
                out.push(Stored { id: name.clone(), name, size: meta.len() });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        fs::read(self.checked(&file.id)?).map_err(|e| fault(&e))
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        match fs::remove_file(self.checked(&file.id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(fault(&error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::block_on;

    #[test]
    fn round_trip_and_hidden_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Folder::new(dir.path().join("wm"));
        assert!(block_on(sink.list()).unwrap().is_empty());
        block_on(sink.readiness()).unwrap();
        let stored = block_on(sink.write("v", b"one")).unwrap();
        block_on(sink.write("v", b"two")).unwrap();
        fs::write(dir.path().join("wm/.other.upload"), b"half").unwrap();
        let files = block_on(sink.list()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(block_on(sink.read(&stored)).unwrap(), b"two");
        block_on(sink.delete(&stored)).unwrap();
        block_on(sink.delete(&stored)).unwrap();
        assert!(block_on(sink.list()).unwrap().is_empty());
    }

    #[test]
    fn names_cannot_walk_out() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Folder::new(dir.path());
        for bad in ["../x", "a/b", "..", "", "a\\b"] {
            assert_eq!(block_on(sink.write(bad, b"x")).err(), Some(Fault::NotConfigured), "{bad}");
        }
        let hostile = Stored { id: "../../etc/passwd".into(), ..Stored::default() };
        assert_eq!(block_on(sink.read(&hostile)).err(), Some(Fault::NotConfigured));
    }
}
