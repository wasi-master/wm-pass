//! `.wmvault` files. The spec for other implementers is `docs/format.md`.
//!
//! The header has a fixed size and every record in it looks like random bytes,
//! so a reader without a password cannot count unlock methods, see a duress slot,
//! or tell a vault from a chaff file.

use zeroize::Zeroizing;

use crate::crypto::{self, CHUNK, KdfParams, Key, NONCE_LEN, STREAM_PREFIX_LEN, TAG_LEN};
use crate::{Error, Result};

/// First bytes of every vault file.
pub const MAGIC: &[u8; 4] = b"WMPV";
/// The only format this build writes and reads.
pub const VERSION: u16 = 1;
/// Slot records per file. Fixed, so the count reveals nothing.
pub const SLOTS: usize = 8;

const SALT_LEN: usize = 32;
const VAULT_ID_LEN: usize = 16;
const SLOT_PLAIN: usize = 2 + 32;
const SLOT_LEN: usize = NONCE_LEN + SLOT_PLAIN + TAG_LEN;
const HINT_PLAIN: usize = 216;
const HINT_LEN: usize = NONCE_LEN + HINT_PLAIN + TAG_LEN;
const WRAPPED_KEY_LEN: usize = NONCE_LEN + 32 + TAG_LEN;
const MAC_LEN: usize = 32;

const OFF_KDF: usize = 8;
const OFF_SALT: usize = OFF_KDF + 12;
const OFF_ID: usize = OFF_SALT + SALT_LEN;
const OFF_SLOTS: usize = OFF_ID + VAULT_ID_LEN;
const OFF_HINTS: usize = OFF_SLOTS + SLOTS * SLOT_LEN;
const OFF_WRAPPED: usize = OFF_HINTS + SLOTS * HINT_LEN;
const OFF_PREFIX: usize = OFF_WRAPPED + WRAPPED_KEY_LEN;
const OFF_MAC: usize = OFF_PREFIX + STREAM_PREFIX_LEN;
/// Total header bytes, MAC included.
pub const HEADER_LEN: usize = OFF_MAC + MAC_LEN;

const INFO_SLOT: &str = "wmpass/slot/v1";
const INFO_HINT: &str = "wmpass/hint/v1";
const INFO_HEADER: &str = "wmpass/header/v1";
const INFO_BODY: &str = "wmpass/body/v1";

/// What the app does when this slot's password is typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotAction {
    /// Opens this file normally. A decoy vault is simply a separate file opened
    /// this way, so nothing inside it marks it as a decoy.
    Open,
    /// Duress: delete local vaults and hardware-wrapped keys.
    WipeLocal,
    /// Duress: lock hard and raise the prepared notice to trustees.
    HardLock,
}

impl SlotAction {
    fn to_byte(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::WipeLocal => 1,
            Self::HardLock => 2,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Open),
            1 => Some(Self::WipeLocal),
            2 => Some(Self::HardLock),
            _ => None,
        }
    }
}

/// A second factor a slot needs on top of the password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FactorKind {
    /// A key file.
    KeyFile,
    /// A FIDO2 key using hmac-secret. Ids are not secret but sit under the hint
    /// key, so a reader without the password cannot see a hardware key is in use.
    Fido2 {
        /// Credential id on the authenticator.
        credential_id: Vec<u8>,
        /// Salt sent to hmac-secret.
        salt: [u8; 32],
    },
    /// A YubiKey HMAC-SHA1 challenge-response slot.
    YubiKey {
        /// Challenge sent to the key.
        challenge: [u8; 32],
        /// OTP slot on the key, 1 or 2.
        slot: u8,
    },
}

impl FactorKind {
    fn encode(&self) -> Result<Zeroizing<[u8; HINT_PLAIN]>> {
        let mut out = Zeroizing::new([0u8; HINT_PLAIN]);
        match self {
            Self::KeyFile => out[0] = 1,
            Self::Fido2 {
                credential_id,
                salt,
            } => {
                if credential_id.len() > HINT_PLAIN - 35 {
                    return Err(Error::TooLong);
                }
                out[0] = 2;
                let len = u16::try_from(credential_id.len()).map_err(|_| Error::TooLong)?;
                out[1..3].copy_from_slice(&len.to_le_bytes());
                out[3..35].copy_from_slice(salt);
                out[35..35 + credential_id.len()].copy_from_slice(credential_id);
            }
            Self::YubiKey { challenge, slot } => {
                out[0] = 3;
                out[1] = *slot;
                out[2..34].copy_from_slice(challenge);
            }
        }
        Ok(out)
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        match *bytes.first()? {
            1 => Some(Self::KeyFile),
            2 => {
                let len = usize::from(u16::from_le_bytes(bytes.get(1..3)?.try_into().ok()?));
                let salt = bytes.get(3..35)?.try_into().ok()?;
                let credential_id = bytes.get(35..35 + len)?.to_vec();
                Some(Self::Fido2 {
                    credential_id,
                    salt,
                })
            }
            3 => {
                let slot = *bytes.get(1)?;
                let challenge = bytes.get(2..34)?.try_into().ok()?;
                Some(Self::YubiKey { challenge, slot })
            }
            _ => None,
        }
    }
}

/// The output of a second factor, as bytes. Device I/O lives in other crates;
/// a key file is hashed by [`key_file_factor`].
pub type FactorOutput = Zeroizing<Vec<u8>>;

/// Turns a key file's contents into a factor output.
pub fn key_file_factor(contents: &[u8]) -> FactorOutput {
    Zeroizing::new(blake3::hash(contents).as_bytes().to_vec())
}

/// The KDF settings and salt shared by a set of vault files, so a password is
/// derived once and then tried against all of them. Unlock time therefore does
/// not reveal how many files, or which kind, are present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KdfConfig {
    /// Cost settings.
    pub params: KdfParams,
    /// Shared salt.
    pub salt: [u8; 32],
}

impl KdfConfig {
    /// A fresh config with a random salt.
    pub fn new(params: KdfParams) -> Result<Self> {
        Ok(Self {
            params: params.check()?,
            salt: crypto::random_array()?,
        })
    }

    /// Derives the root key for a typed password.
    pub fn derive(&self, password: &[u8]) -> Result<RootKey> {
        crypto::argon2id(password, &self.salt, self.params).map(RootKey)
    }

    #[cfg(test)]
    pub(crate) fn derive_for_tests(&self, password: &[u8]) -> Result<RootKey> {
        crypto::argon2id_for_tests(password, &self.salt).map(RootKey)
    }
}

/// A password after Argon2id. Holding it avoids re-deriving per file.
pub struct RootKey(Key);

impl std::fmt::Debug for RootKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RootKey(***)")
    }
}

impl RootKey {
    /// A root key from a value that is already high entropy, such as a recovery
    /// key rebuilt from Shamir shares. Never pass a human-chosen password here.
    pub fn from_high_entropy(secret: &[u8; 32]) -> Result<Self> {
        crypto::hkdf(secret, b"wmpass", "wmpass/recovery-root/v1").map(Self)
    }

    fn slot_key(&self, vault_id: &[u8], factor: Option<&[u8]>) -> Result<Key> {
        match factor {
            None => crypto::hkdf(&self.0[..], vault_id, INFO_SLOT),
            Some(extra) => {
                let mut ikm = Zeroizing::new(Vec::with_capacity(32 + extra.len()));
                ikm.extend_from_slice(&self.0[..]);
                ikm.extend_from_slice(extra);
                crypto::hkdf(&ikm, vault_id, INFO_SLOT)
            }
        }
    }

    fn hint_key(&self, vault_id: &[u8]) -> Result<Key> {
        crypto::hkdf(&self.0[..], vault_id, INFO_HINT)
    }
}

fn record_aad(vault_id: &[u8], index: usize) -> [u8; VAULT_ID_LEN + 1] {
    let mut aad = [0u8; VAULT_ID_LEN + 1];
    aad[..VAULT_ID_LEN].copy_from_slice(vault_id);
    aad[VAULT_ID_LEN] = index as u8;
    aad
}

/// A vault file's header. Holds no plaintext secrets.
#[derive(Clone)]
pub struct Header {
    bytes: Vec<u8>,
}

impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("vault_id", &self.vault_id())
            .finish_non_exhaustive()
    }
}

/// Outcome of trying a password on a file.
#[derive(Debug)]
pub enum Unlock {
    /// A slot accepted the password.
    Opened {
        /// The file's key.
        key: VaultKey,
        /// What the matched slot asks for.
        action: SlotAction,
    },
    /// The password is right but a second factor is needed. Only reachable with
    /// the right password, because the hints are sealed under it.
    NeedsFactor(Vec<FactorKind>),
    /// Nothing matched. The same answer for every other reason, by design.
    NoMatch,
}

/// A vault's master key. Sub-keys come from it; slots wrap it.
pub struct VaultKey(pub(crate) Key);

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultKey(***)")
    }
}

impl VaultKey {
    /// A new random key.
    pub fn generate() -> Result<Self> {
        crypto::random_key().map(Self)
    }

    /// Rebuilds from bytes a platform keystore unwrapped.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// The raw key, only for wrapping by a hardware keystore.
    pub fn expose_for_hardware_wrap(&self) -> &[u8; 32] {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn clone_key(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Header {
    /// A new header with every slot and hint filled with random bytes.
    pub fn new(config: &KdfConfig) -> Result<Self> {
        let params = config.params.check()?;
        let mut bytes = vec![0u8; HEADER_LEN];
        crypto::fill_random(&mut bytes)?;
        bytes[..4].copy_from_slice(MAGIC);
        bytes[4..6].copy_from_slice(&VERSION.to_le_bytes());
        bytes[6..8].fill(0);
        bytes[OFF_KDF..OFF_KDF + 4].copy_from_slice(&params.m_kib.to_le_bytes());
        bytes[OFF_KDF + 4..OFF_KDF + 8].copy_from_slice(&params.t.to_le_bytes());
        bytes[OFF_KDF + 8..OFF_KDF + 12].copy_from_slice(&params.p.to_le_bytes());
        bytes[OFF_SALT..OFF_ID].copy_from_slice(&config.salt);
        Ok(Self { bytes })
    }

    /// Parses the fixed part of a file. Nothing here is authenticated until a
    /// slot opens and [`Header::verify`] passes; the KDF range is checked first
    /// so a hostile file cannot weaken derivation or exhaust memory.
    pub fn parse(file: &[u8]) -> Result<Self> {
        if file.len() < HEADER_LEN || &file[..4] != MAGIC {
            return Err(Error::Malformed);
        }
        let version = u16::from_le_bytes([file[4], file[5]]);
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let header = Self {
            bytes: file[..HEADER_LEN].to_vec(),
        };
        header.config()?;
        Ok(header)
    }

    /// The KDF settings and salt, after range checks.
    pub fn config(&self) -> Result<KdfConfig> {
        let field = |at: usize| -> Result<u32> {
            let raw = self.bytes.get(at..at + 4).ok_or(Error::Malformed)?;
            Ok(u32::from_le_bytes(
                raw.try_into().map_err(|_| Error::Malformed)?,
            ))
        };
        let params = KdfParams {
            m_kib: field(OFF_KDF)?,
            t: field(OFF_KDF + 4)?,
            p: field(OFF_KDF + 8)?,
        };
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&self.bytes[OFF_SALT..OFF_ID]);
        Ok(KdfConfig {
            params: params.check()?,
            salt,
        })
    }

    /// The random id of this file.
    pub fn vault_id(&self) -> &[u8] {
        &self.bytes[OFF_ID..OFF_SLOTS]
    }

    fn slot(&self, index: usize) -> &[u8] {
        let at = OFF_SLOTS + index * SLOT_LEN;
        &self.bytes[at..at + SLOT_LEN]
    }

    fn hint(&self, index: usize) -> &[u8] {
        let at = OFF_HINTS + index * HINT_LEN;
        &self.bytes[at..at + HINT_LEN]
    }

    fn open_slot(&self, index: usize, slot_key: &Key) -> Option<(VaultKey, SlotAction)> {
        let aad = record_aad(self.vault_id(), index);
        let plain = crypto::open(slot_key, self.slot(index), &aad)?;
        let action = SlotAction::from_byte(*plain.get(1)?)?;
        let key: [u8; 32] = plain.get(2..34)?.try_into().ok()?;
        Some((VaultKey::from_bytes(key), action))
    }

    /// Tries a password, with any second-factor outputs the caller already has.
    /// Every record is tried even after a match, so timing stays uniform.
    pub fn unlock(&self, root: &RootKey, factors: &[FactorOutput]) -> Result<Unlock> {
        let id = self.vault_id().to_vec();
        let plain_key = root.slot_key(&id, None)?;
        let hint_key = root.hint_key(&id)?;
        let mut factor_keys = Vec::with_capacity(factors.len());
        for factor in factors {
            factor_keys.push(root.slot_key(&id, Some(factor))?);
        }
        let mut opened = None;
        let mut needed = Vec::new();
        for index in 0..SLOTS {
            if let Some(found) = self.open_slot(index, &plain_key) {
                opened.get_or_insert(found);
            }
            for key in &factor_keys {
                if let Some(found) = self.open_slot(index, key) {
                    opened.get_or_insert(found);
                }
            }
            let aad = record_aad(&id, index);
            if let Some(hint) = crypto::open(&hint_key, self.hint(index), &aad)
                && let Some(kind) = FactorKind::decode(&hint)
            {
                needed.push(kind);
            }
        }
        match opened {
            Some((key, action)) => {
                self.verify(&key)?;
                Ok(Unlock::Opened { key, action })
            }
            None if !needed.is_empty() => Ok(Unlock::NeedsFactor(needed)),
            None => Ok(Unlock::NoMatch),
        }
    }

    /// Installs an unlock method in a slot. The caller tracks which slots are in
    /// use inside the encrypted body, because the header cannot show it.
    pub fn write_slot(
        &mut self,
        index: usize,
        root: &RootKey,
        key: &VaultKey,
        action: SlotAction,
        factor: Option<(&FactorKind, &FactorOutput)>,
    ) -> Result<()> {
        if index >= SLOTS {
            return Err(Error::NoFreeSlot);
        }
        let id = self.vault_id().to_vec();
        let aad = record_aad(&id, index);
        let slot_key = root.slot_key(&id, factor.map(|(_, output)| output.as_slice()))?;
        let mut plain = Zeroizing::new([0u8; SLOT_PLAIN]);
        plain[0] = 1;
        plain[1] = action.to_byte();
        plain[2..34].copy_from_slice(&key.0[..]);
        let sealed = crypto::seal(&slot_key, &plain[..], &aad)?;
        let at = OFF_SLOTS + index * SLOT_LEN;
        self.bytes[at..at + SLOT_LEN].copy_from_slice(&sealed);
        let hint_at = OFF_HINTS + index * HINT_LEN;
        match factor {
            Some((kind, _)) => {
                let hint_key = root.hint_key(&id)?;
                let encoded = kind.encode()?;
                let sealed = crypto::seal(&hint_key, &encoded[..], &aad)?;
                self.bytes[hint_at..hint_at + HINT_LEN].copy_from_slice(&sealed);
            }
            None => crypto::fill_random(&mut self.bytes[hint_at..hint_at + HINT_LEN])?,
        }
        Ok(())
    }

    /// Clears a slot back to random bytes.
    pub fn clear_slot(&mut self, index: usize) -> Result<()> {
        if index >= SLOTS {
            return Err(Error::NoFreeSlot);
        }
        let at = OFF_SLOTS + index * SLOT_LEN;
        crypto::fill_random(&mut self.bytes[at..at + SLOT_LEN])?;
        let hint_at = OFF_HINTS + index * HINT_LEN;
        crypto::fill_random(&mut self.bytes[hint_at..hint_at + HINT_LEN])
    }

    fn header_key(key: &VaultKey, id: &[u8]) -> Result<Key> {
        crypto::hkdf(&key.0[..], id, INFO_HEADER)
    }

    /// Checks the MAC over the whole header. Runs before any body is decrypted.
    pub fn verify(&self, key: &VaultKey) -> Result<()> {
        let mac_key = Self::header_key(key, self.vault_id())?;
        crypto::hmac_sha256_verify(&mac_key[..], &self.bytes[..OFF_MAC], &self.bytes[OFF_MAC..])
    }

    fn sign(&mut self, key: &VaultKey) -> Result<()> {
        let mac_key = Self::header_key(key, self.vault_id())?;
        let mac = crypto::hmac_sha256(&mac_key[..], &self.bytes[..OFF_MAC])?;
        self.bytes[OFF_MAC..].copy_from_slice(&mac);
        Ok(())
    }
}

/// Encrypts `plain` into a complete file. A new file key and nonce prefix are
/// used every time, so no nonce can repeat across saves.
pub fn seal_file(header: &Header, key: &VaultKey, plain: &[u8]) -> Result<Vec<u8>> {
    let mut header = header.clone();
    let body_key = crypto::hkdf(&key.0[..], header.vault_id(), INFO_BODY)?;
    let file_key = crypto::random_key()?;
    let wrapped = crypto::seal(&body_key, &file_key[..], header.vault_id())?;
    header.bytes[OFF_WRAPPED..OFF_PREFIX].copy_from_slice(&wrapped);
    let prefix: [u8; STREAM_PREFIX_LEN] = crypto::random_array()?;
    header.bytes[OFF_PREFIX..OFF_MAC].copy_from_slice(&prefix);
    header.sign(key)?;

    let padded = pad(plain)?;
    let aad = header.bytes[OFF_MAC..].to_vec();
    let body = crypto::stream_seal(&file_key, &prefix, &padded, &aad)?;
    let mut out = header.bytes;
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decrypts a whole file with an already opened key. Fails unless the header MAC
/// and every chunk verify.
pub fn open_file(file: &[u8], key: &VaultKey) -> Result<(Header, Zeroizing<Vec<u8>>)> {
    let header = Header::parse(file)?;
    header.verify(key)?;
    let body_key = crypto::hkdf(&key.0[..], header.vault_id(), INFO_BODY)?;
    let file_key = crypto::open(
        &body_key,
        &header.bytes[OFF_WRAPPED..OFF_PREFIX],
        header.vault_id(),
    )
    .ok_or(Error::Integrity)?;
    let file_key: [u8; 32] = file_key
        .as_slice()
        .try_into()
        .map_err(|_| Error::Integrity)?;
    let file_key = Zeroizing::new(file_key);
    let mut prefix = [0u8; STREAM_PREFIX_LEN];
    prefix.copy_from_slice(&header.bytes[OFF_PREFIX..OFF_MAC]);
    let aad = header.bytes[OFF_MAC..].to_vec();
    let padded = crypto::stream_open(&file_key, &prefix, &file[HEADER_LEN..], &aad)?;
    let plain = unpad(&padded)?;
    Ok((header, plain))
}

fn pad(plain: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let framed = plain.len() as u64 + 8;
    let total = crypto::padme(framed);
    let total = usize::try_from(total).map_err(|_| Error::TooLong)?;
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    out.extend_from_slice(&(plain.len() as u64).to_le_bytes());
    out.extend_from_slice(plain);
    out.resize(total, 0);
    Ok(out)
}

fn unpad(padded: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let raw = padded.get(..8).ok_or(Error::Integrity)?;
    let len = u64::from_le_bytes(raw.try_into().map_err(|_| Error::Integrity)?);
    let len = usize::try_from(len).map_err(|_| Error::Integrity)?;
    let end = len.checked_add(8).ok_or(Error::Integrity)?;
    let body = padded.get(8..end).ok_or(Error::Integrity)?;
    Ok(Zeroizing::new(body.to_vec()))
}

/// A file of random bytes with a real-looking public header and a plausible
/// size, which stands beside real vaults so the file list reveals nothing.
pub fn chaff_file(config: &KdfConfig, plausible_plain_len: usize) -> Result<Vec<u8>> {
    let header = Header::new(config)?;
    let padded = crypto::padme(plausible_plain_len as u64 + 8);
    let padded = usize::try_from(padded).map_err(|_| Error::TooLong)?;
    let chunks = padded.div_ceil(CHUNK).max(1);
    let mut out = header.bytes;
    let at = out.len();
    out.resize(at + padded + chunks * TAG_LEN, 0);
    crypto::fill_random(&mut out[at..])?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> KdfConfig {
        KdfConfig::new(KdfParams::default()).unwrap()
    }

    fn vault(config: &KdfConfig, password: &[u8]) -> (Header, VaultKey) {
        let mut header = Header::new(config).unwrap();
        let key = VaultKey::generate().unwrap();
        let root = config.derive_for_tests(password).unwrap();
        header
            .write_slot(0, &root, &key, SlotAction::Open, None)
            .unwrap();
        (header, key)
    }

    fn open_with(file: &[u8], password: &[u8], factors: &[FactorOutput]) -> Unlock {
        let header = Header::parse(file).unwrap();
        let root = header.config().unwrap().derive_for_tests(password).unwrap();
        header.unlock(&root, factors).unwrap()
    }

    #[test]
    fn round_trip_with_password() {
        let config = config();
        let (header, key) = vault(&config, b"correct horse");
        let file = seal_file(&header, &key, b"hello vault").unwrap();
        let Unlock::Opened {
            key: opened,
            action,
        } = open_with(&file, b"correct horse", &[])
        else {
            panic!("expected to open");
        };
        assert_eq!(action, SlotAction::Open);
        let (_, plain) = open_file(&file, &opened).unwrap();
        assert_eq!(plain.as_slice(), b"hello vault");
    }

    #[test]
    fn wrong_password_is_no_match() {
        let config = config();
        let (header, key) = vault(&config, b"right");
        let file = seal_file(&header, &key, b"x").unwrap();
        assert!(matches!(open_with(&file, b"wrong", &[]), Unlock::NoMatch));
    }

    #[test]
    fn duress_slot_reports_its_action_and_looks_like_any_other() {
        let config = config();
        let (mut header, key) = vault(&config, b"real");
        let root = config.derive_for_tests(b"duress").unwrap();
        header
            .write_slot(3, &root, &key, SlotAction::WipeLocal, None)
            .unwrap();
        let file = seal_file(&header, &key, b"x").unwrap();
        let Unlock::Opened { action, .. } = open_with(&file, b"duress", &[]) else {
            panic!("expected to open");
        };
        assert_eq!(action, SlotAction::WipeLocal);
    }

    #[test]
    fn factor_slot_needs_the_factor_and_only_the_right_password_learns_that() {
        let config = config();
        let mut header = Header::new(&config).unwrap();
        let key = VaultKey::generate().unwrap();
        let root = config.derive_for_tests(b"pw").unwrap();
        let output = key_file_factor(b"the key file");
        header
            .write_slot(
                0,
                &root,
                &key,
                SlotAction::Open,
                Some((&FactorKind::KeyFile, &output)),
            )
            .unwrap();
        let file = seal_file(&header, &key, b"x").unwrap();

        assert!(matches!(open_with(&file, b"bad", &[]), Unlock::NoMatch));
        match open_with(&file, b"pw", &[]) {
            Unlock::NeedsFactor(kinds) => assert_eq!(kinds, vec![FactorKind::KeyFile]),
            other => panic!("expected a factor request, got {other:?}"),
        }
        let wrong = key_file_factor(b"another file");
        assert!(matches!(
            open_with(&file, b"pw", &[wrong]),
            Unlock::NeedsFactor(_)
        ));
        assert!(matches!(
            open_with(&file, b"pw", &[output]),
            Unlock::Opened { .. }
        ));
    }

    #[test]
    fn hardware_hints_round_trip() {
        let config = config();
        let mut header = Header::new(&config).unwrap();
        let key = VaultKey::generate().unwrap();
        let root = config.derive_for_tests(b"pw").unwrap();
        let fido = FactorKind::Fido2 {
            credential_id: vec![9; 64],
            salt: [4; 32],
        };
        let yubi = FactorKind::YubiKey {
            challenge: [5; 32],
            slot: 2,
        };
        let output: FactorOutput = Zeroizing::new(vec![1; 32]);
        header
            .write_slot(1, &root, &key, SlotAction::Open, Some((&fido, &output)))
            .unwrap();
        header
            .write_slot(2, &root, &key, SlotAction::Open, Some((&yubi, &output)))
            .unwrap();
        let file = seal_file(&header, &key, b"x").unwrap();
        match open_with(&file, b"pw", &[]) {
            Unlock::NeedsFactor(kinds) => assert_eq!(kinds, vec![fido, yubi]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn over_long_credential_id_is_refused() {
        let kind = FactorKind::Fido2 {
            credential_id: vec![0; 300],
            salt: [0; 32],
        };
        assert_eq!(kind.encode().err(), Some(Error::TooLong));
    }

    #[test]
    fn cleared_slot_stops_working() {
        let config = config();
        let (mut header, key) = vault(&config, b"pw");
        header.clear_slot(0).unwrap();
        let root = config.derive_for_tests(b"new").unwrap();
        header
            .write_slot(1, &root, &key, SlotAction::Open, None)
            .unwrap();
        let file = seal_file(&header, &key, b"x").unwrap();
        assert!(matches!(open_with(&file, b"pw", &[]), Unlock::NoMatch));
        assert!(matches!(
            open_with(&file, b"new", &[]),
            Unlock::Opened { .. }
        ));
    }

    #[test]
    fn any_flipped_byte_is_rejected() {
        let config = config();
        let (header, key) = vault(&config, b"pw");
        let file = seal_file(&header, &key, b"some contents").unwrap();
        // Salt and KDF bytes change the derived key; the rest fail the MAC or AEAD.
        for at in [
            OFF_ID + 1,
            OFF_HINTS + 5,
            OFF_WRAPPED + 30,
            OFF_PREFIX + 1,
            OFF_MAC + 1,
            HEADER_LEN + 2,
        ] {
            let mut bad = file.clone();
            bad[at] ^= 1;
            assert!(open_file(&bad, &key).is_err(), "byte {at} went unnoticed");
        }
    }

    #[test]
    fn body_cannot_be_moved_to_another_header() {
        let config = config();
        let (header, key) = vault(&config, b"pw");
        let a = seal_file(&header, &key, b"first").unwrap();
        let b = seal_file(&header, &key, b"second").unwrap();
        let mut spliced = a[..HEADER_LEN].to_vec();
        spliced.extend_from_slice(&b[HEADER_LEN..]);
        assert!(open_file(&spliced, &key).is_err());
    }

    #[test]
    fn truncation_is_detected() {
        let config = config();
        let (header, key) = vault(&config, b"pw");
        let file = seal_file(&header, &key, &vec![1u8; CHUNK * 3]).unwrap();
        let cut = &file[..HEADER_LEN + CHUNK + TAG_LEN];
        assert_eq!(open_file(cut, &key).err(), Some(Error::Integrity));
    }

    #[test]
    fn header_below_the_kdf_floor_is_refused_before_any_derivation() {
        let config = config();
        let (header, key) = vault(&config, b"pw");
        let mut file = seal_file(&header, &key, b"x").unwrap();
        file[OFF_KDF..OFF_KDF + 4].copy_from_slice(&1024u32.to_le_bytes());
        assert_eq!(Header::parse(&file).err(), Some(Error::KdfOutOfRange));
    }

    #[test]
    fn newer_version_and_foreign_files_are_named_as_such() {
        let config = config();
        let (header, key) = vault(&config, b"pw");
        let mut file = seal_file(&header, &key, b"x").unwrap();
        file[4] = 9;
        assert_eq!(
            Header::parse(&file).err(),
            Some(Error::UnsupportedVersion(9))
        );
        assert_eq!(Header::parse(b"PK\x03\x04").err(), Some(Error::Malformed));
    }

    #[test]
    fn chaff_has_the_same_shape_and_opens_for_nobody() {
        let config = config();
        let (header, key) = vault(&config, b"pw");
        let real = seal_file(&header, &key, &vec![0u8; 5000]).unwrap();
        let chaff = chaff_file(&config, 5000).unwrap();
        assert_eq!(real.len(), chaff.len());
        assert_eq!(real[..OFF_ID], chaff[..OFF_ID]);
        assert!(matches!(open_with(&chaff, b"pw", &[]), Unlock::NoMatch));
    }

    #[test]
    fn saving_twice_changes_every_key_and_nonce() {
        let config = config();
        let (header, key) = vault(&config, b"pw");
        let a = seal_file(&header, &key, b"same").unwrap();
        let b = seal_file(&header, &key, b"same").unwrap();
        assert_ne!(a[OFF_WRAPPED..OFF_MAC], b[OFF_WRAPPED..OFF_MAC]);
        assert_ne!(a[HEADER_LEN..], b[HEADER_LEN..]);
    }

    #[test]
    fn real_kdf_settings_work_end_to_end() {
        let config = KdfConfig::new(KdfParams {
            m_kib: 64 * 1024,
            t: 3,
            p: 1,
        })
        .unwrap();
        let mut header = Header::new(&config).unwrap();
        let key = VaultKey::generate().unwrap();
        let root = config.derive(b"pw").unwrap();
        header
            .write_slot(0, &root, &key, SlotAction::Open, None)
            .unwrap();
        let file = seal_file(&header, &key, b"x").unwrap();
        let header = Header::parse(&file).unwrap();
        let root = header.config().unwrap().derive(b"pw").unwrap();
        assert!(matches!(
            header.unlock(&root, &[]).unwrap(),
            Unlock::Opened { .. }
        ));
    }
}
