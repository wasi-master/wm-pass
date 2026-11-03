//! SMB 2 and 3 shares: NTLMv2 sign-in, signing and encryption.
//!
//! A file is written to a `.part` name and renamed over the final one, which SMB does
//! atomically and with replace, so a half-written file never carries a name a routine
//! would count.
//!
//! Every verb is one short connection: a backup runs once a day, and a session held open
//! between runs is a session the server dropped.
//!
//! Hand-written, as on Android: the useful subset of the protocol is a dozen fixed-layout
//! messages. What is delegated is the cryptography, to RustCrypto crates.

mod bytes;
mod client;
mod crypto;
#[cfg(test)]
mod fake;
mod ntlm;
mod spnego;
#[cfg(test)]
mod tests;

use std::time::Duration;

use wmpass_core::Secret;
use wmpass_net::NetPolicy;

use crate::{Fault, Result, Sink, Stored};
use client::{
    ACCESS_DELETE, ACCESS_READ_ATTRIBUTES, ACCESS_READ_DATA, ACCESS_SYNCHRONIZE, ACCESS_WRITE_ATTRIBUTES,
    ACCESS_WRITE_DATA, DISPOSITION_OPEN, DISPOSITION_OPEN_IF, DISPOSITION_OVERWRITE_IF, OPTION_DELETE_ON_CLOSE,
    OPTION_DIRECTORY, OPTION_NON_DIRECTORY, SHARE_ALL, Smb2Client, SmbError,
};

/// Suffix while writing.
const PART: &str = ".part";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// A folder on a Windows or Samba share.
#[derive(Clone)]
pub struct SmbConfig {
    /// Server name or address.
    pub host: String,
    /// 445 unless the server says otherwise.
    pub port: u16,
    /// The share name alone, `backups` for `\\nas\backups`.
    pub share: String,
    /// Folder inside the share, with either slash. Empty is the share's root.
    pub path: String,
    /// The Windows domain or workgroup. Usually empty.
    pub domain: String,
    /// Account name.
    pub user: String,
    /// Its password.
    pub password: Secret,
    /// Requires SMB 3 encryption of everything after the sign-in, and refuses a server
    /// that cannot do it. On by default, the same stance as TLS for FTP: without it the
    /// file crosses the network as plain bytes, though it is already encrypted. Old
    /// servers that stop at SMB 2 need it off.
    pub encrypt: bool,
}

impl Default for SmbConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 445,
            share: String::new(),
            path: String::new(),
            domain: String::new(),
            user: String::new(),
            password: Secret::default(),
            encrypt: true,
        }
    }
}

impl std::fmt::Debug for SmbConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Host, share, folder and account all say where the file lives and whose it is.
        f.debug_struct("SmbConfig")
            .field("port", &self.port)
            .field("encrypt", &self.encrypt)
            .field("configured", &self.configured())
            .finish_non_exhaustive()
    }
}

impl SmbConfig {
    /// Whether there is enough to try a connection.
    pub fn configured(&self) -> bool {
        !self.host.trim().is_empty() && !self.share.trim().is_empty() && !self.user.is_empty()
    }
}

/// A folder on an SMB share.
pub struct Smb {
    config: SmbConfig,
    policy: NetPolicy,
    /// The folder inside the share, with the backslashes SMB wants.
    folder: String,
}

impl std::fmt::Debug for Smb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Smb").field("config", &self.config).finish_non_exhaustive()
    }
}

/// What an SMB failure means to the user.
fn fault_of(error: SmbError) -> Fault {
    match error {
        SmbError::Unsafe => Fault::Unsafe,
        SmbError::Io => Fault::Io,
        SmbError::Status(status) => match status {
            client::STATUS_LOGON_FAILURE
            | client::STATUS_ACCESS_DENIED
            | client::STATUS_ACCOUNT_DISABLED
            | client::STATUS_PASSWORD_EXPIRED
            | client::STATUS_ACCOUNT_LOCKED_OUT => Fault::PermissionLost,
            client::STATUS_BAD_NETWORK_NAME | client::STATUS_OBJECT_PATH_NOT_FOUND | client::STATUS_OBJECT_NAME_NOT_FOUND => {
                Fault::TargetMissing
            }
            client::STATUS_DISK_FULL | client::STATUS_QUOTA_EXCEEDED => Fault::OutOfSpace,
            _ => Fault::Io,
        },
    }
}

impl Smb {
    /// A folder on a share, reached under `policy`.
    pub fn new(policy: NetPolicy, config: SmbConfig) -> Self {
        let folder = config.path.replace('/', "\\").trim_matches('\\').to_owned();
        Self { config, policy, folder }
    }

    fn path_of(&self, name: &str) -> std::result::Result<String, Fault> {
        // A name read from elsewhere must not walk out of the folder.
        if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
            return Err(Fault::NotConfigured);
        }
        Ok(if self.folder.is_empty() { name.to_owned() } else { format!("{}\\{name}", self.folder) })
    }

    /// Connects, signs in, opens the share, runs `block`, and maps every way that can
    /// fail onto a [`Fault`].
    fn session<T>(&self, block: impl FnOnce(&mut Smb2Client) -> std::result::Result<T, SmbError>) -> Result<T> {
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        wmpass_net::http::check_socket(self.policy).map_err(|_| Fault::Blocked)?;
        let mut client =
            Smb2Client::connect(self.config.host.trim(), self.config.port, CONNECT_TIMEOUT, IO_TIMEOUT).map_err(fault_of)?;
        let result = (|| {
            client.negotiate()?;
            client.login(&self.config.user, self.config.password.expose(), self.config.domain.trim(), self.config.encrypt)?;
            client.tree_connect(self.config.share.trim().trim_matches(['\\', '/']))?;
            block(&mut client)
        })();
        client.disconnect();
        result.map_err(fault_of)
    }
}

/// Already gone is success: rotation reruns after a failure.
fn delete_file(client: &mut Smb2Client, path: &str) -> std::result::Result<(), SmbError> {
    let file = match client.create(
        path,
        ACCESS_DELETE | ACCESS_READ_ATTRIBUTES,
        DISPOSITION_OPEN,
        OPTION_NON_DIRECTORY | OPTION_DELETE_ON_CLOSE,
        SHARE_ALL,
    ) {
        Ok(file) => file,
        Err(SmbError::Status(client::STATUS_OBJECT_NAME_NOT_FOUND)) => return Ok(()),
        Err(other) => return Err(other),
    };
    client.close(&file)
}

impl Sink for Smb {
    async fn readiness(&self) -> Result<()> {
        // Creates the folder when it is missing, one level at a time, so a new location
        // works on its first run without a trip to the NAS.
        self.session(|client| {
            let mut at = String::new();
            for part in self.folder.split('\\').filter(|part| !part.is_empty()) {
                if !at.is_empty() {
                    at.push('\\');
                }
                at.push_str(part);
                let dir = client.create(&at, ACCESS_READ_ATTRIBUTES | ACCESS_SYNCHRONIZE, DISPOSITION_OPEN_IF, OPTION_DIRECTORY, SHARE_ALL)?;
                client.close(&dir)?;
            }
            Ok(())
        })
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let target = self.path_of(name)?;
        let staged = self.path_of(&format!("{name}{PART}"))?;
        self.session(|client| {
            let part = client.create(
                &staged,
                ACCESS_WRITE_DATA | ACCESS_READ_ATTRIBUTES | ACCESS_WRITE_ATTRIBUTES | ACCESS_DELETE | ACCESS_SYNCHRONIZE,
                DISPOSITION_OVERWRITE_IF,
                OPTION_NON_DIRECTORY,
                0,
            )?;
            let moved = client.write(&part, bytes).and_then(|()| client.rename(&part, &target, true));
            if let Err(failure) = moved {
                let _ = client.close(&part);
                let _ = delete_file(client, &staged);
                return Err(failure);
            }
            client.close(&part)
        })?;
        Ok(Stored { id: name.to_owned(), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        self.session(|client| {
            let dir =
                client.create(&self.folder, ACCESS_READ_DATA | ACCESS_READ_ATTRIBUTES | ACCESS_SYNCHRONIZE, DISPOSITION_OPEN, OPTION_DIRECTORY, SHARE_ALL)?;
            let entries = client.list(&dir);
            let _ = client.close(&dir);
            Ok(entries?
                .into_iter()
                .filter(|entry| !entry.directory && !entry.name.ends_with(PART))
                .map(|entry| Stored { id: entry.name.clone(), name: entry.name, size: entry.size })
                .collect())
        })
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let path = self.path_of(&file.id)?;
        self.session(|client| {
            let handle = client.create(
                &path,
                ACCESS_READ_DATA | ACCESS_READ_ATTRIBUTES | ACCESS_SYNCHRONIZE,
                DISPOSITION_OPEN,
                OPTION_NON_DIRECTORY,
                SHARE_ALL,
            )?;
            let bytes = client.read_all(&handle);
            let _ = client.close(&handle);
            bytes
        })
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let path = self.path_of(&file.id)?;
        self.session(|client| delete_file(client, &path))
    }
}
