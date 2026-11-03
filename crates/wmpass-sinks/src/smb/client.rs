//! An SMB 2 and 3 client over direct TCP, with just the commands a backup needs: create,
//! read, write, close, list, rename and delete, one at a time.
//!
//! Security, in the order it is set up:
//! - Dialects 2.0.2 to 3.1.1. On 3.1.1 the whole negotiation and sign-in are hashed into
//!   the keys (pre-authentication integrity), which is what stops a machine in the middle
//!   from talking both ends down to something weaker.
//! - NTLMv2 inside SPNEGO, with the MIC and mechListMIC Windows and Samba want.
//! - Every request after the sign-in is signed, and a signed reply is checked.
//! - Encryption with AES-128-GCM or AES-128-CCM when the caller asks for it or the server
//!   demands it. An encrypted message is not also signed; its tag is the integrity.
//!
//! One request in flight at a time, one credit each, and I/O in chunks of at most 64 KiB.
//! Slower than it could be, and a backup does not notice.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::bytes::{Le, LeWriter, Malformed, from_utf16, utf16};
use super::crypto::{self, Cipher};
use super::{ntlm, spnego};

/// Why a call failed. Carries no path, name or address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SmbError {
    /// A reply whose NT status was not the one the call wanted.
    Status(u32),
    /// The server cannot give the protection the caller demanded: encryption when it was
    /// asked for, or a reply in the clear on an encrypted session.
    Unsafe,
    /// No answer, a malformed one, or a signature that does not match.
    Io,
}

impl From<std::io::Error> for SmbError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

impl From<Malformed> for SmbError {
    fn from(_: Malformed) -> Self {
        Self::Io
    }
}

impl From<crypto::Refused> for SmbError {
    fn from(_: crypto::Refused) -> Self {
        Self::Io
    }
}

type Result<T> = std::result::Result<T, SmbError>;

pub(crate) const HEADER_BYTES: usize = 64;
pub(crate) const TRANSFORM_BYTES: usize = 52;
const CHUNK: u32 = 65_536;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const CREDITS_ASKED: u16 = 64;

pub(crate) const PROTOCOL_SMB2: &[u8; 4] = b"\xFESMB";
pub(crate) const PROTOCOL_TRANSFORM: &[u8; 4] = b"\xFDSMB";

pub(crate) const DIALECT_202: u16 = 0x0202;
pub(crate) const DIALECT_210: u16 = 0x0210;
pub(crate) const DIALECT_300: u16 = 0x0300;
pub(crate) const DIALECT_302: u16 = 0x0302;
pub(crate) const DIALECT_311: u16 = 0x0311;
const DIALECTS: [u16; 5] = [DIALECT_202, DIALECT_210, DIALECT_300, DIALECT_302, DIALECT_311];

pub(crate) const COMMAND_NEGOTIATE: u16 = 0;
pub(crate) const COMMAND_SESSION_SETUP: u16 = 1;
pub(crate) const COMMAND_LOGOFF: u16 = 2;
pub(crate) const COMMAND_TREE_CONNECT: u16 = 3;
pub(crate) const COMMAND_TREE_DISCONNECT: u16 = 4;
pub(crate) const COMMAND_CREATE: u16 = 5;
pub(crate) const COMMAND_CLOSE: u16 = 6;
pub(crate) const COMMAND_READ: u16 = 8;
pub(crate) const COMMAND_WRITE: u16 = 9;
pub(crate) const COMMAND_QUERY_DIRECTORY: u16 = 0x0E;
pub(crate) const COMMAND_SET_INFO: u16 = 0x11;

pub(crate) const FLAG_ASYNC: u32 = 0x2;
pub(crate) const FLAG_SIGNED: u32 = 0x8;
const SECURITY_SIGNING_ENABLED: u16 = 0x1;
pub(crate) const CAP_ENCRYPTION: u32 = 0x40;
const CONTEXT_PREAUTH: u16 = 1;
pub(crate) const CONTEXT_ENCRYPTION: u16 = 2;
const HASH_SHA512: u16 = 1;
pub(crate) const SESSION_GUEST: u16 = 0x1;
pub(crate) const SESSION_NULL: u16 = 0x2;
pub(crate) const SESSION_ENCRYPT_DATA: u16 = 0x4;
pub(crate) const SHARE_ENCRYPT_DATA: u32 = 0x8000;

const IMPERSONATION: u32 = 2;
pub(crate) const SHARE_ALL: u32 = 0x7;
const INFO_FILE: u8 = 1;
const FILE_RENAME_INFORMATION: u8 = 10;
const FILE_DIRECTORY_INFORMATION: u8 = 1;
const QUERY_RESTART: u8 = 1;
pub(crate) const ATTRIBUTE_DIRECTORY: u32 = 0x10;

pub(crate) const ACCESS_READ_DATA: u32 = 0x1;
pub(crate) const ACCESS_WRITE_DATA: u32 = 0x2;
pub(crate) const ACCESS_READ_ATTRIBUTES: u32 = 0x80;
pub(crate) const ACCESS_WRITE_ATTRIBUTES: u32 = 0x100;
pub(crate) const ACCESS_DELETE: u32 = 0x1_0000;
pub(crate) const ACCESS_SYNCHRONIZE: u32 = 0x10_0000;

pub(crate) const DISPOSITION_OPEN: u32 = 1;
pub(crate) const DISPOSITION_OPEN_IF: u32 = 3;
pub(crate) const DISPOSITION_OVERWRITE_IF: u32 = 5;

pub(crate) const OPTION_DIRECTORY: u32 = 0x1;
pub(crate) const OPTION_NON_DIRECTORY: u32 = 0x40;
pub(crate) const OPTION_DELETE_ON_CLOSE: u32 = 0x1000;

pub(crate) const STATUS_SUCCESS: u32 = 0;
pub(crate) const STATUS_PENDING: u32 = 0x103;
pub(crate) const STATUS_NO_MORE_FILES: u32 = 0x8000_0006;
pub(crate) const STATUS_MORE_PROCESSING: u32 = 0xC000_0016;
pub(crate) const STATUS_END_OF_FILE: u32 = 0xC000_0011;
pub(crate) const STATUS_ACCESS_DENIED: u32 = 0xC000_0022;
pub(crate) const STATUS_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
pub(crate) const STATUS_OBJECT_PATH_NOT_FOUND: u32 = 0xC000_003A;
pub(crate) const STATUS_LOGON_FAILURE: u32 = 0xC000_006D;
pub(crate) const STATUS_PASSWORD_EXPIRED: u32 = 0xC000_0071;
pub(crate) const STATUS_ACCOUNT_DISABLED: u32 = 0xC000_0072;
pub(crate) const STATUS_DISK_FULL: u32 = 0xC000_007F;
pub(crate) const STATUS_QUOTA_EXCEEDED: u32 = 0xC000_0044;
pub(crate) const STATUS_NOT_SUPPORTED: u32 = 0xC000_00BB;
pub(crate) const STATUS_BAD_NETWORK_NAME: u32 = 0xC000_00CC;
pub(crate) const STATUS_ACCOUNT_LOCKED_OUT: u32 = 0xC000_0234;

/// MS-SMB2 writes its KDF labels with the NUL included.
pub(crate) fn label(s: &str) -> Vec<u8> {
    [s.as_bytes(), &[0]].concat()
}

/// A signing key, and the encryption and decryption keys when the dialect has them.
pub(crate) type SessionKeys = ([u8; 16], Option<([u8; 16], [u8; 16])>);

/// Signing, encryption and decryption keys for a dialect, from the session key and, on
/// 3.1.1, the pre-authentication hash. Before 3.0 the session key signs and nothing
/// encrypts.
pub(crate) fn derive_keys(dialect: u16, session_key: &[u8; 16], preauth: &[u8]) -> SessionKeys {
    if dialect == DIALECT_311 {
        (
            crypto::kdf(session_key, &label("SMBSigningKey"), preauth),
            Some((
                crypto::kdf(session_key, &label("SMBC2SCipherKey"), preauth),
                crypto::kdf(session_key, &label("SMBS2CCipherKey"), preauth),
            )),
        )
    } else if dialect >= DIALECT_300 {
        (
            crypto::kdf(session_key, &label("SMB2AESCMAC"), &label("SmbSign")),
            Some((
                crypto::kdf(session_key, &label("SMB2AESCCM"), &label("ServerIn ")),
                crypto::kdf(session_key, &label("SMB2AESCCM"), &label("ServerOut")),
            )),
        )
    } else {
        (*session_key, None)
    }
}

/// The signature of a whole message, with its signature field taken as zeros.
pub(crate) fn signature(dialect: u16, key: &[u8; 16], message: &[u8]) -> [u8; 16] {
    let mut copy = message.to_vec();
    if let Some(field) = copy.get_mut(48..64) {
        field.fill(0);
    }
    if dialect >= DIALECT_300 {
        crypto::aes_cmac(key, &copy)
    } else {
        let mut out = [0u8; 16];
        out.copy_from_slice(&crypto::hmac_sha256(key, &[&copy])[..16]);
        out
    }
}

/// A transform header and the sealed message behind it. `nonce` is the whole 16-byte
/// field; the bytes past what the cipher uses are sent as zeros.
pub(crate) fn seal_frame(cipher: Cipher, key: &[u8; 16], nonce: &[u8; 16], session_id: u64, plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = *nonce;
    nonce[cipher.nonce_len()..].fill(0);
    let mut w = LeWriter::new();
    w.bytes(PROTOCOL_TRANSFORM).zeros(16).bytes(&nonce);
    w.u32(u32::try_from(plain.len()).map_err(|_| SmbError::Io)?).u16(0).u16(1).u64(session_id);
    let mut frame = w.into_vec();
    let mut body = plain.to_vec();
    let tag = crypto::seal(cipher, key, &nonce, &frame[20..TRANSFORM_BYTES], &mut body)?;
    frame[4..20].copy_from_slice(&tag);
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// The reverse of [`seal_frame`]: the SMB2 message inside, or an error when its tag does
/// not match.
pub(crate) fn open_frame(cipher: Cipher, key: &[u8; 16], frame: &[u8]) -> Result<Vec<u8>> {
    let tag = frame.slice_at(4, 16)?;
    let nonce = frame.slice_at(20, 16)?;
    let aad = frame.slice_at(20, TRANSFORM_BYTES - 20)?;
    let size = frame.u32_at(36)? as usize;
    let mut body = frame.get(TRANSFORM_BYTES..).ok_or(SmbError::Io)?.to_vec();
    if body.len() != size {
        return Err(SmbError::Io);
    }
    crypto::open(cipher, key, nonce, aad, &mut body, tag)?;
    Ok(body)
}

/// An open file or folder.
#[derive(Clone, Debug)]
pub(crate) struct Handle {
    id: [u8; 16],
    size: u64,
}

/// One entry of a folder listing.
#[derive(Clone, Debug)]
pub(crate) struct DirEntry {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) directory: bool,
}

/// A whole SMB2 message as it came, after unsealing.
struct Reply(Vec<u8>);

impl Reply {
    fn status(&self) -> u32 {
        self.0.u32_at(8).unwrap_or(u32::MAX)
    }

    fn expect(&self, wanted: u32) -> Result<()> {
        if self.status() == wanted { Ok(()) } else { Err(SmbError::Status(self.status())) }
    }

    fn security_buffer(&self) -> &[u8] {
        let (Ok(at), Ok(len)) = (self.0.u16_at(HEADER_BYTES + 4), self.0.u16_at(HEADER_BYTES + 6)) else { return &[] };
        self.0.slice_at(usize::from(at), usize::from(len)).unwrap_or(&[])
    }
}

/// One connection to one share.
pub(crate) struct Smb2Client {
    stream: TcpStream,
    host: String,
    message_id: u64,
    session_id: u64,
    tree_id: u32,
    dialect: u16,
    max_read: u32,
    max_write: u32,
    max_transact: u32,
    /// 3.1.1 only: the running SHA-512 over negotiate and session setup.
    preauth: [u8; 64],
    cipher: Option<Cipher>,
    signing_key: Option<[u8; 16]>,
    encrypt_key: Option<[u8; 16]>,
    decrypt_key: Option<[u8; 16]>,
    encrypting: bool,
    /// Signed in: from here every reply must be signed or sealed.
    established: bool,
}

impl std::fmt::Debug for Smb2Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Smb2Client").field("dialect", &self.dialect).field("encrypting", &self.encrypting).finish_non_exhaustive()
    }
}

impl Smb2Client {
    /// Connects to the first address `host` resolves to that answers.
    pub(crate) fn connect(host: &str, port: u16, connect_timeout: Duration, io_timeout: Duration) -> Result<Self> {
        let mut last = SmbError::Io;
        for address in (host, port).to_socket_addrs()? {
            match TcpStream::connect_timeout(&address, connect_timeout) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(io_timeout))?;
                    stream.set_write_timeout(Some(io_timeout))?;
                    stream.set_nodelay(true)?;
                    return Ok(Self::over(stream, host));
                }
                Err(error) => last = error.into(),
            }
        }
        Err(last)
    }

    fn over(stream: TcpStream, host: &str) -> Self {
        Self {
            stream,
            host: host.to_owned(),
            message_id: 0,
            session_id: 0,
            tree_id: 0,
            dialect: 0,
            max_read: CHUNK,
            max_write: CHUNK,
            max_transact: CHUNK,
            preauth: [0; 64],
            cipher: None,
            signing_key: None,
            encrypt_key: None,
            decrypt_key: None,
            encrypting: false,
            established: false,
        }
    }

    // ---- Connection set-up ----

    pub(crate) fn negotiate(&mut self) -> Result<()> {
        let mut body = LeWriter::new();
        body.u16(36).u16(DIALECTS.len() as u16).u16(SECURITY_SIGNING_ENABLED).u16(0);
        body.u32(CAP_ENCRYPTION);
        body.bytes(&crypto::random::<16>()?); // ClientGuid
        let context_offset_at = body.len();
        body.u32(0).u16(2).u16(0); // NegotiateContextOffset, Count, Reserved2
        for dialect in DIALECTS {
            body.u16(dialect);
        }
        // Contexts start 8-aligned from the start of the SMB2 header.
        while !(HEADER_BYTES + body.len()).is_multiple_of(8) {
            body.u8(0);
        }
        body.put_u32(context_offset_at, (HEADER_BYTES + body.len()) as u32);
        // Pre-authentication integrity: SHA-512 and a fresh salt.
        body.u16(CONTEXT_PREAUTH).u16(38).u32(0);
        body.u16(1).u16(32).u16(HASH_SHA512).bytes(&crypto::random::<32>()?);
        body.align(8);
        // Encryption: GCM first, CCM for servers that have nothing newer.
        body.u16(CONTEXT_ENCRYPTION).u16(6).u32(0);
        body.u16(2).u16(Cipher::Aes128Gcm.id()).u16(Cipher::Aes128Ccm.id());

        let request = self.message(COMMAND_NEGOTIATE, &body.into_vec(), 0);
        self.write_frame(&request)?;
        let reply = self.receive(request.u64_at(24)?)?;
        reply.expect(STATUS_SUCCESS)?;
        let b = &reply.0;
        self.dialect = b.u16_at(HEADER_BYTES + 4)?;
        let capabilities = b.u32_at(HEADER_BYTES + 24)?;
        self.max_transact = CHUNK.min(b.u32_at(HEADER_BYTES + 28)?);
        self.max_read = CHUNK.min(b.u32_at(HEADER_BYTES + 32)?);
        self.max_write = CHUNK.min(b.u32_at(HEADER_BYTES + 36)?);
        if !DIALECTS.contains(&self.dialect) || self.max_read == 0 || self.max_write == 0 {
            return Err(SmbError::Status(STATUS_NOT_SUPPORTED));
        }
        if self.dialect == DIALECT_311 {
            self.preauth = crypto::sha512(&[&self.preauth, &request]);
            self.preauth = crypto::sha512(&[&self.preauth, b]);
            self.cipher = negotiated_cipher(b);
        } else if self.dialect >= DIALECT_300 && capabilities & CAP_ENCRYPTION != 0 {
            self.cipher = Some(Cipher::Aes128Ccm);
        }
        Ok(())
    }

    /// NTLM in SPNEGO, two round trips. Afterwards every request is signed, and encrypted
    /// when `require_encryption` is set or the server says so.
    pub(crate) fn login(&mut self, user: &str, password: &str, domain: &str, require_encryption: bool) -> Result<()> {
        let mut ntlm = ntlm::Ntlm::new(user, password, domain);
        let mut session_hash = self.preauth;

        let first = self.session_setup_request(&spnego::init(&ntlm.negotiate()))?;
        self.write_frame(&first)?;
        let reply = self.receive(first.u64_at(24)?)?;
        if self.dialect == DIALECT_311 {
            session_hash = crypto::sha512(&[&session_hash, &first]);
            session_hash = crypto::sha512(&[&session_hash, &reply.0]);
        }
        if reply.status() != STATUS_MORE_PROCESSING {
            return Err(SmbError::Status(reply.status()));
        }
        self.session_id = reply.0.u64_at(40)?;
        let challenge = spnego::response_token(reply.security_buffer()).ok_or(SmbError::Status(STATUS_LOGON_FAILURE))?;

        let authenticate = ntlm.authenticate(challenge)?;
        let token = spnego::response(&authenticate, Some(&ntlm.sign(&spnego::mech_type_list())));
        let second = self.session_setup_request(&token)?;
        if self.dialect == DIALECT_311 {
            session_hash = crypto::sha512(&[&session_hash, &second]);
        }
        let (signing, ciphers) = derive_keys(self.dialect, &ntlm.exported_session_key, &session_hash);
        self.signing_key = Some(signing);
        if let Some((encrypt, decrypt)) = ciphers {
            self.encrypt_key = Some(encrypt);
            self.decrypt_key = Some(decrypt);
        }
        self.write_frame(&second)?;
        let reply = self.receive(second.u64_at(24)?)?;
        reply.expect(STATUS_SUCCESS)?;

        let flags = reply.0.u16_at(HEADER_BYTES + 2)?;
        if flags & (SESSION_GUEST | SESSION_NULL) != 0 {
            // Samba's "map to guest = bad user" answers a wrong password with a guest
            // session instead of a refusal. Backing up into whatever guests may write is
            // not what the user set up, and a guest has no key to sign with besides.
            return Err(SmbError::Status(STATUS_LOGON_FAILURE));
        }
        if require_encryption || flags & SESSION_ENCRYPT_DATA != 0 {
            self.start_encryption()?;
        }
        self.established = true;
        Ok(())
    }

    fn start_encryption(&mut self) -> Result<()> {
        if self.cipher.is_none() || self.encrypt_key.is_none() {
            return Err(SmbError::Unsafe);
        }
        self.encrypting = true;
        Ok(())
    }

    fn session_setup_request(&mut self, token: &[u8]) -> Result<Vec<u8>> {
        let mut body = LeWriter::new();
        body.u16(25).u8(0).u8(SECURITY_SIGNING_ENABLED as u8).u32(0).u32(0);
        body.u16((HEADER_BYTES + 24) as u16).u16(u16::try_from(token.len()).map_err(|_| SmbError::Io)?).u64(0);
        body.bytes(token);
        Ok(self.message(COMMAND_SESSION_SETUP, &body.into_vec(), 1))
    }

    pub(crate) fn tree_connect(&mut self, share: &str) -> Result<()> {
        let path = utf16(&format!("\\\\{}\\{share}", self.host));
        let mut body = LeWriter::new();
        body.u16(9).u16(0).u16((HEADER_BYTES + 8) as u16).u16(u16::try_from(path.len()).map_err(|_| SmbError::Io)?).bytes(&path);
        let reply = self.call(COMMAND_TREE_CONNECT, &body.into_vec())?;
        reply.expect(STATUS_SUCCESS)?;
        self.tree_id = reply.0.u32_at(36)?;
        let share_flags = reply.0.u32_at(HEADER_BYTES + 4)?;
        if share_flags & SHARE_ENCRYPT_DATA != 0 && !self.encrypting {
            self.start_encryption()?;
        }
        Ok(())
    }

    // ---- Files ----

    pub(crate) fn create(&mut self, path: &str, access: u32, disposition: u32, options: u32, share: u32) -> Result<Handle> {
        let name = utf16(path);
        let mut body = LeWriter::new();
        body.u16(57).u8(0).u8(0).u32(IMPERSONATION).u64(0).u64(0);
        body.u32(access).u32(0).u32(share).u32(disposition).u32(options);
        body.u16((HEADER_BYTES + 56) as u16).u16(u16::try_from(name.len()).map_err(|_| SmbError::Io)?).u32(0).u32(0);
        // The buffer has to hold at least one byte, even for the share root.
        body.bytes(if name.is_empty() { &[0] } else { &name });
        let reply = self.call(COMMAND_CREATE, &body.into_vec())?;
        reply.expect(STATUS_SUCCESS)?;
        let mut id = [0u8; 16];
        id.copy_from_slice(reply.0.slice_at(HEADER_BYTES + 64, 16)?);
        Ok(Handle { id, size: reply.0.u64_at(HEADER_BYTES + 48)? })
    }

    pub(crate) fn close(&mut self, handle: &Handle) -> Result<()> {
        let mut body = LeWriter::new();
        body.u16(24).u16(0).u32(0).bytes(&handle.id);
        self.call(COMMAND_CLOSE, &body.into_vec())?.expect(STATUS_SUCCESS)
    }

    pub(crate) fn write(&mut self, handle: &Handle, data: &[u8]) -> Result<()> {
        let mut offset = 0usize;
        while offset < data.len() {
            let n = (self.max_write as usize).min(data.len() - offset);
            let mut body = LeWriter::new();
            body.u16(49).u16((HEADER_BYTES + 48) as u16).u32(n as u32).u64(offset as u64).bytes(&handle.id);
            body.u32(0).u32(0).u16(0).u16(0).u32(0);
            body.bytes(&data[offset..offset + n]);
            let reply = self.call(COMMAND_WRITE, &body.into_vec())?;
            reply.expect(STATUS_SUCCESS)?;
            let written = reply.0.u32_at(HEADER_BYTES + 4)? as usize;
            if written == 0 || written > n {
                return Err(SmbError::Status(STATUS_DISK_FULL));
            }
            offset += written;
        }
        Ok(())
    }

    pub(crate) fn read_all(&mut self, handle: &Handle) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(usize::try_from(handle.size).unwrap_or(0).min(MAX_FRAME));
        loop {
            let mut body = LeWriter::new();
            body.u16(49).u8(0x50).u8(0).u32(self.max_read).u64(out.len() as u64).bytes(&handle.id);
            body.u32(0).u32(0).u32(0).u16(0).u16(0).u8(0);
            let reply = self.call(COMMAND_READ, &body.into_vec())?;
            if reply.status() == STATUS_END_OF_FILE {
                break;
            }
            reply.expect(STATUS_SUCCESS)?;
            let data_offset = usize::from(reply.0.u8_at(HEADER_BYTES + 2)?);
            let length = reply.0.u32_at(HEADER_BYTES + 4)? as usize;
            if length == 0 {
                break;
            }
            out.extend_from_slice(reply.0.slice_at(data_offset, length)?);
        }
        Ok(out)
    }

    pub(crate) fn list(&mut self, dir: &Handle) -> Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        let mut first = true;
        loop {
            let pattern = utf16("*");
            let mut body = LeWriter::new();
            body.u16(33).u8(FILE_DIRECTORY_INFORMATION).u8(if first { QUERY_RESTART } else { 0 }).u32(0).bytes(&dir.id);
            body.u16((HEADER_BYTES + 32) as u16).u16(pattern.len() as u16).u32(self.max_transact).bytes(&pattern);
            first = false;
            let reply = self.call(COMMAND_QUERY_DIRECTORY, &body.into_vec())?;
            if reply.status() == STATUS_NO_MORE_FILES {
                break;
            }
            reply.expect(STATUS_SUCCESS)?;
            let b = &reply.0;
            let mut at = usize::from(b.u16_at(HEADER_BYTES + 2)?);
            let end = at + b.u32_at(HEADER_BYTES + 4)? as usize;
            while at < end {
                let next = b.u32_at(at)? as usize;
                let name_length = b.u32_at(at + 60)? as usize;
                let name = from_utf16(b.slice_at(at + 64, name_length)?);
                if name != "." && name != ".." {
                    out.push(DirEntry {
                        name,
                        size: b.u64_at(at + 40)?,
                        directory: b.u32_at(at + 56)? & ATTRIBUTE_DIRECTORY != 0,
                    });
                }
                if next == 0 {
                    break;
                }
                at += next;
            }
        }
        Ok(out)
    }

    /// Renames the open `handle` to `target`, a path from the share root.
    pub(crate) fn rename(&mut self, handle: &Handle, target: &str, replace: bool) -> Result<()> {
        let name = utf16(target);
        let mut info = LeWriter::new();
        info.u8(u8::from(replace)).zeros(7).u64(0).u32(name.len() as u32).bytes(&name);
        let info = info.into_vec();
        let mut body = LeWriter::new();
        body.u16(33).u8(INFO_FILE).u8(FILE_RENAME_INFORMATION).u32(info.len() as u32).u16((HEADER_BYTES + 32) as u16).u16(0).u32(0);
        body.bytes(&handle.id).bytes(&info);
        self.call(COMMAND_SET_INFO, &body.into_vec())?.expect(STATUS_SUCCESS)
    }

    // ---- Messages ----

    fn call(&mut self, command: u16, body: &[u8]) -> Result<Reply> {
        let request = self.message(command, body, 1);
        let id = request.u64_at(24)?;
        let protected = self.protect(request)?;
        self.write_frame(&protected)?;
        self.receive(id)
    }

    fn message(&mut self, command: u16, body: &[u8], credit_charge: u16) -> Vec<u8> {
        let mut h = LeWriter::new();
        h.bytes(PROTOCOL_SMB2).u16(HEADER_BYTES as u16);
        h.u16(if self.dialect == DIALECT_202 || self.dialect == 0 { 0 } else { credit_charge });
        h.u32(0).u16(command).u16(CREDITS_ASKED).u32(0).u32(0);
        h.u64(self.message_id).u32(0).u32(self.tree_id).u64(self.session_id).zeros(16);
        self.message_id += 1;
        h.bytes(body);
        h.into_vec()
    }

    /// Signs or encrypts a finished request, as the session requires.
    fn protect(&self, mut request: Vec<u8>) -> Result<Vec<u8>> {
        if self.encrypting {
            let (Some(cipher), Some(key)) = (self.cipher, self.encrypt_key) else { return Err(SmbError::Unsafe) };
            return seal_frame(cipher, &key, &crypto::random::<16>()?, self.session_id, &request);
        }
        let Some(key) = self.signing_key else { return Ok(request) };
        let flags = request.u32_at(16)? | FLAG_SIGNED;
        request[16..20].copy_from_slice(&flags.to_le_bytes());
        let signed = signature(self.dialect, &key, &request);
        request[48..64].copy_from_slice(&signed);
        Ok(request)
    }

    /// The reply to `id`, skipping anything else the server sends meanwhile: "still
    /// working" interim replies and lease or oplock break notices.
    fn receive(&mut self, id: u64) -> Result<Reply> {
        loop {
            let mut frame = self.read_frame()?;
            let was_sealed = frame.starts_with(PROTOCOL_TRANSFORM);
            if was_sealed {
                let (Some(cipher), Some(key)) = (self.cipher, self.decrypt_key) else { return Err(SmbError::Io) };
                frame = open_frame(cipher, &key, &frame)?;
            }
            if frame.len() < HEADER_BYTES || !frame.starts_with(PROTOCOL_SMB2) {
                return Err(SmbError::Io);
            }
            let reply = Reply(frame);
            if reply.0.u64_at(24)? != id {
                continue;
            }
            let flags = reply.0.u32_at(16)?;
            if reply.status() == STATUS_PENDING && flags & FLAG_ASYNC != 0 {
                continue;
            }
            if was_sealed {
                return Ok(reply);
            }
            if self.encrypting && reply.status() == STATUS_SUCCESS {
                // Asked for encryption and answered in the clear: someone in the middle,
                // or a server ignoring the session's rules.
                return Err(SmbError::Unsafe);
            }
            match self.signing_key {
                Some(key) if flags & FLAG_SIGNED != 0 => {
                    if !crypto::equal(&signature(self.dialect, &key, &reply.0), reply.0.slice_at(48, 16)?) {
                        return Err(SmbError::Io);
                    }
                }
                // Every request after the sign-in is signed, and a server must then sign
                // its reply. An unsigned one is a stripped one.
                Some(_) if self.established => return Err(SmbError::Io),
                _ => {}
            }
            return Ok(reply);
        }
    }

    // ---- Transport: direct TCP, a 4-byte length in front of every message ----

    fn write_frame(&mut self, message: &[u8]) -> Result<()> {
        let n = u32::try_from(message.len()).map_err(|_| SmbError::Io)?;
        if message.len() > MAX_FRAME {
            return Err(SmbError::Io);
        }
        let mut frame = Vec::with_capacity(message.len() + 4);
        frame.extend_from_slice(&n.to_be_bytes());
        frame.extend_from_slice(message);
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }

    fn read_frame(&mut self) -> Result<Vec<u8>> {
        let mut head = [0u8; 4];
        self.stream.read_exact(&mut head)?;
        let n = u32::from_be_bytes(head) as usize;
        if head[0] != 0 || n > MAX_FRAME {
            return Err(SmbError::Io);
        }
        let mut frame = vec![0u8; n];
        self.stream.read_exact(&mut frame)?;
        Ok(frame)
    }

    /// Leaves the share and signs off, as far as the server still answers.
    pub(crate) fn disconnect(&mut self) {
        if self.tree_id != 0 {
            let _ = self.call(COMMAND_TREE_DISCONNECT, &[4, 0, 0, 0]);
            self.tree_id = 0;
        }
        if self.session_id != 0 && self.established {
            let _ = self.call(COMMAND_LOGOFF, &[4, 0, 0, 0]);
        }
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// The cipher the server picked from our encryption context, 3.1.1 only.
fn negotiated_cipher(b: &[u8]) -> Option<Cipher> {
    let count = b.u16_at(HEADER_BYTES + 6).ok()?;
    let mut at = b.u32_at(HEADER_BYTES + 60).ok()? as usize;
    for _ in 0..count {
        let kind = b.u16_at(at).ok()?;
        let len = usize::from(b.u16_at(at + 2).ok()?);
        if kind == CONTEXT_ENCRYPTION && len >= 4 && b.u16_at(at + 8).ok()? >= 1 {
            return Cipher::of(b.u16_at(at + 10).ok()?);
        }
        at = (at + 8 + len + 7) & !7;
    }
    None
}
