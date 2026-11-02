//! Shared vaults without a server: one vault file, plus a manifest listing who holds its key.
//!
//! Each member has an X25519 key that the vault key is wrapped to, and an Ed25519 key. The
//! manifest is signed by the owner, and a member pins the owner's key from the invitation,
//! so a sync location cannot add itself as a member or swap someone's public key. Its version
//! number lets a member notice a replayed old manifest.
//!
//! Removing someone needs a new vault key, which is what [`Manifest::remove_member`] takes.
//! A person who was removed can still read any copy they already downloaded. Holding the key
//! allows writing as well as reading, since the vault format has one key.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::format::VaultKey;
use crate::{Error, Result, crypto};

const WRAP_INFO: &str = "wmpass/shared-vault-key/v1";
const SIGN_CONTEXT: &[u8] = b"wmpass/shared-manifest/v1";

/// A person's keys. Kept per person inside their own vault, never in the shared one.
#[derive(Clone)]
pub struct MemberKeys {
    x25519: Zeroizing<[u8; 32]>,
    ed25519: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for MemberKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemberKeys(..)")
    }
}

/// The public half of someone's keys. This is what is sent to the owner to be added.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberPublic {
    /// Display name, chosen by that person.
    pub name: String,
    /// Wrapping key.
    pub x25519: [u8; 32],
    /// Signing key.
    pub ed25519: [u8; 32],
}

impl MemberKeys {
    /// New random keys.
    pub fn generate() -> Result<Self> {
        Ok(Self {
            x25519: Zeroizing::new(crypto::random_array()?),
            ed25519: Zeroizing::new(crypto::random_array()?),
        })
    }

    /// Restores saved keys.
    pub fn from_bytes(x25519: [u8; 32], ed25519: [u8; 32]) -> Self {
        Self { x25519: Zeroizing::new(x25519), ed25519: Zeroizing::new(ed25519) }
    }

    /// The bytes to save, sealed inside that person's own vault.
    pub fn to_bytes(&self) -> ([u8; 32], [u8; 32]) {
        (*self.x25519, *self.ed25519)
    }

    /// The public half.
    pub fn public(&self, name: &str) -> MemberPublic {
        MemberPublic {
            name: name.to_owned(),
            x25519: PublicKey::from(&StaticSecret::from(*self.x25519)).to_bytes(),
            ed25519: SigningKey::from_bytes(&self.ed25519).verifying_key().to_bytes(),
        }
    }
}

fn wrap(recipient: &[u8; 32], key: &VaultKey, vault_id: &[u8]) -> Result<Vec<u8>> {
    let ephemeral = StaticSecret::from(crypto::random_array::<32>()?);
    let sender = PublicKey::from(&ephemeral);
    let shared = ephemeral.diffie_hellman(&PublicKey::from(*recipient));
    if !shared.was_contributory() {
        return Err(Error::Invalid("that member's key is not valid".into()));
    }
    let mut salt = sender.as_bytes().to_vec();
    salt.extend_from_slice(recipient);
    let wrapping = crypto::hkdf(shared.as_bytes(), &salt, WRAP_INFO)?;
    let mut out = sender.as_bytes().to_vec();
    out.extend_from_slice(&crypto::seal(&wrapping, key.expose_for_hardware_wrap(), vault_id)?);
    Ok(out)
}

fn unwrap(me: &MemberKeys, wrapped: &[u8], vault_id: &[u8]) -> Result<VaultKey> {
    let (sender, sealed) = wrapped.split_at_checked(32).ok_or(Error::Integrity)?;
    let sender: [u8; 32] = sender.try_into().map_err(|_| Error::Integrity)?;
    let secret = StaticSecret::from(*me.x25519);
    let shared = secret.diffie_hellman(&PublicKey::from(sender));
    if !shared.was_contributory() {
        return Err(Error::Integrity);
    }
    let mut salt = sender.to_vec();
    salt.extend_from_slice(PublicKey::from(&secret).as_bytes());
    let wrapping = crypto::hkdf(shared.as_bytes(), &salt, WRAP_INFO)?;
    let plain = crypto::open(&wrapping, sealed, vault_id).ok_or(Error::Integrity)?;
    let key: [u8; 32] = plain.as_slice().try_into().map_err(|_| Error::Integrity)?;
    Ok(VaultKey::from_bytes(key))
}

/// One person on a manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// Their public keys.
    pub public: MemberPublic,
    /// Whether this is the owner.
    pub owner: bool,
    /// The vault key, wrapped to them.
    pub wrapped: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Body {
    vault_id: Vec<u8>,
    version: u64,
    members: Vec<Member>,
}

/// Who holds a shared vault's key. Store it next to the vault file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    body: Vec<u8>,
    signature: Vec<u8>,
}

impl Manifest {
    fn seal(owner: &MemberKeys, body: &Body) -> Result<Self> {
        let bytes = rmp_serde::to_vec_named(body).map_err(|_| Error::Encoding)?;
        let signature = SigningKey::from_bytes(&owner.ed25519).sign(&signed_message(&bytes));
        Ok(Self { body: bytes, signature: signature.to_bytes().to_vec() })
    }

    /// A manifest with the owner as the only member.
    pub fn create(owner: &MemberKeys, name: &str, vault_id: &[u8], key: &VaultKey) -> Result<Self> {
        let public = owner.public(name);
        let wrapped = wrap(&public.x25519, key, vault_id)?;
        let body = Body { vault_id: vault_id.to_vec(), version: 1, members: vec![Member { public, owner: true, wrapped }] };
        Self::seal(owner, &body)
    }

    /// Reads a manifest and checks its signature against the owner's key, which comes from
    /// the invitation rather than from this file.
    fn verified(&self, owner_key: &[u8; 32], vault_id: &[u8], min_version: u64) -> Result<Body> {
        let key = VerifyingKey::from_bytes(owner_key).map_err(|_| Error::Integrity)?;
        let signature = Signature::from_slice(&self.signature).map_err(|_| Error::Integrity)?;
        key.verify(&signed_message(&self.body), &signature).map_err(|_| Error::Integrity)?;
        let body: Body = rmp_serde::from_slice(&self.body).map_err(|_| Error::Encoding)?;
        if body.vault_id != vault_id {
            return Err(Error::Integrity);
        }
        if body.version < min_version {
            return Err(Error::Invalid("this manifest is older than one already seen".into()));
        }
        Ok(body)
    }

    fn owned_body(&self, owner: &MemberKeys, vault_id: &[u8]) -> Result<Body> {
        let owner_key = owner.public("").ed25519;
        self.verified(&owner_key, vault_id, 0)
    }

    /// Adds someone. The owner should compare the new member's key fingerprint with them
    /// in person or by another channel, since a swapped key here would hand over the vault.
    pub fn add_member(&self, owner: &MemberKeys, vault_id: &[u8], key: &VaultKey, public: MemberPublic) -> Result<Self> {
        let mut body = self.owned_body(owner, vault_id)?;
        if body.members.iter().any(|m| m.public.ed25519 == public.ed25519) {
            return Err(Error::Invalid("that person is already a member".into()));
        }
        let wrapped = wrap(&public.x25519, key, vault_id)?;
        body.members.push(Member { public, owner: false, wrapped });
        body.version += 1;
        Self::seal(owner, &body)
    }

    /// Removes someone and wraps a new vault key to everyone left. Call
    /// [`crate::vault::Vault::rekey_shared`] first and save the file under the new key.
    pub fn remove_member(&self, owner: &MemberKeys, vault_id: &[u8], new_vault_id: &[u8], new_key: &VaultKey, ed25519: &[u8; 32]) -> Result<Self> {
        let mut body = self.owned_body(owner, vault_id)?;
        let target = body.members.iter().position(|m| m.public.ed25519 == *ed25519).ok_or_else(|| Error::Invalid("that person is not a member".into()))?;
        if body.members[target].owner {
            return Err(Error::Invalid("the owner cannot be removed".into()));
        }
        body.members.remove(target);
        for member in &mut body.members {
            member.wrapped = wrap(&member.public.x25519, new_key, new_vault_id)?;
        }
        body.vault_id = new_vault_id.to_vec();
        body.version += 1;
        Self::seal(owner, &body)
    }

    /// Opens as a member. `owner_key` is pinned from the invitation, and `min_version` is the
    /// highest this device has seen.
    pub fn open(&self, me: &MemberKeys, owner_key: &[u8; 32], vault_id: &[u8], min_version: u64) -> Result<(VaultKey, u64)> {
        let body = self.verified(owner_key, vault_id, min_version)?;
        let mine = me.public("").ed25519;
        let member = body.members.iter().find(|m| m.public.ed25519 == mine).ok_or_else(|| Error::Invalid("you are not a member of this vault".into()))?;
        Ok((unwrap(me, &member.wrapped, vault_id)?, body.version))
    }

    /// Opens as a member, following the owner to a new vault id if someone has been removed
    /// since. The id comes from a manifest the pinned owner key signed, so a sync location
    /// cannot redirect anyone. Returns the key, the id now in force, and the version.
    pub fn open_latest(&self, me: &MemberKeys, owner_key: &[u8; 32], min_version: u64) -> Result<(VaultKey, Vec<u8>, u64)> {
        let peek: Body = rmp_serde::from_slice(&self.body).map_err(|_| Error::Encoding)?;
        let (key, version) = self.open(me, owner_key, &peek.vault_id, min_version)?;
        Ok((key, peek.vault_id, version))
    }

    /// Who is on it, after checking the signature.
    pub fn members(&self, owner_key: &[u8; 32], vault_id: &[u8]) -> Result<Vec<MemberPublic>> {
        Ok(self.verified(owner_key, vault_id, 0)?.members.into_iter().map(|m| m.public).collect())
    }

    /// For storing.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        rmp_serde::to_vec_named(self).map_err(|_| Error::Encoding)
    }

    /// From storage.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        rmp_serde::from_slice(bytes).map_err(|_| Error::Encoding)
    }
}

fn signed_message(body: &[u8]) -> Vec<u8> {
    let mut message = SIGN_CONTEXT.to_vec();
    message.extend_from_slice(body);
    message
}

/// A short code two people can compare to confirm a key is really that person's.
pub fn fingerprint(public: &MemberPublic) -> String {
    let mut data = public.x25519.to_vec();
    data.extend_from_slice(&public.ed25519);
    let hash = blake3::hash(&data);
    let hex = crate::vault::hex(&hash.as_bytes()[..10]);
    hex.as_bytes().chunks(4).filter_map(|chunk| std::str::from_utf8(chunk).ok()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::KdfConfig;
    use crate::vault::Vault;
    use crate::crypto::KdfParams;

    fn vault() -> Vault {
        Vault::create_shared("Family", &KdfConfig::new(KdfParams::default()).unwrap(), 1, 1000).unwrap()
    }

    #[test]
    fn members_open_the_vault_and_others_cannot() {
        let mut shared = vault();
        let id = shared.vault_id().to_vec();
        let file = shared.save().unwrap();
        let owner = MemberKeys::generate().unwrap();
        let sibling = MemberKeys::generate().unwrap();
        let stranger = MemberKeys::generate().unwrap();
        let owner_key = owner.public("").ed25519;
        let manifest = Manifest::create(&owner, "Owner", &id, shared.key()).unwrap();
        let manifest = manifest.add_member(&owner, &id, shared.key(), sibling.public("Sibling")).unwrap();

        let (key, version) = manifest.open(&sibling, &owner_key, &id, 0).unwrap();
        assert_eq!(version, 2);
        let opened = Vault::from_file(&file, key, 2).unwrap();
        assert_eq!(opened.doc.name, "Family");
        assert!(manifest.open(&stranger, &owner_key, &id, 0).is_err());

        let round = Manifest::from_bytes(&manifest.to_bytes().unwrap()).unwrap();
        assert_eq!(round.members(&owner_key, &id).unwrap().len(), 2);
    }

    #[test]
    fn a_forged_manifest_is_refused() {
        let shared = vault();
        let id = shared.vault_id().to_vec();
        let owner = MemberKeys::generate().unwrap();
        let attacker = MemberKeys::generate().unwrap();
        let owner_key = owner.public("").ed25519;

        // A sync location writes its own manifest adding itself. The pinned key catches it.
        let forged = Manifest::create(&attacker, "Owner", &id, shared.key()).unwrap();
        assert!(forged.open(&attacker, &owner_key, &id, 0).is_err());

        // Editing a signed manifest breaks the signature.
        let good = Manifest::create(&owner, "Owner", &id, shared.key()).unwrap();
        let mut tampered = good.clone();
        let last = tampered.body.len() - 1;
        tampered.body[last] ^= 1;
        assert!(tampered.open(&owner, &owner_key, &id, 0).is_err());
        // Only the owner can add anyone.
        assert!(good.add_member(&attacker, &id, shared.key(), attacker.public("A")).is_err());
        // A manifest for another vault does not apply here.
        assert!(good.open(&owner, &owner_key, b"another-vault-id", 0).is_err());
    }

    #[test]
    fn an_old_manifest_replayed_by_a_sync_location_is_noticed() {
        let shared = vault();
        let id = shared.vault_id().to_vec();
        let owner = MemberKeys::generate().unwrap();
        let owner_key = owner.public("").ed25519;
        let old = Manifest::create(&owner, "O", &id, shared.key()).unwrap();
        let new = old.add_member(&owner, &id, shared.key(), MemberKeys::generate().unwrap().public("B")).unwrap();
        let (_, seen) = new.open(&owner, &owner_key, &id, 0).unwrap();
        assert!(old.open(&owner, &owner_key, &id, seen).is_err());
    }

    #[test]
    fn removal_rotates_the_key_and_cuts_off_new_copies() {
        let mut shared = vault();
        let id = shared.vault_id().to_vec();
        let owner = MemberKeys::generate().unwrap();
        let (keep, gone) = (MemberKeys::generate().unwrap(), MemberKeys::generate().unwrap());
        let owner_key = owner.public("").ed25519;
        let manifest = Manifest::create(&owner, "O", &id, shared.key()).unwrap();
        let manifest = manifest.add_member(&owner, &id, shared.key(), keep.public("K")).unwrap();
        let manifest = manifest.add_member(&owner, &id, shared.key(), gone.public("G")).unwrap();
        let (old_key, _) = manifest.open(&gone, &owner_key, &id, 0).unwrap();
        let old_key_bytes = *old_key.expose_for_hardware_wrap();

        shared.rekey_shared(&KdfConfig::new(KdfParams::default()).unwrap(), 2000).unwrap();
        let new_id = shared.vault_id().to_vec();
        let file = shared.save().unwrap();
        let manifest = manifest.remove_member(&owner, &id, &new_id, shared.key(), &gone.public("").ed25519).unwrap();

        assert!(manifest.open(&gone, &owner_key, &new_id, 0).is_err());
        assert!(Vault::from_file(&file, VaultKey::from_bytes(old_key_bytes), 3).is_err(), "the old key opens nothing new");
        let (key, _) = manifest.open(&keep, &owner_key, &new_id, 0).unwrap();
        assert!(Vault::from_file(&file, key, 3).is_ok());
        assert!(manifest.remove_member(&owner, &new_id, &new_id, shared.key(), &owner_key).is_err(), "the owner stays");
    }

    #[test]
    fn fingerprints_are_short_and_differ() {
        let (a, b) = (MemberKeys::generate().unwrap(), MemberKeys::generate().unwrap());
        let shown = fingerprint(&a.public("A"));
        assert_eq!(shown.len(), 24);
        assert_ne!(shown, fingerprint(&b.public("B")));
        assert_eq!(shown, fingerprint(&a.public("A again")), "names are not part of it");
    }
}
