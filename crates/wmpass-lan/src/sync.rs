//! Delta sync between two paired devices. Turn-taking is strict, so two large transfers
//! can never fill both sockets and deadlock.

use std::collections::BTreeSet;
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use wmpass_core::vault::{Delta, Digests, Vault};
use wmpass_core::crypto;

use crate::channel::Channel;
use crate::{Identity, LanError, Peer, Result, Role};

const VERSION: u32 = 1;

/// Where attachment blobs live. They are already encrypted under their own keys.
pub trait BlobStore {
    /// Reads a blob, if present.
    fn get(&self, name: &str) -> Option<Vec<u8>>;
    /// Stores a blob.
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<()>;
}

/// For vaults with nowhere to keep blobs, such as a watch.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoBlobs;

impl BlobStore for NoBlobs {
    fn get(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn put(&mut self, _: &str, _: &[u8]) -> Result<()> {
        Ok(())
    }
}

/// What a sync did. The caller saves the vault afterwards.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanReport {
    /// The device on the other end.
    pub peer: Peer,
    /// Entries sent.
    pub sent: usize,
    /// Entries received.
    pub received: usize,
    /// Blobs sent.
    pub blobs_sent: usize,
    /// Blobs received.
    pub blobs_received: usize,
    /// Whether both now hold the same contents. False only if something else changed the
    /// vault meanwhile, or the devices run different versions. The next sync settles it.
    pub converged: bool,
}

#[derive(Serialize, Deserialize)]
struct Blob {
    name: String,
    bytes: Vec<u8>,
}

/// Runs one sync over a connection, as either end. `trusted` is the list of paired devices.
pub fn sync<S: Read + Write, B: BlobStore>(
    stream: S,
    identity: &Identity,
    trusted: &[Peer],
    vault: &mut Vault,
    blobs: &mut B,
    role: Role,
) -> Result<LanReport> {
    let mut channel = Channel::handshake(stream, identity, role, None)?;
    let peer = trusted
        .iter()
        .find(|peer| constant_eq(&peer.key, channel.remote()))
        .cloned()
        .ok_or(LanError::UnknownPeer)?;
    let lan_key = vault.purpose_key("wmpass/lan/vault-proof/v1")?;
    channel.prove_shared_key(&lan_key, role)?;

    let mut report = LanReport { peer, ..LanReport::default() };
    match role {
        Role::Initiator => {
            channel.send_value(&VERSION)?;
            check_version(channel.recv_value()?)?;
            channel.send_value(&vault.digests()?)?;
            let theirs: Digests = channel.recv_value()?;
            let delta: Delta = channel.recv_value()?;
            let mine = vault.delta_for(&theirs)?;
            report.received = delta.entries.len();
            report.sent = mine.entries.len();
            vault.apply_delta(delta)?;
            channel.send_value(&mine)?;
        }
        Role::Responder => {
            let theirs: u32 = channel.recv_value()?;
            channel.send_value(&VERSION)?;
            check_version(theirs)?;
            let theirs: Digests = channel.recv_value()?;
            channel.send_value(&vault.digests()?)?;
            let mine = vault.delta_for(&theirs)?;
            report.sent = mine.entries.len();
            channel.send_value(&mine)?;
            let delta: Delta = channel.recv_value()?;
            report.received = delta.entries.len();
            vault.apply_delta(delta)?;
        }
    }
    swap_blobs(&mut channel, vault, blobs, role, &mut report)?;
    let state = vault.state_hash()?;
    let theirs: [u8; 16] = match role {
        Role::Initiator => {
            channel.send_value(&state)?;
            channel.recv_value()?
        }
        Role::Responder => {
            let theirs = channel.recv_value()?;
            channel.send_value(&state)?;
            theirs
        }
    };
    report.converged = state == theirs;
    Ok(report)
}

fn check_version(theirs: u32) -> Result<()> {
    if theirs == VERSION { Ok(()) } else { Err(LanError::Version) }
}

fn constant_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    // Keyed and compared in constant time, so timing says nothing about how far a guess matched.
    let key = [0u8; 32];
    crypto::hmac_sha256(&key, a).is_ok_and(|tag| crypto::hmac_sha256_verify(&key, b, &tag).is_ok())
}

fn swap_blobs<S: Read + Write, B: BlobStore>(
    channel: &mut Channel<S>,
    vault: &Vault,
    blobs: &mut B,
    role: Role,
    report: &mut LanReport,
) -> Result<()> {
    let referenced = vault.referenced_blobs();
    let wanted: Vec<String> = referenced.iter().filter(|name| blobs.get(name).is_none()).cloned().collect();
    let asked: Vec<String> = match role {
        Role::Initiator => {
            channel.send_value(&wanted)?;
            channel.recv_value()?
        }
        Role::Responder => {
            let asked = channel.recv_value()?;
            channel.send_value(&wanted)?;
            asked
        }
    };
    match role {
        Role::Initiator => {
            send_blobs(channel, &referenced, &asked, blobs, report)?;
            recv_blobs(channel, &wanted, blobs, report)
        }
        Role::Responder => {
            recv_blobs(channel, &wanted, blobs, report)?;
            send_blobs(channel, &referenced, &asked, blobs, report)
        }
    }
}

fn send_blobs<S: Read + Write, B: BlobStore>(
    channel: &mut Channel<S>,
    referenced: &[String],
    asked: &[String],
    blobs: &B,
    report: &mut LanReport,
) -> Result<()> {
    // Only blobs this vault refers to, so a peer cannot read anything else by naming it.
    let available: Vec<&String> = asked
        .iter()
        .filter(|name| referenced.contains(name))
        .filter(|name| blobs.get(name).is_some())
        .collect();
    channel.send_value(&available.len())?;
    for name in available {
        if let Some(bytes) = blobs.get(name) {
            channel.send_value(&Blob { name: name.clone(), bytes })?;
            report.blobs_sent += 1;
        }
    }
    Ok(())
}

fn recv_blobs<S: Read + Write, B: BlobStore>(
    channel: &mut Channel<S>,
    wanted: &[String],
    blobs: &mut B,
    report: &mut LanReport,
) -> Result<()> {
    let count: usize = channel.recv_value()?;
    let wanted: BTreeSet<&String> = wanted.iter().collect();
    for _ in 0..count {
        let blob: Blob = channel.recv_value()?;
        // Only names this end asked for, so a peer cannot write anywhere else.
        if !wanted.contains(&blob.name) {
            return Err(LanError::Decode);
        }
        blobs.put(&blob.name, &blob.bytes)?;
        report.blobs_received += 1;
    }
    Ok(())
}
