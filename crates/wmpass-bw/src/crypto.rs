//! Bitwarden's key derivation and encrypted strings.

use aes::Aes256;
use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use sha1_digest010::Sha1;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::{BwError, Result};

/// Refused below this, so a hostile server cannot make a captured hash cheap to crack.
pub const MIN_PBKDF2: u32 = 100_000;
/// Refused above this, so a hostile server cannot hang the app.
pub const MAX_PBKDF2: u32 = 2_000_000;
const MIN_ARGON_MIB: u32 = 16;
const MAX_ARGON_MIB: u32 = 1024;

/// Password stretching settings, which the server stores per account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kdf {
    /// PBKDF2-SHA256.
    Pbkdf2 {
        /// Rounds.
        iterations: u32,
    },
    /// Argon2id.
    Argon2id {
        /// Passes.
        iterations: u32,
        /// Memory in MiB.
        memory_mib: u32,
        /// Lanes.
        parallelism: u32,
    },
}

impl Default for Kdf {
    fn default() -> Self {
        Self::Argon2id { iterations: 3, memory_mib: 64, parallelism: 4 }
    }
}

impl Kdf {
    /// Reads the numbers as the API sends them.
    pub fn from_api(kind: u64, iterations: u64, memory: Option<u64>, parallelism: Option<u64>) -> Result<Self> {
        let small = |value: u64| u32::try_from(value).map_err(|_| BwError::BadAnswer);
        match kind {
            0 => Ok(Self::Pbkdf2 { iterations: small(iterations)? }),
            1 => Ok(Self::Argon2id {
                iterations: small(iterations)?,
                memory_mib: small(memory.ok_or(BwError::BadAnswer)?)?,
                parallelism: small(parallelism.ok_or(BwError::BadAnswer)?)?,
            }),
            _ => Err(BwError::Unsupported("this account uses an unknown password protection")),
        }
    }

    /// Refuses settings outside the sane range.
    pub fn check(self) -> Result<Self> {
        let fine = match self {
            Self::Pbkdf2 { iterations } => (MIN_PBKDF2..=MAX_PBKDF2).contains(&iterations),
            Self::Argon2id { iterations, memory_mib, parallelism } => {
                (2..=64).contains(&iterations) && (MIN_ARGON_MIB..=MAX_ARGON_MIB).contains(&memory_mib) && (1..=16).contains(&parallelism)
            }
        };
        if fine { Ok(self) } else { Err(BwError::WeakKdf) }
    }
}

fn normal_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// A key pair for encryption and authentication, as Bitwarden uses everywhere.
#[derive(Clone, PartialEq, Eq)]
pub struct SymKey {
    enc: Zeroizing<[u8; 32]>,
    mac: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for SymKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SymKey(..)")
    }
}

/// The master key, which never leaves the device.
#[derive(Clone)]
pub struct MasterKey(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(..)")
    }
}

impl MasterKey {
    /// Stretches a password under the account's settings.
    pub fn derive(password: &[u8], email: &str, kdf: Kdf) -> Result<Self> {
        let kdf = kdf.check()?;
        let email = normal_email(email);
        let mut key = Zeroizing::new([0u8; 32]);
        match kdf {
            Kdf::Pbkdf2 { iterations } => {
                pbkdf2::pbkdf2_hmac::<Sha256>(password, email.as_bytes(), iterations, &mut key[..]);
            }
            Kdf::Argon2id { iterations, memory_mib, parallelism } => {
                let salt = Sha256::digest(email.as_bytes());
                let params = argon2::Params::new(memory_mib * 1024, iterations, parallelism, Some(32)).map_err(|_| BwError::WeakKdf)?;
                argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
                    .hash_password_into(password, &salt, &mut key[..])
                    .map_err(|_| BwError::Crypto)?;
            }
        }
        Ok(Self(key))
    }

    /// What is sent to log in. One more round over the master key, so the key itself stays here.
    pub fn login_hash(&self, password: &[u8]) -> String {
        let mut out = Zeroizing::new([0u8; 32]);
        pbkdf2::pbkdf2_hmac::<Sha256>(&self.0[..], password, 1, &mut out[..]);
        STANDARD.encode(&out[..])
    }

    /// Opens the account's own key. Most accounts hold it as type 2 under the stretched key.
    /// Accounts made years ago may still hold it as type 0: AES-CBC under the master key itself,
    /// with no tag. Bitwarden's clients read those, so this does too, for this one key and
    /// nowhere else; every item after it still has to pass its tag.
    pub fn unwrap_user_key(&self, text: &str) -> Result<SymKey> {
        let Some(body) = text.strip_prefix("0.") else {
            return self.stretch()?.unwrap_key(text);
        };
        let (iv, ciphertext) = body.split_once('|').ok_or(BwError::Crypto)?;
        let iv = STANDARD.decode(iv).map_err(|_| BwError::Crypto)?;
        let ciphertext = STANDARD.decode(ciphertext).map_err(|_| BwError::Crypto)?;
        if iv.len() != 16 {
            return Err(BwError::Crypto);
        }
        let decryptor = cbc::Decryptor::<Aes256>::new_from_slices(&self.0[..], &iv).map_err(|_| BwError::Crypto)?;
        let plain = Zeroizing::new(decryptor.decrypt_padded_vec::<Pkcs7>(&ciphertext).map_err(|_| BwError::Crypto)?);
        SymKey::from_bytes(&plain)
    }

    /// The key that wraps the account's real key.
    pub fn stretch(&self) -> Result<SymKey> {
        let expand = |info: &[u8]| -> Result<Zeroizing<[u8; 32]>> {
            // HKDF-Expand alone, as Bitwarden does: the master key is already a strong PRK.
            let prk = hkdf::Hkdf::<Sha256>::from_prk(&self.0[..]).map_err(|_| BwError::Crypto)?;
            let mut out = Zeroizing::new([0u8; 32]);
            prk.expand(info, &mut out[..]).map_err(|_| BwError::Crypto)?;
            Ok(out)
        };
        Ok(SymKey { enc: expand(b"enc")?, mac: expand(b"mac")? })
    }
}

impl SymKey {
    /// A new random key.
    pub fn generate() -> Result<Self> {
        let mut raw = Zeroizing::new([0u8; 64]);
        getrandom::fill(&mut raw[..]).map_err(|_| BwError::Crypto)?;
        Self::from_bytes(&raw[..])
    }

    /// From 64 bytes: encryption key, then authentication key.
    pub fn from_bytes(raw: &[u8]) -> Result<Self> {
        if raw.len() != 64 {
            return Err(BwError::Crypto);
        }
        let mut enc = Zeroizing::new([0u8; 32]);
        let mut mac = Zeroizing::new([0u8; 32]);
        enc.copy_from_slice(&raw[..32]);
        mac.copy_from_slice(&raw[32..]);
        Ok(Self { enc, mac })
    }

    /// As 64 bytes.
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(64));
        out.extend_from_slice(&self.enc[..]);
        out.extend_from_slice(&self.mac[..]);
        out
    }

    fn tag(&self, iv: &[u8], ciphertext: &[u8]) -> Result<[u8; 32]> {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.mac[..]).map_err(|_| BwError::Crypto)?;
        mac.update(iv);
        mac.update(ciphertext);
        let mut out = [0u8; 32];
        out.copy_from_slice(&mac.finalize().into_bytes());
        Ok(out)
    }

    /// Encrypts to an encrypted string: type 2, AES-256-CBC with HMAC-SHA256.
    pub fn encrypt(&self, plain: &[u8]) -> Result<String> {
        let mut iv = [0u8; 16];
        getrandom::fill(&mut iv).map_err(|_| BwError::Crypto)?;
        let encryptor = cbc::Encryptor::<Aes256>::new_from_slices(&self.enc[..], &iv).map_err(|_| BwError::Crypto)?;
        let ciphertext = encryptor.encrypt_padded_vec::<Pkcs7>(plain);
        let tag = self.tag(&iv, &ciphertext)?;
        Ok(format!("2.{}|{}|{}", STANDARD.encode(iv), STANDARD.encode(&ciphertext), STANDARD.encode(tag)))
    }

    /// Decrypts an encrypted string. The tag is checked before anything else touches the
    /// ciphertext, and types without a tag are refused, since CBC without one allows
    /// padding-oracle and bit-flipping attacks.
    pub fn decrypt(&self, text: &str) -> Result<Zeroizing<Vec<u8>>> {
        let body = text.strip_prefix("2.").ok_or(BwError::Crypto)?;
        let mut parts = body.split('|');
        let (Some(iv), Some(ciphertext), Some(tag), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(BwError::Crypto);
        };
        let iv = STANDARD.decode(iv).map_err(|_| BwError::Crypto)?;
        let ciphertext = STANDARD.decode(ciphertext).map_err(|_| BwError::Crypto)?;
        let tag = STANDARD.decode(tag).map_err(|_| BwError::Crypto)?;
        if iv.len() != 16 || !bool::from(self.tag(&iv, &ciphertext)?.as_slice().ct_eq(&tag)) {
            return Err(BwError::Crypto);
        }
        let decryptor = cbc::Decryptor::<Aes256>::new_from_slices(&self.enc[..], &iv).map_err(|_| BwError::Crypto)?;
        decryptor.decrypt_padded_vec::<Pkcs7>(&ciphertext).map(Zeroizing::new).map_err(|_| BwError::Crypto)
    }

    /// Decrypts to text.
    pub fn decrypt_text(&self, text: &str) -> Result<wmpass_core::Secret> {
        let plain = self.decrypt(text)?;
        let text = std::str::from_utf8(&plain).map_err(|_| BwError::Crypto)?;
        Ok(wmpass_core::Secret::new(text))
    }

    /// Unwraps another key held in an encrypted string.
    pub fn unwrap_key(&self, text: &str) -> Result<SymKey> {
        Self::from_bytes(&self.decrypt(text)?)
    }
}

/// A new account key pair: the public key to publish, and the private key wrapped under
/// the account key.
pub fn generate_key_pair(account: &SymKey) -> Result<(String, String)> {
    let private = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).map_err(|_| BwError::Crypto)?;
    let public = RsaPublicKey::from(&private);
    let public = public.to_public_key_der().map_err(|_| BwError::Crypto)?;
    let private = private.to_pkcs8_der().map_err(|_| BwError::Crypto)?;
    Ok((STANDARD.encode(public.as_bytes()), account.encrypt(private.as_bytes())?))
}

/// Opens a key an organisation wrapped to this account's public key.
pub fn unwrap_with_private(private_der: &[u8], text: &str) -> Result<SymKey> {
    let body = text.strip_prefix("4.").ok_or(BwError::Crypto)?;
    let sealed = STANDARD.decode(body).map_err(|_| BwError::Crypto)?;
    let key = RsaPrivateKey::from_pkcs8_der(private_der).map_err(|_| BwError::Crypto)?;
    let plain = key.decrypt(Oaep::new::<Sha1>(), &sealed).map_err(|_| BwError::Crypto).map(Zeroizing::new)?;
    SymKey::from_bytes(&plain)
}

/// Wraps a key to a published public key.
pub fn wrap_to_public(public_b64: &str, key: &SymKey) -> Result<String> {
    let der = STANDARD.decode(public_b64).map_err(|_| BwError::Crypto)?;
    let public = RsaPublicKey::from_public_key_der(&der).map_err(|_| BwError::Crypto)?;
    let sealed = public
        .encrypt(&mut rsa::rand_core::OsRng, Oaep::new::<Sha1>(), &key.to_bytes())
        .map_err(|_| BwError::Crypto)?;
    Ok(format!("4.{}", STANDARD.encode(sealed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PBKDF: Kdf = Kdf::Pbkdf2 { iterations: 100_000 };

    #[test]
    fn encrypted_strings_round_trip_and_catch_any_change() {
        let key = SymKey::generate().unwrap();
        let text = key.encrypt(b"correct horse").unwrap();
        assert!(text.starts_with("2."));
        assert_eq!(&key.decrypt(&text).unwrap()[..], b"correct horse");
        assert_ne!(text, key.encrypt(b"correct horse").unwrap(), "a fresh iv every time");

        let other = SymKey::generate().unwrap();
        assert_eq!(other.decrypt(&text).err(), Some(BwError::Crypto));
        let mut flipped = text.clone().into_bytes();
        let index = flipped.len() / 2;
        flipped[index] = if flipped[index] == b'A' { b'B' } else { b'A' };
        assert!(key.decrypt(std::str::from_utf8(&flipped).unwrap()).is_err());
    }

    #[test]
    fn strings_without_authentication_are_refused() {
        let key = SymKey::generate().unwrap();
        let text = key.encrypt(b"x").unwrap();
        let body = text.strip_prefix("2.").unwrap();
        let without_mac = body.rsplit_once('|').unwrap().0;
        assert!(key.decrypt(&format!("0.{without_mac}")).is_err());
        assert!(key.decrypt(&format!("2.{without_mac}")).is_err());
        assert!(key.decrypt("garbage").is_err());
    }

    #[test]
    fn weak_settings_from_a_server_are_refused() {
        for weak in [
            Kdf::Pbkdf2 { iterations: 1 },
            Kdf::Pbkdf2 { iterations: 5000 },
            Kdf::Argon2id { iterations: 1, memory_mib: 1, parallelism: 1 },
            Kdf::Argon2id { iterations: 3, memory_mib: 8, parallelism: 1 },
        ] {
            assert_eq!(MasterKey::derive(b"pw", "a@b.c", weak).err().unwrap(), BwError::WeakKdf);
        }
        assert_eq!(Kdf::Pbkdf2 { iterations: u32::MAX }.check().err(), Some(BwError::WeakKdf));
        assert_eq!(Kdf::from_api(7, 1, None, None).err(), Some(BwError::Unsupported("this account uses an unknown password protection")));
    }

    #[test]
    fn the_email_is_normalised_and_the_hash_differs_from_the_key() {
        let a = MasterKey::derive(b"pw", " Me@Example.com ", PBKDF).unwrap();
        let b = MasterKey::derive(b"pw", "me@example.com", PBKDF).unwrap();
        assert_eq!(a.login_hash(b"pw"), b.login_hash(b"pw"));
        assert_ne!(a.login_hash(b"pw"), STANDARD.encode(&a.0[..]));
        assert_ne!(MasterKey::derive(b"other", "me@example.com", PBKDF).unwrap().login_hash(b"other"), b.login_hash(b"pw"));
    }

    #[test]
    fn an_old_accounts_key_under_the_master_key_opens() {
        let master = MasterKey::derive(b"pw", "me@example.com", PBKDF).unwrap();
        let account = SymKey::generate().unwrap();
        let iv = [7u8; 16];
        let encryptor = cbc::Encryptor::<Aes256>::new_from_slices(&master.0[..], &iv).unwrap();
        let sealed = encryptor.encrypt_padded_vec::<Pkcs7>(&account.to_bytes());
        let legacy = format!("0.{}|{}", STANDARD.encode(iv), STANDARD.encode(&sealed));
        assert_eq!(master.unwrap_user_key(&legacy).unwrap(), account);
        let current = master.stretch().unwrap().encrypt(&account.to_bytes()).unwrap();
        assert_eq!(master.unwrap_user_key(&current).unwrap(), account);
        let other = MasterKey::derive(b"other", "me@example.com", PBKDF).unwrap();
        assert!(other.unwrap_user_key(&legacy).is_err());
        assert!(other.unwrap_user_key(&current).is_err());
    }

    #[test]
    fn organisation_keys_wrap_and_unwrap() {
        let account = SymKey::generate().unwrap();
        let (public, wrapped_private) = generate_key_pair(&account).unwrap();
        let private = account.decrypt(&wrapped_private).unwrap();
        let organisation = SymKey::generate().unwrap();
        let sealed = wrap_to_public(&public, &organisation).unwrap();
        assert!(sealed.starts_with("4."));
        assert_eq!(unwrap_with_private(&private, &sealed).unwrap(), organisation);
    }
}
