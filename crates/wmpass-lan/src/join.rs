//! Bringing a new device into a vault.
//!
//! A new device has no vault yet, so the proof every sync starts with, that both ends hold the
//! vault key, cannot run. The person holding both devices vouches for the connection instead:
//! by scanning a QR code that carries a full secret, or by comparing six digits on both
//! screens (see [`crate::pair`]). The device with the vault then sends its sealed file and
//! the attachments, and the two stay paired for later syncs. Nothing here opens the file: the
//! new device still needs the master password.
//!
//! The new device listens and the one with the vault connects, so a phone can scan a code
//! shown by a computer or a terminal, and a computer with no camera can still send by digits.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::channel::Channel;
use crate::pair::{PairingQr, PendingCode, begin_code_pairing};
use crate::sync::BlobStore;
use crate::{Identity, LanError, Peer, Result, Role};

const VERSION: u32 = 1;
/// The first byte the sending device writes, before the handshake, saying how it was vouched
/// for. It is not protected, and needs no protection: each way has its own handshake.
const BY_QR: u8 = b'Q';
const BY_CODE: u8 = b'C';
/// More attachments than any vault holds, so a peer cannot keep a new device busy for ever.
const MAX_BLOBS: usize = 100_000;

#[derive(Serialize, Deserialize)]
struct Header {
    version: u32,
    /// The sealed vault file, as it sits on disk.
    file: Vec<u8>,
    /// How many attachment messages follow.
    blobs: usize,
}

#[derive(Serialize, Deserialize)]
struct Blob {
    name: String,
    bytes: Vec<u8>,
}

/// What a new device received.
#[derive(Debug)]
pub struct Joined {
    /// The device that sent it, to trust for later syncs.
    pub peer: Peer,
    /// The sealed vault file.
    pub file: Vec<u8>,
    /// Attachments written to the blob store.
    pub blobs: usize,
}

/// What came of a connection to a new device.
#[derive(Debug)]
pub enum Arrival<S> {
    /// Vouched for by the QR code, so the vault is already here.
    Done(Joined),
    /// Waiting for the person to compare the six digits on both screens.
    Code(PendingJoin<S>),
}

/// A new device's half of a digits comparison.
#[derive(Debug)]
pub struct PendingJoin<S> {
    pending: PendingCode<S>,
}

impl<S: Read + Write> PendingJoin<S> {
    /// The six digits to compare with the other screen.
    pub fn code(&self) -> &str {
        self.pending.code()
    }

    /// The device offering its vault, for showing.
    pub fn peer(&self) -> &Peer {
        self.pending.peer()
    }

    /// Reports what the person decided, and takes the vault if both sides accepted.
    pub fn finish<B: BlobStore>(self, accepted: bool, blobs: &mut B) -> Result<Joined> {
        let (peer, mut channel) = self.pending.finish_open(accepted)?;
        receive(&mut channel, peer, blobs)
    }
}

/// The new device's side of one connection from the device with the vault. `qr` is the code
/// on show, if any: a sender that scanned it needs no digits compared.
pub fn arrive<S: Read + Write, B: BlobStore>(
    mut stream: S,
    identity: &Identity,
    name: &str,
    qr: Option<&PairingQr>,
    now_ms: u64,
    blobs: &mut B,
) -> Result<Arrival<S>> {
    let mut mode = [0u8; 1];
    stream.read_exact(&mut mode)?;
    match mode[0] {
        BY_QR => {
            let qr = qr.ok_or(LanError::BadCode)?;
            let (peer, mut channel) = qr.pair_open(stream, identity, name, Role::Responder, now_ms)?;
            Ok(Arrival::Done(receive(&mut channel, peer, blobs)?))
        }
        BY_CODE => Ok(Arrival::Code(PendingJoin { pending: begin_code_pairing(stream, identity, name, Role::Responder)? })),
        _ => Err(LanError::Version),
    }
}

/// The vault's side, after scanning the new device's QR code. `names` are the attachments the
/// vault refers to; only those are read from `blobs`.
#[allow(clippy::too_many_arguments)]
pub fn send_by_qr<S: Read + Write, B: BlobStore>(
    mut stream: S,
    qr: &PairingQr,
    identity: &Identity,
    name: &str,
    now_ms: u64,
    file: &[u8],
    names: &[String],
    blobs: &B,
) -> Result<Peer> {
    stream.write_all(&[BY_QR])?;
    let (peer, mut channel) = qr.pair_open(stream, identity, name, Role::Initiator, now_ms)?;
    send(&mut channel, file, names, blobs)?;
    Ok(peer)
}

/// The vault's side by digits: shows [`PendingCode::code`], then [`finish_send`].
pub fn begin_send_by_code<S: Read + Write>(mut stream: S, identity: &Identity, name: &str) -> Result<PendingCode<S>> {
    stream.write_all(&[BY_CODE])?;
    begin_code_pairing(stream, identity, name, Role::Initiator)
}

/// Sends the vault once both people have accepted the digits.
pub fn finish_send<S: Read + Write, B: BlobStore>(
    pending: PendingCode<S>,
    accepted: bool,
    file: &[u8],
    names: &[String],
    blobs: &B,
) -> Result<Peer> {
    let (peer, mut channel) = pending.finish_open(accepted)?;
    send(&mut channel, file, names, blobs)?;
    Ok(peer)
}

fn send<S: Read + Write, B: BlobStore>(channel: &mut Channel<S>, file: &[u8], names: &[String], blobs: &B) -> Result<()> {
    let present: Vec<(&String, Vec<u8>)> = names.iter().filter_map(|name| Some((name, blobs.get(name)?))).collect();
    channel.send_value(&Header { version: VERSION, file: file.to_vec(), blobs: present.len() })?;
    for (name, bytes) in present {
        channel.send_value(&Blob { name: name.clone(), bytes })?;
    }
    Ok(())
}

fn receive<S: Read + Write, B: BlobStore>(channel: &mut Channel<S>, peer: Peer, blobs: &mut B) -> Result<Joined> {
    let header: Header = channel.recv_value()?;
    if header.version != VERSION {
        return Err(LanError::Version);
    }
    if header.blobs > MAX_BLOBS {
        return Err(LanError::TooLarge);
    }
    // Only a vault file is kept, though it cannot be opened here.
    wmpass_core::format::Header::parse(&header.file).map_err(|_| LanError::Decode)?;
    for _ in 0..header.blobs {
        let blob: Blob = channel.recv_value()?;
        let plain = !blob.name.is_empty()
            && blob.name.len() <= 128
            && !blob.name.starts_with('.')
            && blob.name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
        if !plain {
            return Err(LanError::Decode);
        }
        blobs.put(&blob.name, &blob.bytes)?;
    }
    Ok(Joined { peer, file: header.file, blobs: header.blobs })
}
