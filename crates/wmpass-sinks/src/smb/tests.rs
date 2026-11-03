//! The SMB sink's cryptography against published vectors (MS-NLMP 4.2.4 for NTLMv2,
//! RFC 1320 for MD4, RFC 4493 for AES-CMAC, and the key derivation, signing and
//! encryption examples Microsoft's Open Specifications team published for SMB 3.0 and
//! 3.1.1), then the whole sink against the scripted server in `fake`.
//!
//! A wrong byte anywhere in these is a sign-in every server refuses, with no message
//! saying why.

use super::bytes::{Le, LeWriter, from_utf16, utf16};
use super::client::*;
use super::crypto::{self, Cipher};
use super::fake::{self, Options};
use super::{Smb, SmbConfig, ntlm, spnego};
use crate::http::block_on;
use crate::{Fault, Sink, Stored};
use wmpass_net::NetPolicy;

fn hex(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn key(s: &str) -> [u8; 16] {
    hex(s).try_into().unwrap()
}

// ---- Primitives ----

#[test]
fn md4_matches_rfc_1320() {
    assert_eq!(crypto::md4(b"").to_vec(), hex("31d6cfe0d16ae931b73c59d7e0c089c0"));
    assert_eq!(crypto::md4(b"abc").to_vec(), hex("a448017aaf21d8525fc10ae87aa6729d"));
}

#[test]
fn aes_cmac_matches_rfc_4493() {
    let k = key("2b7e151628aed2a6abf7158809cf4f3c");
    assert_eq!(crypto::aes_cmac(&k, &[]).to_vec(), hex("bb1d6929e95937287fa37d129b756746"));
    assert_eq!(crypto::aes_cmac(&k, &hex("6bc1bee22e409f96e93d7e117393172a")).to_vec(), hex("070a16b46b4d4144f79bdd9dd04a287c"));
}

// ---- NTLMv2 ----

/// The CHALLENGE_MESSAGE of MS-NLMP 4.2.4.3, rebuilt from its parts.
fn spec_challenge() -> Vec<u8> {
    let target_name = utf16("Server");
    let target_info = ntlm::av_encode(&[(2, utf16("Domain")), (1, utf16("Server"))]);
    let mut w = LeWriter::new();
    w.bytes(ntlm::SIGNATURE).u32(2);
    w.u16(target_name.len() as u16).u16(target_name.len() as u16).u32(56);
    w.u32(0xe28a_8233);
    w.bytes(&hex("0123456789abcdef")).zeros(8);
    w.u16(target_info.len() as u16).u16(target_info.len() as u16).u32(56 + target_name.len() as u32);
    w.bytes(&[6, 0, 0x70, 0x17, 0, 0, 0, 15]);
    w.bytes(&target_name).bytes(&target_info);
    w.into_vec()
}

#[test]
fn ntlmv2_authenticate_matches_ms_nlmp_4_2_4() {
    let mut ntlm = ntlm::Ntlm::new("User", "Password", "Domain");
    ntlm.fixed_client_challenge = Some([0xaa; 8]);
    ntlm.fixed_session_key = Some([0x55; 16]);
    ntlm.fixed_now = Some(0);
    ntlm.negotiate();
    let auth = ntlm.authenticate(&spec_challenge()).unwrap();

    let field = |at: usize| {
        let len = usize::from(auth.u16_at(at).unwrap());
        auth.slice_at(auth.u32_at(at + 4).unwrap() as usize, len).unwrap().to_vec()
    };
    // No timestamp in the target info, so an LMv2 response and no MIC.
    assert_eq!(field(12), hex("86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa"));
    assert_eq!(field(20)[..16], hex("68cd0ab851e51c96aabc927bebef6a1c")[..]);
    assert_eq!(field(52), hex("c5dad2544fc9799094ce1ce90bc9d03e"));
    assert_eq!(ntlm.exported_session_key, [0x55; 16]);
    assert_eq!(from_utf16(&field(36)), "User");
}

#[test]
fn a_server_timestamp_turns_on_the_mic_and_blanks_the_lm_response() {
    let info = ntlm::av_encode(&[(2, utf16("D")), (7, vec![1; 8])]);
    let mut w = LeWriter::new();
    w.bytes(ntlm::SIGNATURE).u32(2).u16(0).u16(0).u32(56).u32(ntlm::CLIENT_FLAGS);
    w.bytes(&[9; 8]).zeros(8).u16(info.len() as u16).u16(info.len() as u16).u32(56).zeros(8).bytes(&info);
    let mut ntlm = ntlm::Ntlm::new("u", "p", "");
    ntlm.negotiate();
    let auth = ntlm.authenticate(&w.into_vec()).unwrap();
    let lm = auth.slice_at(auth.u32_at(16).unwrap() as usize, usize::from(auth.u16_at(12).unwrap())).unwrap();
    assert_eq!(lm, [0u8; 24]);
    assert!(auth[72..88].iter().any(|b| *b != 0), "MIC must be filled in");
    // The flags pair in the echoed target info says a MIC is present.
    let nt = auth.slice_at(auth.u32_at(24).unwrap() as usize, usize::from(auth.u16_at(20).unwrap())).unwrap();
    let pairs = ntlm::av_parse(&nt[16 + 28..nt.len() - 4]);
    assert_eq!(ntlm::av_get(&pairs, ntlm::AV_FLAGS).unwrap().u32_at(0).unwrap() & 2, 2);
}

#[test]
fn a_short_or_foreign_challenge_is_refused_without_a_panic() {
    let mut ntlm = ntlm::Ntlm::new("u", "p", "");
    assert!(ntlm.authenticate(b"NTLMSSP\0").is_err());
    assert!(ntlm.authenticate(&[0; 64]).is_err());
    let mut bad_offsets = spec_challenge();
    bad_offsets[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(ntlm.authenticate(&bad_offsets).is_ok(), "target info out of range is read as empty");
}

#[test]
fn spnego_finds_the_ntlm_token_in_a_server_answer() {
    let token = [1, 2, 3, 4, 5];
    let answer = spnego::tlv(
        0xa1,
        &spnego::tlv(0x30, &[spnego::tlv(0xa0, &spnego::tlv(0x0a, &[1])), spnego::tlv(0xa2, &spnego::tlv(0x04, &token))].concat()),
    );
    assert_eq!(spnego::response_token(&answer), Some(&token[..]));
    // A long token takes the multi-byte DER length.
    let big: Vec<u8> = (0..300).map(|i| i as u8).collect();
    let wrapped = spnego::tlv(0xa1, &spnego::tlv(0x30, &spnego::tlv(0xa2, &spnego::tlv(0x04, &big))));
    assert_eq!(spnego::response_token(&wrapped), Some(&big[..]));
    // A bare NTLM message is taken as it is.
    let bare = [&ntlm::SIGNATURE[..], &[2, 0, 0, 0]].concat();
    assert_eq!(spnego::response_token(&bare), Some(&bare[..]));
    assert_eq!(spnego::response_token(&[0xa1, 0x85, 1]), None);
}

// ---- SMB 3 keys, signatures and encryption: Microsoft's published examples ----

#[test]
fn smb_3_0_keys_match_the_published_derivation() {
    let session = key("7CD451825D0450D235424E44BA6E78CC");
    let (signing, ciphers) = derive_keys(DIALECT_300, &session, &[]);
    let (encryption, decryption) = ciphers.unwrap();
    assert_eq!(signing, key("0B7E9C5CAC36C0F6EA9AB275298CEDCE"));
    assert_eq!(encryption, key("FAD27796665B313EBB578F388632B4F7"));
    assert_eq!(decryption, key("B0F0427F7CEB416D1D9DCC0CD4F99447"));
    assert_eq!(crypto::kdf(&session, &label("SMB2APP"), &label("SmbRpc")), key("BB23A4575AA26C721AF525AF15A87B4F"));
    // 3.0.2 derives the same way.
    assert_eq!(derive_keys(DIALECT_302, &session, &[]).0, signing);
    // Before 3.0 the session key signs and nothing encrypts.
    assert_eq!(derive_keys(DIALECT_210, &session, &[]), (session, None));
}

#[test]
fn smb_3_1_1_preauth_hash_keys_and_signature_match_the_published_example() {
    // The first step of the pre-authentication hash: 64 zero bytes, then the negotiate
    // request without its 4-byte transport header.
    let negotiate = hex(
        "FE534D4240000100000000000000800000000000000000000100000000000000FFFE000000000000
         00000000000000000000000000000000000000000000000024000500000000003F000000ECD86F32
         6276024F9F7752B89BB33F3A70000000020000000202100200030203110300000100260000000000
         010020000100FA49E6578F1F3A9F4CD3E9CC14A67AA884B3D05844E0E5A118225C15887F32FF0000
         0200060000000000020002000100",
    );
    assert_eq!(
        crypto::sha512(&[&[0; 64], &negotiate]).to_vec(),
        hex("DD94EFC5321BB618A2E208BA8920D2F422992526947A409B5037DE1E0FE8C7362B8C47122594CDE0
             CE26AA9DFC8BCDBDE0621957672623351A7540F1E54A0426")
    );

    let preauth = hex(
        "B23F3CBFD69487D9832B79B1594A367CDD950909B774C3A4C412B4FCEA9EDDDBA7DB256BA2EA30E9
         77F11F9B113247578E0E915C6D2A513B8F2FCA5707DC8770",
    );
    let (signing, ciphers) = derive_keys(DIALECT_311, &key("419FDDF34C1E001909D362AE7FB6AF79"), &preauth);
    assert_eq!(signing, key("8765949DFEAEE105CE9118B45BE988F0"));
    assert_eq!(ciphers, Some((key("A2F5E80E5D59103034F32E52F698E5EC"), key("748C50868C90F302962A5C35F5F9A8BF"))));

    // The server's final session setup reply, signed with that key by AES-CMAC.
    let reply = hex(
        "FE534D4240000100000000000100800009000000000000000200000000000000FFFE000000000000
         25000000001000006B85A4519A0F3EEA35BA946DD3AFE6B80900000048001D00A11B3019A0030A01
         00A3120410010000003932A87523AB660100000000",
    );
    assert_eq!(signature(DIALECT_311, &signing, &reply).to_vec(), hex("6B85A4519A0F3EEA35BA946DD3AFE6B8"));
    assert_eq!(&reply[48..64], &hex("6B85A4519A0F3EEA35BA946DD3AFE6B8")[..]);

    let preauth = hex(
        "0DD13628CC3ED218EF9DF9772D436D0887AB9814BFAE63A80AA845F36909DB7928622DDDAD522D97
         51640A459762C5A9D6BB084CBB3CE6BDADEF5D5BCE3C6C01",
    );
    let (signing, ciphers) = derive_keys(DIALECT_311, &key("270E1BA896585EEB7AF3472D3B4C75A7"), &preauth);
    assert_eq!(signing, key("73FE7A9A77BEF0BDE49C650D8CCB5F76"));
    assert_eq!(ciphers, Some((key("629BCBC54422A0F572B97F45989B6073"), key("E2AF0DCEFAC68DA71A0DFBD0D1350D74"))));
}

const SESSION_GCM: u64 = 0x1000_0000_0025;
const SESSION_CCM: u64 = 0x1000_0000_0021;

fn nonce(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    let bytes = hex(s);
    out[..bytes.len()].copy_from_slice(&bytes);
    out
}

#[test]
fn smb_3_1_1_gcm_encryption_matches_the_published_example() {
    let write = hex(
        "FE534D4240000100000000000900010008000000000000000500000000000000FFFE000001000000
         25000000001000000000000000000000000000000000000031007000170000000000000000000000
         0600000004000000010000000400000000000000000000007000000000000000536D623320656E63
         72797074696F6E2074657374696E67",
    );
    let transformed = hex(
        "FD534D42BD73D97D2BC9001BCAFAC0FDFF5FEEBCC7D6822D269CAF48904C664C0000000087000000
         0000010025000000001000006ECDD2A7AFC7B47763057A041B8FD4DAFFE990B70C9E09D36C084E02
         D14EF247F8BDE38ACF6256F8B1D3B56F77FBDEB312FEA5E92CBCC1ED8FB2EBBFAA75E49A4A394BB4
         4576545567C24D4C014D47C9FBDFDAFD2C4F9B72F8D256452620A299F48E29E53D6B61D1C13A19E9
         1AF013F00D17E3ABC2FC3D36C8C1B6B93973253852DBD442E46EE8",
    );
    let encryption = key("A2F5E80E5D59103034F32E52F698E5EC");
    // The four bytes past GCM's 12 are sent as zeros whatever the caller passed.
    let mut n = nonce("C7D6822D269CAF48904C664C");
    n[12..].fill(0xEE);
    assert_eq!(seal_frame(Cipher::Aes128Gcm, &encryption, &n, SESSION_GCM, &write).unwrap(), transformed);

    let reply = hex(
        "FD534D42ACBE1CB7ED343ADF1725EF144D90D4B0E06831DD2E8EB7B4000000000000000050000000
         00000100250000000010000026BBBF949983A6C1C796559D0F2C510CB651D1F7B6AC8DED32A2A0B8
         F2D793A815C6F6B848D69767A215841A42D400AE6DDB5F0B44173A014973321FDD7950DA6179159B
         82E03C9E18A050FF0EA1C967",
    );
    let decryption = key("748C50868C90F302962A5C35F5F9A8BF");
    assert_eq!(
        open_frame(Cipher::Aes128Gcm, &decryption, &reply).unwrap(),
        hex("FE534D4240000100000000000900010001000000000000000500000000000000FFFE000001000000
             25000000001000000000000000000000000000000000000011000000170000000000000000000000")
    );
    let mut forged = reply.clone();
    forged[60] ^= 1;
    assert_eq!(open_frame(Cipher::Aes128Gcm, &decryption, &forged), Err(SmbError::Io));
    assert_eq!(open_frame(Cipher::Aes128Gcm, &encryption, &reply), Err(SmbError::Io), "the other direction's key");
}

#[test]
fn smb_3_1_1_ccm_encryption_matches_the_published_example() {
    let preauth = hex(
        "DECF98A420718718F22090D3580FCC5E484BD310FA1268210C6E86335A8891E767F5BCD99FA5A785
         9D665AD07A73EA94E1BCDB7CFA69A6962A28A244138340B1",
    );
    let (signing, ciphers) = derive_keys(DIALECT_311, &key("07B7F69C1E2581662DF6987E88F9E891"), &preauth);
    assert_eq!(signing, key("3DCC82C5795AE27F383242761078C59B"));
    let (encryption, decryption) = ciphers.unwrap();
    assert_eq!(encryption, key("DFAAA31AAE40A2485D47AC4DF09FDA1D"));
    assert_eq!(decryption, key("95C544AEF6072680DA1CE49A68A97FA6"));

    let write = hex(
        "FE534D4240000100000000000900010008000000000000000500000000000000FFFE000001000000
         21000000001000000000000000000000000000000000000031007000170000000000000000000000
         0500000004000000010000000400000000000000000000007000000000000000536D623320656E63
         72797074696F6E2074657374696E67",
    );
    let transformed = hex(
        "FD534D42E89551D666DAB8993488F5A97103116C9F6F1EAAD7E9F24AACD38F000000000087000000
         00000100210000000010000056A74778199A9D2B6E9C3A376FD88D27680694FED253A313BEB07381
         AE8689F973ACDB8D716E4477803BCE53A92E1B81FA3E965AD9AF2C89C08CE66A344664453B8FC881
         18EDC9814CF58E92AA465E6EFB09958A9FDAD96FBD55B36A710C30D5E7C64AD7B9449F9F17EDD024
         FE8BA79154F340A82740D1D5180C69B0A2DE6A4BA893BD55D3210E",
    );
    assert_eq!(seal_frame(Cipher::Aes128Ccm, &encryption, &nonce("9F6F1EAAD7E9F24AACD38F"), SESSION_CCM, &write).unwrap(), transformed);

    let reply = hex(
        "FD534D42DD33EC41A927DD51476FE887C2D3C136D96831DD2E8EB7B4000000000000000050000000
         000001002100000000100000F783157E0F6F1C055D746753CA16D20C21088E2A67564E056C2F68A7
         F14F226C3BD809B7A2D52E5FE4ECF49821BC6001733430CF174E2764B3CCB213AAD8BB9FBAF6C15E
         13D9120965390E004A96A3F7",
    );
    assert_eq!(
        open_frame(Cipher::Aes128Ccm, &decryption, &reply).unwrap(),
        hex("FE534D4240000100000000000900010001000000000000000500000000000000FFFE000001000000
             21000000001000000000000000000000000000000000000011000000170000000000000000000000")
    );
    let mut forged = reply;
    forged[10] ^= 1; // in the tag
    assert_eq!(open_frame(Cipher::Aes128Ccm, &decryption, &forged), Err(SmbError::Io));
}

#[test]
fn seal_and_open_round_trip_for_both_ciphers_and_a_flipped_bit_is_refused() {
    let k: [u8; 16] = core::array::from_fn(|i| (i * 7) as u8);
    let plain: Vec<u8> = (0..1000).map(|i| (i * 13) as u8).collect();
    for cipher in [Cipher::Aes128Ccm, Cipher::Aes128Gcm] {
        let mut sealed = seal_frame(cipher, &k, &[3; 16], 9, &plain).unwrap();
        assert_eq!(sealed.len(), TRANSFORM_BYTES + plain.len());
        assert_eq!(open_frame(cipher, &k, &sealed).unwrap(), plain);
        sealed[TRANSFORM_BYTES + 10] ^= 1;
        assert!(open_frame(cipher, &k, &sealed).is_err());
    }
    assert!(open_frame(Cipher::Aes128Gcm, &k, &[0xFD, b'S', b'M', b'B']).is_err(), "short frame");
}

// ---- The sink against the scripted server ----

fn config(port: u16) -> SmbConfig {
    SmbConfig {
        host: "127.0.0.1".into(),
        port,
        share: "backups".into(),
        path: "wm/pass".into(),
        user: "alice".into(),
        password: "secret".into(),
        ..SmbConfig::default()
    }
}

fn round_trip(options: Options, encrypt: bool) -> std::sync::Arc<std::sync::Mutex<fake::Store>> {
    let (port, store) = fake::serve(options);
    let sink = Smb::new(NetPolicy::Any, SmbConfig { encrypt, ..config(port) });
    block_on(sink.readiness()).unwrap();
    assert!(store.lock().unwrap().dirs.contains("wm\\pass"), "folders are made level by level");
    assert!(block_on(sink.list()).unwrap().is_empty());

    let big: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    let stored = block_on(sink.write("vault.wmvault", &big)).unwrap();
    assert_eq!(stored, Stored { id: "vault.wmvault".into(), name: "vault.wmvault".into(), size: 10_000 });
    block_on(sink.write("other.wmvault", b"two")).unwrap();
    block_on(sink.write("other.wmvault", b"three")).unwrap();
    store.lock().unwrap().files.insert("wm\\pass\\half.wmvault.part".into(), b"x".to_vec());
    store.lock().unwrap().dirs.insert("wm\\pass\\sub".into());

    let mut listed = block_on(sink.list()).unwrap();
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(listed.iter().map(|f| (f.name.as_str(), f.size)).collect::<Vec<_>>(), vec![("other.wmvault", 5), ("vault.wmvault", 10_000)]);
    assert_eq!(block_on(sink.read(&stored)).unwrap(), big, "read in chunks of the server's size");
    assert_eq!(block_on(sink.read(&listed[0])).unwrap(), b"three");

    block_on(sink.delete(&stored)).unwrap();
    block_on(sink.delete(&stored)).unwrap();
    assert_eq!(block_on(sink.read(&stored)), Err(Fault::TargetMissing));
    let files: Vec<String> = store.lock().unwrap().files.keys().cloned().collect();
    assert_eq!(files, vec!["wm\\pass\\half.wmvault.part".to_owned(), "wm\\pass\\other.wmvault".to_owned()]);
    store
}

#[test]
fn smb_3_1_1_with_gcm_encrypts_everything_after_the_sign_in() {
    let store = round_trip(Options::default(), true);
    let store = store.lock().unwrap();
    assert!(store.sealed_requests > 20);
    assert_eq!(store.signed_requests, 0, "a sealed message is not also signed");
    assert!(store.mics_checked > 0, "the server checked the NTLM MIC");
}

#[test]
fn smb_3_1_1_with_ccm_and_interim_replies() {
    let store = round_trip(Options { cipher: Some(Cipher::Aes128Ccm), pending: true, ..Options::default() }, true);
    assert!(store.lock().unwrap().sealed_requests > 20);
}

#[test]
fn smb_3_0_encrypts_with_ccm_and_its_own_keys() {
    let store = round_trip(Options { dialect: DIALECT_300, timestamp: false, ..Options::default() }, true);
    assert!(store.lock().unwrap().sealed_requests > 20);
}

#[test]
fn without_encryption_every_request_is_signed() {
    for dialect in [DIALECT_311, DIALECT_302, DIALECT_210, DIALECT_202] {
        let store = round_trip(Options { dialect, ..Options::default() }, false);
        let store = store.lock().unwrap();
        assert_eq!(store.sealed_requests, 0, "{dialect:x}");
        assert!(store.signed_requests > 20, "{dialect:x}");
    }
}

#[test]
fn a_server_or_share_that_demands_encryption_gets_it() {
    for options in [Options { encrypt_session: true, ..Options::default() }, Options { encrypt_share: true, ..Options::default() }] {
        let store = round_trip(options, false);
        assert!(store.lock().unwrap().sealed_requests > 20);
    }
}

#[test]
fn a_server_that_cannot_encrypt_is_unsafe_when_encryption_is_asked_for() {
    for options in [
        Options { dialect: DIALECT_210, ..Options::default() },
        Options { cipher: None, ..Options::default() },
        Options { dialect: DIALECT_302, cipher: None, ..Options::default() },
    ] {
        let (port, store) = fake::serve(options);
        assert_eq!(block_on(Smb::new(NetPolicy::Any, config(port)).readiness()), Err(Fault::Unsafe));
        assert!(store.lock().unwrap().files.is_empty());
    }
}

#[test]
fn a_reply_in_the_clear_on_an_encrypted_session_is_unsafe() {
    let (port, _) = fake::serve(Options { cleartext: true, ..Options::default() });
    assert_eq!(block_on(Smb::new(NetPolicy::Any, config(port)).readiness()), Err(Fault::Unsafe));
}

#[test]
fn a_tampered_reply_is_refused() {
    for encrypt in [true, false] {
        let (port, _) = fake::serve(Options { tamper: true, ..Options::default() });
        assert_eq!(block_on(Smb::new(NetPolicy::Any, SmbConfig { encrypt, ..config(port) }).readiness()), Err(Fault::Io));
    }
}

#[test]
fn a_wrong_password_and_a_guest_session_both_lose_permission() {
    let (port, _) = fake::serve(Options::default());
    let wrong = SmbConfig { password: "nope".into(), ..config(port) };
    assert_eq!(block_on(Smb::new(NetPolicy::Any, wrong.clone()).readiness()), Err(Fault::PermissionLost));
    let (port, store) = fake::serve(Options { guest_on_bad_password: true, ..Options::default() });
    assert_eq!(block_on(Smb::new(NetPolicy::Any, SmbConfig { port, ..wrong }).write("v", b"x")), Err(Fault::PermissionLost));
    assert!(store.lock().unwrap().files.is_empty(), "nothing is written as a guest");
}

#[test]
fn offline_and_unconfigured_never_connect() {
    let (port, store) = fake::serve(Options::default());
    assert_eq!(block_on(Smb::new(NetPolicy::Offline, config(port)).list()), Err(Fault::Blocked));
    assert_eq!(block_on(Smb::new(NetPolicy::WifiOnly { on_wifi: false }, config(port)).list()), Err(Fault::Blocked));
    let unconfigured = SmbConfig { share: String::new(), ..config(port) };
    assert_eq!(block_on(Smb::new(NetPolicy::Any, unconfigured).readiness()), Err(Fault::NotConfigured));
    assert_eq!(store.lock().unwrap().connections, 0);
}

#[test]
fn names_cannot_walk_out_of_the_folder() {
    let (port, store) = fake::serve(Options::default());
    let sink = Smb::new(NetPolicy::Any, config(port));
    for bad in ["..\\x", "a/b", "..", "", "c:x"] {
        assert_eq!(block_on(sink.write(bad, b"x")).err(), Some(Fault::NotConfigured), "{bad}");
    }
    let hostile = Stored { id: "..\\..\\x".into(), ..Stored::default() };
    assert_eq!(block_on(sink.delete(&hostile)), Err(Fault::NotConfigured));
    assert_eq!(store.lock().unwrap().connections, 0);
}

#[test]
fn a_missing_folder_is_target_missing_and_nothing_is_left_half_written() {
    let (port, _) = fake::serve(Options::default());
    let sink = Smb::new(NetPolicy::Any, config(port));
    assert_eq!(block_on(sink.list()), Err(Fault::TargetMissing));
    assert_eq!(block_on(sink.write("v", b"x")), Err(Fault::TargetMissing));
}

#[test]
fn debug_output_names_no_host_share_account_or_password() {
    let sink = Smb::new(NetPolicy::Any, SmbConfig { password: "hunter2".into(), ..config(445) });
    let printed = format!("{sink:?}");
    for secret in ["127.0.0.1", "backups", "alice", "hunter2", "wm"] {
        assert!(!printed.contains(secret), "{printed}");
    }
}

/// A real Samba: `WMPASS_LIVE_SMB=port:share:user:password[:noenc]`, on localhost.
#[test]
#[ignore = "needs a Samba server"]
fn live_samba_round_trip() {
    let Ok(spec) = std::env::var("WMPASS_LIVE_SMB") else { return };
    let parts: Vec<&str> = spec.split(':').collect();
    let live = |encrypt: bool, password: &str| {
        Smb::new(
            NetPolicy::Any,
            SmbConfig {
                host: "127.0.0.1".into(),
                port: parts[0].parse().unwrap(),
                share: parts[1].into(),
                path: format!("wm/enc-{encrypt}"),
                user: parts[2].into(),
                password: password.into(),
                encrypt,
                ..SmbConfig::default()
            },
        )
    };
    // A fifth part, "noenc", says the server stops at SMB 2 and cannot encrypt: then
    // asking for encryption must be refused, not ignored.
    let can_encrypt = parts.get(4) != Some(&"noenc");
    if !can_encrypt {
        assert_eq!(block_on(live(true, parts[3]).readiness()), Err(Fault::Unsafe));
    }
    for encrypt in if can_encrypt { vec![true, false] } else { vec![false] } {
        let sink = live(encrypt, parts[3]);
        block_on(sink.readiness()).unwrap();
        let stored = block_on(sink.write("wmpass-live.bin", b"live bytes")).unwrap();
        assert_eq!(block_on(sink.read(&stored)).unwrap(), b"live bytes");
        assert!(block_on(sink.list()).unwrap().iter().any(|f| f.name == "wmpass-live.bin"));
        block_on(sink.delete(&stored)).unwrap();
    }
    assert_eq!(block_on(live(false, "wrong").readiness()), Err(Fault::PermissionLost));
}
