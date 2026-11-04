//! FIDO2's hmac-secret extension: a key that stays on the authenticator produces a value
//! from a salt, and that value becomes the vault's second factor.

use aes::Aes256;
use aes::cipher::block_padding::NoPadding;
use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use ciborium::value::{Integer, Value};
use hmac::{Hmac, KeyInit, Mac};
use p256::ecdh::EphemeralSecret;
use p256::elliptic_curve::Generate;
use p256::elliptic_curve::sec1::{FromSec1Point, ToSec1Point};
use p256::{PublicKey, Sec1Point};
use sha2::{Digest, Sha256};
use wmpass_core::format::{FactorKind, FactorOutput};
use zeroize::Zeroizing;

use crate::transport::Ctap2Transport;
use crate::{HwError, Result};

/// What WM Pass registered keys under before browsers could reach them. Still asked for keys
/// set up that way.
pub const RP_ID: &str = "wmpass.local";
/// What WM Pass registers keys under now: the web vault's own domain, which the browser
/// extension may claim too. A browser asks hmac-secret only through PRF, so this is the one
/// relying party every shell can reach.
pub const WEB_RP_ID: &str = "wmpass.pages.dev";
const CMD_MAKE_CREDENTIAL: u8 = 0x01;
const CMD_GET_ASSERTION: u8 = 0x02;
const CMD_GET_INFO: u8 = 0x04;
const CMD_CLIENT_PIN: u8 = 0x06;
const FLAG_EXTENSIONS: u8 = 0x80;
const FLAG_ATTESTED: u8 = 0x40;

fn int(n: i64) -> Value {
    Value::Integer(Integer::from(n))
}

fn encode(command: u8, map: Vec<(Value, Value)>) -> Result<Vec<u8>> {
    let mut out = vec![command];
    ciborium::into_writer(&Value::Map(map), &mut out).map_err(|_| HwError::Protocol)?;
    Ok(out)
}

fn status(code: u8) -> Result<()> {
    match code {
        0x00 => Ok(()),
        0x2e => Err(HwError::WrongKey),
        0x27 | 0x2f | 0x30 | 0x3a | 0x35 => Err(HwError::NotConfirmed),
        0x36 | 0x31 | 0x34 => Err(HwError::PinRequired),
        0x2c | 0x2b => Err(HwError::Unsupported),
        _ => Err(HwError::Protocol),
    }
}

fn call(transport: &mut dyn Ctap2Transport, request: &[u8]) -> Result<Value> {
    let answer = transport.exchange(request)?;
    let (&code, body) = answer.split_first().ok_or(HwError::Protocol)?;
    status(code)?;
    if body.is_empty() {
        return Ok(Value::Map(Vec::new()));
    }
    ciborium::from_reader(body).map_err(|_| HwError::Protocol)
}

fn lookup(map: &Value, key: i64) -> Option<&Value> {
    map.as_map()?.iter().find(|(k, _)| k.as_integer() == Some(Integer::from(key))).map(|(_, v)| v)
}

/// Whether a key can do this at all.
pub fn supports_hmac_secret(transport: &mut dyn Ctap2Transport) -> Result<bool> {
    let info = call(transport, &[CMD_GET_INFO])?;
    let extensions = lookup(&info, 2).and_then(Value::as_array);
    Ok(extensions.is_some_and(|list| list.iter().any(|v| v.as_text() == Some("hmac-secret"))))
}

fn random_32() -> Result<[u8; 32]> {
    wmpass_core::crypto::random_array().map_err(|_| HwError::Crypto)
}

/// What WebAuthn's PRF hands hmac-secret for an input, from the WebAuthn text.
pub fn prf_salt(input: &[u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"WebAuthn PRF");
    hash.update([0]);
    hash.update(input);
    hash.finalize().into()
}

/// Creates a credential for unlocking, one a browser can ask too. The caller stores the result
/// in a vault slot.
pub fn enroll(transport: &mut dyn Ctap2Transport) -> Result<FactorKind> {
    let credential_id = make_credential(transport, WEB_RP_ID)?;
    Ok(FactorKind::WebAuthn { credential_id, input: random_32()? })
}

fn make_credential(transport: &mut dyn Ctap2Transport, rp: &str) -> Result<Vec<u8>> {
    if !supports_hmac_secret(transport)? {
        return Err(HwError::Unsupported);
    }
    let client_data_hash = random_32()?;
    let user_id = random_32()?;
    let request = encode(CMD_MAKE_CREDENTIAL, vec![
        (int(1), Value::Bytes(client_data_hash.to_vec())),
        (int(2), Value::Map(vec![(Value::Text("id".into()), Value::Text(rp.into())), (Value::Text("name".into()), Value::Text("WM Pass".into()))])),
        (int(3), Value::Map(vec![(Value::Text("id".into()), Value::Bytes(user_id.to_vec())), (Value::Text("name".into()), Value::Text("WM Pass vault".into()))])),
        (int(4), Value::Array(vec![Value::Map(vec![(Value::Text("alg".into()), int(-7)), (Value::Text("type".into()), Value::Text("public-key".into()))])])),
        (int(6), Value::Map(vec![(Value::Text("hmac-secret".into()), Value::Bool(true))])),
    ])?;
    let answer = call(transport, &request)?;
    let data = lookup(&answer, 2).and_then(Value::as_bytes).ok_or(HwError::Protocol)?;
    // Hash 32, flags 1, counter 4, aaguid 16, then the id's length and the id.
    let flags = *data.get(32).ok_or(HwError::Protocol)?;
    if flags & FLAG_ATTESTED == 0 {
        return Err(HwError::Protocol);
    }
    let length = usize::from(u16::from_be_bytes([*data.get(53).ok_or(HwError::Protocol)?, *data.get(54).ok_or(HwError::Protocol)?]));
    Ok(data.get(55..55 + length).ok_or(HwError::Protocol)?.to_vec())
}

fn cose_point(key: &PublicKey) -> Result<Value> {
    let point = key.to_sec1_point(false);
    Ok(Value::Map(vec![
        (int(1), int(2)),
        (int(3), int(-25)),
        (int(-1), int(1)),
        (int(-2), Value::Bytes(point.x().ok_or(HwError::Crypto)?.to_vec())),
        (int(-3), Value::Bytes(point.y().ok_or(HwError::Crypto)?.to_vec())),
    ]))
}

fn read_cose_point(value: &Value) -> Result<PublicKey> {
    let x = lookup(value, -2).and_then(Value::as_bytes).ok_or(HwError::Protocol)?;
    let y = lookup(value, -3).and_then(Value::as_bytes).ok_or(HwError::Protocol)?;
    if x.len() != 32 || y.len() != 32 {
        return Err(HwError::Protocol);
    }
    let mut sec1 = vec![0x04];
    sec1.extend_from_slice(x);
    sec1.extend_from_slice(y);
    let point = Sec1Point::from_bytes(&sec1).map_err(|_| HwError::Protocol)?;
    Option::from(PublicKey::from_sec1_point(&point)).ok_or(HwError::Protocol)
}

fn cbc_encrypt(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>> {
    let cipher = cbc::Encryptor::<Aes256>::new_from_slices(key, &[0u8; 16]).map_err(|_| HwError::Crypto)?;
    Ok(cipher.encrypt_padded_vec::<NoPadding>(plain))
}

fn cbc_decrypt(key: &[u8; 32], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let cipher = cbc::Decryptor::<Aes256>::new_from_slices(key, &[0u8; 16]).map_err(|_| HwError::Crypto)?;
    cipher.decrypt_padded_vec::<NoPadding>(sealed).map(Zeroizing::new).map_err(|_| HwError::Crypto)
}

fn left_16_hmac(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| HwError::Crypto)?;
    mac.update(data);
    Ok(mac.finalize().into_bytes()[..16].to_vec())
}

/// Asks the key for the value that unlocks. It waits for a touch.
pub fn derive(transport: &mut dyn Ctap2Transport, factor: &FactorKind) -> Result<FactorOutput> {
    let (rp, credential_id, salt) = match factor {
        FactorKind::Fido2 { credential_id, salt } => (RP_ID, credential_id, *salt),
        FactorKind::WebAuthn { credential_id, input } => (WEB_RP_ID, credential_id, prf_salt(input)),
        _ => return Err(HwError::Protocol),
    };
    // Agreeing a key with the authenticator, protocol one, so the salt and the result are
    // not readable on the bus.
    let agreement = call(transport, &encode(CMD_CLIENT_PIN, vec![(int(1), int(1)), (int(2), int(2))])?)?;
    let theirs = read_cose_point(lookup(&agreement, 1).ok_or(HwError::Protocol)?)?;
    let mine = EphemeralSecret::generate();
    let shared = mine.diffie_hellman(&theirs);
    let mut wrapping = Zeroizing::new([0u8; 32]);
    wrapping.copy_from_slice(&Sha256::digest(shared.raw_secret_bytes()));

    let salt_enc = cbc_encrypt(&wrapping, &salt)?;
    let salt_auth = left_16_hmac(&wrapping, &salt_enc)?;
    let extension = Value::Map(vec![
        (int(1), cose_point(&mine.public_key())?),
        (int(2), Value::Bytes(salt_enc)),
        (int(3), Value::Bytes(salt_auth)),
        (int(4), int(1)),
    ]);
    let client_data_hash = random_32()?;
    let request = encode(CMD_GET_ASSERTION, vec![
        (int(1), Value::Text(rp.into())),
        (int(2), Value::Bytes(client_data_hash.to_vec())),
        (int(3), Value::Array(vec![Value::Map(vec![
            (Value::Text("id".into()), Value::Bytes(credential_id.clone())),
            (Value::Text("type".into()), Value::Text("public-key".into())),
        ])])),
        (int(4), Value::Map(vec![(Value::Text("hmac-secret".into()), extension)])),
    ])?;
    let answer = call(transport, &request)?;
    let data = lookup(&answer, 2).and_then(Value::as_bytes).ok_or(HwError::Protocol)?;
    let flags = *data.get(32).ok_or(HwError::Protocol)?;
    if flags & FLAG_EXTENSIONS == 0 {
        return Err(HwError::Unsupported);
    }
    let extensions: Value = ciborium::from_reader(data.get(37..).ok_or(HwError::Protocol)?).map_err(|_| HwError::Protocol)?;
    let sealed = extensions
        .as_map()
        .and_then(|map| map.iter().find(|(k, _)| k.as_text() == Some("hmac-secret")))
        .and_then(|(_, v)| v.as_bytes())
        .ok_or(HwError::Protocol)?;
    if sealed.len() != 32 {
        return Err(HwError::Protocol);
    }
    cbc_decrypt(&wrapping, sealed)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A software authenticator following the CTAP 2.1 text for these commands.
    pub struct Simulated {
        /// Id, the secret behind hmac-secret, and the relying party it belongs to.
        credentials: Vec<(Vec<u8>, [u8; 32], String)>,
        agreement: Option<EphemeralSecret>,
        pub user_present: bool,
    }

    impl Simulated {
        pub fn new() -> Self {
            Self { credentials: Vec::new(), agreement: None, user_present: true }
        }
    }

    impl Ctap2Transport for Simulated {
        fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>> {
            let (&command, body) = request.split_first().unwrap();
            let map: Value = if body.is_empty() { Value::Map(Vec::new()) } else { ciborium::from_reader(body).unwrap() };
            let reply = |value: Value| {
                let mut out = vec![0u8];
                ciborium::into_writer(&value, &mut out).unwrap();
                Ok(out)
            };
            match command {
                CMD_GET_INFO => reply(Value::Map(vec![(
                    int(2),
                    Value::Array(vec![Value::Text("credProtect".into()), Value::Text("hmac-secret".into())]),
                )])),
                CMD_MAKE_CREDENTIAL => {
                    if !self.user_present {
                        return Ok(vec![0x2f]);
                    }
                    let extensions = lookup(&map, 6).unwrap().as_map().unwrap();
                    assert!(extensions.iter().any(|(k, v)| k.as_text() == Some("hmac-secret") && *v == Value::Bool(true)));
                    let rp = lookup(&map, 2).unwrap().as_map().unwrap().iter().find(|(k, _)| k.as_text() == Some("id")).unwrap().1.as_text().unwrap().to_owned();
                    let id = random_32().unwrap().to_vec();
                    self.credentials.push((id.clone(), random_32().unwrap(), rp.clone()));
                    let mut data = Sha256::digest(&rp).to_vec();
                    data.push(FLAG_ATTESTED | 1);
                    data.extend_from_slice(&[0; 4]);
                    data.extend_from_slice(&[0; 16]);
                    data.extend_from_slice(&(id.len() as u16).to_be_bytes());
                    data.extend_from_slice(&id);
                    reply(Value::Map(vec![(int(1), Value::Text("none".into())), (int(2), Value::Bytes(data))]))
                }
                CMD_CLIENT_PIN => {
                    assert_eq!(lookup(&map, 2).unwrap(), &int(2), "getKeyAgreement");
                    let secret = EphemeralSecret::generate();
                    let point = cose_point(&secret.public_key()).unwrap();
                    self.agreement = Some(secret);
                    reply(Value::Map(vec![(int(1), point)]))
                }
                CMD_GET_ASSERTION => {
                    if !self.user_present {
                        return Ok(vec![0x2f]);
                    }
                    let rp = lookup(&map, 1).unwrap().as_text().unwrap();
                    let allow = lookup(&map, 3).unwrap().as_array().unwrap();
                    let wanted = allow[0].as_map().unwrap().iter().find(|(k, _)| k.as_text() == Some("id")).unwrap().1.as_bytes().unwrap();
                    // A credential belongs to one relying party: asked under another, it is not there.
                    let Some((_, random, _)) = self.credentials.iter().find(|(id, _, owner)| id == wanted && owner == rp) else {
                        return Ok(vec![0x2e]);
                    };
                    let extensions = lookup(&map, 4).unwrap().as_map().unwrap();
                    let inputs = &extensions.iter().find(|(k, _)| k.as_text() == Some("hmac-secret")).unwrap().1;
                    let theirs = read_cose_point(lookup(inputs, 1).unwrap()).unwrap();
                    let shared = self.agreement.as_ref().unwrap().diffie_hellman(&theirs);
                    let mut wrapping = [0u8; 32];
                    wrapping.copy_from_slice(&Sha256::digest(shared.raw_secret_bytes()));
                    let salt_enc = lookup(inputs, 2).unwrap().as_bytes().unwrap();
                    let salt_auth = lookup(inputs, 3).unwrap().as_bytes().unwrap();
                    if left_16_hmac(&wrapping, salt_enc).unwrap() != *salt_auth {
                        return Ok(vec![0x33]);
                    }
                    let salt = cbc_decrypt(&wrapping, salt_enc).unwrap();
                    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(random).unwrap();
                    mac.update(&salt);
                    let output = mac.finalize().into_bytes();
                    let sealed = cbc_encrypt(&wrapping, &output).unwrap();
                    let mut data = Sha256::digest(rp).to_vec();
                    data.push(FLAG_EXTENSIONS | 1);
                    data.extend_from_slice(&[0; 4]);
                    ciborium::into_writer(&Value::Map(vec![(Value::Text("hmac-secret".into()), Value::Bytes(sealed))]), &mut data).unwrap();
                    reply(Value::Map(vec![(int(2), Value::Bytes(data)), (int(3), Value::Bytes(vec![0; 64]))]))
                }
                other => panic!("unexpected command {other:#x}"),
            }
        }
    }

    #[test]
    fn the_same_salt_gives_the_same_value_and_another_salt_does_not() {
        let mut key = Simulated::new();
        let factor = enroll(&mut key).unwrap();
        let first = derive(&mut key, &factor).unwrap();
        let again = derive(&mut key, &factor).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(first, again, "the agreement key changes each time, the value does not");

        let FactorKind::WebAuthn { credential_id, .. } = &factor else { panic!() };
        let other = FactorKind::WebAuthn { credential_id: credential_id.clone(), input: [9; 32] };
        assert_ne!(derive(&mut key, &other).unwrap(), first);
    }

    /// A browser's PRF answers HMAC(secret, SHA-256("WebAuthn PRF" || 0 || input)), so a key set
    /// up here must answer the same, or a vault opened here would not open in the web vault.
    #[test]
    fn it_answers_what_a_browser_would() {
        let mut key = Simulated::new();
        let factor = enroll(&mut key).unwrap();
        let FactorKind::WebAuthn { input, .. } = &factor else { panic!() };
        let (_, random, rp) = key.credentials[0].clone();
        assert_eq!(rp, WEB_RP_ID);
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&random).unwrap();
        mac.update(&prf_salt(input));
        assert_eq!(derive(&mut key, &factor).unwrap().as_slice(), mac.finalize().into_bytes().as_slice());
    }

    #[test]
    fn a_key_set_up_before_still_opens() {
        let mut key = Simulated::new();
        let credential_id = make_credential(&mut key, RP_ID).unwrap();
        let factor = FactorKind::Fido2 { credential_id: credential_id.clone(), salt: [3; 32] };
        assert_eq!(derive(&mut key, &factor).unwrap(), derive(&mut key, &factor).unwrap());
        // The same credential cannot be reached as a browser-style one: it is under the old relying party.
        let moved = FactorKind::WebAuthn { credential_id, input: [3; 32] };
        assert_eq!(derive(&mut key, &moved).err(), Some(HwError::WrongKey));
    }

    #[test]
    fn a_different_key_is_named_as_such() {
        let mut key = Simulated::new();
        let factor = enroll(&mut key).unwrap();
        let mut another = Simulated::new();
        assert_eq!(derive(&mut another, &factor).err(), Some(HwError::WrongKey));
    }

    #[test]
    fn no_touch_means_no_value() {
        let mut key = Simulated::new();
        let factor = enroll(&mut key).unwrap();
        key.user_present = false;
        assert_eq!(derive(&mut key, &factor).err(), Some(HwError::NotConfirmed));
    }

    #[test]
    fn it_unlocks_a_real_vault_slot() {
        use wmpass_core::crypto::KdfParams;
        use wmpass_core::format::{KdfConfig, SlotAction};
        use wmpass_core::vault::{OpenOutcome, Vault, open_set};
        let config = KdfConfig::new(KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }).unwrap();
        let mut key = Simulated::new();
        let mut vault = Vault::create("V", b"pw", &config, 1, 1000).unwrap();
        let factor = enroll(&mut key).unwrap();
        let output = derive(&mut key, &factor).unwrap();
        vault.add_slot(b"pw2", "Key", SlotAction::Open, Some((&factor, &output)), 2000).unwrap();
        let file = vault.save().unwrap();

        let needed = open_set(&[&file], b"pw2", &[], 2).unwrap();
        let OpenOutcome::NeedsFactor(kinds) = needed else { panic!("the password alone must not open it") };
        let again = derive(&mut key, &kinds[0]).unwrap();
        assert!(matches!(open_set(&[&file], b"pw2", &[again], 2).unwrap(), OpenOutcome::Opened { .. }));
    }
}
