//! SSH keys: making them, reading them, and their fingerprints. RSA keys can be
//! stored but not generated here, because the `rsa` crate carries an open timing
//! advisory (RUSTSEC-2023-0071).

use ssh_key::rand_core::OsRng;
use signature::Signer;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey, Signature};
use zeroize::Zeroizing;

use crate::{Error, Result, Secret};

/// A key in the forms an entry stores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedKey {
    /// OpenSSH private key text.
    pub private_key: Secret,
    /// `ssh-ed25519 AAAA… comment`.
    pub public_key: String,
    /// `SHA256:…`.
    pub fingerprint: String,
}

/// Makes a new Ed25519 key.
pub fn generate_ed25519(comment: &str) -> Result<GeneratedKey> {
    let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(|_| Error::Crypto)?;
    key.set_comment(comment);
    describe(&key)
}

fn describe(key: &PrivateKey) -> Result<GeneratedKey> {
    let private = key
        .to_openssh(LineEnding::LF)
        .map_err(|_| Error::Encoding)?;
    let public = key.public_key().to_openssh().map_err(|_| Error::Encoding)?;
    Ok(GeneratedKey {
        private_key: Secret::new(private.as_str()),
        public_key: public,
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
    })
}

/// A public key line as the agent protocol wants it: its binary form, and its comment.
pub fn public_blob(line: &str) -> Result<(Vec<u8>, String)> {
    let key = PublicKey::from_openssh(line.trim()).map_err(|_| Error::Invalid("that is not an OpenSSH public key".into()))?;
    let blob = key.to_bytes().map_err(|_| Error::Encoding)?;
    Ok((blob, key.comment().to_owned()))
}

/// Signs for the SSH agent protocol, with a key held as OpenSSH text. Returns the encoded
/// signature, which is not secret, in wipeable bytes all the same.
pub fn agent_sign(private: &str, passphrase: &str, data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let mut key = PrivateKey::from_openssh(private.trim()).map_err(|_| Error::Invalid("that is not an OpenSSH private key".into()))?;
    if key.is_encrypted() {
        key = key.decrypt(passphrase).map_err(|_| Error::Invalid("the key's passphrase is wrong".into()))?;
    }
    let signature: Signature = key.try_sign(data).map_err(|_| Error::Crypto)?;
    // string algorithm, string signature: what RFC 8332 and the agent protocol carry.
    let name = signature.algorithm();
    let name = name.as_str().as_bytes();
    let body = signature.as_bytes();
    let mut encoded = Vec::with_capacity(name.len() + body.len() + 8);
    for run in [name, body] {
        encoded.extend_from_slice(&u32::try_from(run.len()).map_err(|_| Error::Encoding)?.to_be_bytes());
        encoded.extend_from_slice(run);
    }
    Ok(Zeroizing::new(encoded))
}

/// Reads an OpenSSH private key for storing. A passphrase-protected key stays
/// encrypted, and only its public half and fingerprint are read.
pub fn read_private(text: &str) -> Result<GeneratedKey> {
    let key = PrivateKey::from_openssh(text.trim())
        .map_err(|_| Error::Invalid("that is not an OpenSSH private key".into()))?;
    let public = key.public_key().to_openssh().map_err(|_| Error::Encoding)?;
    Ok(GeneratedKey {
        private_key: Secret::new(text),
        public_key: public,
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
    })
}

/// The fingerprint of a public key line.
pub fn public_fingerprint(line: &str) -> Result<String> {
    let key = PublicKey::from_openssh(line.trim())
        .map_err(|_| Error::Invalid("that is not an OpenSSH public key".into()))?;
    Ok(key.fingerprint(HashAlg::Sha256).to_string())
}

/// Protects an OpenSSH key with a passphrase, returning the new private key text.
pub fn encrypt_private(text: &str, passphrase: &str) -> Result<Secret> {
    let key = PrivateKey::from_openssh(text.trim())
        .map_err(|_| Error::Invalid("that is not an OpenSSH private key".into()))?;
    let locked = key
        .encrypt(&mut OsRng, passphrase)
        .map_err(|_| Error::Crypto)?;
    let out = locked
        .to_openssh(LineEnding::LF)
        .map_err(|_| Error::Encoding)?;
    Ok(Secret::new(out.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_read_back() {
        let key = generate_ed25519("me@phone").unwrap();
        assert!(key.public_key.starts_with("ssh-ed25519 "));
        assert!(key.public_key.ends_with("me@phone"));
        assert!(key.fingerprint.starts_with("SHA256:"));
        let again = read_private(key.private_key.expose()).unwrap();
        assert_eq!(again.fingerprint, key.fingerprint);
        assert_eq!(
            public_fingerprint(&key.public_key).unwrap(),
            key.fingerprint
        );
    }

    #[test]
    fn two_keys_differ() {
        assert_ne!(
            generate_ed25519("a").unwrap().fingerprint,
            generate_ed25519("a").unwrap().fingerprint
        );
    }

    #[test]
    fn the_agent_signature_checks_out() {
        use signature::Verifier;
        let key = generate_ed25519("test").expect("generate");
        let encoded = agent_sign(key.private_key.expose(), "", b"hello").expect("sign");
        // string "ssh-ed25519", then the 64 bytes.
        assert_eq!(&encoded[..4], &11u32.to_be_bytes());
        assert_eq!(&encoded[4..15], b"ssh-ed25519");
        let signature = Signature::new(Algorithm::Ed25519, encoded[19..].to_vec()).expect("parse");
        let public = PublicKey::from_openssh(&key.public_key).expect("public");
        public.key_data().verify(b"hello", &signature).expect("verifies");
        assert!(public.key_data().verify(b"goodbye", &signature).is_err());
        let (blob, comment) = public_blob(&key.public_key).expect("blob");
        assert_eq!(comment, "test");
        assert_eq!(blob, public.to_bytes().expect("bytes"));
    }

    #[test]
    fn rubbish_is_refused() {
        assert!(read_private("nope").is_err());
        assert!(public_fingerprint("nope").is_err());
    }
}
