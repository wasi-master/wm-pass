//! A scripted SMB server on 127.0.0.1 for the tests: the server half of negotiate, NTLMv2
//! (it checks the proof and the MIC, and unwraps the session key the way a real server
//! does), signing, sealing and the file commands the sink uses, over an in-memory tree.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use super::bytes::{Le, LeWriter, from_utf16, utf16};
use super::client::*;
use super::crypto::{self, Cipher};
use super::{ntlm, spnego};

/// How the server behaves.
#[derive(Clone, Debug)]
pub(super) struct Options {
    pub dialect: u16,
    /// The cipher it offers; `None` is a server that cannot encrypt.
    pub cipher: Option<Cipher>,
    pub user: &'static str,
    pub password: &'static str,
    pub domain: &'static str,
    /// Samba's "map to guest = bad user".
    pub guest_on_bad_password: bool,
    pub encrypt_session: bool,
    pub encrypt_share: bool,
    /// A timestamp in the target info, which makes the MIC mandatory.
    pub timestamp: bool,
    /// Corrupts every reply after the sign-in.
    pub tamper: bool,
    /// Answers sealed requests in the clear.
    pub cleartext: bool,
    /// Sends a "still working" interim reply before each create.
    pub pending: bool,
    pub max_io: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dialect: DIALECT_311,
            cipher: Some(Cipher::Aes128Gcm),
            user: "alice",
            password: "secret",
            domain: "",
            guest_on_bad_password: false,
            encrypt_session: false,
            encrypt_share: false,
            timestamp: true,
            tamper: false,
            cleartext: false,
            pending: false,
            max_io: 4096,
        }
    }
}

/// The share's contents and what the server saw.
#[derive(Debug, Default)]
pub(super) struct Store {
    pub files: BTreeMap<String, Vec<u8>>,
    pub dirs: BTreeSet<String>,
    pub sealed_requests: usize,
    pub signed_requests: usize,
    pub mics_checked: usize,
    pub connections: usize,
}

/// Starts a server and returns its port and its store.
pub(super) fn serve(options: Options) -> (u16, Arc<Mutex<Store>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let store = Arc::new(Mutex::new(Store::default()));
    let shared = Arc::clone(&store);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            shared.lock().unwrap().connections += 1;
            let _ = Conn::new(stream, options.clone(), &shared).run();
        }
    });
    (port, store)
}

struct Open {
    path: String,
    dir: bool,
    delete_on_close: bool,
    listed: bool,
}

struct Conn<'a> {
    stream: TcpStream,
    options: Options,
    store: &'a Mutex<Store>,
    preauth: [u8; 64],
    session_hash: [u8; 64],
    session_id: u64,
    cipher: Option<Cipher>,
    signing: Option<[u8; 16]>,
    c2s: Option<[u8; 16]>,
    s2c: Option<[u8; 16]>,
    authed: bool,
    negotiate_message: Vec<u8>,
    challenge_message: Vec<u8>,
    server_challenge: [u8; 8],
    handles: HashMap<[u8; 16], Open>,
    next_handle: u64,
}

type Answer = (u32, Vec<u8>);

const STATUS_OBJECT_NAME_COLLISION: u32 = 0xC000_0035;
const ERROR_BODY: [u8; 9] = [9, 0, 0, 0, 0, 0, 0, 0, 0];

fn error(status: u32) -> Answer {
    (status, ERROR_BODY.to_vec())
}

fn parent(path: &str) -> &str {
    path.rsplit_once('\\').map_or("", |(parent, _)| parent)
}

impl<'a> Conn<'a> {
    fn new(stream: TcpStream, options: Options, store: &'a Mutex<Store>) -> Self {
        Self {
            stream,
            options,
            store,
            preauth: [0; 64],
            session_hash: [0; 64],
            session_id: 0x0000_1000_0000_0025,
            cipher: None,
            signing: None,
            c2s: None,
            s2c: None,
            authed: false,
            negotiate_message: Vec::new(),
            challenge_message: Vec::new(),
            server_challenge: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
            handles: HashMap::new(),
            next_handle: 1,
        }
    }

    fn read_frame(&mut self) -> std::io::Result<Vec<u8>> {
        let mut head = [0u8; 4];
        self.stream.read_exact(&mut head)?;
        let mut frame = vec![0u8; u32::from_be_bytes(head) as usize];
        self.stream.read_exact(&mut frame)?;
        Ok(frame)
    }

    fn write_frame(&mut self, message: &[u8]) -> std::io::Result<()> {
        let mut frame = (message.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(message);
        self.stream.write_all(&frame)
    }

    fn header(&self, request: &[u8], status: u32, flags: u32) -> LeWriter {
        let mut h = LeWriter::new();
        h.bytes(PROTOCOL_SMB2).u16(64).u16(1).u32(status);
        h.u16(request.u16_at(12).unwrap()).u16(64).u32(1 | flags).u32(0);
        h.u64(request.u64_at(24).unwrap()).u32(0).u32(request.u32_at(36).unwrap()).u64(self.session_id).zeros(16);
        h
    }

    fn run(mut self) -> std::io::Result<()> {
        loop {
            let frame = self.read_frame()?;
            let sealed = frame.starts_with(PROTOCOL_TRANSFORM);
            let message = if sealed {
                self.store.lock().unwrap().sealed_requests += 1;
                let Ok(plain) = open_frame(self.cipher.unwrap(), &self.c2s.unwrap(), &frame) else { return Ok(()) };
                plain
            } else {
                frame
            };
            let command = message.u16_at(12).unwrap();
            if !sealed && self.authed {
                let flags = message.u32_at(16).unwrap();
                let key = self.signing.unwrap();
                if flags & FLAG_SIGNED == 0 || signature(self.options.dialect, &key, &message)[..] != message[48..64] {
                    return Ok(()); // an unsigned or forged request: hang up
                }
                self.store.lock().unwrap().signed_requests += 1;
            }
            let (status, body) = match command {
                COMMAND_NEGOTIATE => {
                    self.negotiate(&message)?;
                    continue;
                }
                COMMAND_SESSION_SETUP => {
                    self.session_setup(&message)?;
                    continue;
                }
                COMMAND_TREE_CONNECT => {
                    let flags = if self.options.encrypt_share { SHARE_ENCRYPT_DATA } else { 0 };
                    let mut body = LeWriter::new();
                    body.u16(16).u8(1).u8(0).u32(flags).u32(0).u32(0x001F_01FF);
                    (STATUS_SUCCESS, body.into_vec())
                }
                COMMAND_TREE_DISCONNECT | COMMAND_LOGOFF => (STATUS_SUCCESS, vec![4, 0, 0, 0]),
                COMMAND_CREATE => {
                    if self.options.pending {
                        let interim = self.header(&message, STATUS_PENDING, FLAG_ASYNC);
                        let mut interim = interim.into_vec();
                        interim.extend_from_slice(&ERROR_BODY);
                        self.write_frame(&interim)?;
                    }
                    self.create(&message)
                }
                COMMAND_CLOSE => self.close(&message),
                COMMAND_WRITE => self.write(&message),
                COMMAND_READ => self.read(&message),
                COMMAND_QUERY_DIRECTORY => self.query_directory(&message),
                COMMAND_SET_INFO => self.rename(&message),
                _ => error(STATUS_NOT_SUPPORTED),
            };
            let mut reply = self.header(&message, status, 0).into_vec();
            if command == COMMAND_TREE_CONNECT {
                reply[36..40].copy_from_slice(&7u32.to_le_bytes());
            }
            reply.extend_from_slice(&body);
            self.send(reply, sealed)?;
        }
    }

    fn send(&mut self, mut reply: Vec<u8>, sealed: bool) -> std::io::Result<()> {
        let out = if sealed && !self.options.cleartext {
            let mut out = seal_frame(self.cipher.unwrap(), &self.s2c.unwrap(), &crypto::random().unwrap(), self.session_id, &reply).unwrap();
            if self.options.tamper {
                let last = out.len() - 1;
                out[last] ^= 1;
            }
            out
        } else {
            if let Some(key) = self.signing.filter(|_| self.authed) {
                let flags = reply.u32_at(16).unwrap() | FLAG_SIGNED;
                reply[16..20].copy_from_slice(&flags.to_le_bytes());
                let signed = signature(self.options.dialect, &key, &reply);
                reply[48..64].copy_from_slice(&signed);
                if self.options.tamper {
                    reply[50] ^= 1;
                }
            }
            reply
        };
        self.write_frame(&out)
    }

    fn negotiate(&mut self, request: &[u8]) -> std::io::Result<()> {
        let dialect = self.options.dialect;
        self.cipher = match dialect {
            DIALECT_311 => self.options.cipher,
            DIALECT_300 | DIALECT_302 => self.options.cipher.map(|_| Cipher::Aes128Ccm),
            _ => None,
        };
        let contexts = u16::from(dialect == DIALECT_311 && self.cipher.is_some());
        let capabilities = if matches!(dialect, DIALECT_300 | DIALECT_302) && self.cipher.is_some() { CAP_ENCRYPTION } else { 0 };
        let max = self.options.max_io;
        let mut h = self.header(request, STATUS_SUCCESS, 0);
        h.u16(65).u16(1).u16(dialect).u16(contexts).zeros(16).u32(capabilities).u32(max).u32(max).u32(max);
        h.u64(0).u64(0).u16(128).u16(0).u32(128);
        if let Some(cipher) = self.cipher.filter(|_| contexts == 1) {
            h.u16(CONTEXT_ENCRYPTION).u16(4).u32(0).u16(1).u16(cipher.id());
        }
        let reply = h.into_vec();
        if dialect == DIALECT_311 {
            self.preauth = crypto::sha512(&[&self.preauth, request]);
            self.preauth = crypto::sha512(&[&self.preauth, &reply]);
        }
        self.write_frame(&reply)
    }

    fn session_setup(&mut self, request: &[u8]) -> std::io::Result<()> {
        let offset = usize::from(request.u16_at(HEADER_BYTES + 12).unwrap());
        let length = usize::from(request.u16_at(HEADER_BYTES + 14).unwrap());
        let token = request[offset..offset + length].to_vec();
        let is311 = self.options.dialect == DIALECT_311;
        if self.negotiate_message.is_empty() {
            self.session_hash = self.preauth;
            if is311 {
                self.session_hash = crypto::sha512(&[&self.session_hash, request]);
            }
            let at = token.windows(8).position(|w| w == ntlm::SIGNATURE).unwrap();
            self.negotiate_message = token[at..].to_vec();
            self.challenge_message = self.challenge();
            let ntlm_oid = [0x2b, 0x06, 0x01, 0x04, 0x01, 0x82, 0x37, 0x02, 0x02, 0x0a];
            let answer = spnego::tlv(
                0xa1,
                &spnego::tlv(
                    0x30,
                    &[
                        spnego::tlv(0xa0, &spnego::tlv(0x0a, &[1])),
                        spnego::tlv(0xa1, &spnego::tlv(0x06, &ntlm_oid)),
                        spnego::tlv(0xa2, &spnego::tlv(0x04, &self.challenge_message)),
                    ]
                    .concat(),
                ),
            );
            let mut h = self.header(request, STATUS_MORE_PROCESSING, 0);
            h.u16(9).u16(0).u16(72).u16(answer.len() as u16).bytes(&answer);
            let reply = h.into_vec();
            if is311 {
                self.session_hash = crypto::sha512(&[&self.session_hash, &reply]);
            }
            return self.write_frame(&reply);
        }
        if is311 {
            self.session_hash = crypto::sha512(&[&self.session_hash, request]);
        }
        let auth = spnego::response_token(&token).unwrap().to_vec();
        let field = |at: usize| -> Vec<u8> {
            let len = usize::from(auth.u16_at(at).unwrap());
            let off = auth.u32_at(at + 4).unwrap() as usize;
            auth[off..off + len].to_vec()
        };
        let nt = field(20);
        let domain = from_utf16(&field(28));
        let user = from_utf16(&field(36));
        let encrypted_key = field(52);
        let flags = auth.u32_at(60).unwrap();

        let key = ntlm::response_key(&ntlm::nt_hash(self.options.password), &user, &domain);
        let proof = crypto::hmac_md5(&key, &[&self.server_challenge, &nt[16..]]);
        let mut valid = user == self.options.user && domain == self.options.domain && proof[..] == nt[..16];
        let base = crypto::hmac_md5(&key, &[&proof]);
        let mut exported = base;
        if flags & ntlm::FLAG_KEY_EXCH != 0 {
            exported.copy_from_slice(&crypto::rc4(&base, &encrypted_key));
        }
        if valid && self.options.timestamp {
            let mut zeroed = auth.clone();
            zeroed[72..88].fill(0);
            let mic = crypto::hmac_md5(&exported, &[&self.negotiate_message, &self.challenge_message, &zeroed]);
            valid = mic[..] == auth[72..88];
            self.store.lock().unwrap().mics_checked += 1;
        }
        if !valid && !self.options.guest_on_bad_password {
            let mut h = self.header(request, STATUS_LOGON_FAILURE, 0);
            h.bytes(&ERROR_BODY);
            return self.write_frame(&h.into_vec());
        }
        let mut session_flags = 0;
        if valid {
            let (signing, ciphers) = derive_keys(self.options.dialect, &exported, &self.session_hash);
            self.signing = Some(signing);
            if let Some((c2s, s2c)) = ciphers {
                self.c2s = Some(c2s);
                self.s2c = Some(s2c);
            }
            if self.options.encrypt_session {
                session_flags |= SESSION_ENCRYPT_DATA;
            }
        } else {
            session_flags |= SESSION_GUEST;
        }
        let mut h = self.header(request, STATUS_SUCCESS, 0);
        h.u16(9).u16(session_flags).u16(72).u16(0);
        let reply = h.into_vec();
        self.authed = valid;
        // The final reply is signed with the new key, as MS-SMB2 requires of 3.x.
        let tamper = std::mem::replace(&mut self.options.tamper, false);
        let sent = self.send(reply, false);
        self.options.tamper = tamper;
        sent
    }

    /// The CHALLENGE_MESSAGE, laid out as in MS-NLMP 4.2.4.3.
    fn challenge(&self) -> Vec<u8> {
        let target_name = utf16("Server");
        let mut pairs = vec![(2, utf16("Domain")), (1, utf16("Server"))];
        if self.options.timestamp {
            pairs.push((ntlm::AV_TIMESTAMP, 0x01DC_0000_0000_0000u64.to_le_bytes().to_vec()));
        }
        let info = ntlm::av_encode(&pairs);
        let mut w = LeWriter::new();
        w.bytes(ntlm::SIGNATURE).u32(2);
        w.u16(target_name.len() as u16).u16(target_name.len() as u16).u32(56);
        w.u32(ntlm::CLIENT_FLAGS).bytes(&self.server_challenge).zeros(8);
        w.u16(info.len() as u16).u16(info.len() as u16).u32(56 + target_name.len() as u32);
        w.bytes(&[6, 0, 0x70, 0x17, 0, 0, 0, 15]).bytes(&target_name).bytes(&info);
        w.into_vec()
    }

    fn create(&mut self, m: &[u8]) -> Answer {
        let disposition = m.u32_at(HEADER_BYTES + 36).unwrap();
        let options = m.u32_at(HEADER_BYTES + 40).unwrap();
        let at = usize::from(m.u16_at(HEADER_BYTES + 44).unwrap());
        let len = usize::from(m.u16_at(HEADER_BYTES + 46).unwrap());
        let path = from_utf16(&m[at..at + len]);
        let mut store = self.store.lock().unwrap();
        let dir_exists = |store: &Store, p: &str| p.is_empty() || store.dirs.contains(p);
        if !dir_exists(&store, parent(&path)) {
            return error(STATUS_OBJECT_PATH_NOT_FOUND);
        }
        let dir = options & OPTION_DIRECTORY != 0;
        let mut size = 0;
        if dir {
            if store.files.contains_key(&path) {
                return error(STATUS_OBJECT_NAME_COLLISION);
            }
            match disposition {
                DISPOSITION_OPEN_IF => {
                    if !path.is_empty() {
                        store.dirs.insert(path.clone());
                    }
                }
                DISPOSITION_OPEN if dir_exists(&store, &path) => {}
                _ => return error(STATUS_OBJECT_NAME_NOT_FOUND),
            }
        } else {
            match disposition {
                DISPOSITION_OVERWRITE_IF => {
                    store.files.insert(path.clone(), Vec::new());
                }
                DISPOSITION_OPEN => match store.files.get(&path) {
                    Some(bytes) => size = bytes.len() as u64,
                    None => return error(STATUS_OBJECT_NAME_NOT_FOUND),
                },
                _ => return error(STATUS_NOT_SUPPORTED),
            }
        }
        drop(store);
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&self.next_handle.to_le_bytes());
        self.next_handle += 1;
        self.handles.insert(id, Open { path, dir, delete_on_close: options & OPTION_DELETE_ON_CLOSE != 0, listed: false });
        let mut body = LeWriter::new();
        body.u16(89).u8(0).u8(0).u32(1).u64(0).u64(0).u64(0).u64(0).u64(size).u64(size);
        body.u32(if dir { ATTRIBUTE_DIRECTORY } else { 0x80 }).u32(0).bytes(&id).u32(0).u32(0);
        (STATUS_SUCCESS, body.into_vec())
    }

    fn handle(&mut self, m: &[u8], at: usize) -> Option<&mut Open> {
        let mut id = [0u8; 16];
        id.copy_from_slice(&m[at..at + 16]);
        self.handles.get_mut(&id)
    }

    fn close(&mut self, m: &[u8]) -> Answer {
        let mut id = [0u8; 16];
        id.copy_from_slice(&m[HEADER_BYTES + 8..HEADER_BYTES + 24]);
        let Some(open) = self.handles.remove(&id) else { return error(0xC000_0008) };
        if open.delete_on_close {
            self.store.lock().unwrap().files.remove(&open.path);
        }
        let mut body = LeWriter::new();
        body.u16(60).zeros(58);
        (STATUS_SUCCESS, body.into_vec())
    }

    fn write(&mut self, m: &[u8]) -> Answer {
        let data_at = usize::from(m.u16_at(HEADER_BYTES + 2).unwrap());
        let len = m.u32_at(HEADER_BYTES + 4).unwrap() as usize;
        let offset = m.u64_at(HEADER_BYTES + 8).unwrap() as usize;
        let max = self.options.max_io as usize;
        let Some(path) = self.handle(m, HEADER_BYTES + 16).map(|open| open.path.clone()) else { return error(0xC000_0008) };
        if len > max {
            return error(0xC000_000D);
        }
        let mut store = self.store.lock().unwrap();
        let file = store.files.entry(path).or_default();
        if file.len() < offset + len {
            file.resize(offset + len, 0);
        }
        file[offset..offset + len].copy_from_slice(&m[data_at..data_at + len]);
        let mut body = LeWriter::new();
        body.u16(17).u16(0).u32(len as u32).u32(0).u16(0).u16(0).u8(0);
        (STATUS_SUCCESS, body.into_vec())
    }

    fn read(&mut self, m: &[u8]) -> Answer {
        let len = m.u32_at(HEADER_BYTES + 4).unwrap() as usize;
        let offset = m.u64_at(HEADER_BYTES + 8).unwrap() as usize;
        let max = self.options.max_io as usize;
        let Some(path) = self.handle(m, HEADER_BYTES + 16).map(|open| open.path.clone()) else { return error(0xC000_0008) };
        let store = self.store.lock().unwrap();
        let file = store.files.get(&path).cloned().unwrap_or_default();
        if offset >= file.len() {
            return error(STATUS_END_OF_FILE);
        }
        let n = len.min(max).min(file.len() - offset);
        let mut body = LeWriter::new();
        body.u16(17).u8(80).u8(0).u32(n as u32).u32(0).u32(0).bytes(&file[offset..offset + n]);
        (STATUS_SUCCESS, body.into_vec())
    }

    fn query_directory(&mut self, m: &[u8]) -> Answer {
        let restart = m[HEADER_BYTES + 3] & 1 != 0;
        let store = self.store.lock().unwrap();
        let mut entries: Vec<(String, u64, bool)> = vec![(".".into(), 0, true), ("..".into(), 0, true)];
        let Some(open) = ({
            let mut id = [0u8; 16];
            id.copy_from_slice(&m[HEADER_BYTES + 8..HEADER_BYTES + 24]);
            self.handles.get_mut(&id)
        }) else {
            return error(0xC000_0008);
        };
        if !open.dir {
            return error(0xC000_000D);
        }
        if open.listed && !restart {
            return error(STATUS_NO_MORE_FILES);
        }
        open.listed = true;
        let inside = |p: &str| parent(p) == open.path && !p.is_empty();
        let child = |p: &str| p.rsplit('\\').next().unwrap_or(p).to_owned();
        entries.extend(store.dirs.iter().filter(|d| inside(d)).map(|d| (child(d), 0, true)));
        entries.extend(store.files.iter().filter(|(f, _)| inside(f)).map(|(f, bytes)| (child(f), bytes.len() as u64, false)));
        let mut data = Vec::new();
        let count = entries.len();
        for (i, (name, size, dir)) in entries.into_iter().enumerate() {
            let name = utf16(&name);
            let mut e = LeWriter::new();
            e.u32(0).u32(0).u64(0).u64(0).u64(0).u64(0).u64(size).u64(size);
            e.u32(if dir { ATTRIBUTE_DIRECTORY } else { 0x80 }).u32(name.len() as u32).bytes(&name);
            if i + 1 < count {
                e.align(8);
                let next = e.len() as u32;
                e.put_u32(0, next);
            }
            data.extend_from_slice(&e.into_vec());
        }
        let mut body = LeWriter::new();
        body.u16(9).u16(72).u32(data.len() as u32).bytes(&data);
        (STATUS_SUCCESS, body.into_vec())
    }

    fn rename(&mut self, m: &[u8]) -> Answer {
        let at = usize::from(m.u16_at(HEADER_BYTES + 8).unwrap());
        let replace = m[at] != 0;
        let len = m.u32_at(at + 16).unwrap() as usize;
        let target = from_utf16(&m[at + 20..at + 20 + len]);
        let store = self.store;
        let Some(open) = self.handle(m, HEADER_BYTES + 16) else { return error(0xC000_0008) };
        let mut store = store.lock().unwrap();
        if store.files.contains_key(&target) && !replace {
            return error(STATUS_OBJECT_NAME_COLLISION);
        }
        let bytes = store.files.remove(&open.path).unwrap_or_default();
        store.files.insert(target.clone(), bytes);
        open.path = target;
        (STATUS_SUCCESS, vec![2, 0])
    }
}
