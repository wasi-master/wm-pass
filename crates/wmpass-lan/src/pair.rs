//! Introducing two devices.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use wmpass_core::crypto;
use zeroize::Zeroizing;

use crate::channel::Channel;
use crate::{Identity, LanError, Peer, Result, Role, decode, encode};

/// How long a pairing QR stays good.
pub const QR_TTL_MS: u64 = 120_000;
const QR_PREFIX: &str = "wmpass-pair:1:";

#[derive(Serialize, Deserialize)]
struct Hello {
    name: String,
}

fn exchange_names<S: Read + Write>(channel: &mut Channel<S>, name: &str, role: Role) -> Result<String> {
    let mine = Hello { name: name.to_owned() };
    let theirs: Hello = match role {
        Role::Initiator => {
            channel.send_value(&mine)?;
            channel.recv_value()?
        }
        Role::Responder => {
            let theirs = channel.recv_value()?;
            channel.send_value(&mine)?;
            theirs
        }
    };
    // A name is only ever displayed, and a peer chooses its own, so it is kept short.
    Ok(theirs.name.chars().filter(|c| !c.is_control()).take(64).collect())
}

/// A pairing waiting for a person to compare the codes.
#[derive(Debug)]
pub struct PendingCode<S> {
    channel: Channel<S>,
    code: String,
    peer: Peer,
    role: Role,
}

impl<S: Read + Write> PendingCode<S> {
    /// The six digits to compare with the other screen.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The device being paired, for showing.
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// Reports what the person decided. Pairing holds only if both sides accept.
    pub fn finish(mut self, accepted: bool) -> Result<Peer> {
        let theirs: bool = match self.role {
            Role::Initiator => {
                self.channel.send_value(&accepted)?;
                self.channel.recv_value()?
            }
            Role::Responder => {
                let theirs = self.channel.recv_value()?;
                self.channel.send_value(&accepted)?;
                theirs
            }
        };
        if accepted && theirs { Ok(self.peer) } else { Err(LanError::Declined) }
    }
}

fn commit_message(hash: &[u8; 32]) -> Vec<u8> {
    let mut message = b"wmpass/lan/commit/v1".to_vec();
    message.extend_from_slice(hash);
    message
}

fn commit(nonce: &[u8; 32], hash: &[u8; 32]) -> Result<[u8; 32]> {
    Ok(crypto::hmac_sha256(nonce, &commit_message(hash))?)
}

/// Six digits from both nonces and the session.
pub fn short_code(initiator: &[u8; 32], responder: &[u8; 32], hash: &[u8; 32]) -> Result<String> {
    let mut key = Zeroizing::new(initiator.to_vec());
    key.extend_from_slice(responder);
    let mut message = b"wmpass/lan/code/v1".to_vec();
    message.extend_from_slice(hash);
    let tag = crypto::hmac_sha256(&key, &message)?;
    let mut head = [0u8; 8];
    head.copy_from_slice(&tag[..8]);
    // The bias from reducing 64 bits to a million values is under 2^-44.
    Ok(format!("{:06}", u64::from_be_bytes(head) % 1_000_000))
}

/// Starts a pairing by comparing codes on both screens.
///
/// The initiator commits to its nonce before seeing the responder's, and the responder
/// reveals before seeing the initiator's. A party in the middle therefore has to fix its
/// values on one side before it can know how the other side's code will come out, and
/// cannot try many values to make the two screens agree.
pub fn begin_code_pairing<S: Read + Write>(stream: S, identity: &Identity, name: &str, role: Role) -> Result<PendingCode<S>> {
    let mut channel = Channel::handshake(stream, identity, role, None)?;
    let hash = *channel.hash();
    let peer_name = exchange_names(&mut channel, name, role)?;
    let nonce: [u8; 32] = crypto::random_array()?;
    let (initiator, responder) = match role {
        Role::Initiator => {
            channel.send_value(&commit(&nonce, &hash)?)?;
            let theirs: [u8; 32] = channel.recv_value()?;
            channel.send_value(&nonce)?;
            (nonce, theirs)
        }
        Role::Responder => {
            let committed: [u8; 32] = channel.recv_value()?;
            channel.send_value(&nonce)?;
            let theirs: [u8; 32] = channel.recv_value()?;
            crypto::hmac_sha256_verify(&theirs, &commit_message(&hash), &committed).map_err(|_| LanError::Tampered)?;
            (theirs, nonce)
        }
    };
    let code = short_code(&initiator, &responder, &hash)?;
    let peer = Peer { key: *channel.remote(), name: peer_name };
    Ok(PendingCode { channel, code, peer, role })
}

/// What a pairing QR holds.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingQr {
    secret: Zeroizing<[u8; 32]>,
    /// Where the showing device listens.
    pub address: String,
    /// When it stops working.
    pub expires_ms: u64,
}

impl std::fmt::Debug for PairingQr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingQr").field("address", &self.address).field("expires_ms", &self.expires_ms).finish_non_exhaustive()
    }
}

impl PairingQr {
    /// A new code for a device listening at `address`.
    pub fn new(address: &str, now_ms: u64) -> Result<Self> {
        Ok(Self {
            secret: Zeroizing::new(crypto::random_array()?),
            address: address.to_owned(),
            expires_ms: now_ms.saturating_add(QR_TTL_MS),
        })
    }

    /// The text to draw as a QR code.
    pub fn text(&self) -> String {
        format!("{QR_PREFIX}{}:{}:{}", encode(&self.secret[..]), self.expires_ms, encode(self.address.as_bytes()))
    }

    /// Reads a scanned code.
    pub fn parse(text: &str, now_ms: u64) -> Result<Self> {
        let body = text.strip_prefix(QR_PREFIX).ok_or(LanError::BadCode)?;
        let mut parts = body.split(':');
        let secret = decode(parts.next().ok_or(LanError::BadCode)?)?;
        let secret: [u8; 32] = secret.as_slice().try_into().map_err(|_| LanError::BadCode)?;
        let expires_ms: u64 = parts.next().ok_or(LanError::BadCode)?.parse().map_err(|_| LanError::BadCode)?;
        let address = decode(parts.next().ok_or(LanError::BadCode)?)?;
        let address = String::from_utf8(address).map_err(|_| LanError::BadCode)?;
        if now_ms > expires_ms {
            return Err(LanError::Expired);
        }
        Ok(Self { secret: Zeroizing::new(secret), address, expires_ms })
    }

    /// Completes a pairing over a connection. The code is a full 256-bit secret, so no
    /// comparison is needed, and the handshake fails without it.
    pub fn pair<S: Read + Write>(&self, stream: S, identity: &Identity, name: &str, role: Role, now_ms: u64) -> Result<Peer> {
        if now_ms > self.expires_ms {
            return Err(LanError::Expired);
        }
        let mut channel = Channel::handshake(stream, identity, role, Some(&self.secret))?;
        let peer_name = exchange_names(&mut channel, name, role)?;
        Ok(Peer { key: *channel.remote(), name: peer_name })
    }
}
