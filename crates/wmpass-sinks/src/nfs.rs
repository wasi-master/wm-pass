//! NFS version 3 over TCP, with the mount and portmap services it needs.
//!
//! XDR (RFC 4506) and ONC RPC version 2 over TCP (RFC 5531) carry NFS 3 (RFC 1813), its
//! MOUNT protocol and the portmapper. Everything is big-endian and padded to four bytes.
//! Hand-written: what NFS needs of them is a few hundred lines.
//!
//! A session is three short connections: the portmapper (port 111) to find the other two,
//! MOUNT to turn the export path into a file handle, and NFS itself. A server with fixed
//! ports can skip the first. A file goes to a `.part` name and is renamed over the final
//! one, which NFS does atomically.
//!
//! NFS without Kerberos has no sign-in and no encryption. The server trusts the user and
//! group ids the client says it is ([`NfsConfig::uid`]), and the file crosses the network
//! as it is; what is stored here is already encrypted.
//!
//! Many servers want the client's port below 1024 unless the export allows otherwise
//! (`insecure` on Linux, "Allow connections from non-privileged ports" on a Synology), and
//! an app cannot open one. Such a server answers the MOUNT call with an auth error, which
//! is reported as [`Fault::PermissionLost`].

use std::io::{BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use wmpass_net::NetPolicy;

use crate::{Fault, Result, Sink, Stored};

const PART: &str = ".part";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const PORTMAP_PORT: u16 = 111;
const NFS_DEFAULT_PORT: u16 = 2049;
const DEFAULT_ID: u32 = 1000;

const AUTH_NONE: u32 = 0;
const AUTH_SYS: u32 = 1;
const RPC_VERSION: u32 = 2;
const MSG_CALL: u32 = 0;
const MSG_REPLY: u32 = 1;
const MSG_ACCEPTED: u32 = 0;
const ACCEPT_SUCCESS: u32 = 0;
const PROG_UNAVAIL: u32 = 1;
const PROG_MISMATCH: u32 = 2;
const PROC_UNAVAIL: u32 = 3;
const REJECT_AUTH_ERROR: u32 = 1;
const LAST_FRAGMENT: u32 = 0x8000_0000;
const MAX_RECORD: usize = 64 * 1024 * 1024;

const PORTMAP_PROGRAM: u32 = 100_000;
const PORTMAP_VERSION: u32 = 2;
const PORTMAP_GETPORT: u32 = 3;
const IPPROTO_TCP: u32 = 6;

const MOUNT_PROGRAM: u32 = 100_005;
const MOUNT_VERSION: u32 = 3;
const MOUNT_MNT: u32 = 1;
const MOUNT_UMNT: u32 = 3;

const NFS_PROGRAM: u32 = 100_003;
const NFS_VERSION: u32 = 3;
const PROC_LOOKUP: u32 = 3;
const PROC_READ: u32 = 6;
const PROC_WRITE: u32 = 7;
const PROC_CREATE: u32 = 8;
const PROC_MKDIR: u32 = 9;
const PROC_REMOVE: u32 = 12;
const PROC_RENAME: u32 = 14;
const PROC_READDIRPLUS: u32 = 17;
const PROC_FSINFO: u32 = 19;

const NFS_OK: u32 = 0;
const NFS3ERR_PERM: u32 = 1;
const NFS3ERR_NOENT: u32 = 2;
const NFS3ERR_ACCES: u32 = 13;
const NFS3ERR_EXIST: u32 = 17;
const NFS3ERR_NOTDIR: u32 = 20;
const NFS3ERR_FBIG: u32 = 27;
const NFS3ERR_NOSPC: u32 = 28;
const NFS3ERR_ROFS: u32 = 30;
const NFS3ERR_DQUOT: u32 = 69;
const NFS3ERR_STALE: u32 = 70;

const CREATE_UNCHECKED: u32 = 0;
const FILE_SYNC: u32 = 2;
const TYPE_DIR: u32 = 2;
const FILE_MODE: u32 = 0o644;
const DIR_MODE: u32 = 0o755;

const VERIFIER_BYTES: usize = 8;
const DIR_COUNT: u32 = 8192;
const DIR_MAX: u32 = 32768;
const DEFAULT_CHUNK: usize = 32 * 1024;
const MIN_CHUNK: u64 = 4 * 1024;
const MAX_CHUNK: u64 = 1024 * 1024;

/// fattr3 after its type: mode, nlink, uid, gid (4 x 4); then size.
const FATTR_AFTER_TYPE_TO_SIZE: usize = 16;
/// After size: used (8), rdev (8), fsid (8), fileid (8), atime (8); then mtime.
const FATTR_SIZE_TO_MTIME: usize = 40;
/// mtime and ctime.
const FATTR_TIMES: usize = 16;
/// fattr3 is 84 bytes: five 4-byte fields and eight 8-byte ones.
const FATTR_BYTES: usize = 84;
/// wcc_attr: size (8), mtime (8), ctime (8).
const WCC_ATTR_BYTES: usize = 24;

/// An NFS export, version 3.
#[derive(Clone)]
pub struct NfsConfig {
    /// Server name or address.
    pub host: String,
    /// The exported directory, as the server names it: `/srv/nfs/backups`.
    pub export: String,
    /// Folder inside the export. Empty is the export's top.
    pub path: String,
    /// The user id files are written as. NFS without Kerberos takes the client's word for
    /// it, so it decides whose files these are on the server.
    pub uid: u32,
    /// The group id, likewise.
    pub gid: u32,
    /// 0 asks the server's portmapper; NFS itself is nearly always 2049.
    pub port: u16,
    /// 0 asks the portmapper for the mount service's port.
    pub mount_port: u16,
}

impl Default for NfsConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            export: String::new(),
            path: String::new(),
            uid: DEFAULT_ID,
            gid: DEFAULT_ID,
            port: 0,
            mount_port: 0,
        }
    }
}

impl std::fmt::Debug for NfsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NfsConfig")
            .field("configured", &self.configured())
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .finish_non_exhaustive()
    }
}

impl NfsConfig {
    /// Whether there is enough to try a connection.
    pub fn configured(&self) -> bool {
        !self.host.trim().is_empty() && !self.export.trim().is_empty()
    }
}

// ---- XDR ----

#[derive(Default)]
struct Xdr(Vec<u8>);

fn pad(length: usize) -> usize {
    (4 - length % 4) % 4
}

impl Xdr {
    fn int(mut self, value: u32) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn long(mut self, value: u64) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn bool(self, value: bool) -> Self {
        self.int(u32::from(value))
    }

    /// Fixed-length opaque data: the bytes and their padding, no length.
    fn fixed(mut self, bytes: &[u8]) -> Self {
        self.0.extend_from_slice(bytes);
        self.0.resize(self.0.len() + pad(bytes.len()), 0);
        self
    }

    /// Variable-length opaque data: the length, then [`Self::fixed`].
    fn opaque(self, bytes: &[u8]) -> Self {
        self.int(bytes.len() as u32).fixed(bytes)
    }

    fn string(self, value: &str) -> Self {
        self.opaque(value.as_bytes())
    }
}

struct XdrReader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> XdrReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, at: 0 }
    }

    fn skip(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(Fault::Io)?;
        let out = self.buf.get(self.at..end).ok_or(Fault::Io)?;
        self.at = end;
        Ok(out)
    }

    fn int(&mut self) -> Result<u32> {
        let b = self.skip(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn long(&mut self) -> Result<u64> {
        Ok((u64::from(self.int()?) << 32) | u64::from(self.int()?))
    }

    fn bool(&mut self) -> Result<bool> {
        Ok(self.int()? != 0)
    }

    fn fixed(&mut self, length: usize) -> Result<&'a [u8]> {
        let out = self.skip(length)?;
        self.skip(pad(length))?;
        Ok(out)
    }

    fn opaque(&mut self) -> Result<&'a [u8]> {
        let length = self.int()? as usize;
        self.fixed(length)
    }

    fn string(&mut self) -> Result<String> {
        Ok(String::from_utf8_lossy(self.opaque()?).into_owned())
    }
}

// ---- ONC RPC ----

/// Why an NFS session failed, before it is told to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// An NFS or MOUNT status other than OK.
    Status(u32),
    /// The server refused the call itself: the credentials, or the client's port.
    Denied,
    /// The program, version or procedure is not there.
    Unavailable,
    Broken(Fault),
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Broken(fault)
    }
}

type Answer<T> = std::result::Result<T, Failure>;

/// What an NFS failure means to the user. A refused MOUNT is nearly always the export
/// not allowing this client, or its port.
fn fault_of(failure: Failure) -> Fault {
    match failure {
        Failure::Denied => Fault::PermissionLost,
        Failure::Unavailable => Fault::TargetMissing,
        Failure::Status(NFS3ERR_PERM | NFS3ERR_ACCES | NFS3ERR_ROFS) => Fault::PermissionLost,
        Failure::Status(NFS3ERR_NOENT | NFS3ERR_NOTDIR | NFS3ERR_STALE) => Fault::TargetMissing,
        Failure::Status(NFS3ERR_NOSPC | NFS3ERR_DQUOT | NFS3ERR_FBIG) => Fault::OutOfSpace,
        Failure::Status(_) => Fault::Io,
        Failure::Broken(fault) => fault,
    }
}

/// AUTH_SYS: the client's word for who it is. `uid` and `gid` are what the server checks
/// file permissions against; nothing proves them.
fn auth_sys(uid: u32, gid: u32) -> Vec<u8> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    let body = Xdr::default().int(stamp).string("wmpass").int(uid).int(gid).int(1).int(gid);
    Xdr::default().int(AUTH_SYS).opaque(&body.0).0
}

/// No credentials, for the portmapper.
fn auth_none() -> Vec<u8> {
    Xdr::default().int(AUTH_NONE).int(0).0
}

/// One TCP connection to one RPC program. Calls are sequential: send one record, read
/// records until the reply with the same transaction id.
struct Rpc {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    xid: u32,
}

impl Rpc {
    fn connect(host: &str, port: u16) -> Result<Self> {
        let host = host.trim().trim_start_matches('[').trim_end_matches(']');
        for address in (host, port).to_socket_addrs().map_err(|_| Fault::Io)? {
            if let Ok(stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                stream.set_read_timeout(Some(READ_TIMEOUT)).map_err(|_| Fault::Io)?;
                stream.set_write_timeout(Some(READ_TIMEOUT)).map_err(|_| Fault::Io)?;
                stream.set_nodelay(true).map_err(|_| Fault::Io)?;
                let reader = BufReader::new(stream.try_clone().map_err(|_| Fault::Io)?);
                let mut seed = [0u8; 4];
                getrandom::fill(&mut seed).map_err(|_| Fault::Io)?;
                return Ok(Self { reader, writer: stream, xid: u32::from_be_bytes(seed) });
            }
        }
        Err(Fault::Io)
    }

    fn call(&mut self, program: u32, version: u32, procedure: u32, credential: &[u8], args: &[u8]) -> Answer<Vec<u8>> {
        self.xid = self.xid.wrapping_add(1);
        let id = self.xid;
        let header = Xdr::default()
            .int(id)
            .int(MSG_CALL)
            .int(RPC_VERSION)
            .int(program)
            .int(version)
            .int(procedure)
            .fixed(credential)
            // Verifier: AUTH_NONE, no body.
            .int(AUTH_NONE)
            .int(0);
        let length = header.0.len() + args.len();
        // Record marking: one fragment, the last, so the top bit is set.
        let mut record = Vec::with_capacity(4 + length);
        record.extend_from_slice(&(LAST_FRAGMENT | length as u32).to_be_bytes());
        record.extend_from_slice(&header.0);
        record.extend_from_slice(args);
        self.writer.write_all(&record).map_err(|_| Fault::Io)?;
        loop {
            let record = self.read_record()?;
            let mut reply = XdrReader::new(&record);
            if reply.int()? != id {
                continue;
            }
            if reply.int()? != MSG_REPLY {
                return Err(Fault::Io.into());
            }
            if reply.int()? != MSG_ACCEPTED {
                return Err(if reply.int()? == REJECT_AUTH_ERROR { Failure::Denied } else { Fault::Io.into() });
            }
            reply.int()?; // verifier flavour
            reply.opaque()?; // verifier body
            return match reply.int()? {
                ACCEPT_SUCCESS => Ok(record[reply.at..].to_vec()),
                PROG_UNAVAIL | PROG_MISMATCH | PROC_UNAVAIL => Err(Failure::Unavailable),
                _ => Err(Fault::Io.into()),
            };
        }
    }

    fn read_record(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let mut mark = [0u8; 4];
            self.reader.read_exact(&mut mark).map_err(|_| Fault::Io)?;
            let mark = u32::from_be_bytes(mark);
            let length = (mark & !LAST_FRAGMENT) as usize;
            if out.len() + length > MAX_RECORD {
                return Err(Fault::Io);
            }
            let start = out.len();
            out.resize(start + length, 0);
            self.reader.read_exact(&mut out[start..]).map_err(|_| Fault::Io)?;
            if mark & LAST_FRAGMENT != 0 {
                return Ok(out);
            }
        }
    }
}

// ---- NFS 3 ----

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    directory: bool,
    size: Option<u64>,
}

fn skip_post_op_attr(r: &mut XdrReader<'_>) -> Result<()> {
    if r.bool()? {
        r.skip(FATTR_BYTES)?;
    }
    Ok(())
}

fn skip_wcc(r: &mut XdrReader<'_>) -> Result<()> {
    if r.bool()? {
        r.skip(WCC_ATTR_BYTES)?;
    }
    skip_post_op_attr(r)
}

/// sattr3: `mode`, and a size of `size` when given; owner and times left to the server.
fn sattr(w: Xdr, mode: u32, size: Option<u64>) -> Xdr {
    let w = w.bool(true).int(mode).bool(false).bool(false);
    let w = match size {
        Some(size) => w.bool(true).long(size),
        None => w.bool(false),
    };
    // atime and mtime: don't change.
    w.int(0).int(0)
}

fn pick(preferred: u64, max: u64) -> usize {
    let value = if preferred > 0 { preferred } else { max };
    value.clamp(MIN_CHUNK, MAX_CHUNK) as usize
}

struct Nfs3 {
    rpc: Rpc,
    credential: Vec<u8>,
    root: Vec<u8>,
    read_chunk: usize,
    write_chunk: usize,
}

impl Nfs3 {
    fn call(&mut self, procedure: u32, args: Xdr) -> Answer<Vec<u8>> {
        self.rpc.call(NFS_PROGRAM, NFS_VERSION, procedure, &self.credential, &args.0)
    }

    /// Reads FSINFO for the transfer sizes the server prefers. Best effort.
    fn fsinfo(&mut self) {
        let Ok(reply) = self.call(PROC_FSINFO, Xdr::default().opaque(&self.root.clone())) else { return };
        let mut r = XdrReader::new(&reply);
        let sizes = (|| -> Result<Option<(u64, u64, u64, u64)>> {
            let status = r.int()?;
            skip_post_op_attr(&mut r)?;
            if status != NFS_OK {
                return Ok(None);
            }
            let rtmax = u64::from(r.int()?);
            let rtpref = u64::from(r.int()?);
            r.int()?; // rtmult
            let wtmax = u64::from(r.int()?);
            let wtpref = u64::from(r.int()?);
            Ok(Some((rtmax, rtpref, wtmax, wtpref)))
        })();
        if let Ok(Some((rtmax, rtpref, wtmax, wtpref))) = sizes {
            self.read_chunk = pick(rtpref, rtmax);
            self.write_chunk = pick(wtpref, wtmax);
        }
    }

    /// The handle of `name` in `dir`, or `None` when there is nothing by that name.
    fn lookup(&mut self, dir: &[u8], name: &str) -> Answer<Option<Vec<u8>>> {
        let reply = self.call(PROC_LOOKUP, Xdr::default().opaque(dir).string(name))?;
        let mut r = XdrReader::new(&reply);
        match r.int()? {
            NFS_OK => Ok(Some(r.opaque()?.to_vec())),
            NFS3ERR_NOENT => Ok(None),
            status => Err(Failure::Status(status)),
        }
    }

    /// CREATE and MKDIR may answer without the handle (post_op_fh3 with none).
    fn handle_or_lookup(&mut self, r: &mut XdrReader<'_>, dir: &[u8], name: &str) -> Answer<Vec<u8>> {
        if r.bool()? {
            return Ok(r.opaque()?.to_vec());
        }
        self.lookup(dir, name)?.ok_or(Failure::Status(NFS3ERR_NOENT))
    }

    /// Makes `name` in `dir`, or finds it when it is already there.
    fn mkdir(&mut self, dir: &[u8], name: &str) -> Answer<Vec<u8>> {
        if let Some(found) = self.lookup(dir, name)? {
            return Ok(found);
        }
        let reply = self.call(PROC_MKDIR, sattr(Xdr::default().opaque(dir).string(name), DIR_MODE, None))?;
        let mut r = XdrReader::new(&reply);
        match r.int()? {
            NFS_OK => self.handle_or_lookup(&mut r, dir, name),
            NFS3ERR_EXIST => self.lookup(dir, name)?.ok_or(Failure::Status(NFS3ERR_EXIST)),
            status => Err(Failure::Status(status)),
        }
    }

    /// Makes `name` in `dir` empty, replacing what is there (UNCHECKED, size 0).
    fn create(&mut self, dir: &[u8], name: &str) -> Answer<Vec<u8>> {
        let args = sattr(Xdr::default().opaque(dir).string(name).int(CREATE_UNCHECKED), FILE_MODE, Some(0));
        let reply = self.call(PROC_CREATE, args)?;
        let mut r = XdrReader::new(&reply);
        match r.int()? {
            NFS_OK => self.handle_or_lookup(&mut r, dir, name),
            status => Err(Failure::Status(status)),
        }
    }

    /// Writes `bytes` from the start of `file`, each piece committed to disk before the next.
    fn write(&mut self, file: &[u8], bytes: &[u8]) -> Answer<()> {
        let mut offset = 0usize;
        while offset < bytes.len() {
            let count = self.write_chunk.min(bytes.len() - offset);
            let args = Xdr::default()
                .opaque(file)
                .long(offset as u64)
                .int(count as u32)
                .int(FILE_SYNC)
                .opaque(&bytes[offset..offset + count]);
            let reply = self.call(PROC_WRITE, args)?;
            let mut r = XdrReader::new(&reply);
            let status = r.int()?;
            skip_wcc(&mut r)?;
            if status != NFS_OK {
                return Err(Failure::Status(status));
            }
            let written = r.int()? as usize;
            if written == 0 || written > count {
                return Err(Fault::Io.into());
            }
            offset += written;
        }
        Ok(())
    }

    fn read(&mut self, file: &[u8]) -> Answer<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let args = Xdr::default().opaque(file).long(out.len() as u64).int(self.read_chunk as u32);
            let reply = self.call(PROC_READ, args)?;
            let mut r = XdrReader::new(&reply);
            let status = r.int()?;
            skip_post_op_attr(&mut r)?;
            if status != NFS_OK {
                return Err(Failure::Status(status));
            }
            r.int()?; // count
            let eof = r.bool()?;
            let data = r.opaque()?;
            out.extend_from_slice(data);
            if eof || data.is_empty() {
                return Ok(out);
            }
        }
    }

    /// Removes `name` from `dir`; already gone is fine.
    fn remove(&mut self, dir: &[u8], name: &str) -> Answer<()> {
        let reply = self.call(PROC_REMOVE, Xdr::default().opaque(dir).string(name))?;
        match XdrReader::new(&reply).int()? {
            NFS_OK | NFS3ERR_NOENT => Ok(()),
            status => Err(Failure::Status(status)),
        }
    }

    /// Renames within `dir`, replacing `to` when it exists, which NFS does atomically.
    fn rename(&mut self, dir: &[u8], from: &str, to: &str) -> Answer<()> {
        let reply = self.call(PROC_RENAME, Xdr::default().opaque(dir).string(from).opaque(dir).string(to))?;
        match XdrReader::new(&reply).int()? {
            NFS_OK => Ok(()),
            status => Err(Failure::Status(status)),
        }
    }

    /// Everything in `dir`, through READDIRPLUS so sizes come with the names.
    fn list(&mut self, dir: &[u8]) -> Answer<Vec<Entry>> {
        let mut out = Vec::new();
        let mut cookie = 0u64;
        let mut verifier = [0u8; VERIFIER_BYTES];
        loop {
            let args =
                Xdr::default().opaque(dir).long(cookie).fixed(&verifier).int(DIR_COUNT).int(DIR_MAX);
            let reply = self.call(PROC_READDIRPLUS, args)?;
            let mut r = XdrReader::new(&reply);
            let status = r.int()?;
            if status != NFS_OK {
                return Err(Failure::Status(status));
            }
            skip_post_op_attr(&mut r)?;
            verifier.copy_from_slice(r.fixed(VERIFIER_BYTES)?);
            let mut any = false;
            while r.bool()? {
                any = true;
                r.long()?; // fileid
                let name = r.string()?;
                cookie = r.long()?;
                let mut directory = false;
                let mut size = None;
                if r.bool()? {
                    directory = r.int()? == TYPE_DIR;
                    r.skip(FATTR_AFTER_TYPE_TO_SIZE)?;
                    size = Some(r.long()?);
                    r.skip(FATTR_SIZE_TO_MTIME + FATTR_TIMES)?;
                }
                if r.bool()? {
                    r.opaque()?; // the entry's handle, unused
                }
                if name != "." && name != ".." {
                    out.push(Entry { name, directory, size });
                }
            }
            if r.bool()? || !any {
                return Ok(out);
            }
        }
    }
}

// ---- The destination ----

/// A folder in an NFS export.
#[derive(Debug)]
pub struct Nfs {
    policy: NetPolicy,
    config: NfsConfig,
    /// 111 everywhere but in tests.
    portmap_port: u16,
}

impl Nfs {
    /// A destination.
    pub fn new(policy: NetPolicy, config: NfsConfig) -> Self {
        Self { policy, config, portmap_port: PORTMAP_PORT }
    }

    fn parts(&self) -> Vec<&str> {
        self.config.path.split(['/', '\\']).map(str::trim).filter(|part| !part.is_empty()).collect()
    }

    fn checked(name: &str) -> Result<&str> {
        // A name read from elsewhere must not walk out of the folder.
        if name.is_empty() || name.contains(['/', '\0']) || name == "." || name == ".." {
            return Err(Fault::NotConfigured);
        }
        Ok(name)
    }

    /// The TCP port the portmapper on the host has for `program`, or 0 for none.
    fn port_of(&self, host: &str, program: u32, version: u32) -> Answer<u16> {
        let mut portmap = Rpc::connect(host, self.portmap_port)?;
        let args = Xdr::default().int(program).int(version).int(IPPROTO_TCP).int(0);
        let reply = portmap.call(PORTMAP_PROGRAM, PORTMAP_VERSION, PORTMAP_GETPORT, &auth_none(), &args.0)?;
        Ok(u16::try_from(XdrReader::new(&reply).int()?).unwrap_or(0))
    }

    fn mount_port(&self, host: &str) -> Answer<u16> {
        match self.config.mount_port {
            0 => match self.port_of(host, MOUNT_PROGRAM, MOUNT_VERSION)? {
                0 => Err(Failure::Unavailable),
                port => Ok(port),
            },
            port => Ok(port),
        }
    }

    /// Mounts the export and opens the NFS connection. NFS falls back to 2049 when the
    /// portmapper cannot be reached.
    fn open(&self, host: &str, export: &str) -> Answer<Nfs3> {
        let credential = auth_sys(self.config.uid, self.config.gid);
        let mut mount = Rpc::connect(host, self.mount_port(host)?)?;
        let reply = mount.call(MOUNT_PROGRAM, MOUNT_VERSION, MOUNT_MNT, &credential, &Xdr::default().string(export).0)?;
        let mut r = XdrReader::new(&reply);
        let status = r.int()?;
        if status != NFS_OK {
            return Err(Failure::Status(status));
        }
        let root = r.opaque()?.to_vec();
        drop(mount);
        let nfs_port = match self.config.port {
            0 => match self.port_of(host, NFS_PROGRAM, NFS_VERSION) {
                Ok(port) if port > 0 => port,
                _ => NFS_DEFAULT_PORT,
            },
            port => port,
        };
        let mut client = Nfs3 {
            rpc: Rpc::connect(host, nfs_port)?,
            credential,
            root,
            read_chunk: DEFAULT_CHUNK,
            write_chunk: DEFAULT_CHUNK,
        };
        client.fsinfo();
        Ok(client)
    }

    /// Tells the server the mount is done with. Best effort: servers keep no lasting state
    /// for it.
    fn unmount(&self, host: &str, export: &str) {
        let Ok(port) = self.mount_port(host) else { return };
        if let Ok(mut mount) = Rpc::connect(host, port) {
            let credential = auth_sys(self.config.uid, self.config.gid);
            let _ = mount.call(MOUNT_PROGRAM, MOUNT_VERSION, MOUNT_UMNT, &credential, &Xdr::default().string(export).0);
        }
    }

    /// Mounts, runs `block` with the backup folder's handle (made level by level where
    /// missing), unmounts, and maps every way that can fail onto a [`Fault`].
    fn session<T>(&self, block: impl FnOnce(&mut Nfs3, Vec<u8>) -> Answer<T>) -> Result<T> {
        wmpass_net::http::check_socket(self.policy).map_err(|_| Fault::Blocked)?;
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        let host = self.config.host.trim();
        let export = self.config.export.trim();
        let mut client = self.open(host, export).map_err(fault_of)?;
        let result = (|| {
            let mut at = client.root.clone();
            for part in self.parts() {
                at = client.mkdir(&at, part)?;
            }
            block(&mut client, at)
        })();
        drop(client);
        self.unmount(host, export);
        result.map_err(fault_of)
    }
}

impl Sink for Nfs {
    async fn readiness(&self) -> Result<()> {
        self.session(|_, _| Ok(()))
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let name = Self::checked(name)?;
        let part = format!("{name}{PART}");
        self.session(|nfs, dir| {
            let done = nfs
                .create(&dir, &part)
                .and_then(|file| nfs.write(&file, bytes))
                .and_then(|()| nfs.rename(&dir, &part, name));
            if done.is_err() {
                let _ = nfs.remove(&dir, &part);
            }
            done
        })?;
        Ok(Stored { id: name.to_owned(), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let mut out: Vec<Stored> = self
            .session(|nfs, dir| nfs.list(&dir))?
            .into_iter()
            .filter(|entry| !entry.directory && !entry.name.ends_with(PART))
            .map(|entry| Stored { id: entry.name.clone(), name: entry.name, size: entry.size.unwrap_or(0) })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let name = Self::checked(&file.id)?;
        self.session(|nfs, dir| match nfs.lookup(&dir, name)? {
            Some(handle) => nfs.read(&handle),
            None => Err(Fault::TargetMissing.into()),
        })
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let name = Self::checked(&file.id)?;
        self.session(|nfs, dir| nfs.remove(&dir, name))
    }
}

#[cfg(test)]
mod tests;
