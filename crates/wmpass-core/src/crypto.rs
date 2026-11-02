//! Thin, hard-to-misuse wrappers over audited primitives. No new constructions:
//! Argon2id (RFC 9106), XChaCha20-Poly1305, HKDF-SHA256, HMAC-SHA256, the STREAM
//! online-AEAD layout (Hoang, Reyhanitabar, Rogaway, Vizár) and Padmé padding
//! (Nikitin et al.).

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{Error, Result};

/// A 32-byte key that wipes itself.
pub type Key = Zeroizing<[u8; 32]>;

/// Bytes of nonce in front of every sealed value.
pub const NONCE_LEN: usize = 24;
/// Bytes of authentication tag behind every sealed value.
pub const TAG_LEN: usize = 16;

/// Lowest accepted memory cost: 64 MiB.
pub const KDF_MIN_M_KIB: u32 = 64 * 1024;
/// Lowest accepted time cost.
pub const KDF_MIN_T: u32 = 3;
/// Highest accepted memory cost, so a hostile file cannot exhaust memory.
pub const KDF_MAX_M_KIB: u32 = 4 * 1024 * 1024;
/// Highest accepted time cost.
pub const KDF_MAX_T: u32 = 64;
/// Highest accepted parallelism.
pub const KDF_MAX_P: u32 = 16;

/// Fills `out` from the operating system. Failure is an error, never a fallback.
pub fn fill_random(out: &mut [u8]) -> Result<()> {
    getrandom::fill(out).map_err(|_| Error::Random)
}

/// A random array.
pub fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    fill_random(&mut out)?;
    Ok(out)
}

/// A random key.
pub fn random_key() -> Result<Key> {
    let mut key = Zeroizing::new([0u8; 32]);
    fill_random(&mut key[..])?;
    Ok(key)
}

/// A uniform number below `n`, by rejection sampling: plain modulo would favour
/// low values. `n` must be above zero.
pub fn random_below(n: u32) -> Result<u32> {
    if n == 0 {
        return Err(Error::Invalid("empty range".into()));
    }
    // Largest multiple of n that fits; values at or above it are thrown away.
    let zone = u32::MAX - (u32::MAX % n + 1) % n;
    loop {
        let value = u32::from_le_bytes(random_array::<4>()?);
        if value <= zone {
            return Ok(value % n);
        }
    }
}

/// Argon2id cost settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Passes.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self { m_kib: 128 * 1024, t: 3, p: 4 }
    }
}

impl KdfParams {
    /// Refuses settings a tampered or hostile header could use against us.
    pub fn check(self) -> Result<Self> {
        let ok = (KDF_MIN_M_KIB..=KDF_MAX_M_KIB).contains(&self.m_kib)
            && (KDF_MIN_T..=KDF_MAX_T).contains(&self.t)
            && (1..=KDF_MAX_P).contains(&self.p);
        if ok { Ok(self) } else { Err(Error::KdfOutOfRange) }
    }
}

/// Runs Argon2id over `password`.
pub fn argon2id(password: &[u8], salt: &[u8], params: KdfParams) -> Result<Key> {
    let params = params.check()?;
    argon2id_unchecked(password, salt, params)
}

fn argon2id_unchecked(password: &[u8], salt: &[u8], params: KdfParams) -> Result<Key> {
    let settings =
        Params::new(params.m_kib, params.t, params.p, Some(32)).map_err(|_| Error::KdfOutOfRange)?;
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, settings)
        .hash_password_into(password, salt, &mut out[..])
        .map_err(|_| Error::Crypto)?;
    Ok(out)
}

/// Test-only derivation below the floor, so the suite stays fast. Not reachable
/// from production code paths.
#[cfg(test)]
pub(crate) fn argon2id_for_tests(password: &[u8], salt: &[u8]) -> Result<Key> {
    argon2id_unchecked(password, salt, KdfParams { m_kib: 8, t: 1, p: 1 })
}

/// Expands `ikm` into a sub-key for one purpose, named by `info`.
pub fn hkdf(ikm: &[u8], salt: &[u8], info: &str) -> Result<Key> {
    let mut out = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info.as_bytes(), &mut out[..])
        .map_err(|_| Error::Crypto)?;
    Ok(out)
}

/// HMAC-SHA256.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> Result<[u8; 32]> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| Error::Crypto)?;
    mac.update(message);
    let mut out = [0u8; 32];
    out.copy_from_slice(&mac.finalize().into_bytes());
    Ok(out)
}

/// Constant-time HMAC-SHA256 check.
pub fn hmac_sha256_verify(key: &[u8], message: &[u8], tag: &[u8]) -> Result<()> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| Error::Crypto)?;
    mac.update(message);
    mac.verify_slice(tag).map_err(|_| Error::Integrity)
}

fn cipher(key: &[u8; 32]) -> Result<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Crypto)
}

fn xnonce(bytes: &[u8]) -> Result<XNonce> {
    XNonce::try_from(bytes).map_err(|_| Error::Crypto)
}

/// Encrypts with a fresh random nonce: `nonce || ciphertext || tag`.
pub fn seal(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let nonce = random_array::<NONCE_LEN>()?;
    let sealed = cipher(key)?
        .encrypt(&xnonce(&nonce)?, Payload { msg: plain, aad })
        .map_err(|_| Error::Crypto)?;
    let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Reverses [`seal`]. `None` means the key is wrong or the data was changed.
pub fn open(key: &[u8; 32], sealed: &[u8], aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let nonce = xnonce(nonce).ok()?;
    cipher(key).ok()?.decrypt(&nonce, Payload { msg: body, aad }).ok().map(Zeroizing::new)
}

/// Plaintext bytes per STREAM chunk.
pub const CHUNK: usize = 64 * 1024;
/// Random nonce prefix for a STREAM: 19 bytes, then a 4-byte counter and a last-chunk flag.
pub const STREAM_PREFIX_LEN: usize = 19;

fn stream_nonce(prefix: &[u8; STREAM_PREFIX_LEN], index: u32, last: bool) -> Result<XNonce> {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[..STREAM_PREFIX_LEN].copy_from_slice(prefix);
    nonce[STREAM_PREFIX_LEN..STREAM_PREFIX_LEN + 4].copy_from_slice(&index.to_be_bytes());
    nonce[NONCE_LEN - 1] = u8::from(last);
    xnonce(&nonce)
}

/// Encrypts `plain` as a STREAM. Dropping, reordering or truncating chunks makes
/// [`stream_open`] fail, because the position and the last-chunk flag are in the nonce.
pub fn stream_seal(
    key: &[u8; 32],
    prefix: &[u8; STREAM_PREFIX_LEN],
    plain: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    let cipher = cipher(key)?;
    let chunks = plain.len().div_ceil(CHUNK).max(1);
    let mut out = Vec::with_capacity(plain.len() + chunks * TAG_LEN);
    for index in 0..chunks {
        let start = index * CHUNK;
        let end = (start + CHUNK).min(plain.len());
        let last = index + 1 == chunks;
        let count = u32::try_from(index).map_err(|_| Error::TooLong)?;
        let nonce = stream_nonce(prefix, count, last)?;
        let sealed = cipher
            .encrypt(&nonce, Payload { msg: &plain[start..end], aad })
            .map_err(|_| Error::Crypto)?;
        out.extend_from_slice(&sealed);
    }
    Ok(out)
}

/// Reverses [`stream_seal`]. Any change, truncation or extension is [`Error::Integrity`].
pub fn stream_open(
    key: &[u8; 32],
    prefix: &[u8; STREAM_PREFIX_LEN],
    sealed: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let cipher = cipher(key)?;
    let step = CHUNK + TAG_LEN;
    let chunks = sealed.len().div_ceil(step).max(1);
    let mut out = Zeroizing::new(Vec::with_capacity(sealed.len()));
    for index in 0..chunks {
        let start = index * step;
        let end = (start + step).min(sealed.len());
        let last = index + 1 == chunks;
        let count = u32::try_from(index).map_err(|_| Error::Integrity)?;
        let nonce = stream_nonce(prefix, count, last)?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(&nonce, Payload { msg: &sealed[start..end], aad })
                .map_err(|_| Error::Integrity)?,
        );
        out.extend_from_slice(&plain);
    }
    Ok(out)
}

/// The padded length for `len` under Padmé: at most about 12% overhead, and the
/// result reveals only about log log of the real size.
pub fn padme(len: u64) -> u64 {
    if len < 2 {
        return len;
    }
    let e = 63 - u64::from(len.leading_zeros());
    let s = 64 - u64::from(e.leading_zeros());
    let bits = e - s;
    let mask = (1u64 << bits) - 1;
    (len + mask) & !mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2id_matches_rfc_9106_shape_and_is_deterministic() {
        let a = argon2id_for_tests(b"password", b"somesaltsomesalt").unwrap();
        let b = argon2id_for_tests(b"password", b"somesaltsomesalt").unwrap();
        let c = argon2id_for_tests(b"passworD", b"somesaltsomesalt").unwrap();
        assert_eq!(*a, *b);
        assert_ne!(*a, *c);
    }

    #[test]
    fn kdf_floor_and_ceiling_are_enforced() {
        let weak = KdfParams { m_kib: 1024, t: 1, p: 1 };
        assert_eq!(argon2id(b"x", b"saltsaltsaltsalt", weak).err(), Some(Error::KdfOutOfRange));
        let huge = KdfParams { m_kib: u32::MAX, t: 3, p: 4 };
        assert_eq!(huge.check().err(), Some(Error::KdfOutOfRange));
        assert!(KdfParams::default().check().is_ok());
    }

    #[test]
    fn seal_round_trip_and_tamper_detection() {
        let key = random_key().unwrap();
        let mut sealed = seal(&key, b"secret", b"aad").unwrap();
        assert_eq!(open(&key, &sealed, b"aad").unwrap().as_slice(), b"secret");
        assert!(open(&key, &sealed, b"other").is_none());
        let wrong = random_key().unwrap();
        assert!(open(&wrong, &sealed, b"aad").is_none());
        sealed[30] ^= 1;
        assert!(open(&key, &sealed, b"aad").is_none());
    }

    #[test]
    fn seal_never_repeats_a_nonce() {
        let key = random_key().unwrap();
        assert_ne!(seal(&key, b"x", b"").unwrap(), seal(&key, b"x", b"").unwrap());
    }

    fn stream_fixture(len: usize) -> (Key, [u8; STREAM_PREFIX_LEN], Vec<u8>, Vec<u8>) {
        let key = random_key().unwrap();
        let prefix = random_array().unwrap();
        let plain = vec![7u8; len];
        let sealed = stream_seal(&key, &prefix, &plain, b"head").unwrap();
        (key, prefix, plain, sealed)
    }

    #[test]
    fn stream_round_trips_at_chunk_boundaries() {
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, CHUNK * 3] {
            let (key, prefix, plain, sealed) = stream_fixture(len);
            let opened = stream_open(&key, &prefix, &sealed, b"head").unwrap();
            assert_eq!(opened.as_slice(), plain.as_slice(), "len {len}");
        }
    }

    #[test]
    fn stream_detects_truncation_at_a_chunk_boundary() {
        let (key, prefix, _, sealed) = stream_fixture(CHUNK * 3);
        let cut = &sealed[..(CHUNK + TAG_LEN) * 2];
        assert_eq!(stream_open(&key, &prefix, cut, b"head").err(), Some(Error::Integrity));
    }

    #[test]
    fn stream_detects_reordering() {
        let (key, prefix, _, sealed) = stream_fixture(CHUNK * 2 + 10);
        let step = CHUNK + TAG_LEN;
        let mut swapped = Vec::new();
        swapped.extend_from_slice(&sealed[step..step * 2]);
        swapped.extend_from_slice(&sealed[..step]);
        swapped.extend_from_slice(&sealed[step * 2..]);
        assert_eq!(stream_open(&key, &prefix, &swapped, b"head").err(), Some(Error::Integrity));
    }

    #[test]
    fn stream_binds_its_aad() {
        let (key, prefix, _, sealed) = stream_fixture(100);
        assert_eq!(stream_open(&key, &prefix, &sealed, b"nope").err(), Some(Error::Integrity));
    }

    #[test]
    fn random_below_stays_in_range_and_reaches_every_value() {
        let mut seen = [false; 7];
        for _ in 0..2000 {
            let v = random_below(7).unwrap();
            seen[v as usize] = true;
        }
        assert!(seen.iter().all(|s| *s));
        assert!(random_below(0).is_err());
        assert_eq!(random_below(1).unwrap(), 0);
    }

    #[test]
    fn padme_never_shrinks_and_overhead_stays_small() {
        for len in [2u64, 3, 9, 100, 1000, 65_536, 1_000_000, 123_456_789] {
            let padded = padme(len);
            assert!(padded >= len);
            assert!((padded - len) as f64 / len as f64 <= 0.12, "len {len} -> {padded}");
        }
        assert_eq!(padme(0), 0);
        assert_eq!(padme(1), 1);
        // The paper's table: lengths collapse to shared sizes.
        assert_eq!(padme(1025), padme(1024 + 30));
    }
}
