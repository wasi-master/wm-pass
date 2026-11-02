//! Handing one entry to someone in the same room, without pairing or a shared vault.
//!
//! Two ways. For an entry that fits in a QR code, the receiver shows an offer holding a
//! throwaway public key, and the sender shows back a code only that key can open, so a
//! photograph of either code is useless. A larger entry goes over the network, with the QR
//! holding the secret for one connection.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use wmpass_core::crypto;
use wmpass_core::model::{Entry, Lww};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::channel::Channel;
use crate::{Identity, LanError, Result, Role, decode, encode};

/// How long a code stays good.
pub const SHARE_TTL_MS: u64 = 120_000;
/// Allowance for clocks disagreeing when the sender checks an offer.
const SKEW_MS: u64 = 600_000;
/// What fits a QR code that phone cameras read reliably.
pub const MAX_QR_BYTES: usize = 2_000;
const OFFER: &str = "wmpass-offer:1:";
const SEALED: &str = "wmpass-share:1:";
const INVITE: &str = "wmpass-invite:1:";

/// A copy safe to hand over: no history, no attachments or their keys, a new id, nothing
/// that says where it lived.
pub fn shareable(entry: &Entry) -> Entry {
    let mut copy = entry.clone();
    copy.id = String::new();
    copy.history.clear();
    copy.common.attachments = Lww::default();
    copy.common.folder = Lww::default();
    copy.common.links = Lww::default();
    copy.common.tags = Lww::default();
    copy.common.hidden = Lww::default();
    copy.common.pinned = Lww::default();
    copy.common.favourite = Lww::default();
    copy
}

/// The receiving end of a QR handover. It holds the only key that can open what comes back,
/// and opening consumes it.
pub struct ShareReceiver {
    secret: StaticSecret,
    public: PublicKey,
    expires_ms: u64,
}

impl std::fmt::Debug for ShareReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShareReceiver").field("expires_ms", &self.expires_ms).finish_non_exhaustive()
    }
}

fn share_key(shared: &[u8; 32], sender: &[u8; 32], receiver: &[u8; 32]) -> Result<crypto::Key> {
    let mut salt = sender.to_vec();
    salt.extend_from_slice(receiver);
    Ok(crypto::hkdf(shared, &salt, "wmpass/share/v1")?)
}

impl ShareReceiver {
    /// A new offer.
    pub fn new(now_ms: u64) -> Result<Self> {
        let secret = StaticSecret::from(crypto::random_array::<32>()?);
        let public = PublicKey::from(&secret);
        Ok(Self { secret, public, expires_ms: now_ms.saturating_add(SHARE_TTL_MS) })
    }

    /// The text to draw as a QR code.
    pub fn offer_text(&self) -> String {
        format!("{OFFER}{}:{}", encode(self.public.as_bytes()), self.expires_ms)
    }

    /// Opens what the sender showed back.
    pub fn open(self, text: &str, now_ms: u64) -> Result<Entry> {
        if now_ms > self.expires_ms {
            return Err(LanError::Expired);
        }
        let body = decode(text.strip_prefix(SEALED).ok_or(LanError::BadCode)?)?;
        let (sender, sealed) = body.split_at_checked(32).ok_or(LanError::BadCode)?;
        let sender: [u8; 32] = sender.try_into().map_err(|_| LanError::BadCode)?;
        let shared = self.secret.diffie_hellman(&PublicKey::from(sender));
        if !shared.was_contributory() {
            return Err(LanError::BadCode);
        }
        let key = share_key(shared.as_bytes(), &sender, self.public.as_bytes())?;
        let plain = crypto::open(&key, sealed, b"wmpass/share/v1").ok_or(LanError::BadCode)?;
        rmp_serde::from_slice(&plain).map_err(|_| LanError::Decode)
    }
}

/// Seals an entry to a scanned offer, giving the text to show back. Refuses one too large
/// for a code, which then goes through [`ShareInvite`] instead.
pub fn seal_to_offer(offer: &str, entry: &Entry, now_ms: u64) -> Result<String> {
    let body = offer.strip_prefix(OFFER).ok_or(LanError::BadCode)?;
    let (public, expires) = body.split_once(':').ok_or(LanError::BadCode)?;
    let expires: u64 = expires.parse().map_err(|_| LanError::BadCode)?;
    if now_ms > expires.saturating_add(SKEW_MS) {
        return Err(LanError::Expired);
    }
    let receiver: [u8; 32] = decode(public)?.as_slice().try_into().map_err(|_| LanError::BadCode)?;
    let secret = StaticSecret::from(crypto::random_array::<32>()?);
    let sender = PublicKey::from(&secret);
    let shared = secret.diffie_hellman(&PublicKey::from(receiver));
    if !shared.was_contributory() {
        return Err(LanError::BadCode);
    }
    let key = share_key(shared.as_bytes(), sender.as_bytes(), &receiver)?;
    let plain = Zeroizing::new(rmp_serde::to_vec_named(&shareable(entry)).map_err(|_| LanError::Decode)?);
    let sealed = crypto::seal(&key, &plain, b"wmpass/share/v1")?;
    let mut out = sender.as_bytes().to_vec();
    out.extend_from_slice(&sealed);
    let text = format!("{SEALED}{}", encode(&out));
    if text.len() > MAX_QR_BYTES {
        return Err(LanError::TooLarge);
    }
    Ok(text)
}

/// A one-connection invitation for entries too large for a QR code.
#[derive(Clone, PartialEq, Eq)]
pub struct ShareInvite {
    secret: Zeroizing<[u8; 32]>,
    /// Where the sender listens.
    pub address: String,
    /// When it stops working.
    pub expires_ms: u64,
}

impl std::fmt::Debug for ShareInvite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShareInvite").field("address", &self.address).field("expires_ms", &self.expires_ms).finish_non_exhaustive()
    }
}

impl ShareInvite {
    /// A new invitation.
    pub fn new(address: &str, now_ms: u64) -> Result<Self> {
        Ok(Self {
            secret: Zeroizing::new(crypto::random_array()?),
            address: address.to_owned(),
            expires_ms: now_ms.saturating_add(SHARE_TTL_MS),
        })
    }

    /// The text to draw as a QR code.
    pub fn text(&self) -> String {
        format!("{INVITE}{}:{}:{}", encode(&self.secret[..]), self.expires_ms, encode(self.address.as_bytes()))
    }

    /// Reads a scanned invitation.
    pub fn parse(text: &str, now_ms: u64) -> Result<Self> {
        let body = text.strip_prefix(INVITE).ok_or(LanError::BadCode)?;
        let mut parts = body.split(':');
        let secret: [u8; 32] = decode(parts.next().ok_or(LanError::BadCode)?)?.as_slice().try_into().map_err(|_| LanError::BadCode)?;
        let expires_ms: u64 = parts.next().ok_or(LanError::BadCode)?.parse().map_err(|_| LanError::BadCode)?;
        let address = String::from_utf8(decode(parts.next().ok_or(LanError::BadCode)?)?).map_err(|_| LanError::BadCode)?;
        if now_ms > expires_ms.saturating_add(SKEW_MS) {
            return Err(LanError::Expired);
        }
        Ok(Self { secret: Zeroizing::new(secret), address, expires_ms })
    }

    /// Sends the entry to whoever connected. Consumes the invitation, so it works once.
    pub fn serve<S: Read + Write>(self, stream: S, entry: &Entry, now_ms: u64) -> Result<()> {
        if now_ms > self.expires_ms {
            return Err(LanError::Expired);
        }
        let identity = Identity::generate()?;
        let mut channel = Channel::handshake(stream, &identity, Role::Responder, Some(&self.secret))?;
        channel.send_value(&Handover { entry: shareable(entry) })
    }

    /// Fetches the entry.
    pub fn fetch<S: Read + Write>(self, stream: S) -> Result<Entry> {
        let identity = Identity::generate()?;
        let mut channel = Channel::handshake(stream, &identity, Role::Initiator, Some(&self.secret))?;
        let handover: Handover = channel.recv_value()?;
        Ok(handover.entry)
    }
}

#[derive(Serialize, Deserialize)]
struct Handover {
    entry: Entry,
}
