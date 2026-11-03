//! Every primitive SMB and NTLM need. MD4 (the NT password hash), AES-CMAC (SMB 3
//! signing) and AES-CCM (SMB 3.0 encryption) are why these are RustCrypto crates and not
//! whatever a platform happens to ship.

use aes::Aes128;
use aes_gcm::aead::array::Array;
use aes_gcm::aead::consts::{U11, U12, U16};
use aes_gcm::aead::{AeadInOut, KeyInit as AeadKeyInit};
use aes_gcm::AesGcm;
use ccm::Ccm;
use cmac::Cmac;
use hmac::{Hmac, KeyInit, Mac};
use md4::Md4;
use md5::Md5;
use sha2::{Digest, Sha256, Sha512};
use subtle::ConstantTimeEq;

pub(crate) fn md4(data: &[u8]) -> [u8; 16] {
    Md4::digest(data).into()
}

pub(crate) fn md5(parts: &[&[u8]]) -> [u8; 16] {
    let mut hash = Md5::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}

pub(crate) fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut hash = Sha512::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}

pub(crate) fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    // HMAC takes a key of any length, so this cannot fail; an empty answer would only
    // ever be a sign-in the server refuses.
    let Ok(mut mac) = <Hmac<Md5> as KeyInit>::new_from_slice(key) else { return [0; 16] };
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

pub(crate) fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let Ok(mut mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(key) else { return [0; 32] };
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// AES-128-CMAC, RFC 4493: the SMB 3.x signature.
pub(crate) fn aes_cmac(key: &[u8; 16], data: &[u8]) -> [u8; 16] {
    let Ok(mut mac) = <Cmac<Aes128> as KeyInit>::new_from_slice(key) else { return [0; 16] };
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// RC4, which NTLM still uses to wrap the session key and the signature checksum.
/// Each use here is one fresh keystream, so this is a function rather than a stream.
pub(crate) fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    if key.is_empty() {
        return data.to_vec();
    }
    let mut s: [u8; 256] = core::array::from_fn(|i| i as u8);
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, usize::from(j));
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|byte| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[usize::from(i)]);
            s.swap(usize::from(i), usize::from(j));
            byte ^ s[usize::from(s[usize::from(i)].wrapping_add(s[usize::from(j)]))]
        })
        .collect()
}

/// SP 800-108 in counter mode over HMAC-SHA256, one round, 128 bits out: the only shape
/// SMB 3 uses. `label` and `context` come with their NUL terminators already on, exactly
/// as MS-SMB2 writes them.
pub(crate) fn kdf(key: &[u8], label: &[u8], context: &[u8]) -> [u8; 16] {
    let out = hmac_sha256(key, &[&[0, 0, 0, 1], label, &[0], context, &[0, 0, 0, 128]]);
    let mut key = [0u8; 16];
    key.copy_from_slice(&out[..16]);
    key
}

/// Which AEAD cipher a session negotiated, by its MS-SMB2 id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cipher {
    Aes128Ccm,
    Aes128Gcm,
}

impl Cipher {
    pub(crate) fn id(self) -> u16 {
        match self {
            Self::Aes128Ccm => 1,
            Self::Aes128Gcm => 2,
        }
    }

    pub(crate) fn of(id: u16) -> Option<Self> {
        match id {
            1 => Some(Self::Aes128Ccm),
            2 => Some(Self::Aes128Gcm),
            _ => None,
        }
    }

    /// How much of the transform header's 16-byte nonce field the cipher uses.
    pub(crate) fn nonce_len(self) -> usize {
        match self {
            Self::Aes128Ccm => 11,
            Self::Aes128Gcm => 12,
        }
    }
}

/// A tag that did not match, or a key of the wrong size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Refused;

type Ccm128 = Ccm<Aes128, U16, U11>;
type Gcm128 = AesGcm<Aes128, U12>;

/// Encrypts `data` in place and returns the 16-byte tag.
pub(crate) fn seal(cipher: Cipher, key: &[u8], nonce: &[u8], aad: &[u8], data: &mut [u8]) -> Result<[u8; 16], Refused> {
    let tag = match cipher {
        Cipher::Aes128Ccm => {
            let aead = <Ccm128 as AeadKeyInit>::new_from_slice(key).map_err(|_| Refused)?;
            let nonce = Array::try_from(nonce.get(..11).ok_or(Refused)?).map_err(|_| Refused)?;
            aead.encrypt_inout_detached(&nonce, aad, data.into()).map_err(|_| Refused)?
        }
        Cipher::Aes128Gcm => {
            let aead = <Gcm128 as AeadKeyInit>::new_from_slice(key).map_err(|_| Refused)?;
            let nonce = Array::try_from(nonce.get(..12).ok_or(Refused)?).map_err(|_| Refused)?;
            aead.encrypt_inout_detached(&nonce, aad, data.into()).map_err(|_| Refused)?
        }
    };
    let mut out = [0u8; 16];
    out.copy_from_slice(&tag);
    Ok(out)
}

/// The reverse of [`seal`]. Refuses when the tag does not match, and then `data` is not
/// to be used.
pub(crate) fn open(cipher: Cipher, key: &[u8], nonce: &[u8], aad: &[u8], data: &mut [u8], tag: &[u8]) -> Result<(), Refused> {
    let tag: Array<u8, U16> = Array::try_from(tag).map_err(|_| Refused)?;
    match cipher {
        Cipher::Aes128Ccm => {
            let aead = <Ccm128 as AeadKeyInit>::new_from_slice(key).map_err(|_| Refused)?;
            let nonce = Array::try_from(nonce.get(..11).ok_or(Refused)?).map_err(|_| Refused)?;
            aead.decrypt_inout_detached(&nonce, aad, data.into(), &tag).map_err(|_| Refused)
        }
        Cipher::Aes128Gcm => {
            let aead = <Gcm128 as AeadKeyInit>::new_from_slice(key).map_err(|_| Refused)?;
            let nonce = Array::try_from(nonce.get(..12).ok_or(Refused)?).map_err(|_| Refused)?;
            aead.decrypt_inout_detached(&nonce, aad, data.into(), &tag).map_err(|_| Refused)
        }
    }
}

/// Constant time, so a signature check does not leak how much of it matched.
pub(crate) fn equal(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// Bytes from the OS random source. A failure is an error: there is no safe fallback.
pub(crate) fn random<const N: usize>() -> Result<[u8; N], Refused> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| Refused)?;
    Ok(out)
}
