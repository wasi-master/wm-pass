//! The SSH transport SFTP runs over: key exchange, encryption, host key and user authentication.
//!
//! Hand-written after RFC 4253 (transport), 4252 (user authentication) and 4254 (channels),
//! with what OpenSSH added since and every current server speaks: curve25519-sha256
//! (RFC 8731), ssh-ed25519 (RFC 8709), rsa-sha2-256/512 (RFC 8332), server-sig-algs
//! (RFC 8308), aes-gcm@openssh.com (RFC 5647 as OpenSSH frames it),
//! chacha20-poly1305@openssh.com, and strict key exchange against the Terrapin attack.
//! WM Keyboard used JSch for this layer; the algorithm lists follow its `SftpAlgorithms`.
//!
//! **Host keys.** The first connection records the server's key; every later one must see
//! the same key or it refuses with [`Fault::Unsafe`]. That is trust on first use, the same
//! as `ssh` asking "are you sure you want to continue connecting" and remembering a yes,
//! minus the question: the Test button in the location sheet is that first use. A key is
//! kept as `<type> <base64>`, the way `known_hosts` and JSch print it.
//!
//! **Legacy.** [`Login::legacy_algorithms`] also offers SHA-1 signatures, SHA-1 MACs and
//! the 2048-bit SHA-1 group, which OpenSSH itself has turned off. Some old NAS firmware
//! speaks nothing newer. Off by default.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use aes::{Aes128, Aes192, Aes256};
use aes_gcm::aead::AeadInOut;
use aes_gcm::{Aes128Gcm, Aes256Gcm, KeyInit};
use base64::Engine;
use chacha20::ChaCha20Legacy;
use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use poly1305::Poly1305;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use wmpass_core::Secret;
use wmpass_net::NetPolicy;
use zeroize::Zeroizing;

use crate::{Fault, Result};

const CLIENT_VERSION: &[u8] = b"SSH-2.0-WMPass_1.0";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// OpenSSH's own limit is 256 KiB; anything near this is a broken or hostile peer.
const MAX_PACKET: usize = 1024 * 1024;
/// Rekeys after this much traffic either way, as RFC 4253 section 9 recommends.
const REKEY_BYTES: u64 = 1 << 30;

#[cfg(not(test))]
fn rekey_bytes() -> u64 {
    REKEY_BYTES
}

/// Live tests set `WMPASS_TEST_REKEY` to see this client start key exchanges itself.
#[cfg(test)]
fn rekey_bytes() -> u64 {
    std::env::var("WMPASS_TEST_REKEY").ok().and_then(|v| v.parse().ok()).unwrap_or(REKEY_BYTES)
}
const WINDOW: u32 = 2 * 1024 * 1024;
const CHANNEL_PACKET: u32 = 32 * 1024;
/// Lines a server may send before its version, and how long each may be.
const MAX_BANNER_LINES: usize = 64;
const MAX_LINE: usize = 1024;

const DISCONNECT: u8 = 1;
const IGNORE: u8 = 2;
const UNIMPLEMENTED: u8 = 3;
const DEBUG: u8 = 4;
const SERVICE_REQUEST: u8 = 5;
const SERVICE_ACCEPT: u8 = 6;
const EXT_INFO: u8 = 7;
const KEXINIT: u8 = 20;
const NEWKEYS: u8 = 21;
const KEX_INIT: u8 = 30;
const KEX_REPLY: u8 = 31;
const USERAUTH_REQUEST: u8 = 50;
const USERAUTH_FAILURE: u8 = 51;
const USERAUTH_SUCCESS: u8 = 52;
const USERAUTH_BANNER: u8 = 53;
/// PK_OK, PASSWD_CHANGEREQ or INFO_REQUEST, depending on the method in progress.
const USERAUTH_60: u8 = 60;
const USERAUTH_INFO_RESPONSE: u8 = 61;
const GLOBAL_REQUEST: u8 = 80;
const REQUEST_FAILURE: u8 = 82;
const CHANNEL_OPEN: u8 = 90;
const CHANNEL_OPEN_CONFIRMATION: u8 = 91;
const CHANNEL_OPEN_FAILURE: u8 = 92;
const CHANNEL_WINDOW_ADJUST: u8 = 93;
const CHANNEL_DATA: u8 = 94;
const CHANNEL_EXTENDED_DATA: u8 = 95;
const CHANNEL_EOF: u8 = 96;
const CHANNEL_CLOSE: u8 = 97;
const CHANNEL_REQUEST: u8 = 98;
const CHANNEL_SUCCESS: u8 = 99;
const CHANNEL_FAILURE: u8 = 100;

const STRICT_CLIENT: &str = "kex-strict-c-v00@openssh.com";
const STRICT_SERVER: &str = "kex-strict-s-v00@openssh.com";
const EXT_INFO_CLIENT: &str = "ext-info-c";

// ---- Wire format (RFC 4251 section 5) ----

/// Builds a message.
#[derive(Default)]
pub(crate) struct Wire(pub(crate) Vec<u8>);

impl Wire {
    pub(crate) fn msg(kind: u8) -> Self {
        Self(vec![kind])
    }

    pub(crate) fn byte(mut self, value: u8) -> Self {
        self.0.push(value);
        self
    }

    pub(crate) fn u32(mut self, value: u32) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub(crate) fn u64(mut self, value: u64) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub(crate) fn bool(self, value: bool) -> Self {
        self.byte(u8::from(value))
    }

    pub(crate) fn bytes(mut self, value: &[u8]) -> Self {
        self.0.extend_from_slice(&(value.len() as u32).to_be_bytes());
        self.0.extend_from_slice(value);
        self
    }

    pub(crate) fn text(self, value: &str) -> Self {
        self.bytes(value.as_bytes())
    }

    pub(crate) fn names(self, values: &[&str]) -> Self {
        self.text(&values.join(","))
    }

    /// An unsigned big-endian magnitude as an mpint: no leading zeros, and one zero byte
    /// in front when the top bit is set, so it does not read as negative.
    pub(crate) fn mpint(self, magnitude: &[u8]) -> Self {
        let start = magnitude.iter().position(|&b| b != 0).unwrap_or(magnitude.len());
        let trimmed = &magnitude[start..];
        if trimmed.first().is_some_and(|&b| b & 0x80 != 0) {
            let mut padded = Vec::with_capacity(trimmed.len() + 1);
            padded.push(0);
            padded.extend_from_slice(trimmed);
            self.bytes(&padded)
        } else {
            self.bytes(trimmed)
        }
    }

    pub(crate) fn raw(mut self, value: &[u8]) -> Self {
        self.0.extend_from_slice(value);
        self
    }
}

/// Reads a message. Anything short or malformed is [`Fault::Io`].
pub(crate) struct Parse<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Parse<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(Fault::Io)?;
        let out = self.data.get(self.at..end).ok_or(Fault::Io)?;
        self.at = end;
        Ok(out)
    }

    pub(crate) fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        let mut raw = [0u8; 8];
        raw.copy_from_slice(b);
        Ok(u64::from_be_bytes(raw))
    }

    pub(crate) fn bool(&mut self) -> Result<bool> {
        Ok(self.byte()? != 0)
    }

    pub(crate) fn bytes(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    pub(crate) fn text(&mut self) -> Result<&'a str> {
        std::str::from_utf8(self.bytes()?).map_err(|_| Fault::Io)
    }

    /// Whether everything has been read.
    pub(crate) fn done(&self) -> bool {
        self.at >= self.data.len()
    }

    pub(crate) fn names(&mut self) -> Result<Vec<&'a str>> {
        let list = self.text()?;
        Ok(if list.is_empty() { Vec::new() } else { list.split(',').collect() })
    }

    /// A non-negative mpint's magnitude, without the sign byte.
    pub(crate) fn mpint(&mut self) -> Result<&'a [u8]> {
        let raw = self.bytes()?;
        if raw.first().is_some_and(|&b| b & 0x80 != 0) {
            return Err(Fault::Io);
        }
        let start = raw.iter().position(|&b| b != 0).unwrap_or(raw.len());
        Ok(&raw[start..])
    }
}

/// Constant-time equality, for tags and MACs.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random(n: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    getrandom::fill(&mut out).map_err(|_| Fault::Io)?;
    Ok(out)
}

fn left_pad(bytes: &[u8], len: usize) -> Result<Vec<u8>> {
    if bytes.len() > len {
        return Err(Fault::Io);
    }
    let mut out = vec![0u8; len - bytes.len()];
    out.extend_from_slice(bytes);
    Ok(out)
}

// ---- Algorithms ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KexHash {
    Sha1,
    Sha256,
}

impl KexHash {
    fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        match self {
            Self::Sha1 => {
                let mut h = Sha1::new();
                for part in parts {
                    h.update(part);
                }
                h.finalize().to_vec()
            }
            Self::Sha256 => {
                let mut h = Sha256::new();
                for part in parts {
                    h.update(part);
                }
                h.finalize().to_vec()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KexKind {
    Curve25519,
    Group14(KexHash),
}

const KEX: &[(&str, KexKind)] = &[
    ("curve25519-sha256", KexKind::Curve25519),
    ("curve25519-sha256@libssh.org", KexKind::Curve25519),
    ("diffie-hellman-group14-sha256", KexKind::Group14(KexHash::Sha256)),
];
const KEX_LEGACY: &[(&str, KexKind)] = &[("diffie-hellman-group14-sha1", KexKind::Group14(KexHash::Sha1))];

/// How a host key or a client key signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SigKind {
    Ed25519,
    EcdsaP256,
    Rsa(RsaHash),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RsaHash {
    Sha1,
    Sha256,
    Sha512,
}

impl SigKind {
    fn name(self) -> &'static str {
        match self {
            Self::Ed25519 => "ssh-ed25519",
            Self::EcdsaP256 => "ecdsa-sha2-nistp256",
            Self::Rsa(RsaHash::Sha512) => "rsa-sha2-512",
            Self::Rsa(RsaHash::Sha256) => "rsa-sha2-256",
            Self::Rsa(RsaHash::Sha1) => "ssh-rsa",
        }
    }

    /// The key type a key blob of this algorithm starts with.
    fn key_type(self) -> &'static str {
        match self {
            Self::Rsa(_) => "ssh-rsa",
            other => other.name(),
        }
    }
}

const HOST_KEYS: &[SigKind] =
    &[SigKind::Ed25519, SigKind::EcdsaP256, SigKind::Rsa(RsaHash::Sha512), SigKind::Rsa(RsaHash::Sha256)];
const HOST_KEYS_LEGACY: &[SigKind] = &[SigKind::Rsa(RsaHash::Sha1)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CipherKind {
    ChaCha,
    Aes128Gcm,
    Aes256Gcm,
    Aes128Ctr,
    Aes192Ctr,
    Aes256Ctr,
}

impl CipherKind {
    fn key_len(self) -> usize {
        match self {
            Self::ChaCha => 64,
            Self::Aes128Gcm | Self::Aes128Ctr => 16,
            Self::Aes192Ctr => 24,
            Self::Aes256Gcm | Self::Aes256Ctr => 32,
        }
    }

    fn iv_len(self) -> usize {
        match self {
            Self::ChaCha => 0,
            Self::Aes128Gcm | Self::Aes256Gcm => 12,
            Self::Aes128Ctr | Self::Aes192Ctr | Self::Aes256Ctr => 16,
        }
    }

    /// Carries its own integrity, so the negotiated MAC is not used.
    fn aead(self) -> bool {
        matches!(self, Self::ChaCha | Self::Aes128Gcm | Self::Aes256Gcm)
    }
}

const CIPHERS: &[(&str, CipherKind)] = &[
    ("chacha20-poly1305@openssh.com", CipherKind::ChaCha),
    ("aes128-gcm@openssh.com", CipherKind::Aes128Gcm),
    ("aes256-gcm@openssh.com", CipherKind::Aes256Gcm),
    ("aes128-ctr", CipherKind::Aes128Ctr),
    ("aes192-ctr", CipherKind::Aes192Ctr),
    ("aes256-ctr", CipherKind::Aes256Ctr),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MacKind {
    Sha1,
    Sha256,
    Sha512,
}

impl MacKind {
    fn len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha512 => 64,
        }
    }

    fn compute(self, key: &[u8], seq: u32, data: &[u8]) -> Result<Vec<u8>> {
        macro_rules! hmac {
            ($hash:ty) => {{
                let mut mac = <Hmac<$hash> as KeyInit>::new_from_slice(key).map_err(|_| Fault::Io)?;
                mac.update(&seq.to_be_bytes());
                mac.update(data);
                Ok(mac.finalize().into_bytes().to_vec())
            }};
        }
        match self {
            Self::Sha1 => hmac!(Sha1),
            Self::Sha256 => hmac!(Sha256),
            Self::Sha512 => hmac!(Sha512),
        }
    }
}

/// Name, hash, and whether it is encrypt-then-MAC.
const MACS: &[(&str, MacKind, bool)] = &[
    ("hmac-sha2-256-etm@openssh.com", MacKind::Sha256, true),
    ("hmac-sha2-512-etm@openssh.com", MacKind::Sha512, true),
    ("hmac-sha2-256", MacKind::Sha256, false),
    ("hmac-sha2-512", MacKind::Sha512, false),
];
const MACS_LEGACY: &[(&str, MacKind, bool)] =
    &[("hmac-sha1-etm@openssh.com", MacKind::Sha1, true), ("hmac-sha1", MacKind::Sha1, false)];

/// What this client offers, in order of preference.
struct Offer {
    legacy: bool,
    /// The type of the trusted key, whose algorithms go first so a server with several
    /// keys shows the one already known instead of looking like it changed.
    pinned_type: Option<String>,
}

impl Offer {
    fn kex(&self) -> Vec<(&'static str, KexKind)> {
        let mut out = KEX.to_vec();
        if self.legacy {
            out.extend_from_slice(KEX_LEGACY);
        }
        out
    }

    fn host_keys(&self) -> Vec<SigKind> {
        let mut out = HOST_KEYS.to_vec();
        if self.legacy {
            out.extend_from_slice(HOST_KEYS_LEGACY);
        }
        if let Some(pinned) = &self.pinned_type {
            out.sort_by_key(|kind| kind.key_type() != pinned.as_str());
        }
        out
    }

    fn macs(&self) -> Vec<(&'static str, MacKind, bool)> {
        let mut out = MACS.to_vec();
        if self.legacy {
            out.extend_from_slice(MACS_LEGACY);
        }
        out
    }

    fn kexinit(&self, initial: bool) -> Result<Vec<u8>> {
        let mut kex: Vec<&str> = self.kex().iter().map(|(name, _)| *name).collect();
        if initial {
            kex.push(EXT_INFO_CLIENT);
            kex.push(STRICT_CLIENT);
        }
        let host: Vec<&str> = self.host_keys().iter().map(|kind| kind.name()).collect();
        let ciphers: Vec<&str> = CIPHERS.iter().map(|(name, _)| *name).collect();
        let macs: Vec<&str> = self.macs().iter().map(|(name, _, _)| *name).collect();
        Ok(Wire::msg(KEXINIT)
            .raw(&random(16)?)
            .names(&kex)
            .names(&host)
            .names(&ciphers)
            .names(&ciphers)
            .names(&macs)
            .names(&macs)
            .names(&["none"])
            .names(&["none"])
            .names(&[])
            .names(&[])
            .bool(false)
            .u32(0)
            .0)
    }
}

/// The algorithms both sides settled on.
#[derive(Clone, Copy, Debug)]
struct Chosen {
    kex: KexKind,
    host_key: SigKind,
    c2s: (CipherKind, Option<(MacKind, bool)>),
    s2c: (CipherKind, Option<(MacKind, bool)>),
    /// The server sent a guessed key exchange packet that guessed wrong.
    skip_guess: bool,
    strict: bool,
}

fn pick<T: Copy>(ours: &[(&str, T)], theirs: &[&str]) -> Result<T> {
    ours.iter().find(|(name, _)| theirs.contains(name)).map(|(_, value)| *value).ok_or(Fault::Io)
}

/// Picks what both sides offer, the client's preference first (RFC 4253 section 7.1).
fn negotiate(offer: &Offer, server: &[u8]) -> Result<Chosen> {
    let mut p = Parse::new(server);
    if p.byte()? != KEXINIT {
        return Err(Fault::Io);
    }
    p.take(16)?;
    let kex = p.names()?;
    let host = p.names()?;
    let enc_c2s = p.names()?;
    let enc_s2c = p.names()?;
    let mac_c2s = p.names()?;
    let mac_s2c = p.names()?;
    let comp_c2s = p.names()?;
    let comp_s2c = p.names()?;
    p.names()?;
    p.names()?;
    let follows = p.bool()?;
    if !comp_c2s.contains(&"none") || !comp_s2c.contains(&"none") {
        return Err(Fault::Io);
    }
    let kex_offer = offer.kex();
    let chosen_kex = kex_offer.iter().find(|(name, _)| kex.contains(name)).ok_or(Fault::Io)?;
    let host_offer = offer.host_keys();
    let host_key = *host_offer.iter().find(|kind| host.contains(&kind.name())).ok_or(Fault::Io)?;
    let macs = offer.macs();
    let direction = |enc: &[&str], mac: &[&str]| -> Result<(CipherKind, Option<(MacKind, bool)>)> {
        let cipher = pick(CIPHERS, enc)?;
        if cipher.aead() {
            return Ok((cipher, None));
        }
        let (_, kind, etm) = macs.iter().find(|(name, _, _)| mac.contains(name)).ok_or(Fault::Io)?;
        Ok((cipher, Some((*kind, *etm))))
    };
    // A guess is right when the server's first choices are the ones agreed on.
    let guessed_right = kex.first() == Some(&chosen_kex.0) && host.first() == Some(&host_key.name());
    Ok(Chosen {
        kex: chosen_kex.1,
        host_key,
        c2s: direction(&enc_c2s, &mac_c2s)?,
        s2c: direction(&enc_s2c, &mac_s2c)?,
        skip_guess: follows && !guessed_right,
        strict: kex.contains(&STRICT_SERVER),
    })
}

// ---- Packets (RFC 4253 section 6) ----

enum Cipher {
    None,
    Ctr128(Box<ctr::Ctr128BE<Aes128>>),
    Ctr192(Box<ctr::Ctr128BE<Aes192>>),
    Ctr256(Box<ctr::Ctr128BE<Aes256>>),
    Gcm128(Box<Aes128Gcm>, [u8; 12]),
    Gcm256(Box<Aes256Gcm>, [u8; 12]),
    /// The second half of the key encrypts lengths, the first everything else.
    ChaCha { main: Zeroizing<[u8; 32]>, header: Zeroizing<[u8; 32]> },
}

impl Cipher {
    fn new(kind: CipherKind, key: &[u8], iv: &[u8]) -> Result<Self> {
        let bad = |_| Fault::Io;
        Ok(match kind {
            CipherKind::Aes128Ctr => Self::Ctr128(Box::new(ctr::Ctr128BE::new_from_slices(key, iv).map_err(bad)?)),
            CipherKind::Aes192Ctr => Self::Ctr192(Box::new(ctr::Ctr128BE::new_from_slices(key, iv).map_err(bad)?)),
            CipherKind::Aes256Ctr => Self::Ctr256(Box::new(ctr::Ctr128BE::new_from_slices(key, iv).map_err(bad)?)),
            CipherKind::Aes128Gcm => Self::Gcm128(
                Box::new(Aes128Gcm::new_from_slice(key).map_err(|_| Fault::Io)?),
                iv.try_into().map_err(|_| Fault::Io)?,
            ),
            CipherKind::Aes256Gcm => Self::Gcm256(
                Box::new(Aes256Gcm::new_from_slice(key).map_err(|_| Fault::Io)?),
                iv.try_into().map_err(|_| Fault::Io)?,
            ),
            CipherKind::ChaCha => {
                let mut main = Zeroizing::new([0u8; 32]);
                let mut header = Zeroizing::new([0u8; 32]);
                main.copy_from_slice(key.get(..32).ok_or(Fault::Io)?);
                header.copy_from_slice(key.get(32..64).ok_or(Fault::Io)?);
                Self::ChaCha { main, header }
            }
        })
    }

    fn block(&self) -> usize {
        match self {
            Self::None | Self::ChaCha { .. } => 8,
            _ => 16,
        }
    }

    fn ctr(&mut self) -> Option<&mut dyn StreamCipher> {
        match self {
            Self::Ctr128(c) => Some(c.as_mut()),
            Self::Ctr192(c) => Some(c.as_mut()),
            Self::Ctr256(c) => Some(c.as_mut()),
            _ => None,
        }
    }
}

/// The invocation counter in the last eight bytes of a GCM nonce.
fn next_iv(iv: &mut [u8; 12]) {
    let mut counter = [0u8; 8];
    counter.copy_from_slice(&iv[4..]);
    iv[4..].copy_from_slice(&u64::from_be_bytes(counter).wrapping_add(1).to_be_bytes());
}

fn chacha(key: &[u8; 32], seq: u32) -> Result<ChaCha20Legacy> {
    ChaCha20Legacy::new_from_slices(key, &u64::from(seq).to_be_bytes()).map_err(|_| Fault::Io)
}

fn poly_key(main: &[u8; 32], seq: u32) -> Result<(ChaCha20Legacy, Zeroizing<[u8; 32]>)> {
    let mut stream = chacha(main, seq)?;
    let mut key = Zeroizing::new([0u8; 32]);
    stream.apply_keystream(key.as_mut());
    stream.seek(64u64);
    Ok((stream, key))
}

fn poly_tag(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    Ok(Poly1305::new_from_slice(key).map_err(|_| Fault::Io)?.compute_unpadded(data).to_vec())
}

/// One direction of the connection: its cipher, MAC and sequence number.
struct Direction {
    cipher: Cipher,
    mac: Option<(MacKind, Zeroizing<Vec<u8>>, bool)>,
    seq: u32,
}

impl Direction {
    fn plain() -> Self {
        Self { cipher: Cipher::None, mac: None, seq: 0 }
    }

    /// Whether the length field stays outside the cipher's block alignment.
    fn length_apart(&self) -> bool {
        matches!(self.cipher, Cipher::ChaCha { .. } | Cipher::Gcm128(..) | Cipher::Gcm256(..))
            || self.mac.as_ref().is_some_and(|(_, _, etm)| *etm)
    }

    fn mac_len(&self) -> usize {
        match (&self.cipher, &self.mac) {
            (Cipher::ChaCha { .. } | Cipher::Gcm128(..) | Cipher::Gcm256(..), _) => 16,
            (_, Some((kind, _, _))) => kind.len(),
            _ => 0,
        }
    }


    fn seal(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let block = self.cipher.block();
        let covered = if self.length_apart() { 1 + payload.len() } else { 5 + payload.len() };
        let mut pad = block - covered % block;
        if pad < 4 {
            pad += block;
        }
        let length = 1 + payload.len() + pad;
        let mut packet = Vec::with_capacity(4 + length + 64);
        packet.extend_from_slice(&(length as u32).to_be_bytes());
        packet.push(pad as u8);
        packet.extend_from_slice(payload);
        packet.extend_from_slice(&random(pad)?);
        let seq = self.seq;
        self.seq = seq.wrapping_add(1);
        let etm = self.mac.as_ref().is_some_and(|(_, _, etm)| *etm);
        match &mut self.cipher {
            Cipher::None => {}
            Cipher::Gcm128(aead, iv) => {
                let (head, body) = packet.split_at_mut(4);
                let tag = aead.encrypt_inout_detached(&(*iv).into(), head, body.into()).map_err(|_| Fault::Io)?;
                packet.extend_from_slice(&tag);
                next_iv(iv);
            }
            Cipher::Gcm256(aead, iv) => {
                let (head, body) = packet.split_at_mut(4);
                let tag = aead.encrypt_inout_detached(&(*iv).into(), head, body.into()).map_err(|_| Fault::Io)?;
                packet.extend_from_slice(&tag);
                next_iv(iv);
            }
            Cipher::ChaCha { main, header } => {
                chacha(header, seq)?.apply_keystream(&mut packet[..4]);
                let (mut stream, key) = poly_key(main, seq)?;
                stream.apply_keystream(&mut packet[4..]);
                let tag = poly_tag(&key, &packet)?;
                packet.extend_from_slice(&tag);
            }
            ctr => {
                let Some(stream) = ctr.ctr() else { return Err(Fault::Io) };
                let Some((kind, key, _)) = &self.mac else { return Err(Fault::Io) };
                if etm {
                    stream.apply_keystream(&mut packet[4..]);
                    let tag = kind.compute(key, seq, &packet)?;
                    packet.extend_from_slice(&tag);
                } else {
                    let tag = kind.compute(key, seq, &packet)?;
                    stream.apply_keystream(&mut packet);
                    packet.extend_from_slice(&tag);
                }
            }
        }
        Ok(packet)
    }

    fn checked_length(raw: [u8; 4], block: usize, apart: bool) -> Result<usize> {
        let length = u32::from_be_bytes(raw) as usize;
        let aligned = if apart { length } else { length + 4 };
        if !(5..=MAX_PACKET).contains(&length) || aligned % block != 0 {
            return Err(Fault::Io);
        }
        Ok(length)
    }

    /// Reads one packet and returns its payload.
    fn open(&mut self, input: &mut impl Read) -> Result<Vec<u8>> {
        let seq = self.seq;
        self.seq = seq.wrapping_add(1);
        let block = self.cipher.block();
        let tag_len = self.mac_len();
        let etm = self.mac.as_ref().is_some_and(|(_, _, etm)| *etm);
        let mut head = [0u8; 4];
        let mut packet: Vec<u8>;
        match &mut self.cipher {
            Cipher::None => {
                read(input, &mut head)?;
                let length = u32::from_be_bytes(head) as usize;
                if !(5..=MAX_PACKET).contains(&length) {
                    return Err(Fault::Io);
                }
                packet = head.to_vec();
                packet.resize(4 + length, 0);
                read(input, &mut packet[4..])?;
            }
            Cipher::Gcm128(..) | Cipher::Gcm256(..) => {
                read(input, &mut head)?;
                let length = Self::checked_length(head, block, true)?;
                let mut body = vec![0u8; length + 16];
                read(input, &mut body)?;
                let tag = body.split_off(length);
                let tag = aes_gcm::Tag::try_from(tag.as_slice()).map_err(|_| Fault::Io)?;
                let done = match &mut self.cipher {
                    Cipher::Gcm128(aead, iv) => {
                        let done = aead.decrypt_inout_detached(&(*iv).into(), &head, body.as_mut_slice().into(), &tag);
                        next_iv(iv);
                        done
                    }
                    Cipher::Gcm256(aead, iv) => {
                        let done = aead.decrypt_inout_detached(&(*iv).into(), &head, body.as_mut_slice().into(), &tag);
                        next_iv(iv);
                        done
                    }
                    _ => return Err(Fault::Io),
                };
                done.map_err(|_| Fault::Io)?;
                packet = head.to_vec();
                packet.extend_from_slice(&body);
            }
            Cipher::ChaCha { main, header } => {
                read(input, &mut head)?;
                let mut plain_head = head;
                chacha(header, seq)?.apply_keystream(&mut plain_head);
                let length = Self::checked_length(plain_head, block, true)?;
                let mut rest = vec![0u8; length + 16];
                read(input, &mut rest)?;
                let tag = rest.split_off(length);
                let (mut stream, key) = poly_key(main, seq)?;
                let mut authenticated = head.to_vec();
                authenticated.extend_from_slice(&rest);
                if !same(&poly_tag(&key, &authenticated)?, &tag) {
                    return Err(Fault::Io);
                }
                stream.apply_keystream(&mut rest);
                packet = plain_head.to_vec();
                packet.extend_from_slice(&rest);
            }
            ctr if etm => {
                read(input, &mut head)?;
                let length = Self::checked_length(head, block, true)?;
                packet = head.to_vec();
                packet.resize(4 + length, 0);
                read(input, &mut packet[4..])?;
                let mut tag = vec![0u8; tag_len];
                read(input, &mut tag)?;
                // The MAC is over the ciphertext: checked before anything is decrypted.
                let Some((kind, key, _)) = &self.mac else { return Err(Fault::Io) };
                if !same(&kind.compute(key, seq, &packet)?, &tag) {
                    return Err(Fault::Io);
                }
                let Some(stream) = ctr.ctr() else { return Err(Fault::Io) };
                stream.apply_keystream(&mut packet[4..]);
            }
            ctr => {
                let Some(stream) = ctr.ctr() else { return Err(Fault::Io) };
                let mut first = [0u8; 16];
                read(input, &mut first)?;
                stream.apply_keystream(&mut first);
                let length = Self::checked_length([first[0], first[1], first[2], first[3]], block, false)?;
                if length + 4 < 16 {
                    return Err(Fault::Io);
                }
                packet = first.to_vec();
                packet.resize(4 + length, 0);
                read(input, &mut packet[16..])?;
                stream.apply_keystream(&mut packet[16..]);
                let mut tag = vec![0u8; tag_len];
                read(input, &mut tag)?;
                let Some((kind, key, _)) = &self.mac else { return Err(Fault::Io) };
                if !same(&kind.compute(key, seq, &packet)?, &tag) {
                    return Err(Fault::Io);
                }
            }
        }
        let length = packet.len() - 4;
        let pad = usize::from(packet[4]);
        if pad + 2 > length {
            return Err(Fault::Io);
        }
        Ok(packet[5..4 + length - pad].to_vec())
    }
}

fn read(input: &mut impl Read, buf: &mut [u8]) -> Result<()> {
    input.read_exact(buf).map_err(|_| Fault::Io)
}

// ---- Keys ----

/// A host key, client key or signature, checked against the bytes it signs.
mod sig {
    use super::*;

    const SHA1_INFO: &[u8] = &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14];
    const SHA256_INFO: &[u8] =
        &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20];
    const SHA512_INFO: &[u8] =
        &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40];
    /// OpenSSH refuses smaller RSA keys, host or client.
    const RSA_MIN_BITS: u64 = 1024;

    /// EMSA-PKCS1-v1_5 (RFC 8017 section 9.2) of `data` for a `k`-byte modulus.
    pub(super) fn emsa(hash: RsaHash, data: &[u8], k: usize) -> Result<Vec<u8>> {
        let (info, digest) = match hash {
            RsaHash::Sha1 => (SHA1_INFO, Sha1::digest(data).to_vec()),
            RsaHash::Sha256 => (SHA256_INFO, Sha256::digest(data).to_vec()),
            RsaHash::Sha512 => (SHA512_INFO, Sha512::digest(data).to_vec()),
        };
        let t_len = info.len() + digest.len();
        if k < t_len + 11 {
            return Err(Fault::Io);
        }
        let mut out = Vec::with_capacity(k);
        out.extend_from_slice(&[0, 1]);
        out.resize(k - t_len - 1, 0xff);
        out.push(0);
        out.extend_from_slice(info);
        out.extend_from_slice(&digest);
        Ok(out)
    }

    pub(super) fn rsa_verify(n: &BigUint, e: &BigUint, hash: RsaHash, data: &[u8], signature: &[u8]) -> bool {
        if n.bits() < RSA_MIN_BITS || e <= &BigUint::from(1u8) {
            return false;
        }
        let k = n.bits().div_ceil(8) as usize;
        if signature.len() > k {
            return false;
        }
        let s = BigUint::from_bytes_be(signature);
        if &s >= n {
            return false;
        }
        let Ok(em) = left_pad(&s.modpow(e, n).to_bytes_be(), k) else { return false };
        emsa(hash, data, k).is_ok_and(|expected| expected == em)
    }

    /// Checks a host key's `signature` blob over `data`. A key that does not sign what it
    /// shows is someone else holding a copy of the public half: [`Fault::Unsafe`].
    pub(super) fn verify(kind: SigKind, key_blob: &[u8], signature: &[u8], data: &[u8]) -> Result<()> {
        let mut key = Parse::new(key_blob);
        if key.text()? != kind.key_type() {
            return Err(Fault::Io);
        }
        let mut sig = Parse::new(signature);
        if sig.text()? != kind.name() {
            return Err(Fault::Io);
        }
        let bytes = sig.bytes()?;
        let good = match kind {
            SigKind::Ed25519 => {
                let public: [u8; 32] = key.bytes()?.try_into().map_err(|_| Fault::Io)?;
                let bytes: [u8; 64] = bytes.try_into().map_err(|_| Fault::Io)?;
                let Ok(verifying) = ed25519_dalek::VerifyingKey::from_bytes(&public) else { return Err(Fault::Io) };
                verifying.verify_strict(data, &ed25519_dalek::Signature::from_bytes(&bytes)).is_ok()
            }
            SigKind::EcdsaP256 => {
                use p256::ecdsa::signature::Verifier;
                if key.text()? != "nistp256" {
                    return Err(Fault::Io);
                }
                let Ok(verifying) = p256::ecdsa::VerifyingKey::from_sec1_bytes(key.bytes()?) else {
                    return Err(Fault::Io);
                };
                let mut inner = Parse::new(bytes);
                let mut rs = left_pad(inner.mpint()?, 32)?;
                rs.extend_from_slice(&left_pad(inner.mpint()?, 32)?);
                match p256::ecdsa::Signature::from_slice(&rs) {
                    Ok(parsed) => verifying.verify(data, &parsed).is_ok(),
                    Err(_) => false,
                }
            }
            SigKind::Rsa(hash) => {
                let e = BigUint::from_bytes_be(key.mpint()?);
                let n = BigUint::from_bytes_be(key.mpint()?);
                rsa_verify(&n, &e, hash, data, bytes)
            }
        };
        if good { Ok(()) } else { Err(Fault::Unsafe) }
    }
}

struct RsaKey {
    n: BigUint,
    e: BigUint,
    d: BigUint,
    p: BigUint,
    q: BigUint,
    iqmp: BigUint,
}

impl RsaKey {
    /// A key whose parts fit together. A damaged one (a zero prime, a modulus that is not
    /// their product) would otherwise divide by zero in signing, or loop for ever looking
    /// for a blinding factor.
    fn new(n: BigUint, e: BigUint, d: BigUint, p: BigUint, q: BigUint, iqmp: BigUint) -> Result<Self> {
        let one = BigUint::from(1u8);
        if p <= one || q <= one || e <= one || &p * &q != n {
            return Err(Fault::Io);
        }
        Ok(Self { n, e, d, p, q, iqmp })
    }

    /// PKCS#1 v1.5 signing, with the CRT for speed and blinding so the time it takes says
    /// nothing about the key; the result is checked with the public half before use.
    fn sign(&self, hash: RsaHash, data: &[u8]) -> Result<Vec<u8>> {
        let k = self.n.bits().div_ceil(8) as usize;
        let m = BigUint::from_bytes_be(&sig::emsa(hash, data, k)?);
        let one = BigUint::from(1u8);
        let (r, r_inv) = loop {
            let r = BigUint::from_bytes_be(&random(k)?) % &self.n;
            if r > one
                && let Some(inverse) = r.modinv(&self.n)
            {
                break (r, inverse);
            }
        };
        let blinded = (&m * r.modpow(&self.e, &self.n)) % &self.n;
        let m1 = blinded.modpow(&(&self.d % (&self.p - &one)), &self.p);
        let m2 = blinded.modpow(&(&self.d % (&self.q - &one)), &self.q);
        let diff = (&m1 + &self.p - (&m2 % &self.p)) % &self.p;
        let h = (&self.iqmp * diff) % &self.p;
        let s = ((m2 + h * &self.q) * r_inv) % &self.n;
        if s.modpow(&self.e, &self.n) != m {
            return Err(Fault::PermissionLost);
        }
        left_pad(&s.to_bytes_be(), k)
    }
}

/// A private key that signs in.
enum ClientKey {
    Ed25519 { seed: Zeroizing<[u8; 32]>, public: [u8; 32] },
    Rsa(Box<RsaKey>),
    P256 { signing: Box<p256::ecdsa::SigningKey>, point: Vec<u8> },
}

impl ClientKey {
    fn blob(&self) -> Vec<u8> {
        match self {
            Self::Ed25519 { public, .. } => Wire::default().text("ssh-ed25519").bytes(public).0,
            Self::Rsa(key) => Wire::default().text("ssh-rsa").mpint(&key.e.to_bytes_be()).mpint(&key.n.to_bytes_be()).0,
            Self::P256 { point, .. } => Wire::default().text("ecdsa-sha2-nistp256").text("nistp256").bytes(point).0,
        }
    }

    /// The algorithms to try with this key, best first.
    fn algorithms(&self, server_sig_algs: Option<&[String]>, legacy: bool) -> Vec<SigKind> {
        match self {
            Self::Ed25519 { .. } => vec![SigKind::Ed25519],
            Self::P256 { .. } => vec![SigKind::EcdsaP256],
            Self::Rsa(_) => {
                let mut all = vec![SigKind::Rsa(RsaHash::Sha512), SigKind::Rsa(RsaHash::Sha256)];
                if legacy {
                    all.push(SigKind::Rsa(RsaHash::Sha1));
                }
                match server_sig_algs {
                    // The server said what it takes: only those, so no attempt is wasted.
                    Some(listed) => all.into_iter().filter(|kind| listed.iter().any(|name| name == kind.name())).collect(),
                    None => all,
                }
            }
        }
    }

    fn sign(&self, kind: SigKind, data: &[u8]) -> Result<Vec<u8>> {
        let bytes = match (self, kind) {
            (Self::Ed25519 { seed, .. }, SigKind::Ed25519) => {
                use ed25519_dalek::Signer;
                ed25519_dalek::SigningKey::from_bytes(seed).sign(data).to_bytes().to_vec()
            }
            (Self::P256 { signing, .. }, SigKind::EcdsaP256) => {
                use p256::ecdsa::signature::Signer;
                let signature: p256::ecdsa::Signature = signing.sign(data);
                let (r, s) = signature.split_bytes();
                Wire::default().mpint(&r).mpint(&s).0
            }
            (Self::Rsa(key), SigKind::Rsa(hash)) => key.sign(hash, data)?,
            _ => return Err(Fault::PermissionLost),
        };
        Ok(Wire::default().text(kind.name()).bytes(&bytes).0)
    }
}

/// The base64 inside a PEM armour, header lines skipped.
fn pem_body(text: &str, label: &str) -> Option<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = text.find(&begin)? + begin.len();
    let stop = start + text[start..].find(&end)?;
    let body: String = text[start..stop]
        .lines()
        .filter(|line| !line.contains(':'))
        .flat_map(|line| line.chars().filter(|c| !c.is_whitespace()))
        .collect();
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

/// Parses a pasted private key. OpenSSH's own format (what `ssh-keygen` writes since 7.8),
/// plain or under a passphrase, and an unencrypted PKCS#1 RSA key in PEM. Anything else,
/// and a wrong passphrase, is [`Fault::PermissionLost`]: JSch's "invalid privatekey".
fn parse_private_key(text: &str, passphrase: &str) -> Result<ClientKey> {
    let refused = |_| Fault::PermissionLost;
    if let Some(body) = pem_body(text, "OPENSSH PRIVATE KEY") {
        return parse_openssh_key(&body, passphrase).map_err(refused);
    }
    if text.contains("BEGIN RSA PRIVATE KEY") && !text.contains("ENCRYPTED") {
        let body = pem_body(text, "RSA PRIVATE KEY").ok_or(Fault::PermissionLost)?;
        return parse_pkcs1(&body).map_err(refused);
    }
    Err(Fault::PermissionLost)
}

fn parse_openssh_key(body: &[u8], passphrase: &str) -> Result<ClientKey> {
    const MAGIC: &[u8] = b"openssh-key-v1\0";
    let rest = body.strip_prefix(MAGIC).ok_or(Fault::Io)?;
    let mut p = Parse::new(rest);
    let cipher = p.text()?;
    let kdf = p.text()?;
    let options = p.bytes()?;
    if p.u32()? != 1 {
        return Err(Fault::Io);
    }
    p.bytes()?; // the public key, again inside the private section
    let sealed = p.bytes()?;
    let mut plain = Zeroizing::new(sealed.to_vec());
    match (cipher, kdf) {
        ("none", "none") => {}
        ("aes256-ctr", "bcrypt") => {
            if passphrase.is_empty() {
                return Err(Fault::Io);
            }
            let mut opts = Parse::new(options);
            let salt = opts.bytes()?;
            let rounds = opts.u32()?;
            let mut derived = Zeroizing::new([0u8; 48]);
            bcrypt_pbkdf::bcrypt_pbkdf(passphrase.as_bytes(), salt, rounds, derived.as_mut()).map_err(|_| Fault::Io)?;
            let mut stream =
                ctr::Ctr128BE::<Aes256>::new_from_slices(&derived[..32], &derived[32..]).map_err(|_| Fault::Io)?;
            stream.apply_keystream(&mut plain);
        }
        _ => return Err(Fault::Io),
    }
    let mut k = Parse::new(&plain);
    // Two copies of one random number: unequal means the passphrase was wrong.
    if k.u32()? != k.u32()? {
        return Err(Fault::Io);
    }
    match k.text()? {
        "ssh-ed25519" => {
            let public: [u8; 32] = k.bytes()?.try_into().map_err(|_| Fault::Io)?;
            let secret = k.bytes()?;
            if secret.len() != 64 || secret[32..] != public {
                return Err(Fault::Io);
            }
            let mut seed = Zeroizing::new([0u8; 32]);
            seed.copy_from_slice(&secret[..32]);
            if ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().to_bytes() != public {
                return Err(Fault::Io);
            }
            Ok(ClientKey::Ed25519 { seed, public })
        }
        "ssh-rsa" => {
            let mut next = || k.mpint().map(BigUint::from_bytes_be);
            let (n, e, d, iqmp, p, q) = (next()?, next()?, next()?, next()?, next()?, next()?);
            Ok(ClientKey::Rsa(Box::new(RsaKey::new(n, e, d, p, q, iqmp)?)))
        }
        "ecdsa-sha2-nistp256" => {
            if k.text()? != "nistp256" {
                return Err(Fault::Io);
            }
            let point = k.bytes()?.to_vec();
            let scalar = Zeroizing::new(left_pad(k.mpint()?, 32)?);
            let signing = p256::ecdsa::SigningKey::from_slice(&scalar).map_err(|_| Fault::Io)?;
            let shown = p256::ecdsa::VerifyingKey::from_sec1_bytes(&point).map_err(|_| Fault::Io)?;
            if *signing.verifying_key() != shown {
                return Err(Fault::Io);
            }
            Ok(ClientKey::P256 { signing: Box::new(signing), point })
        }
        _ => Err(Fault::Io),
    }
}

/// One DER element of the expected tag; returns its contents.
fn der<'a>(data: &'a [u8], at: &mut usize, tag: u8) -> Result<&'a [u8]> {
    let mut p = Parse::new(data.get(*at..).ok_or(Fault::Io)?);
    if p.byte()? != tag {
        return Err(Fault::Io);
    }
    let first = p.byte()?;
    let length = if first < 0x80 {
        usize::from(first)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 {
            return Err(Fault::Io);
        }
        p.take(count)?.iter().fold(0usize, |acc, &b| (acc << 8) | usize::from(b))
    };
    let content = p.take(length)?;
    *at += p.at;
    Ok(content)
}

/// RFC 8017 appendix A.1.2: version, n, e, d, p, q, d mod (p-1), d mod (q-1), q^-1 mod p.
fn parse_pkcs1(body: &[u8]) -> Result<ClientKey> {
    let mut at = 0;
    let sequence = der(body, &mut at, 0x30)?;
    let mut at = 0;
    let mut ints = Vec::with_capacity(9);
    for _ in 0..9 {
        ints.push(BigUint::from_bytes_be(der(sequence, &mut at, 0x02)?));
    }
    let mut ints = ints.into_iter().skip(1);
    let mut next = || ints.next().ok_or(Fault::Io);
    let (n, e, d, p, q) = (next()?, next()?, next()?, next()?, next()?);
    next()?;
    next()?;
    let iqmp = next()?;
    Ok(ClientKey::Rsa(Box::new(RsaKey::new(n, e, d, p, q, iqmp)?)))
}

// ---- Host keys kept between connections ----

/// The blob in a stored `<type> <base64>` host key.
fn stored_blob(stored: &str) -> Option<Vec<u8>> {
    let (_, encoded) = stored.trim().split_once(' ')?;
    base64::engine::general_purpose::STANDARD.decode(encoded.split_whitespace().next()?).ok()
}

/// The key type a blob starts with.
fn blob_type(blob: &[u8]) -> Result<&str> {
    Parse::new(blob).text()
}

/// Fingerprint of a stored `<type> <base64>` key, as `ssh-keygen -l` prints it.
pub fn fingerprint(host_key: &str) -> Option<String> {
    let blob = stored_blob(host_key)?;
    Some(format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(&blob))))
}

// ---- Transport ----

/// The group 14 prime, RFC 3526 section 3. Its generator is 2.
const GROUP14: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A0879",
    "8E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B",
    "0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA4836",
    "1C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804",
    "F1746C08CA18217C32905E462E36CE3BE39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6",
    "955817183995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFFFFFFFFFF",
);

/// Derives one key (RFC 4253 section 7.2): HASH(K || H || letter || session_id), extended.
fn derive(hash: KexHash, k: &[u8], h: &[u8], letter: u8, session_id: &[u8], len: usize) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(hash.digest(&[k, h, &[letter], session_id]));
    while out.len() < len {
        let more = hash.digest(&[k, h, &out]);
        out.extend_from_slice(&more);
    }
    out.truncate(len);
    out
}

/// Keys for one direction from the derived material.
fn direction(
    hash: KexHash,
    k: &[u8],
    h: &[u8],
    session_id: &[u8],
    letters: [u8; 3],
    (cipher, mac): (CipherKind, Option<(MacKind, bool)>),
) -> Result<Direction> {
    let iv = derive(hash, k, h, letters[0], session_id, cipher.iv_len());
    let key = derive(hash, k, h, letters[1], session_id, cipher.key_len());
    let mac = mac.map(|(kind, etm)| (kind, derive(hash, k, h, letters[2], session_id, kind.len()), etm));
    Ok(Direction { cipher: Cipher::new(cipher, &key, &iv)?, mac, seq: 0 })
}

/// What a disconnect from the server means. Turned away during sign-in is a refusal,
/// whatever reason code the server chose for it, except too many connections (12), which
/// says nothing about the password: it is a server that is full for now.
fn disconnect_fault(payload: &[u8], authenticating: bool) -> Fault {
    const TOO_MANY_CONNECTIONS: u32 = 12;
    let reason = Parse::new(payload.get(1..).unwrap_or_default()).u32().unwrap_or(0);
    match reason {
        TOO_MANY_CONNECTIONS => Fault::Io,
        // No more methods to try, or a user name it will not take.
        13..=15 => Fault::PermissionLost,
        _ if authenticating => Fault::PermissionLost,
        _ => Fault::Io,
    }
}

struct Transport {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    send: Direction,
    recv: Direction,
    offer: Offer,
    server_version: Vec<u8>,
    session_id: Vec<u8>,
    /// The host key this connection is held to: the stored one, then the one seen.
    host_key: Option<Vec<u8>>,
    strict: bool,
    received: u64,
    since_kex: u64,
    /// Channel messages that arrived while keys were being changed.
    deferred: VecDeque<Vec<u8>>,
    server_sig_algs: Option<Vec<String>>,
    authenticating: bool,
}

impl Transport {
    fn write_raw(&mut self, payload: &[u8]) -> Result<()> {
        let packet = self.send.seal(payload)?;
        self.since_kex += packet.len() as u64;
        self.writer.write_all(&packet).map_err(|_| Fault::Io)
    }

    fn read_raw(&mut self) -> Result<Vec<u8>> {
        let payload = self.recv.open(&mut self.reader)?;
        self.received += 1;
        self.since_kex += payload.len() as u64;
        Ok(payload)
    }

    /// Sends one message, changing keys first when enough has crossed under these.
    fn write(&mut self, payload: &[u8]) -> Result<()> {
        if self.since_kex >= rekey_bytes() {
            self.kex(None)?;
        }
        self.write_raw(payload)
    }

    /// The next message for the layers above, with the transport's own handled here.
    fn next(&mut self) -> Result<Vec<u8>> {
        loop {
            let payload = match self.deferred.pop_front() {
                Some(payload) => payload,
                None => self.read_raw()?,
            };
            match payload.first().copied() {
                Some(IGNORE | DEBUG | UNIMPLEMENTED) => {}
                Some(DISCONNECT) => return Err(disconnect_fault(&payload, self.authenticating)),
                Some(EXT_INFO) => self.ext_info(&payload)?,
                Some(KEXINIT) => self.kex(Some(payload))?,
                Some(_) => return Ok(payload),
                None => return Err(Fault::Io),
            }
        }
    }

    /// RFC 8308: which signature algorithms the server takes for user keys.
    fn ext_info(&mut self, payload: &[u8]) -> Result<()> {
        let mut p = Parse::new(payload);
        p.byte()?;
        for _ in 0..p.u32()? {
            let name = p.text()?;
            let value = p.bytes()?;
            if name == "server-sig-algs" {
                let list = std::str::from_utf8(value).map_err(|_| Fault::Io)?;
                self.server_sig_algs = Some(list.split(',').map(str::to_owned).collect());
            }
        }
        Ok(())
    }

    /// The next key exchange message. In the first exchange under strict kex, anything
    /// else ends the connection (that is the Terrapin fix); in a later one, channel
    /// traffic still in flight waits in [`Self::deferred`].
    fn kex_packet(&mut self, initial: bool) -> Result<Vec<u8>> {
        loop {
            let payload = self.read_raw()?;
            let kind = payload.first().copied().ok_or(Fault::Io)?;
            match kind {
                DISCONNECT => return Err(disconnect_fault(&payload, false)),
                IGNORE | DEBUG | UNIMPLEMENTED if !(initial && self.strict) => {}
                KEXINIT | NEWKEYS | KEX_INIT..=49 => return Ok(payload),
                USERAUTH_REQUEST.. if !initial => self.deferred.push_back(payload),
                _ => return Err(Fault::Io),
            }
        }
    }

    fn expect_kex(&mut self, initial: bool, kind: u8) -> Result<Vec<u8>> {
        let payload = self.kex_packet(initial)?;
        if payload.first() == Some(&kind) { Ok(payload) } else { Err(Fault::Io) }
    }

    /// One key exchange, first or later. `server_init` is the server's KEXINIT when it
    /// started this one.
    fn kex(&mut self, server_init: Option<Vec<u8>>) -> Result<()> {
        let initial = self.session_id.is_empty();
        let ours = self.offer.kexinit(initial)?;
        self.write_raw(&ours)?;
        let theirs = match server_init {
            Some(payload) => payload,
            None => self.expect_kex(initial, KEXINIT)?,
        };
        let chosen = negotiate(&self.offer, &theirs)?;
        if initial {
            self.strict = chosen.strict;
            // Strict kex: the server's KEXINIT must be the very first packet.
            if self.strict && self.received != 1 {
                return Err(Fault::Io);
            }
        }
        if chosen.skip_guess {
            self.kex_packet(initial)?;
        }
        let hash = match chosen.kex {
            KexKind::Curve25519 => KexHash::Sha256,
            KexKind::Group14(hash) => hash,
        };
        let prefix = Wire::default()
            .bytes(CLIENT_VERSION)
            .bytes(&self.server_version)
            .bytes(&ours)
            .bytes(&theirs);
        let (k, h, blob, signature) = match chosen.kex {
            KexKind::Curve25519 => {
                let mut secret_bytes = Zeroizing::new([0u8; 32]);
                getrandom::fill(secret_bytes.as_mut()).map_err(|_| Fault::Io)?;
                let secret = x25519_dalek::StaticSecret::from(*secret_bytes);
                let public = x25519_dalek::PublicKey::from(&secret);
                self.write_raw(&Wire::msg(KEX_INIT).bytes(public.as_bytes()).0)?;
                let reply = self.expect_kex(initial, KEX_REPLY)?;
                let mut p = Parse::new(&reply[1..]);
                let blob = p.bytes()?.to_vec();
                let server_public: [u8; 32] = p.bytes()?.try_into().map_err(|_| Fault::Io)?;
                let signature = p.bytes()?.to_vec();
                let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(server_public));
                if !shared.was_contributory() {
                    return Err(Fault::Io);
                }
                let k = Zeroizing::new(Wire::default().mpint(shared.as_bytes()).0);
                let h = hash.digest(&[
                    &Wire(prefix.0).bytes(&blob).bytes(public.as_bytes()).bytes(&server_public).0,
                    &k,
                ]);
                (k, h, blob, signature)
            }
            KexKind::Group14(_) => {
                let prime = BigUint::parse_bytes(GROUP14.as_bytes(), 16).ok_or(Fault::Io)?;
                let x = BigUint::from_bytes_be(&Zeroizing::new(random(64)?));
                let e = BigUint::from(2u8).modpow(&x, &prime);
                let e_bytes = e.to_bytes_be();
                self.write_raw(&Wire::msg(KEX_INIT).mpint(&e_bytes).0)?;
                let reply = self.expect_kex(initial, KEX_REPLY)?;
                let mut p = Parse::new(&reply[1..]);
                let blob = p.bytes()?.to_vec();
                let f_bytes = p.mpint()?.to_vec();
                let signature = p.bytes()?.to_vec();
                let f = BigUint::from_bytes_be(&f_bytes);
                let one = BigUint::from(1u8);
                if f <= one || f >= &prime - &one {
                    return Err(Fault::Io);
                }
                let shared = Zeroizing::new(f.modpow(&x, &prime).to_bytes_be());
                let k = Zeroizing::new(Wire::default().mpint(&shared).0);
                let h = hash.digest(&[&Wire(prefix.0).bytes(&blob).mpint(&e_bytes).mpint(&f_bytes).0, &k]);
                (k, h, blob, signature)
            }
        };
        // Which key it is first, then whether it signed: a stranger's key is refused
        // before anything is computed with it.
        if blob_type(&blob)? != chosen.host_key.key_type() {
            return Err(Fault::Io);
        }
        if let Some(trusted) = &self.host_key
            && *trusted != blob
        {
            return Err(Fault::Unsafe);
        }
        sig::verify(chosen.host_key, &blob, &signature, &h)?;
        self.host_key = Some(blob);
        if initial {
            self.session_id = h.clone();
        }
        let session_id = self.session_id.clone();
        // Strict kex starts both sequence numbers again at every NEWKEYS; otherwise they run
        // on across the exchange, as RFC 4253 has it.
        self.write_raw(&[NEWKEYS])?;
        let seq = if self.strict { 0 } else { self.send.seq };
        self.send = direction(hash, &k, &h, &session_id, *b"ACE", chosen.c2s)?;
        self.send.seq = seq;
        self.expect_kex(initial, NEWKEYS)?;
        let seq = if self.strict { 0 } else { self.recv.seq };
        self.recv = direction(hash, &k, &h, &session_id, *b"BDF", chosen.s2c)?;
        self.recv.seq = seq;
        self.since_kex = 0;
        Ok(())
    }
}

// ---- Sessions ----

/// Who to connect to and how to sign in. Borrowed from the destination's config.
#[derive(Clone, Copy)]
pub struct Login<'a> {
    /// Server name or address.
    pub host: &'a str,
    /// TCP port, 22 by default.
    pub port: u16,
    /// User name.
    pub user: &'a str,
    /// Empty when the key signs in. Also answers keyboard-interactive prompts.
    pub password: &'a Secret,
    /// An OpenSSH or PKCS#1 PEM private key, pasted whole. Tried before the password.
    pub private_key: &'a Secret,
    /// Unlocks the private key.
    pub key_passphrase: &'a Secret,
    /// The trusted host key as `<type> <base64>`, or empty to take the first one shown.
    pub host_key: &'a str,
    /// Also offers SHA-1 signatures, SHA-1 MACs and the SHA-1 group.
    pub legacy_algorithms: bool,
}

impl std::fmt::Debug for Login<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("pinned", &!self.host_key.is_empty())
            .field("legacy_algorithms", &self.legacy_algorithms)
            .finish_non_exhaustive()
    }
}

struct ChannelState {
    remote: u32,
    remote_window: u64,
    remote_max: usize,
    local_window: u32,
    inbound: VecDeque<u8>,
    eof: bool,
}

/// A signed-in SSH connection with at most one channel open.
pub struct Session {
    transport: Transport,
    channel: Option<ChannelState>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("strict_kex", &self.transport.strict).finish_non_exhaustive()
    }
}

fn dial(host: &str, port: u16) -> Result<TcpStream> {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    let addresses = (host, port).to_socket_addrs().map_err(|_| Fault::Io)?;
    for address in addresses {
        if let Ok(stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            stream.set_read_timeout(Some(READ_TIMEOUT)).map_err(|_| Fault::Io)?;
            stream.set_write_timeout(Some(READ_TIMEOUT)).map_err(|_| Fault::Io)?;
            stream.set_nodelay(true).map_err(|_| Fault::Io)?;
            return Ok(stream);
        }
    }
    Err(Fault::Io)
}

/// Sends ours, then reads lines until the server's version (RFC 4253 section 4.2).
fn exchange_versions(reader: &mut BufReader<TcpStream>, writer: &mut TcpStream) -> Result<Vec<u8>> {
    let mut line = CLIENT_VERSION.to_vec();
    line.extend_from_slice(b"\r\n");
    writer.write_all(&line).map_err(|_| Fault::Io)?;
    for _ in 0..MAX_BANNER_LINES {
        let mut line = Vec::new();
        reader.by_ref().take(MAX_LINE as u64).read_until(b'\n', &mut line).map_err(|_| Fault::Io)?;
        if !line.ends_with(b"\n") {
            return Err(Fault::Io);
        }
        while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            line.pop();
        }
        if line.starts_with(b"SSH-") {
            return if line.starts_with(b"SSH-2.0-") || line.starts_with(b"SSH-1.99-") { Ok(line) } else { Err(Fault::Io) };
        }
    }
    Err(Fault::Io)
}

enum AuthReply {
    Success,
    Failure(Vec<String>),
    Sixty(Vec<u8>),
}

impl Session {
    /// Connects, checks the host key, and signs in. The password is used only when there is
    /// no key or the key is refused, which is what `ssh` does too.
    pub fn connect(policy: NetPolicy, login: &Login<'_>) -> Result<Self> {
        wmpass_net::http::check_socket(policy).map_err(|_| Fault::Blocked)?;
        if login.host.trim().is_empty()
            || login.user.is_empty()
            || (login.password.is_empty() && login.private_key.is_empty())
        {
            return Err(Fault::NotConfigured);
        }
        let key = if login.private_key.is_empty() {
            None
        } else {
            Some(parse_private_key(login.private_key.expose().trim(), login.key_passphrase.expose())?)
        };
        let pinned = if login.host_key.trim().is_empty() {
            None
        } else {
            // A stored key that does not read is held to, too: nothing will match it.
            Some(stored_blob(login.host_key).unwrap_or_default())
        };
        let pinned_type = pinned.as_deref().and_then(|blob| blob_type(blob).ok()).map(str::to_owned);
        let mut writer = dial(login.host, login.port)?;
        let mut reader = BufReader::new(writer.try_clone().map_err(|_| Fault::Io)?);
        let server_version = exchange_versions(&mut reader, &mut writer)?;
        let mut transport = Transport {
            reader,
            writer,
            send: Direction::plain(),
            recv: Direction::plain(),
            offer: Offer { legacy: login.legacy_algorithms, pinned_type },
            server_version,
            session_id: Vec::new(),
            host_key: pinned,
            strict: false,
            received: 0,
            since_kex: 0,
            deferred: VecDeque::new(),
            server_sig_algs: None,
            authenticating: false,
        };
        transport.kex(None)?;
        let mut session = Self { transport, channel: None };
        session.transport.authenticating = true;
        session.authenticate(login, key.as_ref())?;
        session.transport.authenticating = false;
        Ok(session)
    }

    /// The server's key as `<type> <base64>`, for the caller to keep.
    pub fn host_key(&self) -> String {
        let blob = self.transport.host_key.as_deref().unwrap_or_default();
        let kind = blob_type(blob).unwrap_or_default();
        format!("{kind} {}", base64::engine::general_purpose::STANDARD.encode(blob))
    }

    fn auth_reply(&mut self) -> Result<AuthReply> {
        loop {
            let reply = self.transport.next()?;
            match reply.first().copied() {
                Some(USERAUTH_BANNER) => {}
                Some(USERAUTH_SUCCESS) => return Ok(AuthReply::Success),
                Some(USERAUTH_FAILURE) => {
                    let mut p = Parse::new(&reply[1..]);
                    return Ok(AuthReply::Failure(p.names()?.into_iter().map(str::to_owned).collect()));
                }
                Some(USERAUTH_60) => return Ok(AuthReply::Sixty(reply)),
                Some(GLOBAL_REQUEST) => self.global_request(&reply)?,
                _ => return Err(Fault::Io),
            }
        }
    }

    fn request(user: &str, method: &str) -> Wire {
        Wire::msg(USERAUTH_REQUEST).text(user).text("ssh-connection").text(method)
    }

    fn authenticate(&mut self, login: &Login<'_>, key: Option<&ClientKey>) -> Result<()> {
        self.transport.write(&Wire::msg(SERVICE_REQUEST).text("ssh-userauth").0)?;
        let accepted = self.transport.next()?;
        if accepted.first() != Some(&SERVICE_ACCEPT) {
            return Err(Fault::Io);
        }
        self.transport.write(&Self::request(login.user, "none").0)?;
        let mut methods = match self.auth_reply()? {
            AuthReply::Success => return Ok(()),
            AuthReply::Failure(methods) => methods,
            AuthReply::Sixty(_) => return Err(Fault::Io),
        };
        let offered = |methods: &[String], name: &str| methods.iter().any(|m| m == name);
        if let Some(key) = key
            && offered(&methods, "publickey")
        {
            let blob = key.blob();
            let algorithms =
                key.algorithms(self.transport.server_sig_algs.as_deref(), self.transport.offer.legacy);
            for kind in algorithms {
                let unsigned = Self::request(login.user, "publickey").bool(true).text(kind.name()).bytes(&blob);
                let signed_data = Wire::default().bytes(&self.transport.session_id).raw(&unsigned.0).0;
                let signature = key.sign(kind, &signed_data)?;
                self.transport.write(&unsigned.bytes(&signature).0)?;
                match self.auth_reply()? {
                    AuthReply::Success => return Ok(()),
                    AuthReply::Failure(next) => methods = next,
                    AuthReply::Sixty(_) => return Err(Fault::Io),
                }
                if !offered(&methods, "publickey") {
                    break;
                }
            }
        }
        let password = login.password.expose();
        if password.is_empty() {
            return Err(Fault::PermissionLost);
        }
        if offered(&methods, "keyboard-interactive") {
            match self.keyboard_interactive(login.user, password)? {
                Interactive::SignedIn => return Ok(()),
                Interactive::Refused => return Err(Fault::PermissionLost),
                Interactive::Unanswerable(next) => methods = next.unwrap_or(methods),
            }
        }
        if offered(&methods, "password") {
            self.transport.write(&Self::request(login.user, "password").bool(false).text(password).0)?;
            if let AuthReply::Success = self.auth_reply()? {
                return Ok(());
            }
        }
        Err(Fault::PermissionLost)
    }

    /// RFC 4256. A hidden prompt is the password; anything shown in the clear is a question
    /// this app cannot answer, so the method is left for plain password sign-in.
    fn keyboard_interactive(&mut self, user: &str, password: &str) -> Result<Interactive> {
        self.transport.write(&Self::request(user, "keyboard-interactive").text("").text("").0)?;
        let mut answered = false;
        // A server may send prompt-less rounds; a bound keeps a broken one from looping.
        for _ in 0..8 {
            let info = match self.auth_reply()? {
                AuthReply::Success => return Ok(Interactive::SignedIn),
                AuthReply::Failure(_) if answered => return Ok(Interactive::Refused),
                AuthReply::Failure(methods) => return Ok(Interactive::Unanswerable(Some(methods))),
                AuthReply::Sixty(info) => info,
            };
            let mut p = Parse::new(&info[1..]);
            p.text()?; // name
            p.text()?; // instruction
            p.text()?; // language
            let count = p.u32()?;
            let mut hidden = true;
            for _ in 0..count {
                p.text()?;
                hidden &= !p.bool()?;
            }
            if count > 0 && (!hidden || answered) {
                // Asked again after the password went: it was wrong, or this is a second
                // factor. Either way the password cannot help further.
                return Ok(if answered { Interactive::Refused } else { Interactive::Unanswerable(None) });
            }
            let mut response = Wire::msg(USERAUTH_INFO_RESPONSE).u32(count);
            for _ in 0..count {
                response = response.text(password);
            }
            answered |= count > 0;
            self.transport.write(&response.0)?;
        }
        Ok(Interactive::Refused)
    }

    fn global_request(&mut self, message: &[u8]) -> Result<()> {
        let mut p = Parse::new(&message[1..]);
        p.text()?;
        if p.bool()? {
            self.transport.write(&[REQUEST_FAILURE])?;
        }
        Ok(())
    }

    /// Handles channel bookkeeping; returns any message it does not consume.
    fn dispatch(&mut self, message: Vec<u8>) -> Result<Option<Vec<u8>>> {
        let kind = message.first().copied().ok_or(Fault::Io)?;
        if kind == GLOBAL_REQUEST {
            self.global_request(&message)?;
            return Ok(None);
        }
        let Some(channel) = self.channel.as_mut() else { return Ok(Some(message)) };
        let mut p = Parse::new(&message[1..]);
        match kind {
            CHANNEL_WINDOW_ADJUST => {
                p.u32()?;
                channel.remote_window = (channel.remote_window + u64::from(p.u32()?)).min(u64::from(u32::MAX));
            }
            CHANNEL_DATA | CHANNEL_EXTENDED_DATA => {
                p.u32()?;
                if kind == CHANNEL_EXTENDED_DATA {
                    p.u32()?;
                }
                let data = p.bytes()?;
                let length = u32::try_from(data.len()).map_err(|_| Fault::Io)?;
                channel.local_window = channel.local_window.checked_sub(length).ok_or(Fault::Io)?;
                // Standard error from the server is counted against the window and dropped.
                if kind == CHANNEL_DATA {
                    channel.inbound.extend(data);
                }
                if channel.local_window < WINDOW / 2 {
                    let grant = WINDOW - channel.local_window;
                    channel.local_window = WINDOW;
                    let remote = channel.remote;
                    self.transport.write(&Wire::msg(CHANNEL_WINDOW_ADJUST).u32(remote).u32(grant).0)?;
                }
            }
            CHANNEL_EOF | CHANNEL_CLOSE => channel.eof = true,
            CHANNEL_REQUEST => {
                p.u32()?;
                p.text()?;
                if p.bool()? {
                    let remote = channel.remote;
                    self.transport.write(&Wire::msg(CHANNEL_FAILURE).u32(remote).0)?;
                }
            }
            _ => return Ok(Some(message)),
        }
        Ok(None)
    }

    /// The next message that is not channel bookkeeping.
    fn next_unhandled(&mut self) -> Result<Vec<u8>> {
        loop {
            let message = self.transport.next()?;
            if let Some(message) = self.dispatch(message)? {
                return Ok(message);
            }
        }
    }

    /// Opens a session channel and starts `name` on it ("sftp").
    pub fn open_subsystem(&mut self, name: &str) -> Result<()> {
        if self.channel.is_some() {
            return Err(Fault::Io);
        }
        self.transport.write(&Wire::msg(CHANNEL_OPEN).text("session").u32(0).u32(WINDOW).u32(CHANNEL_PACKET).0)?;
        let reply = self.next_unhandled()?;
        let mut p = Parse::new(&reply[1..]);
        match reply[0] {
            CHANNEL_OPEN_CONFIRMATION => {
                if p.u32()? != 0 {
                    return Err(Fault::Io);
                }
                let remote = p.u32()?;
                let remote_window = u64::from(p.u32()?);
                let remote_max = (p.u32()? as usize).clamp(1, MAX_PACKET / 2);
                self.channel = Some(ChannelState {
                    remote,
                    remote_window,
                    remote_max,
                    local_window: WINDOW,
                    inbound: VecDeque::new(),
                    eof: false,
                });
                self.transport.write(&Wire::msg(CHANNEL_REQUEST).u32(remote).text("subsystem").bool(true).text(name).0)?;
                match self.next_unhandled()?.first() {
                    Some(&CHANNEL_SUCCESS) => Ok(()),
                    _ => Err(Fault::Io),
                }
            }
            CHANNEL_OPEN_FAILURE => {
                p.u32()?;
                // 1 is SSH_OPEN_ADMINISTRATIVELY_PROHIBITED: this account may not do it.
                Err(if p.u32()? == 1 { Fault::PermissionLost } else { Fault::Io })
            }
            _ => Err(Fault::Io),
        }
    }

    /// Sends bytes on the channel, waiting for window as needed.
    pub fn send(&mut self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let (remote, window, max) = match &self.channel {
                Some(c) if c.eof => return Err(Fault::Io),
                Some(c) => (c.remote, c.remote_window, c.remote_max),
                None => return Err(Fault::Io),
            };
            if window == 0 {
                // One message at a time: the window adjustment is handled in there.
                let message = self.transport.next()?;
                self.dispatch(message)?;
                continue;
            }
            let n = data.len().min(max).min(usize::try_from(window).unwrap_or(usize::MAX));
            self.transport.write(&Wire::msg(CHANNEL_DATA).u32(remote).bytes(&data[..n]).0)?;
            if let Some(channel) = self.channel.as_mut() {
                channel.remote_window -= n as u64;
            }
            data = &data[n..];
        }
        Ok(())
    }

    /// Fills `buf` from the channel. The channel ending first is [`Fault::Io`].
    pub fn recv(&mut self, buf: &mut [u8]) -> Result<()> {
        loop {
            let Some(channel) = self.channel.as_mut() else { return Err(Fault::Io) };
            if channel.inbound.len() >= buf.len() {
                let wanted = buf.len();
                for (slot, byte) in buf.iter_mut().zip(channel.inbound.drain(..wanted)) {
                    *slot = byte;
                }
                return Ok(());
            }
            if channel.eof {
                return Err(Fault::Io);
            }
            let message = self.transport.next()?;
            self.dispatch(message)?;
        }
    }

    /// Closes the channel and the connection. Best effort: nothing is waited for.
    pub fn close(mut self) {
        if let Some(channel) = self.channel.take() {
            let _ = self.transport.write(&Wire::msg(CHANNEL_CLOSE).u32(channel.remote).0);
        }
        let _ = self.transport.write(&Wire::msg(DISCONNECT).u32(11).text("done").text("").0);
        let _ = self.transport.writer.shutdown(std::net::Shutdown::Both);
    }
}

enum Interactive {
    SignedIn,
    Refused,
    /// No answer was given; the methods the server named when it said so, if it did.
    Unanswerable(Option<Vec<String>>),
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
