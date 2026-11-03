//! The OpenPGP Proton Drive needs, written by hand over RustCrypto and dalek primitives.
//!
//! Proton Drive keeps everything as OpenPGP: each file and folder has a key of its own
//! whose passphrase is encrypted to its parent's key, names are messages encrypted to
//! the parent and signed by the user's address key, and file content is one session
//! key's worth of encrypted blocks. What is here is that much and no more, in the shapes
//! Proton's own SDK produces: v4 keys on Curve25519 (EdDSA for signing, ECDH for
//! encryption), SEIPD version 1, AES-256. Old accounts whose user and address keys are
//! RSA can still decrypt and sign.
//!
//! Signatures are made everywhere Proton asks for one, and not checked on what comes
//! back: the sink only reads what it wrote itself, over TLS, and a name that does not
//! decrypt is skipped either way.

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{Fault, Result};

const AES_BLOCK: usize = 16;
const AES_256: u8 = 9;
const AES_128: u8 = 7;
const SHA_256: u8 = 8;
const SHA_512: u8 = 10;
const RSA: u8 = 1;
const RSA_ENCRYPT: u8 = 2;
const RSA_SIGN: u8 = 3;
const ECDH: u8 = 18;
const EDDSA_LEGACY: u8 = 22;
const TAG_PKESK: u8 = 1;
const TAG_SIGNATURE: u8 = 2;
const TAG_ONE_PASS: u8 = 4;
const TAG_SECRET_KEY: u8 = 5;
const TAG_PUBLIC_KEY: u8 = 6;
const TAG_SECRET_SUBKEY: u8 = 7;
const TAG_COMPRESSED: u8 = 8;
const TAG_LITERAL: u8 = 11;
const TAG_USER_ID: u8 = 13;
const TAG_PUBLIC_SUBKEY: u8 = 14;
const TAG_SEIPD: u8 = 18;
const S2K_COUNT: u8 = 0x60;
const MDC_HEADER: [u8; 2] = [0xD3, 0x14];
/// Most a compressed packet may grow to: a Drive block is 4 MiB.
const INFLATE_LIMIT: usize = 64 << 20;
const OID_ED25519: &[u8] = &[0x2B, 0x06, 0x01, 0x04, 0x01, 0xDA, 0x47, 0x0F, 0x01];
const OID_CURVE25519: &[u8] = &[0x2B, 0x06, 0x01, 0x04, 0x01, 0x97, 0x55, 0x01, 0x05, 0x01];
/// DER prefix of a SHA-256 DigestInfo, for RSA signatures.
const SHA256_INFO: &[u8] = &[
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20,
];

// ---------------------------------------------------------------------------------------
// Primitives.

enum Aes {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
}

impl Aes {
    fn key_len(algorithm: u8) -> Option<usize> {
        match algorithm {
            7 => Some(16),
            8 => Some(24),
            9 => Some(32),
            _ => None,
        }
    }

    fn new(algorithm: u8, key: &[u8]) -> Option<Self> {
        if Self::key_len(algorithm)? != key.len() {
            return None;
        }
        Some(match algorithm {
            7 => Self::A128(aes::Aes128::new_from_slice(key).ok()?),
            8 => Self::A192(aes::Aes192::new_from_slice(key).ok()?),
            _ => Self::A256(aes::Aes256::new_from_slice(key).ok()?),
        })
    }

    fn encrypt(&self, block: &mut [u8; AES_BLOCK]) {
        let mut b = aes::cipher::Array::from(*block);
        match self {
            Self::A128(c) => c.encrypt_block(&mut b),
            Self::A192(c) => c.encrypt_block(&mut b),
            Self::A256(c) => c.encrypt_block(&mut b),
        }
        block.copy_from_slice(&b);
    }

    fn decrypt(&self, block: &mut [u8; AES_BLOCK]) {
        let mut b = aes::cipher::Array::from(*block);
        match self {
            Self::A128(c) => c.decrypt_block(&mut b),
            Self::A192(c) => c.decrypt_block(&mut b),
            Self::A256(c) => c.decrypt_block(&mut b),
        }
        block.copy_from_slice(&b);
    }
}

/// OpenPGP's CFB: plain CFB-128, no resynchronisation, which is what SEIPD and secret
/// keys use.
fn cfb_encrypt(cipher: &Aes, iv: &[u8; AES_BLOCK], data: &mut [u8]) {
    let mut register = *iv;
    for chunk in data.chunks_mut(AES_BLOCK) {
        let mut stream = register;
        cipher.encrypt(&mut stream);
        for (byte, key) in chunk.iter_mut().zip(stream) {
            *byte ^= key;
        }
        register[..chunk.len()].copy_from_slice(chunk);
    }
}

fn cfb_decrypt(cipher: &Aes, iv: &[u8; AES_BLOCK], data: &mut [u8]) {
    let mut register = *iv;
    for chunk in data.chunks_mut(AES_BLOCK) {
        let mut stream = register;
        cipher.encrypt(&mut stream);
        register[..chunk.len()].copy_from_slice(chunk);
        for (byte, key) in chunk.iter_mut().zip(stream) {
            *byte ^= key;
        }
    }
}

const WRAP_IV: [u8; 8] = [0xA6; 8];

/// RFC 3394 key wrap.
fn key_wrap(kek: &Aes, plain: &[u8]) -> Option<Vec<u8>> {
    if !plain.len().is_multiple_of(8) || plain.len() < 16 {
        return None;
    }
    let n = plain.len() / 8;
    let mut a = WRAP_IV;
    let mut r: Vec<[u8; 8]> = plain.chunks(8).map(|c| c.try_into().unwrap_or_default()).collect();
    for j in 0..6 {
        for (i, ri) in r.iter_mut().enumerate() {
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(ri);
            kek.encrypt(&mut block);
            let t = (n * j + i + 1) as u64;
            a.copy_from_slice(&block[..8]);
            for (byte, tb) in a.iter_mut().zip(t.to_be_bytes()) {
                *byte ^= tb;
            }
            ri.copy_from_slice(&block[8..]);
        }
    }
    let mut out = a.to_vec();
    for ri in r {
        out.extend_from_slice(&ri);
    }
    Some(out)
}

fn key_unwrap(kek: &Aes, wrapped: &[u8]) -> Option<Vec<u8>> {
    if !wrapped.len().is_multiple_of(8) || wrapped.len() < 24 {
        return None;
    }
    let n = wrapped.len() / 8 - 1;
    let mut a: [u8; 8] = wrapped[..8].try_into().ok()?;
    let mut r: Vec<[u8; 8]> = wrapped[8..].chunks(8).map(|c| c.try_into().unwrap_or_default()).collect();
    for j in (0..6).rev() {
        for i in (0..n).rev() {
            let t = (n * j + i + 1) as u64;
            let mut block = [0u8; 16];
            for (k, tb) in t.to_be_bytes().iter().enumerate() {
                block[k] = a[k] ^ tb;
            }
            block[8..].copy_from_slice(&r[i]);
            kek.decrypt(&mut block);
            a.copy_from_slice(&block[..8]);
            r[i].copy_from_slice(&block[8..]);
        }
    }
    if a != WRAP_IV {
        return None;
    }
    Some(r.concat())
}

enum Hasher {
    Sha1(Sha1),
    Sha224(Sha224),
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
}

impl Hasher {
    fn new(algorithm: u8) -> Option<Self> {
        Some(match algorithm {
            2 => Self::Sha1(Sha1::new()),
            8 => Self::Sha256(Sha256::new()),
            9 => Self::Sha384(Sha384::new()),
            10 => Self::Sha512(Sha512::new()),
            11 => Self::Sha224(Sha224::new()),
            _ => return None,
        })
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(data),
            Self::Sha224(h) => h.update(data),
            Self::Sha256(h) => h.update(data),
            Self::Sha384(h) => h.update(data),
            Self::Sha512(h) => h.update(data),
        }
    }

    fn finish(self) -> Vec<u8> {
        match self {
            Self::Sha1(h) => h.finalize().to_vec(),
            Self::Sha224(h) => h.finalize().to_vec(),
            Self::Sha256(h) => h.finalize().to_vec(),
            Self::Sha384(h) => h.finalize().to_vec(),
            Self::Sha512(h) => h.finalize().to_vec(),
        }
    }
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| Fault::Io)?;
    Ok(bytes)
}

fn sha1(data: &[u8]) -> [u8; 20] {
    Sha1::digest(data).into()
}

// ---------------------------------------------------------------------------------------
// Packets.

fn take<'a>(data: &'a [u8], at: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = at.checked_add(len)?;
    let out = data.get(*at..end)?;
    *at = end;
    Some(out)
}

fn byte(data: &[u8], at: &mut usize) -> Option<u8> {
    take(data, at, 1).map(|b| b[0])
}

fn be32(data: &[u8], at: &mut usize) -> Option<u32> {
    take(data, at, 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes)
}

/// Every packet in `data`, as tag and body, partial lengths joined.
fn packets(data: &[u8]) -> Option<Vec<(u8, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let head = byte(data, &mut at)?;
        if head & 0x80 == 0 {
            return None;
        }
        if head & 0x40 != 0 {
            let tag = head & 0x3f;
            let mut body = Vec::new();
            loop {
                let first = byte(data, &mut at)?;
                let (len, partial) = match first {
                    0..=191 => (first as usize, false),
                    192..=223 => (((first as usize - 192) << 8) + byte(data, &mut at)? as usize + 192, false),
                    255 => (be32(data, &mut at)? as usize, false),
                    _ => (1usize << (first & 0x1f), true),
                };
                body.extend_from_slice(take(data, &mut at, len)?);
                if !partial {
                    break;
                }
            }
            out.push((tag, body));
        } else {
            let tag = (head >> 2) & 0x0f;
            let len = match head & 3 {
                0 => byte(data, &mut at)? as usize,
                1 => u16::from_be_bytes(take(data, &mut at, 2)?.try_into().ok()?) as usize,
                2 => be32(data, &mut at)? as usize,
                _ => data.len() - at,
            };
            out.push((tag, take(data, &mut at, len)?.to_vec()));
        }
    }
    Some(out)
}

/// A new-format packet with a definite length.
fn packet(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 6);
    out.push(0xC0 | tag);
    let len = body.len();
    if len < 192 {
        out.push(len as u8);
    } else if len < 8384 {
        let v = len - 192;
        out.push(((v >> 8) + 192) as u8);
        out.push((v & 0xff) as u8);
    } else {
        out.push(0xff);
        out.extend_from_slice(&(len as u32).to_be_bytes());
    }
    out.extend_from_slice(body);
    out
}

fn read_mpi<'a>(data: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let bits = u16::from_be_bytes(take(data, at, 2)?.try_into().ok()?) as usize;
    take(data, at, bits.div_ceil(8))
}

fn mpi(value: &[u8]) -> Vec<u8> {
    let start = value.iter().position(|&b| b != 0).unwrap_or(value.len());
    let value = &value[start..];
    let bits = match value.first() {
        Some(&top) => (value.len() - 1) * 8 + (8 - top.leading_zeros() as usize),
        None => 0,
    };
    let mut out = (bits as u16).to_be_bytes().to_vec();
    out.extend_from_slice(value);
    out
}

fn left_pad(value: &[u8], len: usize) -> Option<Vec<u8>> {
    if value.len() > len {
        return None;
    }
    let mut out = vec![0u8; len - value.len()];
    out.extend_from_slice(value);
    Some(out)
}

/// A point on Curve25519 or Ed25519 as OpenPGP stores it: 0x40 and 32 bytes.
fn native_point(value: &[u8]) -> Option<[u8; 32]> {
    match value {
        [0x40, rest @ ..] => rest.try_into().ok(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------
// Keys.

#[derive(Clone)]
enum Public {
    Rsa { n: BigUint, e: BigUint },
    EdDsa([u8; 32]),
    Ecdh { point: [u8; 32], hash: u8, cipher: u8 },
    Unusable,
}

#[derive(Clone)]
enum Private {
    Rsa { d: BigUint },
    EdDsa(SigningKey),
    Ecdh(StaticSecret),
}

/// One key in a ring: the primary or a subkey.
#[derive(Clone)]
struct Component {
    algorithm: u8,
    fingerprint: [u8; 20],
    id: [u8; 8],
    public: Public,
    private: Option<Private>,
}

impl Component {
    /// The public half from a key packet body, and where it ends.
    fn parse(body: &[u8]) -> Option<(Self, usize)> {
        let mut at = 0;
        if byte(body, &mut at)? != 4 {
            return None;
        }
        be32(body, &mut at)?;
        let algorithm = byte(body, &mut at)?;
        let public = match algorithm {
            RSA | RSA_ENCRYPT | RSA_SIGN => {
                let n = BigUint::from_bytes_be(read_mpi(body, &mut at)?);
                let e = BigUint::from_bytes_be(read_mpi(body, &mut at)?);
                Public::Rsa { n, e }
            }
            EDDSA_LEGACY | 19 => {
                let len = byte(body, &mut at)? as usize;
                let oid = take(body, &mut at, len)?;
                let point = read_mpi(body, &mut at)?;
                match (algorithm, oid == OID_ED25519, native_point(point)) {
                    (EDDSA_LEGACY, true, Some(point)) => Public::EdDsa(point),
                    _ => Public::Unusable,
                }
            }
            ECDH => {
                let len = byte(body, &mut at)? as usize;
                let oid = take(body, &mut at, len)?;
                let point = read_mpi(body, &mut at)?;
                let kdf_len = byte(body, &mut at)? as usize;
                let kdf = take(body, &mut at, kdf_len)?;
                match (oid == OID_CURVE25519, native_point(point), kdf) {
                    (true, Some(point), [1, hash, cipher]) => Public::Ecdh { point, hash: *hash, cipher: *cipher },
                    _ => Public::Unusable,
                }
            }
            // DSA and ElGamal: read past them so the secret part could be found.
            16 => {
                for _ in 0..3 {
                    read_mpi(body, &mut at)?;
                }
                Public::Unusable
            }
            17 => {
                for _ in 0..4 {
                    read_mpi(body, &mut at)?;
                }
                Public::Unusable
            }
            _ => {
                at = body.len();
                Public::Unusable
            }
        };
        let fingerprint = fingerprint_of(&body[..at]);
        let mut id = [0u8; 8];
        id.copy_from_slice(&fingerprint[12..]);
        Some((Self { algorithm, fingerprint, id, public, private: None }, at))
    }

    fn can_encrypt(&self) -> bool {
        matches!(self.algorithm, RSA | RSA_ENCRYPT | ECDH) && !matches!(self.public, Public::Unusable)
    }
}

fn fingerprint_of(public_body: &[u8]) -> [u8; 20] {
    let mut hash = Sha1::new();
    hash.update([0x99]);
    hash.update((public_body.len() as u16).to_be_bytes());
    hash.update(public_body);
    hash.finalize().into()
}

/// A string-to-key specifier.
struct S2k<'a> {
    kind: u8,
    hash: u8,
    salt: &'a [u8],
    count: u8,
}

impl S2k<'_> {
    fn derive(&self, passphrase: &[u8], key_len: usize) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(key_len + 64);
        let mut preload = 0;
        while out.len() < key_len {
            let mut hash = Hasher::new(self.hash)?;
            hash.update(&vec![0u8; preload]);
            match self.kind {
                0 => hash.update(passphrase),
                1 => {
                    hash.update(self.salt);
                    hash.update(passphrase);
                }
                3 => {
                    let mut data = self.salt.to_vec();
                    data.extend_from_slice(passphrase);
                    let count = (16usize + (self.count & 15) as usize) << ((self.count >> 4) + 6);
                    let total = count.max(data.len());
                    let mut fed = 0;
                    while fed + data.len() <= total {
                        hash.update(&data);
                        fed += data.len();
                    }
                    hash.update(&data[..total - fed]);
                }
                _ => return None,
            }
            out.extend_from_slice(&hash.finish());
            preload += 1;
        }
        out.truncate(key_len);
        Some(out)
    }
}

/// The secret half of a key packet, past its public part.
fn open_secret(component: &Component, rest: &[u8], passphrase: &[u8]) -> Option<Private> {
    let mut at = 0;
    let usage = byte(rest, &mut at)?;
    let plain = match usage {
        0 => {
            let data = &rest[at..];
            let (mpis, sum) = data.split_at(data.len().checked_sub(2)?);
            if checksum(mpis) != u16::from_be_bytes(sum.try_into().ok()?) {
                return None;
            }
            mpis.to_vec()
        }
        254 | 255 => {
            let cipher = byte(rest, &mut at)?;
            let kind = byte(rest, &mut at)?;
            let hash = byte(rest, &mut at)?;
            let (salt, count) = match kind {
                0 => (&[][..], 0),
                1 => (take(rest, &mut at, 8)?, 0),
                3 => (take(rest, &mut at, 8)?, byte(rest, &mut at)?),
                // 101 is GnuPG's: no secret here at all.
                _ => return None,
            };
            let key = S2k { kind, hash, salt, count }.derive(passphrase, Aes::key_len(cipher)?)?;
            let aes = Aes::new(cipher, &key)?;
            let iv: [u8; AES_BLOCK] = take(rest, &mut at, AES_BLOCK)?.try_into().ok()?;
            let mut data = rest[at..].to_vec();
            cfb_decrypt(&aes, &iv, &mut data);
            if usage == 254 {
                let (mpis, hash) = data.split_at(data.len().checked_sub(20)?);
                if sha1(mpis) != hash {
                    return None;
                }
                mpis.to_vec()
            } else {
                let (mpis, sum) = data.split_at(data.len().checked_sub(2)?);
                if checksum(mpis) != u16::from_be_bytes(sum.try_into().ok()?) {
                    return None;
                }
                mpis.to_vec()
            }
        }
        _ => return None,
    };
    let mut at = 0;
    match &component.public {
        Public::Rsa { .. } => Some(Private::Rsa { d: BigUint::from_bytes_be(read_mpi(&plain, &mut at)?) }),
        Public::EdDsa(public) => {
            let seed: [u8; 32] = left_pad(read_mpi(&plain, &mut at)?, 32)?.try_into().ok()?;
            let key = SigningKey::from_bytes(&seed);
            (key.verifying_key().as_bytes() == public).then_some(Private::EdDsa(key))
        }
        Public::Ecdh { point, .. } => {
            // Stored big-endian: the native little-endian scalar reversed.
            let mut scalar: [u8; 32] = left_pad(read_mpi(&plain, &mut at)?, 32)?.try_into().ok()?;
            scalar.reverse();
            let secret = StaticSecret::from(scalar);
            (PublicKey::from(&secret).as_bytes() == point).then_some(Private::Ecdh(secret))
        }
        Public::Unusable => None,
    }
}

fn checksum(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |sum, &b| sum.wrapping_add(b as u16))
}

/// An unlocked private key: what decrypts with it, what signs with it, and what
/// encrypts to it.
#[derive(Clone)]
pub struct Key {
    encoded: Vec<u8>,
    components: Vec<Component>,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(***)")
    }
}

impl Key {
    /// Unlocks an armored (or binary) private key with its passphrase, or `None` when it
    /// does not open.
    pub fn unlock(armored: &str, passphrase: &[u8]) -> Option<Self> {
        let encoded = unarmor(armored).ok()?;
        let mut components = Vec::new();
        for (tag, body) in packets(&encoded)? {
            match tag {
                TAG_SECRET_KEY | TAG_SECRET_SUBKEY => {
                    let Some((mut component, end)) = Component::parse(&body) else { continue };
                    component.private = open_secret(&component, &body[end..], passphrase);
                    components.push(component);
                }
                TAG_PUBLIC_KEY | TAG_PUBLIC_SUBKEY => {
                    if let Some((component, _)) = Component::parse(&body) {
                        components.push(component);
                    }
                }
                _ => {}
            }
        }
        components.iter().any(|c| c.private.is_some()).then_some(Self { encoded, components })
    }

    /// The key that signs: the primary, which is EdDSA (or RSA on old accounts).
    fn signing(&self) -> Option<&Component> {
        self.components
            .first()
            .filter(|c| c.private.is_some())
            .or_else(|| self.components.iter().find(|c| c.private.is_some()))
    }

    /// The key messages to this one are encrypted to: the newest encryption-capable one.
    fn encryption(&self) -> Option<&Component> {
        self.components.iter().rev().find(|c| c.can_encrypt()).or_else(|| self.components.first())
    }

    fn private_for(&self, id: &[u8]) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id && c.private.is_some())
    }

    /// The whole key, locked, armored again.
    pub fn armored(&self) -> String {
        armor(&self.encoded)
    }
}

// ---------------------------------------------------------------------------------------
// Session keys.

fn ecdh_kek(component: &Component, shared: &[u8; 32]) -> Option<Aes> {
    let Public::Ecdh { hash, cipher, .. } = component.public else { return None };
    let mut param = vec![OID_CURVE25519.len() as u8];
    param.extend_from_slice(OID_CURVE25519);
    param.extend_from_slice(&[ECDH, 3, 1, hash, cipher]);
    param.extend_from_slice(b"Anonymous Sender    ");
    param.extend_from_slice(&component.fingerprint);
    let mut digest = Hasher::new(hash)?;
    digest.update(&[0, 0, 0, 1]);
    digest.update(shared);
    digest.update(&param);
    let len = Aes::key_len(cipher)?;
    Aes::new(cipher, digest.finish().get(..len)?)
}

/// The cipher and session key a key packet's body carries, for `component`.
fn open_session(component: &Component, body: &[u8]) -> Option<(u8, Vec<u8>)> {
    let mut at = 0;
    if byte(body, &mut at)? != 3 {
        return None;
    }
    take(body, &mut at, 8)?;
    let algorithm = byte(body, &mut at)?;
    let message = match (&component.private, &component.public, algorithm) {
        (Some(Private::Ecdh(secret)), Public::Ecdh { .. }, ECDH) => {
            let ephemeral = native_point(read_mpi(body, &mut at)?)?;
            let len = byte(body, &mut at)? as usize;
            let wrapped = take(body, &mut at, len)?;
            let shared = secret.diffie_hellman(&PublicKey::from(ephemeral));
            let padded = key_unwrap(&ecdh_kek(component, shared.as_bytes())?, wrapped)?;
            let pad = *padded.last()? as usize;
            if pad == 0 || pad > 8 || pad > padded.len() || padded[padded.len() - pad..].iter().any(|&b| b as usize != pad) {
                return None;
            }
            padded[..padded.len() - pad].to_vec()
        }
        (Some(Private::Rsa { d }), Public::Rsa { n, .. }, RSA | RSA_ENCRYPT) => {
            let c = BigUint::from_bytes_be(read_mpi(body, &mut at)?);
            let k = (n.bits() as usize).div_ceil(8);
            let m = left_pad(&c.modpow(d, n).to_bytes_be(), k)?;
            // EME-PKCS1-v1_5: 00 02, at least eight nonzero bytes, 00, the message.
            if m.first() != Some(&0) || m.get(1) != Some(&2) {
                return None;
            }
            let zero = m.iter().skip(2).position(|&b| b == 0)? + 2;
            if zero < 10 {
                return None;
            }
            m[zero + 1..].to_vec()
        }
        _ => return None,
    };
    let (&cipher, rest) = message.split_first()?;
    let (key, sum) = rest.split_at(rest.len().checked_sub(2)?);
    if Aes::key_len(cipher)? != key.len() || checksum(key) != u16::from_be_bytes(sum.try_into().ok()?) {
        return None;
    }
    Some((cipher, key.to_vec()))
}

/// The body of a v3 public-key encrypted session key packet for an AES-256 key.
fn session_body(to: &Component, session_key: &[u8]) -> Result<Vec<u8>> {
    let mut message = vec![AES_256];
    message.extend_from_slice(session_key);
    message.extend_from_slice(&checksum(session_key).to_be_bytes());
    let mut body = vec![3];
    body.extend_from_slice(&to.id);
    match &to.public {
        Public::Ecdh { point, .. } => {
            body.push(ECDH);
            let ephemeral = StaticSecret::from(random::<32>()?);
            let ephemeral_public = PublicKey::from(&ephemeral);
            let shared = ephemeral.diffie_hellman(&PublicKey::from(*point));
            let pad = 8 - message.len() % 8;
            message.extend(std::iter::repeat_n(pad as u8, pad));
            let wrapped = key_wrap(&ecdh_kek(to, shared.as_bytes()).ok_or(Fault::Io)?, &message).ok_or(Fault::Io)?;
            let mut point = vec![0x40];
            point.extend_from_slice(ephemeral_public.as_bytes());
            body.extend_from_slice(&mpi(&point));
            body.push(wrapped.len() as u8);
            body.extend_from_slice(&wrapped);
        }
        Public::Rsa { n, e } => {
            body.push(RSA);
            let k = (n.bits() as usize).div_ceil(8);
            let pad_len = k.checked_sub(message.len() + 3).filter(|&p| p >= 8).ok_or(Fault::Io)?;
            let mut em = vec![0, 2];
            while em.len() < 2 + pad_len {
                let fill: [u8; 64] = random()?;
                em.extend(fill.iter().copied().filter(|&b| b != 0).take(2 + pad_len - em.len()));
            }
            em.push(0);
            em.extend_from_slice(&message);
            let c = BigUint::from_bytes_be(&em).modpow(e, n);
            body.extend_from_slice(&mpi(&c.to_bytes_be()));
        }
        _ => return Err(Fault::Io),
    }
    Ok(body)
}

// ---------------------------------------------------------------------------------------
// Signatures.

fn subpacket(kind: u8, data: &[u8]) -> Vec<u8> {
    let len = data.len() + 1;
    let mut out = if len < 192 {
        vec![len as u8]
    } else {
        let v = len - 192;
        vec![((v >> 8) + 192) as u8, (v & 0xff) as u8]
    };
    out.push(kind);
    out.extend_from_slice(data);
    out
}

/// A v4 signature packet body over `data`, already in its hashed form.
fn signature(signer: &Component, kind: u8, data: &[u8], extra_hashed: &[u8], now: u32) -> Result<Vec<u8>> {
    let mut hashed = subpacket(2, &now.to_be_bytes());
    let mut issuer = vec![4];
    issuer.extend_from_slice(&signer.fingerprint);
    hashed.extend_from_slice(&subpacket(33, &issuer));
    hashed.extend_from_slice(extra_hashed);
    let mut head = vec![4, kind, signer.algorithm, SHA_256];
    head.extend_from_slice(&(hashed.len() as u16).to_be_bytes());
    head.extend_from_slice(&hashed);
    let mut digest = Sha256::new();
    digest.update(data);
    digest.update(&head);
    digest.update([4, 0xff]);
    digest.update((head.len() as u32).to_be_bytes());
    let digest = digest.finalize();

    let unhashed = subpacket(16, &signer.id);
    let mut body = head;
    body.extend_from_slice(&(unhashed.len() as u16).to_be_bytes());
    body.extend_from_slice(&unhashed);
    body.extend_from_slice(&digest[..2]);
    match (&signer.private, &signer.public) {
        (Some(Private::EdDsa(key)), _) => {
            // Legacy EdDSA signs the digest itself.
            let sig = key.sign(&digest).to_bytes();
            body.extend_from_slice(&mpi(&sig[..32]));
            body.extend_from_slice(&mpi(&sig[32..]));
        }
        (Some(Private::Rsa { d }), Public::Rsa { n, .. }) => {
            let k = (n.bits() as usize).div_ceil(8);
            let t_len = SHA256_INFO.len() + digest.len();
            let pad = k.checked_sub(t_len + 3).filter(|&p| p >= 8).ok_or(Fault::Io)?;
            let mut em = vec![0, 1];
            em.extend(std::iter::repeat_n(0xff, pad));
            em.push(0);
            em.extend_from_slice(SHA256_INFO);
            em.extend_from_slice(&digest);
            let s = BigUint::from_bytes_be(&em).modpow(d, n);
            body.extend_from_slice(&mpi(&s.to_bytes_be()));
        }
        _ => return Err(Fault::Io),
    }
    Ok(body)
}

/// A detached signature over `data` by `signer`, binary.
pub fn sign(signer: &Key, data: &[u8], now: u32) -> Result<Vec<u8>> {
    let component = signer.signing().ok_or(Fault::Io)?;
    Ok(packet(TAG_SIGNATURE, &signature(component, 0x00, data, &[], now)?))
}

/// [`sign`], armored.
pub fn sign_armored(signer: &Key, data: &[u8], now: u32) -> Result<String> {
    Ok(armor(&sign(signer, data, now)?))
}

fn key_hash_prefix(public_body: &[u8]) -> Vec<u8> {
    let mut out = vec![0x99];
    out.extend_from_slice(&(public_body.len() as u16).to_be_bytes());
    out.extend_from_slice(public_body);
    out
}

// ---------------------------------------------------------------------------------------
// Making keys.

fn lock_secret(mpis: &[u8], passphrase: &[u8]) -> Result<Vec<u8>> {
    let salt: [u8; 8] = random()?;
    let iv: [u8; AES_BLOCK] = random()?;
    let key = S2k { kind: 3, hash: SHA_256, salt: &salt, count: S2K_COUNT }
        .derive(passphrase, 32)
        .ok_or(Fault::Io)?;
    let aes = Aes::new(AES_256, &key).ok_or(Fault::Io)?;
    let mut data = mpis.to_vec();
    data.extend_from_slice(&sha1(mpis));
    cfb_encrypt(&aes, &iv, &mut data);
    let mut out = vec![254, AES_256, 3, SHA_256];
    out.extend_from_slice(&salt);
    out.push(S2K_COUNT);
    out.extend_from_slice(&iv);
    out.extend_from_slice(&data);
    Ok(out)
}

/// A new node key, as Proton's apps make them: EdDSA primary, ECDH subkey, user id
/// "Drive key", locked with `passphrase`. Armored.
pub fn generate_key(passphrase: &str, now: u32) -> Result<String> {
    let signing = SigningKey::from_bytes(&random::<32>()?);
    let mut scalar: [u8; 32] = random()?;
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;
    let encryption = StaticSecret::from(scalar);

    let mut primary_body = vec![4];
    primary_body.extend_from_slice(&now.to_be_bytes());
    primary_body.push(EDDSA_LEGACY);
    primary_body.push(OID_ED25519.len() as u8);
    primary_body.extend_from_slice(OID_ED25519);
    let mut point = vec![0x40];
    point.extend_from_slice(signing.verifying_key().as_bytes());
    primary_body.extend_from_slice(&mpi(&point));

    let mut sub_body = vec![4];
    sub_body.extend_from_slice(&now.to_be_bytes());
    sub_body.push(ECDH);
    sub_body.push(OID_CURVE25519.len() as u8);
    sub_body.extend_from_slice(OID_CURVE25519);
    let mut point = vec![0x40];
    point.extend_from_slice(PublicKey::from(&encryption).as_bytes());
    sub_body.extend_from_slice(&mpi(&point));
    sub_body.extend_from_slice(&[3, 1, SHA_256, AES_128]);

    let (mut primary, _) = Component::parse(&primary_body).ok_or(Fault::Io)?;
    primary.private = Some(Private::EdDsa(signing.clone()));

    let user_id = b"Drive key";
    let mut certified = key_hash_prefix(&primary_body);
    certified.push(0xB4);
    certified.extend_from_slice(&(user_id.len() as u32).to_be_bytes());
    certified.extend_from_slice(user_id);
    let mut flags = subpacket(27, &[0x03]);
    flags.extend_from_slice(&subpacket(11, &[AES_256, AES_128]));
    flags.extend_from_slice(&subpacket(21, &[SHA_256, SHA_512]));
    let certification = signature(&primary, 0x13, &certified, &flags, now)?;

    let mut bound = key_hash_prefix(&primary_body);
    bound.extend_from_slice(&key_hash_prefix(&sub_body));
    let binding = signature(&primary, 0x18, &bound, &subpacket(27, &[0x0C]), now)?;

    let mut primary_secret = primary_body;
    primary_secret.extend_from_slice(&lock_secret(&mpi(signing.as_bytes()), passphrase.as_bytes())?);
    let mut stored = scalar;
    stored.reverse();
    let mut sub_secret = sub_body;
    sub_secret.extend_from_slice(&lock_secret(&mpi(&stored), passphrase.as_bytes())?);

    let mut ring = packet(TAG_SECRET_KEY, &primary_secret);
    ring.extend_from_slice(&packet(TAG_USER_ID, user_id));
    ring.extend_from_slice(&packet(TAG_SIGNATURE, &certification));
    ring.extend_from_slice(&packet(TAG_SECRET_SUBKEY, &sub_secret));
    ring.extend_from_slice(&packet(TAG_SIGNATURE, &binding));
    Ok(armor(&ring))
}

/// 32 random bytes in base64: a node key's passphrase, as the SDK makes it.
pub fn new_passphrase() -> Result<String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(random::<32>()?))
}

/// A fresh AES-256 session key.
pub fn new_session_key() -> Result<Vec<u8>> {
    Ok(random::<32>()?.to_vec())
}

// ---------------------------------------------------------------------------------------
// Messages.

/// A public-key encrypted session key packet for an AES-256 `session_key`, to `to`.
/// Binary.
pub fn key_packet(to: &Key, session_key: &[u8]) -> Result<Vec<u8>> {
    let component = to.encryption().ok_or(Fault::Io)?;
    Ok(packet(TAG_PKESK, &session_body(component, session_key)?))
}

fn seipd(session_key: &[u8], inner: &[u8]) -> Result<Vec<u8>> {
    let aes = Aes::new(AES_256, session_key).ok_or(Fault::Io)?;
    let prefix: [u8; AES_BLOCK] = random()?;
    let mut plain = Vec::with_capacity(inner.len() + 40);
    plain.extend_from_slice(&prefix);
    plain.extend_from_slice(&prefix[AES_BLOCK - 2..]);
    plain.extend_from_slice(inner);
    plain.extend_from_slice(&MDC_HEADER);
    let mdc = sha1(&plain);
    plain.extend_from_slice(&mdc);
    cfb_encrypt(&aes, &[0; AES_BLOCK], &mut plain);
    let mut body = Vec::with_capacity(plain.len() + 1);
    body.push(1);
    body.extend_from_slice(&plain);
    Ok(packet(TAG_SEIPD, &body))
}

fn literal(data: &[u8], date: u32) -> Vec<u8> {
    let mut body = Vec::with_capacity(data.len() + 6);
    body.extend_from_slice(&[b'b', 0]);
    body.extend_from_slice(&date.to_be_bytes());
    body.extend_from_slice(data);
    packet(TAG_LITERAL, &body)
}

/// `data` as a literal data packet, encrypted under `session_key` as a SEIPD v1 packet
/// (AES-256 CFB, MDC), with no key packet in front: a Drive block.
pub fn encrypt_with_session_key(session_key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    seipd(session_key, &literal(data, 0))
}

/// Encrypts `data` to `to`; signed inside the message by `signer` when given (a name, an
/// XAttr), and compressed first when `compress`. Armored.
pub fn encrypt(to: &Key, data: &[u8], signer: Option<&Key>, compress: bool, now: u32) -> Result<String> {
    let mut inner = Vec::with_capacity(data.len() + 200);
    let signature = match signer {
        Some(key) => {
            let component = key.signing().ok_or(Fault::Io)?;
            let mut one_pass = vec![3, 0x00, SHA_256, component.algorithm];
            one_pass.extend_from_slice(&component.id);
            one_pass.push(1);
            inner.extend_from_slice(&packet(TAG_ONE_PASS, &one_pass));
            Some(signature(component, 0x00, data, &[], now)?)
        }
        None => None,
    };
    inner.extend_from_slice(&literal(data, now));
    if let Some(signature) = signature {
        inner.extend_from_slice(&packet(TAG_SIGNATURE, &signature));
    }
    if compress {
        let mut body = vec![2];
        body.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(&inner, 6));
        inner = packet(TAG_COMPRESSED, &body);
    }
    let session_key = new_session_key()?;
    let mut message = key_packet(to, &session_key)?;
    message.extend_from_slice(&seipd(&session_key, &inner)?);
    Ok(armor(&message))
}

/// The plain packets in a SEIPD v1 body, its integrity packet checked; `None` when the
/// key is not the one (the quick check fails), an error when the data was changed.
fn open_seipd(cipher: u8, key: &[u8], body: &[u8]) -> Option<Result<Vec<u8>>> {
    let aes = Aes::new(cipher, key)?;
    let (&version, encrypted) = body.split_first()?;
    if version != 1 || encrypted.len() < AES_BLOCK + 2 + 22 {
        return None;
    }
    let mut plain = encrypted.to_vec();
    cfb_decrypt(&aes, &[0; AES_BLOCK], &mut plain);
    if plain[AES_BLOCK - 2..AES_BLOCK] != plain[AES_BLOCK..AES_BLOCK + 2] {
        return None;
    }
    let mdc_at = plain.len() - 22;
    let intact = plain[mdc_at..mdc_at + 2] == MDC_HEADER && sha1(&plain[..mdc_at + 2]) == plain[mdc_at + 2..];
    if !intact {
        return Some(Err(Fault::NotVerified));
    }
    Some(Ok(plain[AES_BLOCK + 2..mdc_at].to_vec()))
}

/// The literal data in decrypted packets, through compression and past signatures.
fn literal_of(data: &[u8], depth: u8) -> Result<Vec<u8>> {
    for (tag, body) in packets(data).ok_or(Fault::Io)? {
        match tag {
            TAG_COMPRESSED if depth < 4 => {
                let (&algorithm, rest) = body.split_first().ok_or(Fault::Io)?;
                let inflated = match algorithm {
                    0 => rest.to_vec(),
                    1 => miniz_oxide::inflate::decompress_to_vec_with_limit(rest, INFLATE_LIMIT).map_err(|_| Fault::Io)?,
                    2 => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(rest, INFLATE_LIMIT).map_err(|_| Fault::Io)?,
                    _ => return Err(Fault::Io),
                };
                return literal_of(&inflated, depth + 1);
            }
            TAG_LITERAL => {
                let mut at = 0;
                byte(&body, &mut at).ok_or(Fault::Io)?;
                let name = byte(&body, &mut at).ok_or(Fault::Io)? as usize;
                take(&body, &mut at, name + 4).ok_or(Fault::Io)?;
                return Ok(body[at..].to_vec());
            }
            _ => {}
        }
    }
    Err(Fault::Io)
}

fn looks_armored(message: &[u8]) -> bool {
    message.iter().find(|b| !b.is_ascii_whitespace()).is_some_and(|&b| b == b'-')
}

/// Decrypts a message, armored or binary, with whichever of `keys` it was encrypted to,
/// and checks its integrity packet. Signatures inside are skipped over, not checked.
pub fn decrypt(message: &[u8], keys: &[&Key]) -> Result<Vec<u8>> {
    let bytes = if looks_armored(message) {
        unarmor(std::str::from_utf8(message).map_err(|_| Fault::Io)?)?
    } else {
        message.to_vec()
    };
    let all = packets(&bytes).ok_or(Fault::Io)?;
    let data = all.iter().find(|(tag, _)| *tag == TAG_SEIPD).map(|(_, body)| body).ok_or(Fault::Io)?;
    for (_, session) in all.iter().filter(|(tag, _)| *tag == TAG_PKESK) {
        let id = session.get(1..9).ok_or(Fault::Io)?;
        for key in keys {
            let component = key.private_for(id).or_else(|| {
                // A hidden recipient: try the key messages to this one go to.
                (id == [0; 8]).then(|| key.encryption()).flatten().filter(|c| c.private.is_some())
            });
            let Some(component) = component else { continue };
            let Some((cipher, session_key)) = open_session(component, session) else { continue };
            match open_seipd(cipher, &session_key, data) {
                Some(Ok(plain)) => return literal_of(&plain, 0),
                Some(Err(fault)) => return Err(fault),
                None => continue,
            }
        }
    }
    Err(Fault::Io)
}

/// [`decrypt`] for text.
pub fn decrypt_armored(message: &str, keys: &[&Key]) -> Result<Vec<u8>> {
    decrypt(message.as_bytes(), keys)
}

/// The session key a key packet carries, for `key`.
pub fn session_key_of(key_packet: &[u8], key: &Key) -> Option<Vec<u8>> {
    let all = packets(key_packet)?;
    let (_, body) = all.iter().find(|(tag, _)| *tag == TAG_PKESK)?;
    let component = key
        .private_for(body.get(1..9)?)
        .or_else(|| key.encryption().filter(|c| c.private.is_some()))?;
    open_session(component, body).map(|(_, session_key)| session_key)
}

/// HMAC-SHA256 of `name` under a folder's hash key, in hex: a node's lookup hash.
pub fn name_hash(hash_key: &[u8], name: &str) -> String {
    let Ok(mut mac) = <Hmac<Sha256> as hmac::KeyInit>::new_from_slice(hash_key) else { return String::new() };
    mac.update(name.as_bytes());
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------------------
// Armor.

fn crc24(data: &[u8]) -> u32 {
    let mut crc = 0x00B7_04CEu32;
    for &b in data {
        crc ^= (b as u32) << 16;
        for _ in 0..8 {
            crc <<= 1;
            if crc & 0x0100_0000 != 0 {
                crc ^= 0x0186_4CFB;
            }
        }
    }
    crc & 0x00FF_FFFF
}

/// Armors OpenPGP packets; the header (message, signature, private key) follows the
/// first packet.
pub fn armor(bytes: &[u8]) -> String {
    let tag = match bytes.first() {
        Some(&b) if b & 0x40 != 0 => b & 0x3f,
        Some(&b) => (b >> 2) & 0x0f,
        None => 0,
    };
    let kind = match tag {
        TAG_SECRET_KEY => "PRIVATE KEY BLOCK",
        TAG_PUBLIC_KEY => "PUBLIC KEY BLOCK",
        TAG_SIGNATURE => "SIGNATURE",
        _ => "MESSAGE",
    };
    let engine = base64::engine::general_purpose::STANDARD;
    let encoded = engine.encode(bytes);
    let mut out = format!("-----BEGIN PGP {kind}-----\n\n");
    for line in encoded.as_bytes().chunks(64) {
        out.push_str(&String::from_utf8_lossy(line));
        out.push('\n');
    }
    out.push('=');
    out.push_str(&engine.encode(&crc24(bytes).to_be_bytes()[1..]));
    out.push_str(&format!("\n-----END PGP {kind}-----\n"));
    out
}

/// The packets inside armor. Text that is not armored is taken as binary.
pub fn unarmor(text: &str) -> Result<Vec<u8>> {
    let engine = base64::engine::general_purpose::STANDARD;
    let mut lines = text.lines().map(str::trim_end);
    if !lines.by_ref().any(|line| line.starts_with("-----BEGIN PGP")) {
        return Ok(text.as_bytes().to_vec());
    }
    let mut body = String::new();
    let mut check = None;
    let mut in_headers = true;
    for line in lines {
        if line.starts_with("-----END PGP") {
            break;
        }
        if in_headers {
            if line.is_empty() {
                in_headers = false;
                continue;
            }
            if line.contains(": ") {
                continue;
            }
            in_headers = false;
        }
        if let Some(sum) = line.strip_prefix('=').filter(|sum| sum.len() == 4) {
            check = Some(sum.to_owned());
            continue;
        }
        body.push_str(line.trim());
    }
    let bytes = engine.decode(body).map_err(|_| Fault::Io)?;
    if let Some(sum) = check {
        let sum = engine.decode(sum).map_err(|_| Fault::Io)?;
        if sum.as_slice() != &crc24(&bytes).to_be_bytes()[1..] {
            return Err(Fault::Io);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u32 = 1_762_000_000;

    fn key(passphrase: &str) -> Key {
        Key::unlock(&generate_key(passphrase, NOW).unwrap(), passphrase.as_bytes()).unwrap()
    }

    #[test]
    fn keys_names_and_blocks_round_trip() {
        let pass = new_passphrase().unwrap();
        let key = key(&pass);
        assert!(Key::unlock(&key.armored(), b"wrong").is_none());
        assert!(Key::unlock(&key.armored(), pass.as_bytes()).is_some());
        let parent = self::key("p");
        let name = encrypt(&parent, b"wmkeyboard-auto-x.json", Some(&key), false, NOW).unwrap();
        assert!(name.starts_with("-----BEGIN PGP MESSAGE-----\n\n"));
        assert_eq!(decrypt_armored(&name, &[&key, &parent]).unwrap(), b"wmkeyboard-auto-x.json");
        assert_eq!(decrypt_armored(&name, &[&key]), Err(Fault::Io));
        let xattr = encrypt(&key, b"{}", Some(&key), true, NOW).unwrap();
        assert_eq!(decrypt_armored(&xattr, &[&key]).unwrap(), b"{}");

        let session = new_session_key().unwrap();
        let packet = key_packet(&key, &session).unwrap();
        assert_eq!(session_key_of(&packet, &key).unwrap(), session);
        assert!(session_key_of(&packet, &parent).is_none());
        for size in [0usize, 191, 192, 8383, 8384, 300_000] {
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let block = encrypt_with_session_key(&session, &data).unwrap();
            let mut message = packet.clone();
            message.extend_from_slice(&block);
            assert_eq!(decrypt(&message, &[&key]).unwrap(), data);
            let mut changed = block.clone();
            let at = changed.len() - 3;
            changed[at] ^= 1;
            let mut message = packet.clone();
            message.extend_from_slice(&changed);
            assert!(decrypt(&message, &[&key]).is_err());
        }
        assert!(sign_armored(&key, &session, NOW).unwrap().starts_with("-----BEGIN PGP SIGNATURE-----"));
        assert_eq!(name_hash(b"k", "n").len(), 64);
    }

    #[test]
    fn partial_lengths_and_old_format_packets_parse() {
        // A 600-byte literal body sent as 512 partial, then 88.
        let mut body = vec![b'b', 0, 0, 0, 0, 0];
        body.extend((0..594).map(|i| i as u8));
        let mut stream = vec![0xC0 | TAG_LITERAL, 0xE0 | 9];
        stream.extend_from_slice(&body[..512]);
        stream.push(88);
        stream.extend_from_slice(&body[512..]);
        assert_eq!(packets(&stream).unwrap(), vec![(TAG_LITERAL, body.clone())]);
        // Old format, two-byte length.
        let mut old = vec![0x80 | (TAG_LITERAL << 2) | 1];
        old.extend_from_slice(&(body.len() as u16).to_be_bytes());
        old.extend_from_slice(&body);
        assert_eq!(literal_of(&old, 0).unwrap(), &body[6..]);
        assert!(packets(&[0x00, 1, 2]).is_none());
        assert!(packets(&[0xC0 | TAG_LITERAL, 10, 1]).is_none());
    }

    #[test]
    fn rfc_3394_key_wrap_vector() {
        // RFC 3394 4.1: 128 bits of key data with a 128-bit KEK.
        let kek = Aes::new(7, &(0u8..16).collect::<Vec<_>>()).unwrap();
        let data: Vec<u8> = (0u8..16).map(|i| i * 0x11).collect();
        let wrapped = key_wrap(&kek, &data).unwrap();
        let expected = [
            0x1F, 0xA6, 0x8B, 0x0A, 0x81, 0x12, 0xB4, 0x47, 0xAE, 0xF3, 0x4B, 0xD8, 0xFB, 0x5A, 0x7B, 0x82, 0x9D, 0x3E, 0x86,
            0x23, 0x71, 0xD2, 0xCF, 0xE5,
        ];
        assert_eq!(wrapped, expected);
        assert_eq!(key_unwrap(&kek, &wrapped).unwrap(), data);
        let mut broken = wrapped.clone();
        broken[5] ^= 1;
        assert!(key_unwrap(&kek, &broken).is_none());
    }

    #[test]
    fn armor_carries_a_crc_and_refuses_a_wrong_one() {
        let bytes = b"\xC2\x01\x00".to_vec();
        let armored = armor(&bytes);
        assert!(armored.starts_with("-----BEGIN PGP SIGNATURE-----"));
        assert_eq!(unarmor(&armored).unwrap(), bytes);
        // RFC 4880's CRC-24 of nothing is its initial value.
        assert_eq!(crc24(b""), 0xB704CE);
        let broken = armored.replace(&armored[armored.find('=').unwrap()..armored.find('=').unwrap() + 5], "=AAAA");
        assert_eq!(unarmor(&broken), Err(Fault::Io));
    }

    #[test]
    fn mpis_strip_leading_zeros() {
        assert_eq!(mpi(&[0, 0, 1]), vec![0, 1, 1]);
        assert_eq!(mpi(&[0x40, 0]), vec![0, 15, 0x40, 0]);
        assert_eq!(mpi(&[]), vec![0, 0]);
    }

    /// GnuPG reads what this writes and this reads what GnuPG writes: Curve25519 and RSA
    /// keys, messages, detached signatures. Skipped where `gpg` is not installed.
    #[test]
    #[cfg(unix)]
    fn gnupg_agrees() {
        use std::process::Command;
        if Command::new("gpg").arg("--version").output().is_err() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let dir = home.path();
        std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let gpg = |args: &[&str]| {
            let out = Command::new("gpg")
                .env("GNUPGHOME", dir)
                .args(["--batch", "--yes", "--pinentry-mode", "loopback", "--passphrase", "pass", "--trust-model", "always"])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "gpg {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            out.stdout
        };
        let write = |name: &str, data: &[u8]| {
            let path = dir.join(name);
            std::fs::write(&path, data).unwrap();
            path.to_str().unwrap().to_owned()
        };

        // Ours, into GnuPG.
        let ours = generate_key("pass", NOW).unwrap();
        gpg(&["--import", &write("ours.asc", ours.as_bytes())]);
        let key = Key::unlock(&ours, b"pass").unwrap();
        let data = b"vault bytes \x00\x01 and more".repeat(50);
        for compress in [false, true] {
            let message = encrypt(&key, &data, Some(&key), compress, NOW).unwrap();
            assert_eq!(gpg(&["--decrypt", &write("m.asc", message.as_bytes())]), data);
        }
        let signature = sign(&key, &data, NOW).unwrap();
        gpg(&["--verify", &write("d.sig", &signature), &write("d", &data)]);
        let session = new_session_key().unwrap();
        let mut block = key_packet(&key, &session).unwrap();
        block.extend_from_slice(&encrypt_with_session_key(&session, &data).unwrap());
        assert_eq!(gpg(&["--decrypt", &write("b.gpg", &block)]), data);

        // GnuPG encrypting and signing to ours: its compressed, signed messages.
        let listed = String::from_utf8(gpg(&["--with-colons", "--list-keys", "Drive key"])).unwrap();
        let ours_fpr = listed.lines().find(|l| l.starts_with("fpr:")).unwrap().split(':').nth(9).unwrap().to_owned();
        let plain = write("p", &data);
        let message = gpg(&["--armor", "--sign", "--encrypt", "-u", &ours_fpr, "-r", &ours_fpr, "--output", "-", &plain]);
        assert_eq!(decrypt(&message, &[&key]).unwrap(), data);

        // GnuPG's keys, into this: EdDSA with a Curve25519 subkey, then RSA. GnuPG 2.5
        // marks its new keys as taking its OCB packet, which Proton does not use, so the
        // preferences are set again without it first.
        for (uid, algo, sub) in [("c <c@example.com>", "ed25519", "cv25519"), ("r <r@example.com>", "rsa2048", "rsa2048")] {
            gpg(&["--quick-gen-key", uid, algo, "sign", "never"]);
            let listed = String::from_utf8(gpg(&["--with-colons", "--list-keys", uid])).unwrap();
            let fpr = listed.lines().find(|l| l.starts_with("fpr:")).unwrap().split(':').nth(9).unwrap().to_owned();
            gpg(&["--quick-add-key", &fpr, sub, "encr", "never"]);
            let mut edit = Command::new("gpg")
                .env("GNUPGHOME", dir)
                .args(["--batch", "--yes", "--pinentry-mode", "loopback", "--passphrase", "pass", "--command-fd", "0", "--edit-key", &fpr])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            std::io::Write::write_all(&mut edit.stdin.take().unwrap(), b"setpref AES256 AES128 SHA256 SHA512 ZLIB Uncompressed\ny\nsave\n").unwrap();
            assert!(edit.wait().unwrap().success());
            let exported = String::from_utf8(gpg(&["--armor", "--export-secret-keys", uid])).unwrap();
            let theirs = Key::unlock(&exported, b"pass").unwrap();
            assert!(Key::unlock(&exported, b"nope").is_none());
            let message = gpg(&["--armor", "--sign", "--encrypt", "-u", uid, "-r", uid, "--output", "-", &plain]);
            assert_eq!(decrypt(&message, &[&theirs]).unwrap(), data);
            // Signed now: GnuPG refuses a signature older than its key.
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as u32 + 5;
            let signature = sign(&theirs, &data, now).unwrap();
            gpg(&["--verify", &write("t.sig", &signature), &plain]);
            let message = encrypt(&theirs, &data, Some(&theirs), true, now).unwrap();
            assert_eq!(gpg(&["--decrypt", &write("t.asc", message.as_bytes())]), data);
        }
        // The agent this started would otherwise outlive its home.
        let _ = Command::new("gpgconf").env("GNUPGHOME", dir).args(["--kill", "all"]).status();
    }
}

