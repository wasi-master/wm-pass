//! Making two different vaults one, between paired devices.
//!
//! A sync only joins copies of the same vault, and each end proves it holds that vault's key
//! first. Two vaults made apart have different keys, so they can never sync. A merge settles
//! that once: the person picks which vault is kept, and the other device gives its items to
//! it, then takes a copy of the kept vault in place of its own. From then on the two hold the
//! same vault and sync as usual.
//!
//! Being paired is not enough to start one. The person says yes on both devices: by asking on
//! one, and by accepting on the other, which is shown who asks and which vault stays. The
//! giving device sends its items, attachments included, inside the encrypted session; the
//! keeping device sends back its sealed file, which opens only with the kept vault's master
//! password. Should the line drop part way, the kept vault has the items and the other device
//! still has its own vault, so nothing is lost.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use wmpass_core::absorb::{AbsorbReport, Gift, absorb};
use wmpass_core::vault::Vault;

use crate::sync::{BlobStore, Failure, LanReport, Opened, open, recv_blobs, send_blobs};
use crate::{Identity, LanError, Peer, Result};

/// What a merge did on this device.
#[derive(Debug)]
pub enum Merged {
    /// This device kept its vault and took the other's items. Save the vault.
    Kept {
        /// The device that gave its items.
        peer: Peer,
        /// Items added and items already here.
        report: AbsorbReport,
        /// Attachments received.
        blobs: usize,
    },
    /// This device gave its items. `file` is the kept vault, sealed: put it in place of this
    /// device's vault, and open it with that vault's master password.
    Replaced {
        /// The device whose vault is kept.
        peer: Peer,
        /// The kept vault's sealed file.
        file: Vec<u8>,
        /// Items given.
        sent: usize,
        /// Attachments received into the new home.
        blobs: usize,
    },
}

/// The kept vault, sealed, and the attachments it refers to.
#[derive(Serialize, Deserialize)]
struct Offer {
    file: Vec<u8>,
    blobs: Vec<String>,
}

/// A merge both people have agreed to, ready for the vault.
pub struct Agreed<S> {
    opened: Opened<S>,
    keep: bool,
}

impl<S> std::fmt::Debug for Agreed<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agreed").field("peer", &self.opened.peer).field("keep", &self.keep).finish_non_exhaustive()
    }
}

impl<S: Read + Write> Agreed<S> {
    /// The other device.
    pub fn peer(&self) -> &Peer {
        &self.opened.peer
    }

    /// Runs the merge. `own` holds this vault's attachments; `new_home` is where the kept
    /// vault's go if this device is the one that changes vault.
    pub fn run<B: BlobStore, N: BlobStore>(self, vault: &mut Vault, own: &mut B, new_home: &mut N, now_ms: u64) -> std::result::Result<Merged, Failure> {
        run(self.opened, self.keep, vault, own, new_home, now_ms)
    }
}

/// Asks a waiting paired device to merge, and waits for its person to say yes. `keep_mine`
/// says which vault is kept: this one, or the other device's. Needs no vault.
pub fn request<S: Read + Write>(stream: S, identity: &Identity, trusted: &[Peer], keep_mine: bool) -> std::result::Result<Agreed<S>, Failure> {
    Ok(Agreed { opened: open(stream, identity, trusted, Some(keep_mine))?, keep: keep_mine })
}

/// [`request`] and [`Agreed::run`] together.
#[allow(clippy::too_many_arguments)]
pub fn ask<S: Read + Write, B: BlobStore, N: BlobStore>(
    stream: S,
    identity: &Identity,
    trusted: &[Peer],
    keep_mine: bool,
    vault: &mut Vault,
    own: &mut B,
    new_home: &mut N,
    now_ms: u64,
) -> std::result::Result<Merged, Failure> {
    request(stream, identity, trusted, keep_mine)?.run(vault, own, new_home, now_ms)
}

/// Both ends, once both people have said yes.
pub(crate) fn run<S: Read + Write, B: BlobStore, N: BlobStore>(
    opened: Opened<S>,
    keep: bool,
    vault: &mut Vault,
    own: &mut B,
    new_home: &mut N,
    now_ms: u64,
) -> std::result::Result<Merged, Failure> {
    let key = opened.peer.key;
    let at = |error| Failure { error, key: Some(key) };
    if keep { take(opened, vault, own, now_ms).map_err(at) } else { give(opened, vault, own, new_home).map_err(at) }
}

fn take<S: Read + Write, B: BlobStore>(opened: Opened<S>, vault: &mut Vault, own: &mut B, now_ms: u64) -> Result<Merged> {
    let Opened { mut channel, peer } = opened;
    let mut counts = LanReport::default();
    let gift: Gift = channel.recv_value()?;
    let wanted: Vec<String> = gift.blobs().into_iter().filter(|name| !own.has(name)).collect();
    channel.send_value(&wanted)?;
    recv_blobs(&mut channel, &wanted, own, &mut counts)?;
    let report = absorb(vault, gift, now_ms)?;
    let offer = Offer { file: vault.save()?, blobs: vault.referenced_blobs() };
    channel.send_value(&offer)?;
    let asked: Vec<String> = channel.recv_value()?;
    send_blobs(&mut channel, &offer.blobs, &asked, own, &mut counts)?;
    // Only once the other end has everything does it let go of its own vault.
    let done: bool = channel.recv_value()?;
    if !done {
        return Err(LanError::Io);
    }
    Ok(Merged::Kept { peer, report, blobs: counts.blobs_received })
}

fn give<S: Read + Write, B: BlobStore, N: BlobStore>(opened: Opened<S>, vault: &Vault, own: &B, new_home: &mut N) -> Result<Merged> {
    let Opened { mut channel, peer } = opened;
    let mut counts = LanReport::default();
    let gift = Gift::of(vault);
    let sent = gift.entries.len();
    let names = gift.blobs();
    channel.send_value(&gift)?;
    drop(gift);
    let asked: Vec<String> = channel.recv_value()?;
    send_blobs(&mut channel, &names, &asked, own, &mut counts)?;
    let offer: Offer = channel.recv_value()?;
    let wanted: Vec<String> = offer.blobs.iter().filter(|name| !new_home.has(name)).cloned().collect();
    channel.send_value(&wanted)?;
    recv_blobs(&mut channel, &wanted, new_home, &mut counts)?;
    channel.send_value(&true)?;
    Ok(Merged::Replaced { peer, file: offer.file, sent, blobs: counts.blobs_received })
}
