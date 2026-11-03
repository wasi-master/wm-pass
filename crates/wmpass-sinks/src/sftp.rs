//! SFTP version 3, over [`crate::ssh`].
//!
//! Version 3 (draft-ietf-secsh-filexfer-02) is the version every server speaks, and what a
//! backup needs of it is seven requests. Writes and reads are pipelined, [`WINDOW`]
//! requests in flight, so a large file is not one network round trip per 32 KiB.
//!
//! A file goes to a `.part` name and is renamed into place, atomically on OpenSSH through
//! its `posix-rename@openssh.com` extension. Host keys are trusted on first use: see
//! [`crate::ssh`].

use wmpass_core::Secret;
use wmpass_net::NetPolicy;

pub use crate::ssh::fingerprint;
use crate::ssh::{Login, Parse, Session, Wire};
use crate::{Fault, Result, Sink, Stored};

/// Where a file is written before it is renamed over the real name.
const PART: &str = ".part";
const VERSION: u32 = 3;
const CHUNK: usize = 32 * 1024;
const WINDOW: usize = 16;
const MAX_PACKET: usize = 256 * 1024;
const POSIX_RENAME: &str = "posix-rename@openssh.com";

const FXP_INIT: u8 = 1;
const FXP_VERSION: u8 = 2;
const FXP_OPEN: u8 = 3;
const FXP_CLOSE: u8 = 4;
const FXP_READ: u8 = 5;
const FXP_WRITE: u8 = 6;
const FXP_OPENDIR: u8 = 11;
const FXP_READDIR: u8 = 12;
const FXP_REMOVE: u8 = 13;
const FXP_MKDIR: u8 = 14;
const FXP_STAT: u8 = 17;
const FXP_RENAME: u8 = 18;
const FXP_STATUS: u8 = 101;
const FXP_HANDLE: u8 = 102;
const FXP_DATA: u8 = 103;
const FXP_NAME: u8 = 104;
const FXP_ATTRS: u8 = 105;
const FXP_EXTENDED: u8 = 200;

const FXF_READ: u32 = 0x1;
const FXF_WRITE: u32 = 0x2;
const FXF_CREAT: u32 = 0x8;
const FXF_TRUNC: u32 = 0x10;

const ATTR_SIZE: u32 = 0x1;
const ATTR_UIDGID: u32 = 0x2;
const ATTR_PERMISSIONS: u32 = 0x4;
const ATTR_ACMODTIME: u32 = 0x8;
const ATTR_EXTENDED: u32 = 0x8000_0000;
const S_IFMT: u32 = 0xF000;
const S_IFDIR: u32 = 0x4000;

const FX_OK: u32 = 0;
const FX_EOF: u32 = 1;
const FX_NO_SUCH_FILE: u32 = 2;
const FX_PERMISSION_DENIED: u32 = 3;

/// An SFTP server: SSH, with the SFTP subsystem.
#[derive(Clone)]
pub struct SftpConfig {
    /// Server name or address.
    pub host: String,
    /// TCP port, 22 by default.
    pub port: u16,
    /// User name.
    pub user: String,
    /// Empty when [`Self::private_key`] does the signing in.
    pub password: Secret,
    /// A private key in OpenSSH or PEM form, pasted whole. Tried before the password when
    /// both are there, which is what `ssh` does too.
    pub private_key: Secret,
    /// Unlocks [`Self::private_key`] when it is encrypted.
    pub key_passphrase: Secret,
    /// Directory to write into. Relative paths start where the login lands.
    pub path: String,
    /// The server's host key the first connection saw, as `<algorithm> <base64>`. Empty
    /// until then. A later connection that is shown a different key refuses, because that
    /// is exactly what a machine in the middle looks like.
    pub host_key: String,
    /// Also offers SHA-1 signatures, SHA-1 MACs and the 2048-bit SHA-1 group, which
    /// OpenSSH itself has turned off. Some old NAS firmware speaks nothing newer.
    pub legacy_algorithms: bool,
}

impl Default for SftpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            user: String::new(),
            password: Secret::default(),
            private_key: Secret::default(),
            key_passphrase: Secret::default(),
            path: String::new(),
            host_key: String::new(),
            legacy_algorithms: false,
        }
    }
}

impl std::fmt::Debug for SftpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpConfig")
            .field("configured", &self.configured())
            .field("pinned", &!self.host_key.is_empty())
            .field("legacy_algorithms", &self.legacy_algorithms)
            .finish_non_exhaustive()
    }
}

impl SftpConfig {
    /// Whether there is enough to try a connection.
    pub fn configured(&self) -> bool {
        !self.host.trim().is_empty()
            && !self.user.is_empty()
            && (!self.password.is_empty() || !self.private_key.is_empty())
    }
}

// ---- Protocol ----

/// The two directions of a byte stream: an SSH channel, or a fake server in tests.
pub(crate) trait Pipe {
    fn send(&mut self, data: &[u8]) -> Result<()>;
    fn recv(&mut self, buf: &mut [u8]) -> Result<()>;
}

impl Pipe for Session {
    fn send(&mut self, data: &[u8]) -> Result<()> {
        Session::send(self, data)
    }

    fn recv(&mut self, buf: &mut [u8]) -> Result<()> {
        Session::recv(self, buf)
    }
}

impl<T: Pipe + ?Sized> Pipe for &mut T {
    fn send(&mut self, data: &[u8]) -> Result<()> {
        (**self).send(data)
    }

    fn recv(&mut self, buf: &mut [u8]) -> Result<()> {
        (**self).recv(buf)
    }
}

/// An SFTP failure: a status the server answered, or the connection under it.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Status(u32),
    Broken(Fault),
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Broken(fault)
    }
}

type Answer<T> = std::result::Result<T, Failure>;

/// What an SFTP failure means to the user.
fn fault_of(failure: Failure) -> Fault {
    match failure {
        Failure::Status(FX_PERMISSION_DENIED) => Fault::PermissionLost,
        Failure::Status(FX_NO_SUCH_FILE) => Fault::TargetMissing,
        Failure::Status(_) => Fault::Io,
        Failure::Broken(fault) => fault,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Attrs {
    size: Option<u64>,
    directory: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    attrs: Attrs,
}

fn attrs(p: &mut Parse<'_>) -> Result<Attrs> {
    let flags = p.u32()?;
    let mut out = Attrs { size: None, directory: false };
    if flags & ATTR_SIZE != 0 {
        out.size = Some(p.u64()?);
    }
    if flags & ATTR_UIDGID != 0 {
        p.u32()?;
        p.u32()?;
    }
    if flags & ATTR_PERMISSIONS != 0 {
        out.directory = p.u32()? & S_IFMT == S_IFDIR;
    }
    if flags & ATTR_ACMODTIME != 0 {
        p.u32()?;
        p.u32()?;
    }
    if flags & ATTR_EXTENDED != 0 {
        for _ in 0..p.u32()? {
            p.bytes()?;
            p.bytes()?;
        }
    }
    Ok(out)
}

/// The client half of SFTP v3, over any [`Pipe`].
struct SftpClient<P> {
    pipe: P,
    next_id: u32,
    /// Extensions from the server's VERSION reply, `posix-rename@openssh.com` among them.
    extensions: Vec<String>,
}

impl<P: Pipe> SftpClient<P> {
    fn new(pipe: P) -> Self {
        Self { pipe, next_id: 1, extensions: Vec::new() }
    }

    fn init(&mut self) -> Answer<()> {
        self.send_packet(&Wire::msg(FXP_INIT).u32(VERSION).0)?;
        let (kind, body) = self.receive()?;
        if kind != FXP_VERSION {
            return Err(Fault::Io.into());
        }
        let mut p = Parse::new(&body);
        if p.u32()? < VERSION {
            return Err(Fault::Io.into());
        }
        while !p.done() {
            self.extensions.push(p.text()?.to_owned());
            p.bytes()?;
        }
        Ok(())
    }

    fn send_packet(&mut self, payload: &[u8]) -> Result<()> {
        let mut framed = Vec::with_capacity(4 + payload.len());
        framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        framed.extend_from_slice(payload);
        self.pipe.send(&framed)
    }

    fn request(&mut self, kind: u8, body: Wire) -> Result<u32> {
        let id = self.next_id;
        self.next_id = id.wrapping_add(1);
        self.send_packet(&Wire::msg(kind).u32(id).raw(&body.0).0)?;
        Ok(id)
    }

    fn receive(&mut self) -> Result<(u8, Vec<u8>)> {
        let mut head = [0u8; 4];
        self.pipe.recv(&mut head)?;
        let length = u32::from_be_bytes(head) as usize;
        if !(1..=MAX_PACKET).contains(&length) {
            return Err(Fault::Io);
        }
        let mut packet = vec![0u8; length];
        self.pipe.recv(&mut packet)?;
        let body = packet.split_off(1);
        Ok((packet[0], body))
    }

    /// The reply to `id`, without its id. Replies arrive in request order on every real
    /// server.
    fn reply(&mut self, id: u32) -> Result<(u8, Vec<u8>)> {
        let (kind, mut body) = self.receive()?;
        if body.len() < 4 || body[..4] != id.to_be_bytes() {
            return Err(Fault::Io);
        }
        Ok((kind, body.split_off(4)))
    }

    fn status(body: &[u8]) -> Result<u32> {
        Parse::new(body).u32()
    }

    fn expect_ok(&mut self, id: u32) -> Answer<()> {
        let (kind, body) = self.reply(id)?;
        if kind != FXP_STATUS {
            return Err(Fault::Io.into());
        }
        match Self::status(&body)? {
            FX_OK => Ok(()),
            code => Err(Failure::Status(code)),
        }
    }

    fn handle_of(&mut self, id: u32) -> Answer<Vec<u8>> {
        let (kind, body) = self.reply(id)?;
        match kind {
            FXP_HANDLE => Ok(Parse::new(&body).bytes()?.to_vec()),
            FXP_STATUS => Err(Failure::Status(Self::status(&body)?)),
            _ => Err(Fault::Io.into()),
        }
    }

    /// Attributes of `path`, or `None` when nothing is there.
    fn stat(&mut self, path: &str) -> Answer<Option<Attrs>> {
        let id = self.request(FXP_STAT, Wire::default().text(path))?;
        let (kind, body) = self.reply(id)?;
        match kind {
            FXP_ATTRS => Ok(Some(attrs(&mut Parse::new(&body))?)),
            FXP_STATUS => match Self::status(&body)? {
                FX_NO_SUCH_FILE => Ok(None),
                code => Err(Failure::Status(code)),
            },
            _ => Err(Fault::Io.into()),
        }
    }

    fn mkdir(&mut self, path: &str) -> Answer<()> {
        let id = self.request(FXP_MKDIR, Wire::default().text(path).u32(0))?;
        self.expect_ok(id)
    }

    fn remove(&mut self, path: &str) -> Answer<()> {
        let id = self.request(FXP_REMOVE, Wire::default().text(path))?;
        self.expect_ok(id)
    }

    /// Moves `from` over `to`. The OpenSSH extension replaces atomically; plain v3 refuses
    /// an existing target, so there the target goes first.
    fn rename_replacing(&mut self, from: &str, to: &str) -> Answer<()> {
        if self.extensions.iter().any(|name| name == POSIX_RENAME) {
            let id = self.request(FXP_EXTENDED, Wire::default().text(POSIX_RENAME).text(from).text(to))?;
            return self.expect_ok(id);
        }
        let _ = self.remove(to);
        let id = self.request(FXP_RENAME, Wire::default().text(from).text(to))?;
        self.expect_ok(id)
    }

    fn close(&mut self, handle: &[u8]) -> Answer<()> {
        let id = self.request(FXP_CLOSE, Wire::default().bytes(handle))?;
        self.expect_ok(id)
    }

    fn list(&mut self, path: &str) -> Answer<Vec<Entry>> {
        let id = self.request(FXP_OPENDIR, Wire::default().text(path))?;
        let handle = self.handle_of(id)?;
        let listed = self.read_dir(&handle);
        let _ = self.close(&handle);
        listed
    }

    fn read_dir(&mut self, handle: &[u8]) -> Answer<Vec<Entry>> {
        let mut out = Vec::new();
        loop {
            let id = self.request(FXP_READDIR, Wire::default().bytes(handle))?;
            let (kind, body) = self.reply(id)?;
            match kind {
                FXP_STATUS => {
                    return match Self::status(&body)? {
                        FX_EOF => Ok(out),
                        code => Err(Failure::Status(code)),
                    };
                }
                FXP_NAME => {
                    let mut p = Parse::new(&body);
                    for _ in 0..p.u32()? {
                        let name = String::from_utf8_lossy(p.bytes()?).into_owned();
                        p.bytes()?; // longname, for humans
                        let attrs = attrs(&mut p)?;
                        if name != "." && name != ".." {
                            out.push(Entry { name, attrs });
                        }
                    }
                }
                _ => return Err(Fault::Io.into()),
            }
        }
    }

    /// Creates or truncates `path` and writes `data` to it.
    fn put(&mut self, path: &str, data: &[u8]) -> Answer<()> {
        let open = Wire::default().text(path).u32(FXF_WRITE | FXF_CREAT | FXF_TRUNC).u32(0);
        let id = self.request(FXP_OPEN, open)?;
        let handle = self.handle_of(id)?;
        let written = self.write_all(&handle, data);
        // A close that fails after a failed write would hide the write's error.
        let closed = self.close(&handle);
        written?;
        closed
    }

    fn write_all(&mut self, handle: &[u8], data: &[u8]) -> Answer<()> {
        let mut pending = std::collections::VecDeque::new();
        let mut offset = 0usize;
        while offset < data.len() || !pending.is_empty() {
            while offset < data.len() && pending.len() < WINDOW {
                let n = CHUNK.min(data.len() - offset);
                let body = Wire::default().bytes(handle).u64(offset as u64).bytes(&data[offset..offset + n]);
                pending.push_back(self.request(FXP_WRITE, body)?);
                offset += n;
            }
            if let Some(id) = pending.pop_front() {
                self.expect_ok(id)?;
            }
        }
        Ok(())
    }

    fn get(&mut self, path: &str) -> Answer<Vec<u8>> {
        let id = self.request(FXP_OPEN, Wire::default().text(path).u32(FXF_READ).u32(0))?;
        let handle = self.handle_of(id)?;
        let read = self.read_all(&handle);
        let _ = self.close(&handle);
        read
    }

    fn read_all(&mut self, handle: &[u8]) -> Answer<Vec<u8>> {
        let mut out = Vec::new();
        let mut offset = 0u64;
        let mut eof = false;
        while !eof {
            // A window of reads at consecutive offsets. A server may answer any of them
            // short, so after a short one the rest of the window is thrown away and the
            // next window starts where it ended; at the real end that costs one extra
            // request, which says EOF.
            let mut window = Vec::with_capacity(WINDOW);
            for k in 0..WINDOW {
                let at = offset + (k * CHUNK) as u64;
                window.push((at, self.request(FXP_READ, Wire::default().bytes(handle).u64(at).u32(CHUNK as u32))?));
            }
            let mut stop = false;
            for (at, id) in window {
                let (kind, body) = self.reply(id)?;
                if stop {
                    continue;
                }
                match kind {
                    FXP_DATA => {
                        let chunk = Parse::new(&body).bytes()?;
                        out.extend_from_slice(chunk);
                        offset = at + chunk.len() as u64;
                        if chunk.len() < CHUNK {
                            stop = true;
                        }
                    }
                    FXP_STATUS => match Self::status(&body)? {
                        FX_EOF => {
                            eof = true;
                            stop = true;
                        }
                        code => return Err(Failure::Status(code)),
                    },
                    _ => return Err(Fault::Io.into()),
                }
            }
        }
        Ok(out)
    }
}

// ---- The destination ----

/// A folder on an SFTP server.
pub struct Sftp {
    policy: NetPolicy,
    config: SftpConfig,
    on_host_key: Box<dyn Fn(String) + Send + Sync>,
}

impl std::fmt::Debug for Sftp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sftp").field("config", &self.config).finish_non_exhaustive()
    }
}

impl Sftp {
    /// A destination. `on_host_key` is called with `<type> <base64>` when a connection saw a
    /// key and none was stored; keep it in [`SftpConfig::host_key`].
    pub fn new(policy: NetPolicy, config: SftpConfig, on_host_key: Box<dyn Fn(String) + Send + Sync>) -> Self {
        Self { policy, config, on_host_key }
    }

    fn folder(&self) -> &str {
        self.config.path.trim().trim_end_matches('/')
    }

    fn path_of(&self, name: &str) -> Result<String> {
        // A name read from elsewhere must not walk out of the folder.
        if name.is_empty() || name.contains(['/', '\0']) || name == "." || name == ".." {
            return Err(Fault::NotConfigured);
        }
        Ok(match self.folder() {
            "" => name.to_owned(),
            folder => format!("{folder}/{name}"),
        })
    }

    /// Connects, checks the host key, signs in, opens the SFTP subsystem, runs `block`, and
    /// turns every failure into a [`Fault`].
    fn session<T>(&self, block: impl FnOnce(&mut SftpClient<&mut Session>) -> Answer<T>) -> Result<T> {
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        let login = Login {
            host: &self.config.host,
            port: self.config.port,
            user: &self.config.user,
            password: &self.config.password,
            private_key: &self.config.private_key,
            key_passphrase: &self.config.key_passphrase,
            host_key: &self.config.host_key,
            legacy_algorithms: self.config.legacy_algorithms,
        };
        let mut session = Session::connect(self.policy, &login)?;
        if self.config.host_key.trim().is_empty() {
            (self.on_host_key)(session.host_key());
        }
        let result = match session.open_subsystem("sftp") {
            Ok(()) => {
                let mut client = SftpClient::new(&mut session);
                client.init().and_then(|()| block(&mut client))
            }
            Err(fault) => Err(fault.into()),
        };
        session.close();
        result.map_err(fault_of)
    }
}

impl Sink for Sftp {
    async fn readiness(&self) -> Result<()> {
        let folder = self.folder().to_owned();
        self.session(|sftp| {
            // Makes the folder when it is missing, a level at a time.
            if !folder.is_empty() && sftp.stat(&folder)?.is_none() {
                let mut at = if folder.starts_with('/') { String::new() } else { ".".to_owned() };
                for part in folder.split('/').filter(|part| !part.is_empty()) {
                    at = if at == "." { part.to_owned() } else { format!("{at}/{part}") };
                    if sftp.stat(&at)?.is_none() {
                        sftp.mkdir(&at)?;
                    }
                }
            }
            Ok(())
        })
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let target = self.path_of(name)?;
        let part = self.path_of(&format!("{name}{PART}"))?;
        self.session(|sftp| {
            let done = sftp.put(&part, bytes).and_then(|()| sftp.rename_replacing(&part, &target));
            if done.is_err() {
                let _ = sftp.remove(&part);
            }
            done
        })?;
        Ok(Stored { id: name.to_owned(), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let folder = match self.folder() {
            "" => ".".to_owned(),
            folder => folder.to_owned(),
        };
        let mut out: Vec<Stored> = self
            .session(|sftp| sftp.list(&folder))?
            .into_iter()
            .filter(|entry| !entry.attrs.directory && !entry.name.ends_with(PART))
            .map(|entry| Stored { id: entry.name.clone(), name: entry.name, size: entry.attrs.size.unwrap_or(0) })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let path = self.path_of(&file.id)?;
        self.session(|sftp| sftp.get(&path))
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let path = self.path_of(&file.id)?;
        self.session(|sftp| match sftp.remove(&path) {
            Err(Failure::Status(FX_NO_SUCH_FILE)) => Ok(()),
            other => other,
        })
    }
}

#[cfg(test)]
mod tests;
