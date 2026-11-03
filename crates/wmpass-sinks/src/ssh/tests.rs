#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use super::fake::{self, Options};
use super::*;

// The keys under testdata/ were made for these tests with `ssh-keygen` (OpenSSH 10.2) and
// guard nothing. `client_rsa_pem` is `client_rsa` converted with `ssh-keygen -p -m PEM`,
// and the two signatures are `openssl dgst -sha256|-sha512 -sign client_rsa_pem msg`.
const ED25519: &str = include_str!("testdata/client_ed25519");
const ED25519_PUB: &str = include_str!("testdata/client_ed25519.pub");
const ED25519_ENC: &str = include_str!("testdata/client_ed25519_enc");
const ED25519_ENC_PUB: &str = include_str!("testdata/client_ed25519_enc.pub");
const ECDSA: &str = include_str!("testdata/client_ecdsa");
const ECDSA_PUB: &str = include_str!("testdata/client_ecdsa.pub");
const RSA: &str = include_str!("testdata/client_rsa");
const RSA_PUB: &str = include_str!("testdata/client_rsa.pub");
const RSA_PEM: &str = include_str!("testdata/client_rsa_pem");
const MSG: &[u8] = include_bytes!("testdata/msg");
const SIG_256: &[u8] = include_bytes!("testdata/msg.rsa-sha256.sig");
const SIG_512: &[u8] = include_bytes!("testdata/msg.rsa-sha512.sig");

fn pub_blob(line: &str) -> Vec<u8> {
    stored_blob(line).unwrap()
}

#[test]
fn mpints_match_rfc_4251() {
    // The examples in RFC 4251 section 5.
    assert_eq!(Wire::default().mpint(&[]).0, [0, 0, 0, 0]);
    assert_eq!(Wire::default().mpint(&[0x09, 0xa3, 0x78, 0xf9, 0xb2, 0xe3, 0x32, 0xa7]).0, [
        0, 0, 0, 8, 0x09, 0xa3, 0x78, 0xf9, 0xb2, 0xe3, 0x32, 0xa7
    ]);
    assert_eq!(Wire::default().mpint(&[0x80]).0, [0, 0, 0, 2, 0, 0x80]);
    assert_eq!(Wire::default().mpint(&[0, 0, 0x7f]).0, [0, 0, 0, 1, 0x7f]);
    assert_eq!(Parse::new(&[0, 0, 0, 2, 0, 0x80]).mpint().unwrap(), [0x80]);
    // -1234 is 0xfb2e: a negative number has no place in SSH key material.
    assert_eq!(Parse::new(&[0, 0, 0, 2, 0xfb, 0x2e]).mpint(), Err(Fault::Io));
    assert_eq!(Parse::new(&[0, 0, 0, 9, 1]).bytes(), Err(Fault::Io));
}

#[test]
fn private_keys_read_and_match_their_public_halves() {
    let none = Secret::default();
    for (text, public, passphrase) in [
        (ED25519, ED25519_PUB, ""),
        (ED25519_ENC, ED25519_ENC_PUB, "secret"),
        (ECDSA, ECDSA_PUB, ""),
        (RSA, RSA_PUB, "pass phrase"),
        (RSA_PEM, RSA_PUB, ""),
    ] {
        let key = parse_private_key(text, passphrase).unwrap();
        assert_eq!(key.blob(), pub_blob(public));
    }
    assert!(none.is_empty());
    for (text, passphrase) in [(ED25519_ENC, "wrong"), (ED25519_ENC, ""), (RSA, "secret"), ("not a key", "")] {
        assert_eq!(parse_private_key(text, passphrase).err(), Some(Fault::PermissionLost));
    }
    let encrypted_pem = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n\nAAAA\n-----END RSA PRIVATE KEY-----";
    assert_eq!(parse_private_key(encrypted_pem, "x").err(), Some(Fault::PermissionLost));
}

#[test]
fn rsa_signs_exactly_as_openssl_and_verifies() {
    let ClientKey::Rsa(key) = parse_private_key(RSA_PEM, "").unwrap() else { panic!("not RSA") };
    // PKCS#1 v1.5 is deterministic, so blinding and the CRT must change nothing.
    assert_eq!(key.sign(RsaHash::Sha256, MSG).unwrap(), SIG_256);
    assert_eq!(key.sign(RsaHash::Sha512, MSG).unwrap(), SIG_512);
    assert!(sig::rsa_verify(&key.n, &key.e, RsaHash::Sha256, MSG, SIG_256));
    assert!(sig::rsa_verify(&key.n, &key.e, RsaHash::Sha512, MSG, SIG_512));
    assert!(!sig::rsa_verify(&key.n, &key.e, RsaHash::Sha512, MSG, SIG_256));
    assert!(!sig::rsa_verify(&key.n, &key.e, RsaHash::Sha256, b"something else", SIG_256));
    let mut bent = SIG_256.to_vec();
    bent[100] ^= 1;
    assert!(!sig::rsa_verify(&key.n, &key.e, RsaHash::Sha256, MSG, &bent));
}

#[test]
fn every_client_key_signs_what_the_host_key_check_accepts() {
    for (text, passphrase) in [(ED25519, ""), (ECDSA, ""), (RSA, "pass phrase")] {
        let key = parse_private_key(text, passphrase).unwrap();
        for kind in key.algorithms(None, true) {
            let signature = key.sign(kind, b"exchange hash").unwrap();
            sig::verify(kind, &key.blob(), &signature, b"exchange hash").unwrap();
            assert_eq!(sig::verify(kind, &key.blob(), &signature, b"another hash"), Err(Fault::Unsafe));
        }
    }
    let rsa = parse_private_key(RSA, "pass phrase").unwrap();
    let only_256 = vec!["ssh-ed25519".to_owned(), "rsa-sha2-256".to_owned()];
    assert_eq!(rsa.algorithms(Some(&only_256), false), vec![SigKind::Rsa(RsaHash::Sha256)]);
    assert_eq!(rsa.algorithms(None, false).len(), 2, "ssh-rsa only when legacy is on");
}

#[test]
fn every_cipher_and_mac_round_trips_and_refuses_a_flipped_bit() {
    let key = [0x42u8; 64];
    let iv = [0x24u8; 16];
    let mut combos = Vec::new();
    for (_, cipher) in CIPHERS {
        if cipher.aead() {
            combos.push((*cipher, None));
        } else {
            for (_, mac, etm) in MACS.iter().chain(MACS_LEGACY) {
                combos.push((*cipher, Some((*mac, *etm))));
            }
        }
    }
    for (cipher, mac) in combos {
        let make = || Direction {
            cipher: Cipher::new(cipher, &key[..cipher.key_len()], &iv[..cipher.iv_len()]).unwrap(),
            mac: mac.map(|(kind, etm)| (kind, Zeroizing::new(vec![0x11; kind.len()]), etm)),
            seq: 3,
        };
        let (mut sealer, mut opener) = (make(), make());
        let mut wire = Vec::new();
        let payloads: Vec<Vec<u8>> = vec![vec![94], vec![1; 31], (0..1000).map(|i| i as u8).collect()];
        for payload in &payloads {
            wire.extend_from_slice(&sealer.seal(payload).unwrap());
        }
        let mut input = wire.as_slice();
        for payload in &payloads {
            assert_eq!(&opener.open(&mut input).unwrap(), payload, "{cipher:?} {mac:?}");
        }
        assert_eq!(opener.seq, 6);
        let mut tampered = make().seal(b"\x5ehello").unwrap();
        let last = tampered.len() - 1;
        tampered[last - 20] ^= 0x01;
        assert_eq!(make().open(&mut tampered.as_slice()), Err(Fault::Io), "{cipher:?} {mac:?}");
    }
}

fn kexinit(kex: &str, host: &str, cipher: &str, mac: &str, follows: bool) -> Vec<u8> {
    Wire::msg(KEXINIT)
        .raw(&[0; 16])
        .text(kex)
        .text(host)
        .text(cipher)
        .text(cipher)
        .text(mac)
        .text(mac)
        .text("none,zlib@openssh.com")
        .text("none")
        .text("")
        .text("")
        .bool(follows)
        .u32(0)
        .0
}

#[test]
fn negotiation_follows_the_client_and_gates_legacy() {
    let modern = Offer { legacy: false, pinned_type: None };
    let server = kexinit(
        "sntrup761x25519-sha512,curve25519-sha256,kex-strict-s-v00@openssh.com",
        "rsa-sha2-512,ssh-ed25519",
        "aes256-gcm@openssh.com,chacha20-poly1305@openssh.com",
        "hmac-sha2-512",
        false,
    );
    let chosen = negotiate(&modern, &server).unwrap();
    assert_eq!(chosen.kex, KexKind::Curve25519);
    assert_eq!(chosen.host_key, SigKind::Ed25519);
    assert_eq!(chosen.c2s, (CipherKind::ChaCha, None), "AEAD needs no MAC");
    assert!(chosen.strict && !chosen.skip_guess);

    // A server that knows the stored key's type shows that key, not a preferred one.
    let pinned = Offer { legacy: false, pinned_type: Some("ssh-rsa".into()) };
    assert_eq!(negotiate(&pinned, &server).unwrap().host_key, SigKind::Rsa(RsaHash::Sha512));

    let old = kexinit("diffie-hellman-group14-sha1", "ssh-rsa", "aes128-ctr", "hmac-sha1", true);
    assert_eq!(negotiate(&modern, &old).err(), Some(Fault::Io));
    let legacy = Offer { legacy: true, pinned_type: None };
    let chosen = negotiate(&legacy, &old).unwrap();
    assert_eq!(chosen.kex, KexKind::Group14(KexHash::Sha1));
    assert_eq!(chosen.s2c, (CipherKind::Aes128Ctr, Some((MacKind::Sha1, false))));
    assert!(!chosen.strict && !chosen.skip_guess, "a right guess is kept");

    let wrong_guess = kexinit("ecdh-sha2-nistp256,curve25519-sha256", "ssh-ed25519", "aes128-ctr", "hmac-sha2-256", true);
    assert!(negotiate(&modern, &wrong_guess).unwrap().skip_guess);
    let ours = Parse::new(&modern.kexinit(true).unwrap()[17..]).names().unwrap().join(",");
    assert!(ours.ends_with("ext-info-c,kex-strict-c-v00@openssh.com"), "{ours}");
    let rekey = Parse::new(&modern.kexinit(false).unwrap()[17..]).names().unwrap().join(",");
    assert!(!rekey.contains("kex-strict"), "only in the first exchange");
}

#[test]
fn fingerprints_print_like_ssh_keygen() {
    // `ssh-keygen -lf client_ed25519.pub` prints the same.
    let fp = fingerprint(ED25519_PUB.trim()).unwrap();
    let expected = format!(
        "SHA256:{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(pub_blob(ED25519_PUB)))
    );
    assert_eq!(fp, expected);
    assert_eq!(fingerprint("nonsense"), None);
}

fn echo() -> Arc<dyn Fn() -> fake::Handler + Send + Sync> {
    Arc::new(|| Box::new(|data: &[u8]| data.to_vec()))
}

fn login<'a>(port: u16, password: &'a Secret, host_key: &'a str, none: &'a Secret) -> Login<'a> {
    Login {
        host: "127.0.0.1",
        port,
        user: "wm",
        password,
        private_key: none,
        key_passphrase: none,
        host_key,
        legacy_algorithms: false,
    }
}

#[test]
fn first_key_is_taken_then_held_to() {
    let none = Secret::default();
    let right = Secret::from("right");
    for (cipher, strict) in [
        ("chacha20-poly1305@openssh.com", true),
        ("aes256-gcm@openssh.com", false),
        ("aes128-ctr", true),
        ("aes256-ctr", false),
    ] {
        let options = Options { cipher, strict, ..Options::default() };
        let port = fake::spawn(options, echo());
        let mut session = Session::connect(NetPolicy::Any, &login(port, &right, "", &none)).unwrap();
        let seen = session.host_key();
        let expected = format!(
            "ssh-ed25519 {}",
            base64::engine::general_purpose::STANDARD.encode(fake::host_blob(&[7; 32]))
        );
        assert_eq!(seen, expected);
        assert_eq!(session.transport.strict, strict);
        assert_eq!(session.transport.server_sig_algs.as_deref().map(<[String]>::len), Some(2));
        // More than the server's window and packet size, so both are waited on.
        session.open_subsystem("sftp").unwrap();
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        session.send(&data).unwrap();
        let mut back = vec![0u8; data.len()];
        session.recv(&mut back).unwrap();
        assert_eq!(back, data, "{cipher}");
        session.close();

        Session::connect(NetPolicy::Any, &login(port, &right, &seen, &none)).unwrap().close();
        let other = format!(
            "ssh-ed25519 {}",
            base64::engine::general_purpose::STANDARD.encode(fake::host_blob(&[8; 32]))
        );
        let refused = Session::connect(NetPolicy::Any, &login(port, &right, &other, &none));
        assert_eq!(refused.err(), Some(Fault::Unsafe));
        assert_eq!(
            Session::connect(NetPolicy::Any, &login(port, &right, "garbage", &none)).err(),
            Some(Fault::Unsafe)
        );
    }
}

#[test]
fn refusals_are_named() {
    let none = Secret::default();
    let port = fake::spawn(Options::default(), echo());
    let wrong = Secret::from("wrong");
    assert_eq!(Session::connect(NetPolicy::Any, &login(port, &wrong, "", &none)).err(), Some(Fault::PermissionLost));
    // The fake only does passwords, so a key alone has nothing to offer.
    let key = Secret::from(ED25519);
    let with_key = Login { private_key: &key, ..login(port, &none, "", &none) };
    assert_eq!(Session::connect(NetPolicy::Any, &with_key).err(), Some(Fault::PermissionLost));
    assert_eq!(
        Session::connect(NetPolicy::Offline, &login(port, &wrong, "", &none)).err(),
        Some(Fault::Blocked)
    );
    assert_eq!(
        Session::connect(NetPolicy::Any, &login(port, &none, "", &none)).err(),
        Some(Fault::NotConfigured)
    );
    let bad_key = Secret::from("-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----");
    let with_bad_key = Login { private_key: &bad_key, ..login(port, &none, "", &none) };
    assert_eq!(Session::connect(NetPolicy::Any, &with_bad_key).err(), Some(Fault::PermissionLost));
}

#[test]
fn keyboard_interactive_answers_a_hidden_prompt_only() {
    let none = Secret::default();
    let right = Secret::from("right");
    let wrong = Secret::from("wrong");
    let kbint = Options { methods: &["keyboard-interactive"], ..Options::default() };
    let port = fake::spawn(kbint.clone(), echo());
    Session::connect(NetPolicy::Any, &login(port, &right, "", &none)).unwrap().close();
    assert_eq!(Session::connect(NetPolicy::Any, &login(port, &wrong, "", &none)).err(), Some(Fault::PermissionLost));
    // A prompt shown in the clear is not a password prompt: left for plain password sign-in.
    let echoed = Options { methods: &["keyboard-interactive", "password"], echo: true, ..Options::default() };
    let port = fake::spawn(echoed, echo());
    Session::connect(NetPolicy::Any, &login(port, &right, "", &none)).unwrap().close();
    let only_echoed = Options { echo: true, ..kbint };
    let port = fake::spawn(only_echoed, echo());
    assert_eq!(Session::connect(NetPolicy::Any, &login(port, &right, "", &none)).err(), Some(Fault::PermissionLost));
}

#[test]
fn a_server_that_is_not_ssh_is_io() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.write_all(b"220 ftp.example ready\r\n");
        }
    });
    let none = Secret::default();
    let password = Secret::from("x");
    assert_eq!(Session::connect(NetPolicy::Any, &login(port, &password, "", &none)).err(), Some(Fault::Io));
}

#[test]
fn debug_output_names_no_secret() {
    let password = Secret::from("hunter2");
    let none = Secret::default();
    let shown = format!("{:?}", login(22, &password, "ssh-ed25519 AAAA", &none));
    assert!(!shown.contains("hunter2") && !shown.contains("127.0.0.1") && !shown.contains("AAAA"), "{shown}");
}
