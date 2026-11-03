//! NTLMv2, the client half, as MS-NLMP describes it: the three messages, the session
//! key, and the one signature SPNEGO asks of it.
//!
//! Only v2. LM and NTLMv1 are broken and every server this would meet has refused them
//! for years, so there is no fallback to offer.
//!
//! One value is one authentication. It keeps the negotiate and challenge messages because
//! the MIC in the third one covers all three.

use super::bytes::{Le, LeWriter, Malformed, utf16};
use super::crypto;

pub(crate) const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";
const TYPE_NEGOTIATE: u32 = 1;
const TYPE_CHALLENGE: u32 = 2;
const TYPE_AUTHENTICATE: u32 = 3;

const FLAG_UNICODE: u32 = 0x0000_0001;
const FLAG_REQUEST_TARGET: u32 = 0x0000_0004;
const FLAG_SIGN: u32 = 0x0000_0010;
const FLAG_NTLM: u32 = 0x0000_0200;
const FLAG_ALWAYS_SIGN: u32 = 0x0000_8000;
const FLAG_EXTENDED_SESSION_SECURITY: u32 = 0x0008_0000;
const FLAG_TARGET_INFO: u32 = 0x0080_0000;
const FLAG_VERSION: u32 = 0x0200_0000;
const FLAG_128: u32 = 0x2000_0000;
pub(crate) const FLAG_KEY_EXCH: u32 = 0x4000_0000;
const FLAG_56: u32 = 0x8000_0000;

pub(crate) const CLIENT_FLAGS: u32 = FLAG_UNICODE
    | FLAG_REQUEST_TARGET
    | FLAG_SIGN
    | FLAG_NTLM
    | FLAG_ALWAYS_SIGN
    | FLAG_EXTENDED_SESSION_SECURITY
    | FLAG_TARGET_INFO
    | FLAG_VERSION
    | FLAG_128
    | FLAG_KEY_EXCH
    | FLAG_56;

/// Windows 10, build 19041, NTLM revision 15. Servers only log it.
const VERSION: [u8; 8] = [10, 0, 0x61, 0x4A, 0, 0, 0, 15];

pub(crate) const AV_EOL: u16 = 0;
pub(crate) const AV_FLAGS: u16 = 6;
pub(crate) const AV_TIMESTAMP: u16 = 7;
const AV_FLAG_MIC: u32 = 0x2;

const CHALLENGE_MIN: usize = 48;
const AUTH_HEADER_BYTES: usize = 88;
const MIC_AT: usize = 72;
const MIC_BYTES: usize = 16;
const LM_RESPONSE_BYTES: usize = 24;

const CLIENT_SIGN_MAGIC: &[u8] = b"session key to client-to-server signing key magic constant\0";
const CLIENT_SEAL_MAGIC: &[u8] = b"session key to client-to-server sealing key magic constant\0";

/// One NTLM authentication. Holds the NT hash, never the password.
pub(crate) struct Ntlm {
    user: String,
    domain: String,
    nt_hash: [u8; 16],
    negotiate_message: Vec<u8>,
    challenge_message: Vec<u8>,
    flags: u32,
    /// The key both sides share, once [`Ntlm::authenticate`] has run.
    pub(crate) exported_session_key: [u8; 16],
    /// Test hooks: the client challenge, session key and clock are live otherwise.
    pub(crate) fixed_client_challenge: Option<[u8; 8]>,
    pub(crate) fixed_session_key: Option<[u8; 16]>,
    pub(crate) fixed_now: Option<u64>,
}

impl std::fmt::Debug for Ntlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ntlm").finish_non_exhaustive()
    }
}

/// The NT one-way function: MD4 over the UTF-16 password.
pub(crate) fn nt_hash(password: &str) -> [u8; 16] {
    crypto::md4(&utf16(password))
}

/// NTOWFv2: what the responses are keyed with.
pub(crate) fn response_key(nt_hash: &[u8; 16], user: &str, domain: &str) -> [u8; 16] {
    crypto::hmac_md5(nt_hash, &[&utf16(&format!("{}{domain}", user.to_uppercase()))])
}

impl Ntlm {
    pub(crate) fn new(user: &str, password: &str, domain: &str) -> Self {
        Self {
            user: user.to_owned(),
            domain: domain.to_owned(),
            nt_hash: nt_hash(password),
            negotiate_message: Vec::new(),
            challenge_message: Vec::new(),
            flags: 0,
            exported_session_key: [0; 16],
            fixed_client_challenge: None,
            fixed_session_key: None,
            fixed_now: None,
        }
    }

    /// Message 1.
    pub(crate) fn negotiate(&mut self) -> Vec<u8> {
        let mut w = LeWriter::new();
        w.bytes(SIGNATURE).u32(TYPE_NEGOTIATE).u32(CLIENT_FLAGS);
        // Domain and workstation fields, both empty.
        w.u16(0).u16(0).u32(0);
        w.u16(0).u16(0).u32(0);
        w.bytes(&VERSION);
        self.negotiate_message = w.into_vec();
        self.negotiate_message.clone()
    }

    /// Message 3, answering the server's message 2.
    pub(crate) fn authenticate(&mut self, challenge: &[u8]) -> Result<Vec<u8>, Malformed> {
        if challenge.len() < CHALLENGE_MIN
            || challenge.slice_at(0, 8)? != SIGNATURE
            || challenge.u32_at(8)? != TYPE_CHALLENGE
        {
            return Err(Malformed);
        }
        self.challenge_message = challenge.to_vec();
        self.flags = challenge.u32_at(20)? & CLIENT_FLAGS;
        let server_challenge = challenge.slice_at(24, 8)?;
        let info_len = usize::from(challenge.u16_at(40)?);
        let info_at = challenge.u32_at(44)? as usize;
        let target_info = if info_len > 0 { challenge.slice_at(info_at, info_len).unwrap_or(&[]) } else { &[] };
        let mut pairs = av_parse(target_info);
        let server_time = av_get(&pairs, AV_TIMESTAMP).map(<[u8]>::to_vec);
        // With a server timestamp the MIC is mandatory, and the flags pair has to say one
        // is there. Without one there is no MIC at all.
        let with_mic = server_time.is_some();

        let response_key = response_key(&self.nt_hash, &self.user, &self.domain);
        let client_challenge = match self.fixed_client_challenge {
            Some(fixed) => fixed,
            None => crypto::random().map_err(|_| Malformed)?,
        };
        let time = server_time
            .clone()
            .unwrap_or_else(|| self.fixed_now.unwrap_or_else(super::bytes::filetime_now).to_le_bytes().to_vec());
        if with_mic {
            let existing = av_get(&pairs, AV_FLAGS).and_then(|v| v.u32_at(0).ok()).unwrap_or(0);
            av_set(&mut pairs, AV_FLAGS, (existing | AV_FLAG_MIC).to_le_bytes().to_vec());
        }
        let server_name = av_encode(&pairs);
        let mut temp = LeWriter::new();
        temp.u8(1).u8(1).u16(0).u32(0).bytes(&time).bytes(&client_challenge).u32(0).bytes(&server_name).u32(0);
        let temp = temp.into_vec();
        let nt_proof = crypto::hmac_md5(&response_key, &[server_challenge, &temp]);
        let nt_response = [&nt_proof[..], &temp].concat();
        let lm_response = if with_mic {
            vec![0; LM_RESPONSE_BYTES]
        } else {
            [&crypto::hmac_md5(&response_key, &[server_challenge, &client_challenge])[..], &client_challenge].concat()
        };
        let session_base_key = crypto::hmac_md5(&response_key, &[&nt_proof]);

        // Key exchange: a fresh key of ours, sent wrapped in the one both sides derived.
        // Without the flag the derived key is the session key.
        let encrypted_key = if self.flags & FLAG_KEY_EXCH != 0 {
            self.exported_session_key = match self.fixed_session_key {
                Some(fixed) => fixed,
                None => crypto::random().map_err(|_| Malformed)?,
            };
            crypto::rc4(&session_base_key, &self.exported_session_key)
        } else {
            self.exported_session_key = session_base_key;
            Vec::new()
        };

        let domain = utf16(&self.domain);
        let user = utf16(&self.user);
        let workstation: Vec<u8> = Vec::new();
        let fields = [&lm_response, &nt_response, &domain, &user, &workstation, &encrypted_key];
        let mut w = LeWriter::new();
        w.bytes(SIGNATURE).u32(TYPE_AUTHENTICATE);
        let mut offset = AUTH_HEADER_BYTES;
        // Six length/offset fields, in header order, then the flags.
        for field in fields {
            let len = u16::try_from(field.len()).map_err(|_| Malformed)?;
            w.u16(len).u16(len).u32(u32::try_from(offset).map_err(|_| Malformed)?);
            offset += field.len();
        }
        w.u32(self.flags).bytes(&VERSION).zeros(MIC_BYTES);
        debug_assert_eq!(w.len(), AUTH_HEADER_BYTES);
        for field in fields {
            w.bytes(field);
        }
        let mut message = w.into_vec();
        if with_mic {
            let mic = crypto::hmac_md5(&self.exported_session_key, &[&self.negotiate_message, &self.challenge_message, &message]);
            message[MIC_AT..MIC_AT + MIC_BYTES].copy_from_slice(&mic);
        }
        Ok(message)
    }

    /// The NTLM signature over `message` with sequence number 0: what SPNEGO's
    /// mechListMIC is. Client-to-server keys, extended session security.
    pub(crate) fn sign(&self, message: &[u8]) -> Vec<u8> {
        let sign_key = crypto::md5(&[&self.exported_session_key, CLIENT_SIGN_MAGIC]);
        let seq = [0u8; 4];
        let mut checksum = crypto::hmac_md5(&sign_key, &[&seq, message])[..8].to_vec();
        if self.flags & FLAG_KEY_EXCH != 0 {
            let seal_key = crypto::md5(&[&self.exported_session_key, CLIENT_SEAL_MAGIC]);
            checksum = crypto::rc4(&seal_key, &checksum);
        }
        let mut w = LeWriter::new();
        w.u32(1).bytes(&checksum).bytes(&seq);
        w.into_vec()
    }
}

/// Target info: a list of (id, value) ending in EOL. Order is kept, and a repeated id
/// keeps its first place with the last value, as a linked map would.
pub(crate) fn av_parse(b: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let mut out: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut at = 0;
    while let (Ok(id), Ok(len)) = (b.u16_at(at), b.u16_at(at + 2)) {
        let Ok(value) = b.slice_at(at + 4, usize::from(len)) else { break };
        if id == AV_EOL {
            break;
        }
        av_set(&mut out, id, value.to_vec());
        at += 4 + usize::from(len);
    }
    out
}

pub(crate) fn av_get(pairs: &[(u16, Vec<u8>)], id: u16) -> Option<&[u8]> {
    pairs.iter().find(|(key, _)| *key == id).map(|(_, value)| value.as_slice())
}

pub(crate) fn av_set(pairs: &mut Vec<(u16, Vec<u8>)>, id: u16, value: Vec<u8>) {
    match pairs.iter_mut().find(|(key, _)| *key == id) {
        Some(slot) => slot.1 = value,
        None => pairs.push((id, value)),
    }
}

pub(crate) fn av_encode(pairs: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut w = LeWriter::new();
    for (id, value) in pairs {
        w.u16(*id).u16(u16::try_from(value.len()).unwrap_or(u16::MAX)).bytes(value);
    }
    w.u16(AV_EOL).u16(0);
    w.into_vec()
}
