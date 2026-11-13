//! Passkeys: an authenticator in software. ES256 only, which every relying party accepts.
//! Android's credential provider and the browser extension both call this, so signing
//! stays in one place and private keys never need to reach a UI language.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::model::Passkey;
use crate::{Error, Result, Secret, crypto};

/// Identifies this software as the authenticator. Attestation is `none`, so relying parties
/// only see this as a label.
const AAGUID: [u8; 16] = [0x7a, 0x1e, 0x4c, 0x90, 0x5b, 0x3f, 0x4d, 0x2a, 0x9c, 0x61, 0xe8, 0x0d, 0x27, 0xb4, 0x11, 0xf6];
const COSE_ES256: i32 = -7;
const FLAG_USER_PRESENT: u8 = 0x01;
const FLAG_USER_VERIFIED: u8 = 0x04;
/// Backup eligible and backed up: this credential syncs, and relying parties should know.
const FLAG_BACKUP: u8 = 0x08 | 0x10;
const FLAG_ATTESTED: u8 = 0x40;

/// What a new passkey hands back to the relying party, beside the entry to store.
#[derive(Clone, Debug)]
pub struct Created {
    /// Store this.
    pub passkey: Passkey,
    /// Raw credential id.
    pub credential_id: Vec<u8>,
    /// CBOR attestation object, format `none`.
    pub attestation_object: Vec<u8>,
    /// Authenticator data alone.
    pub authenticator_data: Vec<u8>,
    /// SubjectPublicKeyInfo, DER.
    pub public_key_der: Vec<u8>,
}

/// What a sign-in hands back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assertion {
    /// Raw credential id.
    pub credential_id: Vec<u8>,
    /// Authenticator data.
    pub authenticator_data: Vec<u8>,
    /// DER signature.
    pub signature: Vec<u8>,
    /// Raw user handle.
    pub user_handle: Vec<u8>,
}

/// Whether a page at `host` may use `rp_id`: it must be that host or a parent of it, and not a
/// public suffix. Without this, one site could ask for a credential belonging to another.
pub fn rp_id_allowed(host: &str, rp_id: &str) -> bool {
    let (host, rp_id) = (host.trim_end_matches('.').to_ascii_lowercase(), rp_id.trim_end_matches('.').to_ascii_lowercase());
    if rp_id.is_empty() || host.is_empty() {
        return false;
    }
    // An address has no parent domain: its RP ID is the address itself or nothing.
    if host.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>().is_ok() {
        return host == rp_id;
    }
    let covers = host == rp_id || host.strip_suffix(&rp_id).is_some_and(|rest| rest.ends_with('.'));
    if !covers {
        return false;
    }
    // localhost has no registrable domain, and is used only in development.
    rp_id == "localhost" || psl::domain_str(&rp_id).is_some()
}

fn random_signing_key() -> Result<SigningKey> {
    // A random 32-byte value is out of range about once in 2^32 tries, and is then redrawn.
    loop {
        let bytes: Zeroizing<[u8; 32]> = Zeroizing::new(crypto::random_array()?);
        if let Ok(key) = SigningKey::from_slice(&bytes[..]) {
            return Ok(key);
        }
    }
}

fn cose_key(key: &SigningKey) -> Result<Vec<u8>> {
    let point = key.verifying_key().to_sec1_point(false);
    let (x, y) = (point.x().ok_or(Error::Crypto)?, point.y().ok_or(Error::Crypto)?);
    let mut out = vec![0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
    out.extend_from_slice(x.as_slice());
    out.extend_from_slice(&[0x22, 0x58, 0x20]);
    out.extend_from_slice(y.as_slice());
    Ok(out)
}

fn cbor_bytes_header(length: usize, out: &mut Vec<u8>) {
    match length {
        0..=23 => out.push(0x40 | u8::try_from(length).unwrap_or(0)),
        24..=255 => {
            out.push(0x58);
            out.push(u8::try_from(length).unwrap_or(0));
        }
        _ => {
            out.push(0x59);
            out.extend_from_slice(&u16::try_from(length).unwrap_or(u16::MAX).to_be_bytes());
        }
    }
}

/// Makes a passkey for a relying party.
pub fn create(rp_id: &str, rp_name: &str, user_handle: &[u8], user_name: &str, user_verified: bool, now_ms: u64) -> Result<Created> {
    let key = random_signing_key()?;
    let credential_id: [u8; 32] = crypto::random_array()?;
    let pkcs8 = key.to_pkcs8_der().map_err(|_| Error::Crypto)?;
    let public_key_der = key.verifying_key().to_public_key_der().map_err(|_| Error::Crypto)?.as_bytes().to_vec();

    let mut flags = FLAG_USER_PRESENT | FLAG_BACKUP | FLAG_ATTESTED;
    if user_verified {
        flags |= FLAG_USER_VERIFIED;
    }
    let mut data = Sha256::digest(rp_id.as_bytes()).to_vec();
    data.push(flags);
    // A synced credential cannot keep a meaningful counter, and zero tells relying parties so.
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&AAGUID);
    data.extend_from_slice(&u16::try_from(credential_id.len()).map_err(|_| Error::Crypto)?.to_be_bytes());
    data.extend_from_slice(&credential_id);
    data.extend_from_slice(&cose_key(&key)?);

    let mut object = vec![0xa3];
    object.extend_from_slice(b"\x63fmt\x64none\x67attStmt\xa0\x68authData");
    cbor_bytes_header(data.len(), &mut object);
    object.extend_from_slice(&data);

    Ok(Created {
        passkey: Passkey {
            credential_id: URL_SAFE_NO_PAD.encode(credential_id),
            rp_id: rp_id.to_owned(),
            rp_name: rp_name.to_owned(),
            user_handle: URL_SAFE_NO_PAD.encode(user_handle),
            user_name: user_name.to_owned(),
            private_key: Secret::from(URL_SAFE_NO_PAD.encode(pkcs8.as_bytes())),
            algorithm: COSE_ES256,
            counter: 0,
            created_ms: now_ms,
        },
        credential_id: credential_id.to_vec(),
        attestation_object: object,
        authenticator_data: data,
        public_key_der,
    })
}

/// Moves the signature counter on before an assertion, and says whether it moved so the caller
/// saves the passkey. WebAuthn asks an authenticator that keeps a counter to send a strictly
/// higher one each time, and a relying party that sees one repeat may take the credential for a
/// clone. A counter at zero stays there: that is how an authenticator without a counter answers,
/// and it is what every passkey made here sends, since a synced credential cannot keep a
/// meaningful count. Only one imported with a count of its own moves.
pub fn advance_counter(passkey: &mut Passkey) -> Result<bool> {
    if passkey.counter == 0 {
        return Ok(false);
    }
    passkey.counter = passkey
        .counter
        .checked_add(1)
        .ok_or_else(|| Error::Invalid("this passkey's signature counter has run out".into()))?;
    Ok(true)
}

/// Signs a sign-in challenge. Pass `user_verified` only after a real unlock or biometric
/// check for this request: relying parties trust that flag.
pub fn sign(passkey: &Passkey, client_data_hash: &[u8; 32], user_verified: bool) -> Result<Assertion> {
    if passkey.algorithm != COSE_ES256 {
        return Err(Error::Invalid("this passkey uses an algorithm that cannot be used here".into()));
    }
    let der = Zeroizing::new(URL_SAFE_NO_PAD.decode(passkey.private_key.expose()).map_err(|_| Error::Encoding)?);
    let key = SigningKey::from_pkcs8_der(&der).map_err(|_| Error::Crypto)?;
    let mut flags = FLAG_USER_PRESENT | FLAG_BACKUP;
    if user_verified {
        flags |= FLAG_USER_VERIFIED;
    }
    let mut data = Sha256::digest(passkey.rp_id.as_bytes()).to_vec();
    data.push(flags);
    data.extend_from_slice(&passkey.counter.to_be_bytes());
    let mut message = data.clone();
    message.extend_from_slice(client_data_hash);
    let signature: Signature = key.sign(&message);
    Ok(Assertion {
        credential_id: URL_SAFE_NO_PAD.decode(&passkey.credential_id).map_err(|_| Error::Encoding)?,
        authenticator_data: data,
        signature: signature.to_der().as_bytes().to_vec(),
        user_handle: URL_SAFE_NO_PAD.decode(&passkey.user_handle).map_err(|_| Error::Encoding)?,
    })
}

/// The client data a browser would build, for the extension, and its hash. Its members come in
/// the order WebAuthn fixes, `type`, `challenge`, `origin`, `crossOrigin`, since a relying party
/// may check the text against that template rather than parse it.
pub fn client_data(kind: &str, challenge: &[u8], origin: &str, cross_origin: bool) -> (String, [u8; 32]) {
    let text = |value: &str| serde_json::Value::from(value).to_string();
    let json = format!(
        "{{\"type\":{},\"challenge\":{},\"origin\":{},\"crossOrigin\":{cross_origin}}}",
        text(kind),
        text(&URL_SAFE_NO_PAD.encode(challenge)),
        text(origin),
    );
    let hash = Sha256::digest(json.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    (json, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;
    use p256::pkcs8::DecodePublicKey;

    #[test]
    fn signatures_verify_against_the_key_given_at_creation() {
        let created = create("example.com", "Example", b"user-1", "me", true, 1000).unwrap();
        let verifying = VerifyingKey::from_public_key_der(&created.public_key_der).unwrap();
        let (json, hash) = client_data("webauthn.get", b"challenge", "https://example.com", false);
        assert_eq!(json, r#"{"type":"webauthn.get","challenge":"Y2hhbGxlbmdl","origin":"https://example.com","crossOrigin":false}"#, "in WebAuthn's order");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&client_data("x", b"", "a\"b", true).0).unwrap()["origin"], "a\"b");

        let assertion = sign(&created.passkey, &hash, true).unwrap();
        let mut signed = assertion.authenticator_data.clone();
        signed.extend_from_slice(&hash);
        let signature = Signature::from_der(&assertion.signature).unwrap();
        verifying.verify(&signed, &signature).unwrap();

        assert_eq!(assertion.credential_id, created.credential_id);
        assert_eq!(assertion.user_handle, b"user-1");
        assert_eq!(&assertion.authenticator_data[..32], Sha256::digest(b"example.com").as_slice());

        let mut tampered = signed.clone();
        tampered[0] ^= 1;
        assert!(verifying.verify(&tampered, &signature).is_err());
    }

    #[test]
    fn a_counter_in_use_rises_and_one_at_zero_stays_there() {
        let mut fresh = create("example.com", "Example", b"u", "me", true, 1000).unwrap().passkey;
        assert!(!advance_counter(&mut fresh).unwrap());
        assert_eq!(fresh.counter, 0);

        let mut imported = fresh.clone();
        imported.counter = 41;
        assert!(advance_counter(&mut imported).unwrap());
        assert_eq!(imported.counter, 42);
        let data = sign(&imported, &[0u8; 32], true).unwrap().authenticator_data;
        assert_eq!(&data[33..37], &42u32.to_be_bytes());

        imported.counter = u32::MAX;
        assert!(advance_counter(&mut imported).is_err(), "never wraps back to a lower number");
    }

    #[test]
    fn user_verified_is_only_set_when_asked() {
        let created = create("example.com", "Example", b"u", "me", false, 1000).unwrap();
        assert_eq!(created.authenticator_data[32] & FLAG_USER_VERIFIED, 0);
        assert_ne!(created.authenticator_data[32] & FLAG_USER_PRESENT, 0);
        let hash = [0u8; 32];
        assert_eq!(sign(&created.passkey, &hash, false).unwrap().authenticator_data[32] & FLAG_USER_VERIFIED, 0);
        assert_ne!(sign(&created.passkey, &hash, true).unwrap().authenticator_data[32] & FLAG_USER_VERIFIED, 0);
    }

    #[test]
    fn attestation_object_has_the_structure_relying_parties_parse() {
        let created = create("example.com", "Example", b"u", "me", true, 1000).unwrap();
        let object = &created.attestation_object;
        assert_eq!(object[0], 0xa3, "a map of three");
        let text = String::from_utf8_lossy(object);
        for name in ["fmt", "none", "attStmt", "authData"] {
            assert!(text.contains(name));
        }
        // Authenticator data: hash 32, flags, counter 4, aaguid 16, id length 2, id 32, COSE key.
        let data = &created.authenticator_data;
        assert_ne!(data[32] & FLAG_ATTESTED, 0);
        assert_eq!(&data[37..53], &AAGUID);
        assert_eq!(u16::from_be_bytes([data[53], data[54]]), 32);
        assert_eq!(&data[55..87], created.credential_id.as_slice());
        let cose = &data[87..];
        assert_eq!(cose.len(), 77);
        assert_eq!(&cose[..7], &[0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01]);
        // The COSE point must match the published key.
        let verifying = VerifyingKey::from_public_key_der(&created.public_key_der).unwrap();
        let point = verifying.to_sec1_point(false);
        assert_eq!(&cose[10..42], point.x().unwrap().as_slice());
        assert_eq!(&cose[45..77], point.y().unwrap().as_slice());
        assert!(object.ends_with(data));
    }

    #[test]
    fn a_site_cannot_ask_for_another_sites_credential() {
        assert!(rp_id_allowed("example.com", "example.com"));
        assert!(rp_id_allowed("login.example.com", "example.com"));
        assert!(rp_id_allowed("a.b.example.co.uk", "example.co.uk"));
        assert!(rp_id_allowed("LOGIN.Example.COM", "example.com."));
        assert!(rp_id_allowed("localhost", "localhost"));
        assert!(!rp_id_allowed("evil.com", "example.com"));
        assert!(!rp_id_allowed("notexample.com", "example.com"), "a suffix without a dot is another site");
        assert!(!rp_id_allowed("example.com", "login.example.com"), "a parent cannot claim a child");
        assert!(!rp_id_allowed("example.com", "com"), "a public suffix is never a relying party");
        assert!(!rp_id_allowed("example.co.uk", "co.uk"));
        assert!(!rp_id_allowed("foo.github.io", "github.io"), "user pages cannot claim the whole host");
        assert!(!rp_id_allowed("", ""));
        assert!(!rp_id_allowed("192.168.1.1", "1.1"), "an address has no parent domain");
        assert!(!rp_id_allowed("10.0.0.1", "0.1"));
        assert!(!rp_id_allowed("[::1]", "1]"));
        assert!(rp_id_allowed("192.168.1.1", "192.168.1.1"), "an address may claim itself");
    }

    #[test]
    fn a_foreign_algorithm_is_refused_rather_than_misused() {
        let mut created = create("example.com", "Example", b"u", "me", true, 1000).unwrap();
        created.passkey.algorithm = -8;
        assert!(sign(&created.passkey, &[0u8; 32], true).is_err());
    }
}
