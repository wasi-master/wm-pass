//! KeePass databases locked with a YubiKey's HMAC-SHA1 challenge-response, as KeePassXC
//! sets one up. `make_kdbx_yubikey.py` writes the fixtures from the format alone and
//! answers for a slot programmed with `SECRET`, so no key is needed here.
//!
//! With a real YubiKey, by hand. `keepassxc-cli` (2.8) only unlocks with a key (`-y`);
//! adding one takes the KeePassXC app. Make `yk.kdbx` with `keepassxc-cli db-create -p
//! -t 100` (KDBX 3.1) or in the app (KDBX 4), add an entry, then in the app choose
//! Database, Database Security, Add additional protection, Challenge-Response, slot 2.
//! Open it in WM Pass's desktop app as a KeePass account with **Use security key**,
//! slot 2, change a password, and check that `keepassxc-cli show -y 2 yk.kdbx <entry>`
//! shows the change. Do the same through Settings, Import, and on Android by NFC and
//! by USB.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use zeroize::Zeroizing;

use super::*;
use crate::backend::{Backend, KdbxBackend};
use crate::clock::Hlc;

const AT: Hlc = Hlc {
    ms: 1000,
    counter: 0,
    device: 1,
};

/// The slot's HMAC secret in `make_kdbx_yubikey.py`.
const SECRET: [u8; 20] = [
    0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f,
    0x50, 0x51, 0x52, 0x53,
];

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("../../tests/fixtures/keepass/", $name)).as_slice()
    };
}

const KEY_FILE: &[u8] = fixture!("key-binary32.key");

/// A fixture's label, database bytes, password and optional key file.
type Fixture = (&'static str, &'static [u8], &'static str, Option<&'static [u8]>);

/// Each fixture, with the password and key file that go with the YubiKey.
fn fixtures() -> [Fixture; 4] {
    [
        (
            "3.1 password",
            fixture!("yk-kdbx31-password.kdbx"),
            "yk-pw",
            None,
        ),
        ("3.1 key alone", fixture!("yk-kdbx31-only.kdbx"), "", None),
        (
            "4.0 password",
            fixture!("yk-kdbx40-password.kdbx"),
            "yk-pw",
            None,
        ),
        (
            "4.0 key file",
            fixture!("yk-kdbx40-key-file.kdbx"),
            "",
            Some(KEY_FILE),
        ),
    ]
}

/// What a slot programmed with `secret` answers.
fn answer(secret: &[u8], challenge: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret).unwrap();
    mac.update(challenge);
    mac.finalize().into_bytes().to_vec()
}

fn seed(from: u8) -> [u8; 32] {
    std::array::from_fn(|i| from + u8::try_from(i).unwrap())
}

#[test]
fn the_challenge_is_the_kdf_seed_in_kdbx_4_and_the_master_seed_in_kdbx_3() {
    assert_eq!(
        kdbx::challenge(fixture!("yk-kdbx40-password.kdbx")).unwrap(),
        seed(0xa0)
    );
    assert_eq!(
        kdbx::challenge(fixture!("yk-kdbx31-password.kdbx")).unwrap(),
        seed(0x80)
    );
    // KeePassXC's own files, with Argon2 and with AES-KDF.
    assert!(kdbx::challenge(fixture!("kpxc-kdbx40-password.kdbx")).is_ok());
    assert!(kdbx::challenge(fixture!("kpxc-kdbx31-password.kdbx")).is_ok());
    assert!(kdbx::challenge(fixture!("kdb1-password.kdb")).is_err());
    assert!(kdbx::challenge(b"not a database").is_err());
    let cut = &fixture!("yk-kdbx40-password.kdbx")[..40];
    assert!(kdbx::challenge(cut).is_err());
}

#[test]
fn the_answer_is_hmac_sha1_of_the_challenge() {
    // Pinned, so a change to how the answer is worked out shows here first.
    let expected = "8f1ed1eac4a2626e58b24ef64b909e7ffd7842a8";
    let hex: String = answer(&SECRET, &seed(0xa0))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, expected);
}

#[test]
fn a_yubikey_answer_opens_each_mix_of_key_parts() {
    for (label, data, password, key_file) in fixtures() {
        let response = answer(&SECRET, &kdbx::challenge(data).unwrap());
        let imported = import_with_keys(
            Format::Kdbx,
            data,
            Some(password),
            key_file,
            Some(&response),
            AT,
        )
        .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(imported.entries.len(), 1, "{label}");
        let login = imported.entries[0].entry.login().unwrap();
        assert_eq!(login.password.v.expose(), "yk-entry-pw", "{label}");
        assert_eq!(login.username.v, "me", "{label}");

        // Without the key, or with another key, it stays shut and says why.
        let without = import_with_keys(Format::Kdbx, data, Some(password), key_file, None, AT);
        assert!(without.is_err(), "{label}");
        let other = answer(&[7; 20], &kdbx::challenge(data).unwrap());
        let wrong = import_with_keys(
            Format::Kdbx,
            data,
            Some(password),
            key_file,
            Some(&other),
            AT,
        )
        .unwrap_err()
        .to_string();
        assert!(wrong.contains("security key"), "{label}: {wrong}");
    }
}

#[test]
fn in_place_saves_keep_the_challenge_so_the_same_answer_opens_them() {
    for (label, data, password, key_file) in fixtures() {
        let challenge = kdbx::challenge(data).unwrap();
        let response = Zeroizing::new(answer(&SECRET, &challenge));
        let mut backend = KdbxBackend::open_with_response(
            data,
            password.into(),
            key_file.map(<[u8]>::to_vec),
            Some(response.clone()),
        )
        .unwrap_or_else(|e| panic!("{label}: {e}"));
        let mut entry = backend.entries(2000).pop().unwrap();
        entry.set_password(
            "changed".into(),
            Hlc {
                ms: 3000,
                ..Hlc::default()
            },
        );
        backend.upsert(entry, 3000).unwrap();
        let saved = backend.save().unwrap();

        // KDBX 3.1 becomes KDBX 4, its master seed now the KDF seed; KDBX 4 keeps its seed.
        assert!(
            matches!(
                keepass::config::DatabaseVersion::parse(&saved),
                Ok(keepass::config::DatabaseVersion::KDB4(_))
            ),
            "{label}"
        );
        assert_eq!(kdbx::challenge(&saved).unwrap(), challenge, "{label}");
        let again = KdbxBackend::open_with_response(
            &saved,
            password.into(),
            key_file.map(<[u8]>::to_vec),
            Some(response),
        )
        .unwrap_or_else(|e| panic!("{label} reopened: {e}"));
        let back = again.entries(4000).pop().unwrap();
        assert_eq!(
            back.login().unwrap().password.v.expose(),
            "changed",
            "{label}"
        );
        assert!(
            KdbxBackend::open(&saved, password.into(), key_file.map(<[u8]>::to_vec)).is_err(),
            "{label}: opened without the key"
        );
    }
}

#[test]
fn keepass_1_is_not_opened_with_a_security_key() {
    let data = fixture!("kdb1-password.kdb");
    let failed = import_with_keys(Format::Kdbx, data, Some("pw"), None, Some(&[1; 20]), AT)
        .unwrap_err()
        .to_string();
    assert!(failed.contains("KeePass 1"), "{failed}");
}
