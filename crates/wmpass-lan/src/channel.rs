//! The Noise session and message framing.

use std::io::{Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;
use snow::{Builder, TransportState};
use wmpass_core::crypto;
use zeroize::Zeroizing;

use crate::{Identity, LanError, Result, Role};

const XX: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
const XX_PSK: &str = "Noise_XXpsk3_25519_ChaChaPoly_SHA256";
const PROLOGUE: &[u8] = b"wmpass/lan/v1";
/// Noise's own limit on one message.
const FRAME: usize = 65_535;
const TAG: usize = 16;
/// Plaintext in one frame.
const CHUNK: usize = FRAME - TAG;
/// Refused above this, so a peer cannot demand unbounded memory.
pub const MAX_MESSAGE: usize = 256 * 1024 * 1024;

/// An established session.
pub struct Channel<S> {
    stream: S,
    noise: TransportState,
    hash: [u8; 32],
    remote: [u8; 32],
}

impl<S> std::fmt::Debug for Channel<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel").field("remote", &self.remote).finish_non_exhaustive()
    }
}

fn write_frame<S: Write>(stream: &mut S, frame: &[u8]) -> Result<()> {
    let length = u16::try_from(frame.len()).map_err(|_| LanError::TooLarge)?;
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(frame)?;
    stream.flush()?;
    Ok(())
}

fn read_frame<S: Read>(stream: &mut S, buffer: &mut [u8]) -> Result<usize> {
    let mut length = [0u8; 2];
    stream.read_exact(&mut length)?;
    let length = usize::from(u16::from_be_bytes(length));
    let target = buffer.get_mut(..length).ok_or(LanError::TooLarge)?;
    stream.read_exact(target)?;
    Ok(length)
}

impl<S: Read + Write> Channel<S> {
    /// Runs the handshake. With `psk`, a party without that secret cannot complete it.
    pub fn handshake(mut stream: S, identity: &Identity, role: Role, psk: Option<&[u8; 32]>) -> Result<Self> {
        let pattern = if psk.is_some() { XX_PSK } else { XX };
        let params = pattern.parse().map_err(|_| LanError::Handshake)?;
        let mut builder = Builder::new(params).prologue(PROLOGUE)?.local_private_key(identity.secret_bytes())?;
        if let Some(psk) = psk {
            builder = builder.psk(3, psk)?;
        }
        let mut state = match role {
            Role::Initiator => builder.build_initiator()?,
            Role::Responder => builder.build_responder()?,
        };
        let mut message = Zeroizing::new(vec![0u8; FRAME]);
        let mut payload = Zeroizing::new(vec![0u8; FRAME]);
        while !state.is_handshake_finished() {
            if state.is_my_turn() {
                let length = state.write_message(&[], &mut message)?;
                write_frame(&mut stream, &message[..length])?;
            } else {
                let length = read_frame(&mut stream, &mut message)?;
                state.read_message(&message[..length], &mut payload)?;
            }
        }
        let hash: [u8; 32] = state.get_handshake_hash().try_into().map_err(|_| LanError::Handshake)?;
        let remote: [u8; 32] = state
            .get_remote_static()
            .ok_or(LanError::Handshake)?
            .try_into()
            .map_err(|_| LanError::Handshake)?;
        let noise = state.into_transport_mode()?;
        Ok(Self { stream, noise, hash, remote })
    }

    /// The session's transcript hash. Both ends hold the same value only if nobody
    /// interfered with the keys.
    pub fn hash(&self) -> &[u8; 32] {
        &self.hash
    }

    /// The other device's public key.
    pub fn remote(&self) -> &[u8; 32] {
        &self.remote
    }

    fn send_frame(&mut self, plain: &[u8]) -> Result<()> {
        let mut frame = vec![0u8; plain.len() + TAG];
        let length = self.noise.write_message(plain, &mut frame).map_err(|_| LanError::Io)?;
        write_frame(&mut self.stream, &frame[..length])
    }

    fn recv_frame(&mut self, out: &mut [u8]) -> Result<usize> {
        let mut frame = vec![0u8; FRAME];
        let length = read_frame(&mut self.stream, &mut frame)?;
        // A failed tag here means tampering, not a network fault.
        self.noise.read_message(&frame[..length], out).map_err(|_| LanError::Handshake)
    }

    /// Sends one message of any size up to the limit.
    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_MESSAGE {
            return Err(LanError::TooLarge);
        }
        let total = u32::try_from(bytes.len()).map_err(|_| LanError::TooLarge)?;
        self.send_frame(&total.to_be_bytes())?;
        for chunk in bytes.chunks(CHUNK) {
            self.send_frame(chunk)?;
        }
        Ok(())
    }

    /// Reads one message.
    pub fn recv(&mut self) -> Result<Zeroizing<Vec<u8>>> {
        let mut header = [0u8; FRAME - TAG];
        let length = self.recv_frame(&mut header)?;
        let total: [u8; 4] = header.get(..length).and_then(|h| h.try_into().ok()).ok_or(LanError::Decode)?;
        let total = u32::from_be_bytes(total) as usize;
        if total > MAX_MESSAGE {
            return Err(LanError::TooLarge);
        }
        // Grown as frames arrive, so a false length alone cannot claim the memory.
        let mut out = Zeroizing::new(Vec::with_capacity(total.min(1024 * 1024)));
        let mut piece = Zeroizing::new(vec![0u8; CHUNK]);
        while out.len() < total {
            let length = self.recv_frame(&mut piece)?;
            if length == 0 || out.len() + length > total {
                return Err(LanError::Decode);
            }
            out.extend_from_slice(&piece[..length]);
        }
        Ok(out)
    }

    /// Sends a value.
    pub fn send_value<T: Serialize>(&mut self, value: &T) -> Result<()> {
        let bytes = Zeroizing::new(rmp_serde::to_vec_named(value).map_err(|_| LanError::Decode)?);
        self.send(&bytes)
    }

    /// Reads a value.
    pub fn recv_value<T: DeserializeOwned>(&mut self) -> Result<T> {
        let bytes = self.recv()?;
        rmp_serde::from_slice(&bytes).map_err(|_| LanError::Decode)
    }

    /// Proves to the peer that this end holds `key`, and checks that the peer does. Bound
    /// to this session, so a recorded proof is useless elsewhere.
    pub fn prove_shared_key(&mut self, key: &[u8; 32], role: Role) -> Result<()> {
        let (mine, theirs): (&[u8], &[u8]) = match role {
            Role::Initiator => (b"initiator", b"responder"),
            Role::Responder => (b"responder", b"initiator"),
        };
        let mine = crypto::hmac_sha256(key, &[mine, &self.hash[..]].concat())?;
        let theirs = [theirs, &self.hash[..]].concat();
        let check = |tag: &[u8]| crypto::hmac_sha256_verify(key, &theirs, tag).map_err(|_| LanError::NotThisVault);
        // The initiator goes first, and a responder answers only after checking, so a
        // device without the key never receives a proof it could study.
        match role {
            Role::Initiator => {
                self.send(&mine)?;
                check(&self.recv()?)
            }
            Role::Responder => {
                check(&self.recv()?)?;
                self.send(&mine)
            }
        }
    }
}
