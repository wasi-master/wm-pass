//! Delta sync between two paired devices. Turn-taking is strict, so two large transfers
//! can never fill both sockets and deadlock.

use std::collections::BTreeSet;
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use wmpass_core::vault::{Delta, Digests, Vault};
use wmpass_core::crypto;

use crate::channel::Channel;
use crate::{Identity, LanError, Peer, Result, Role};

/// 2 added the opening [`Intent`], so a sync and a merge can share one waiting device.
const VERSION: u32 = 2;

/// Where attachment blobs live. They are already encrypted under their own keys.
pub trait BlobStore {
    /// Reads a blob, if present.
    fn get(&self, name: &str) -> Option<Vec<u8>>;
    /// Whether a blob is present. Asked of every attachment on every sync, so a store that
    /// can answer without reading the blob should.
    fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
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
    fn has(&self, _: &str) -> bool {
        false
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
pub(crate) struct Blob {
    name: String,
    bytes: Vec<u8>,
}

/// Why a sync or merge stopped, and with which device when it got far enough to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    /// What went wrong.
    pub error: LanError,
    /// The other device's public key, once the handshake has shown it. A key that is not
    /// among the paired devices means some other WM Pass answered on this network.
    pub key: Option<[u8; 32]>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for Failure {}

impl From<LanError> for Failure {
    fn from(error: LanError) -> Self {
        Self { error, key: None }
    }
}

/// What the device that connects wants. Sent first, once the handshake is done.
#[derive(Serialize, Deserialize)]
struct Intent {
    version: u32,
    /// For a merge of two different vaults: whether the connecting device keeps its own.
    merge: Option<bool>,
}

/// The waiting device's reply to an [`Intent`].
#[derive(Serialize, Deserialize)]
enum Answer {
    Go,
    Declined,
    Version,
}

/// A connection past the handshake, with a paired device.
pub(crate) struct Opened<S> {
    pub(crate) channel: Channel<S>,
    pub(crate) peer: Peer,
}

/// Runs the handshake and checks the other end is paired.
fn handshake<S: Read + Write>(stream: S, identity: &Identity, trusted: &[Peer], role: Role) -> std::result::Result<Opened<S>, Failure> {
    let channel = Channel::handshake(stream, identity, role, None)?;
    let remote = *channel.remote();
    let at = |error| Failure { error, key: Some(remote) };
    let peer = trusted.iter().find(|peer| constant_eq(&peer.key, &remote)).cloned().ok_or(at(LanError::UnknownPeer))?;
    Ok(Opened { channel, peer })
}

/// The connecting end: says what it wants and waits for the answer. `merge` is whether this
/// end keeps its vault, for a merge.
pub(crate) fn open<S: Read + Write>(stream: S, identity: &Identity, trusted: &[Peer], merge: Option<bool>) -> std::result::Result<Opened<S>, Failure> {
    let mut opened = handshake(stream, identity, trusted, Role::Initiator)?;
    let key = opened.peer.key;
    let at = |error| Failure { error, key: Some(key) };
    opened.channel.send_value(&Intent { version: VERSION, merge }).map_err(at)?;
    // An answer that does not read as one comes from a device that speaks differently.
    let answer = opened.channel.recv_value().map_err(|error| match error {
        LanError::Decode => at(LanError::Version),
        other => at(other),
    })?;
    match answer {
        Answer::Go => Ok(opened),
        Answer::Declined => Err(at(LanError::Declined)),
        Answer::Version => Err(at(LanError::Version)),
    }
}

/// A connection with a paired device, opened and agreed to sync, and ready for the vault.
/// Getting here needs no vault, so a shell can hold the vault only for [`Ready::sync`].
pub struct Ready<S> {
    opened: Opened<S>,
    role: Role,
}

impl<S> std::fmt::Debug for Ready<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ready").field("peer", &self.opened.peer).finish_non_exhaustive()
    }
}

impl<S: Read + Write> Ready<S> {
    /// The other device.
    pub fn peer(&self) -> &Peer {
        &self.opened.peer
    }

    /// Runs the sync itself: the vault-key proof, the deltas, the attachments.
    pub fn sync<B: BlobStore>(self, vault: &mut Vault, blobs: &mut B) -> std::result::Result<LanReport, Failure> {
        let key = self.opened.peer.key;
        exchange(self.opened, vault, blobs, self.role).map_err(|error| Failure { error, key: Some(key) })
    }
}

/// Connects to a waiting device to sync. `trusted` can be narrowed to the devices wanted.
pub fn connect<S: Read + Write>(stream: S, identity: &Identity, trusted: &[Peer]) -> std::result::Result<Ready<S>, Failure> {
    Ok(Ready { opened: open(stream, identity, trusted, None)?, role: Role::Initiator })
}

/// What a waiting device was asked.
#[derive(Debug)]
pub enum Incoming<S> {
    /// To sync. Call [`Ready::sync`].
    Sync(Ready<S>),
    /// To merge two different vaults. Show the person and call [`PendingMerge::finish`].
    Merge(PendingMerge<S>),
}

/// The waiting end, up to what the other device wants. Needs no vault.
pub fn answer<S: Read + Write>(stream: S, identity: &Identity, trusted: &[Peer]) -> std::result::Result<Incoming<S>, Failure> {
    let mut opened = handshake(stream, identity, trusted, Role::Responder)?;
    let key = opened.peer.key;
    let at = |error| Failure { error, key: Some(key) };
    // A device that hangs up before saying anything was looking for some other device.
    let intent: Intent = opened.channel.recv_value().map_err(|error| match error {
        LanError::Decode => at(LanError::Version),
        other => Failure::from(other),
    })?;
    if intent.version != VERSION {
        let _ = opened.channel.send_value(&Answer::Version);
        return Err(at(LanError::Version));
    }
    if let Some(they_keep) = intent.merge {
        return Ok(Incoming::Merge(PendingMerge { opened, they_keep }));
    }
    opened.channel.send_value(&Answer::Go).map_err(at)?;
    Ok(Incoming::Sync(Ready { opened, role: Role::Responder }))
}

/// What came of a connection to a waiting device.
#[derive(Debug)]
pub enum Served<S> {
    /// The two synced.
    Synced(LanReport),
    /// The other device asks to merge two different vaults. Show the person and call
    /// [`PendingMerge::finish`].
    MergeAsked(PendingMerge<S>),
}

/// Runs one sync over a connection, as either end. `trusted` is the list of paired devices;
/// the connecting end can narrow it to the devices it is looking for. A waiting end that is
/// asked to merge vaults declines; use [`serve`] to be able to say yes.
pub fn sync<S: Read + Write, B: BlobStore>(
    stream: S,
    identity: &Identity,
    trusted: &[Peer],
    vault: &mut Vault,
    blobs: &mut B,
    role: Role,
) -> std::result::Result<LanReport, Failure> {
    match role {
        Role::Initiator => connect(stream, identity, trusted)?.sync(vault, blobs),
        Role::Responder => match serve(stream, identity, trusted, vault, blobs)? {
            Served::Synced(report) => Ok(report),
            Served::MergeAsked(pending) => {
                let key = pending.peer().key;
                pending.decline();
                Err(Failure { error: LanError::Declined, key: Some(key) })
            }
        },
    }
}

/// The waiting end of a connection: syncs, or hands back a request to merge vaults.
pub fn serve<S: Read + Write, B: BlobStore>(
    stream: S,
    identity: &Identity,
    trusted: &[Peer],
    vault: &mut Vault,
    blobs: &mut B,
) -> std::result::Result<Served<S>, Failure> {
    match answer(stream, identity, trusted)? {
        Incoming::Sync(ready) => ready.sync(vault, blobs).map(Served::Synced),
        Incoming::Merge(pending) => Ok(Served::MergeAsked(pending)),
    }
}

/// A request from a paired device to merge two different vaults, waiting for a yes.
pub struct PendingMerge<S> {
    opened: Opened<S>,
    they_keep: bool,
}

impl<S> std::fmt::Debug for PendingMerge<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingMerge").field("peer", &self.opened.peer).field("they_keep", &self.they_keep).finish_non_exhaustive()
    }
}

impl<S: Read + Write> PendingMerge<S> {
    /// The device asking.
    pub fn peer(&self) -> &Peer {
        &self.opened.peer
    }

    /// Whether the asking device keeps its vault, so this one gives its items and then
    /// takes that vault. False: this one keeps its vault and takes the other's items.
    pub fn they_keep(&self) -> bool {
        self.they_keep
    }

    /// Says no.
    pub fn decline(mut self) {
        let _ = self.opened.channel.send_value(&Answer::Declined);
    }

    /// Answers, and on a yes runs the merge. `own` holds this vault's attachments; `new_home`
    /// is where the kept vault's attachments go, if this device is the one that changes vault.
    pub fn finish<B: BlobStore, N: BlobStore>(
        mut self,
        accepted: bool,
        vault: &mut Vault,
        own: &mut B,
        new_home: &mut N,
        now_ms: u64,
    ) -> std::result::Result<crate::merge::Merged, Failure> {
        let key = self.opened.peer.key;
        if !accepted {
            self.decline();
            return Err(Failure { error: LanError::Declined, key: Some(key) });
        }
        self.opened.channel.send_value(&Answer::Go).map_err(|error| Failure { error, key: Some(key) })?;
        crate::merge::run(self.opened, !self.they_keep, vault, own, new_home, now_ms)
    }
}

/// Everything after the opening: the proof, the deltas, the attachments.
fn exchange<S: Read + Write, B: BlobStore>(opened: Opened<S>, vault: &mut Vault, blobs: &mut B, role: Role) -> Result<LanReport> {
    let Opened { mut channel, peer } = opened;
    let lan_key = vault.purpose_key("wmpass/lan/vault-proof/v1")?;
    channel.prove_shared_key(&lan_key, role)?;

    let mut report = LanReport { peer, ..LanReport::default() };
    match role {
        Role::Initiator => {
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
    let wanted: Vec<String> = referenced.iter().filter(|name| !blobs.has(name)).cloned().collect();
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

pub(crate) fn send_blobs<S: Read + Write, B: BlobStore>(
    channel: &mut Channel<S>,
    referenced: &[String],
    asked: &[String],
    blobs: &B,
    report: &mut LanReport,
) -> Result<()> {
    // Only blobs this vault refers to, so a peer cannot read anything else by naming it.
    // Counted without reading, then read and sent one at a time: a new device asks for every
    // attachment, and holding them all at once could run a phone out of memory.
    let available: Vec<&String> = asked
        .iter()
        .filter(|name| referenced.contains(name))
        .filter(|name| blobs.has(name))
        .collect();
    channel.send_value(&available.len())?;
    for name in available {
        // The count is already sent, so skipping a blob would leave the other end waiting for
        // one that never comes until its socket times out. Stopping now says why at once.
        let bytes = blobs.get(name).ok_or(LanError::Core)?;
        channel.send_value(&Blob { name: name.clone(), bytes })?;
        report.blobs_sent += 1;
    }
    Ok(())
}

pub(crate) fn recv_blobs<S: Read + Write, B: BlobStore>(
    channel: &mut Channel<S>,
    wanted: &[String],
    blobs: &mut B,
    report: &mut LanReport,
) -> Result<()> {
    let count: usize = channel.recv_value()?;
    let wanted: BTreeSet<&String> = wanted.iter().collect();
    // No more than were asked for, so a peer cannot keep this end writing for ever.
    if count > wanted.len() {
        return Err(LanError::Decode);
    }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::net::{TcpListener, TcpStream};

    use super::*;

    fn channels() -> (Channel<TcpStream>, Channel<TcpStream>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        crate::prepare(&client).unwrap();
        crate::prepare(&server).unwrap();
        let other = std::thread::spawn(move || Channel::handshake(server, &Identity::generate().unwrap(), Role::Responder, None).unwrap());
        let mine = Channel::handshake(client, &Identity::generate().unwrap(), Role::Initiator, None).unwrap();
        (mine, other.join().unwrap())
    }

    #[derive(Default)]
    struct Written(Vec<String>);

    impl BlobStore for Written {
        fn get(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn put(&mut self, name: &str, _: &[u8]) -> Result<()> {
            self.0.push(name.to_owned());
            Ok(())
        }
    }

    #[test]
    fn more_blobs_than_were_asked_for_are_refused_before_any_is_written() {
        let (mut peer, mut mine) = channels();
        peer.send_value(&2usize).unwrap();
        for _ in 0..2 {
            peer.send_value(&Blob { name: "a".into(), bytes: vec![1] }).unwrap();
        }
        let mut store = Written::default();
        let mut report = LanReport::default();
        assert_eq!(recv_blobs(&mut mine, &["a".to_owned()], &mut store, &mut report), Err(LanError::Decode));
        assert!(store.0.is_empty());
        assert_eq!(report.blobs_received, 0);
    }
}
