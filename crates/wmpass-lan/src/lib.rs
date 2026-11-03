//! Device-to-device sync on a local network, without any server.
//!
//! Every connection runs the Noise XX handshake, which gives forward secrecy and mutual
//! authentication against pinned device keys. Holding a pinned key is not enough to get
//! vault data: before anything else, each side proves it holds the vault key too.
//!
//! Pairing has two forms. A QR code carries a full 256-bit secret, mixed into the
//! handshake. Six digits cannot be used that way, because a short secret in a handshake
//! can be guessed offline by whoever answered it. So a typed-code pairing runs the
//! handshake unauthenticated, then both screens show a short code derived from it through
//! a commit and reveal, and the people holding the devices compare them. An attacker in the
//! middle has one chance in a million per attempt.

pub mod channel;
pub mod discovery;
pub mod join;
pub mod pair;
pub mod share;
pub mod sync;

use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

/// Why a LAN operation stopped. No variant carries a key, name or address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LanError {
    /// The connection broke or timed out.
    #[error("the connection was lost")]
    Io,
    /// The handshake failed: a wrong code, or someone tampering.
    #[error("the devices could not establish a secure connection")]
    Handshake,
    /// The other device is not paired with this one.
    #[error("that device is not paired")]
    UnknownPeer,
    /// The other device does not hold this vault's key.
    #[error("that device does not have this vault")]
    NotThisVault,
    /// A person declined on the other device.
    #[error("the pairing was declined")]
    Declined,
    /// A commitment did not match what was revealed.
    #[error("the pairing failed a security check")]
    Tampered,
    /// A message is over the limit.
    #[error("the data is too large")]
    TooLarge,
    /// A message could not be read.
    #[error("the other device sent something unreadable")]
    Decode,
    /// A code or offer has expired.
    #[error("that code has expired")]
    Expired,
    /// A code or offer is not one of ours.
    #[error("that code is not valid")]
    BadCode,
    /// A vault or crypto operation failed.
    #[error("the vault could not be updated")]
    Core,
    /// The other device runs a version that speaks differently.
    #[error("update WM Pass on both devices")]
    Version,
    /// mDNS could not start.
    #[error("device discovery is unavailable")]
    Discovery,
}

/// Shorthand.
pub type Result<T> = std::result::Result<T, LanError>;

impl From<std::io::Error> for LanError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

impl From<snow::Error> for LanError {
    fn from(_: snow::Error) -> Self {
        Self::Handshake
    }
}

impl From<wmpass_core::Error> for LanError {
    fn from(_: wmpass_core::Error) -> Self {
        Self::Core
    }
}

/// This device's long-lived key. Keep it in the app's encrypted settings, one per device,
/// never synced.
#[derive(Clone)]
pub struct Identity {
    secret: Zeroizing<[u8; 32]>,
    public: [u8; 32],
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity").field("public", &self.public).finish_non_exhaustive()
    }
}

impl Identity {
    /// A new key from the OS random source.
    pub fn generate() -> Result<Self> {
        let secret: [u8; 32] = wmpass_core::crypto::random_array()?;
        Ok(Self::from_secret(secret))
    }

    /// Restores a saved key.
    pub fn from_secret(secret: [u8; 32]) -> Self {
        let public = PublicKey::from(&StaticSecret::from(secret)).to_bytes();
        Self { secret: Zeroizing::new(secret), public }
    }

    /// The bytes to save.
    pub fn secret_bytes(&self) -> &[u8; 32] {
        &self.secret
    }

    /// The public half.
    pub fn public(&self) -> &[u8; 32] {
        &self.public
    }
}

/// A device this one has paired with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    /// Its public key, which is what is trusted.
    pub key: [u8; 32],
    /// Its name, for showing only.
    pub name: String,
}

/// Which end of a connection this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Opened the connection.
    Initiator,
    /// Accepted it.
    Responder,
}

/// Sets the timeouts a stalled peer needs, so nothing blocks forever.
pub fn prepare(stream: &std::net::TcpStream) -> Result<()> {
    let limit = Some(std::time::Duration::from_secs(30));
    stream.set_read_timeout(limit)?;
    stream.set_write_timeout(limit)?;
    stream.set_nodelay(true)?;
    Ok(())
}

pub(crate) fn encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn decode(text: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(text).map_err(|_| LanError::BadCode)
}
