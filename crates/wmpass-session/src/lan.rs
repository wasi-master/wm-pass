//! The local network: pairing, sync and joining between devices, and the one-time QR handover.
//! Raw sockets, so native only.

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use wmpass_core::crypto;
use wmpass_lan::discovery::{self, Advert};
use wmpass_lan::join::{self, Arrival, Joined, PendingJoin};
use wmpass_lan::pair::{PairingQr, PendingCode, begin_code_pairing};
use wmpass_lan::share::{self, ShareReceiver};
use wmpass_lan::merge::{self, Agreed, Merged};
use wmpass_lan::sync::{self, BlobStore, Failure, Incoming, PendingMerge};
use wmpass_lan::{Identity, LanError, Peer, Role};
use zeroize::Zeroizing;

use crate::net::{failed, hex, parse, view};
use crate::service::{Reply, Service, SessionError};

/// What outlives a single call: open ports and half-finished pairings.
#[derive(Default)]
struct Held {
    listener: Option<TcpListener>,
    advert: Option<Advert>,
    qr: Option<PairingQr>,
    /// A pairing or handover by digits, with a second handle on its connection for the wait.
    code: Option<(PendingCode<TcpStream>, TcpStream)>,
    /// A new device waiting for its person to compare digits.
    joining: Option<(PendingJoin<TcpStream>, TcpStream)>,
    offers: HashMap<String, ShareReceiver>,
    cancelled: bool,
    /// Waiting for paired devices for as long as the vault is open, apart from the screens
    /// above, so leaving a screen does not stop it.
    background: Option<(TcpListener, Advert)>,
    background_cancelled: bool,
    /// A paired device asking to merge vaults, waiting for the person's answer.
    merge: Option<PendingMerge<TcpStream>>,
}

static HELD: Mutex<Option<Held>> = Mutex::new(None);

fn held<T>(run: impl FnOnce(&mut Held) -> T) -> Result<T, SessionError> {
    let mut guard = HELD.lock().map_err(|_| failed("busy", "try that again"))?;
    Ok(run(guard.get_or_insert_with(Held::default)))
}

fn lan_error(error: LanError) -> SessionError {
    failed("lan", error.to_string())
}

/// How long a device waits on the other person's answer: comparing digits or agreeing to a
/// merge takes longer than the 30 seconds a connection otherwise allows for a reply.
const DECISION_WAIT: Duration = Duration::from_secs(180);

/// A second handle on a connection, kept so the wait can be lengthened once the other end is
/// a person deciding rather than a device answering.
fn second_handle(stream: &TcpStream) -> Result<TcpStream, SessionError> {
    stream.try_clone().map_err(|e| failed("lan", e.to_string()))
}

/// Gives the other person [`DECISION_WAIT`] to answer. A clone shares its socket, so this
/// reaches the connection a pending pairing holds.
fn wait_for_person(stream: &TcpStream) -> Result<(), SessionError> {
    stream.set_read_timeout(Some(DECISION_WAIT)).map_err(|e| failed("lan", e.to_string()))
}

/// The time once a wait is over: the caller's clock moved on by what passed since `started`,
/// so a QR code's expiry is not judged by the moment before a two-minute wait.
fn now_after(now_ms: u64, started: Instant) -> u64 {
    now_ms.saturating_add(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
}

/// A sync or merge that stopped, as a reply rather than an error, so the screen learns which
/// device it was: `key` is the other device's, once the handshake showed it.
fn failure_reply(failure: Failure) -> Reply {
    view(json!({"failed": {
        "code": failure.error.code(),
        "message": failure.error.to_string(),
        "key": failure.key.map(|key| hex(&key)),
    }}))
}

fn merged_reply(merged: Merged) -> Reply {
    match merged {
        Merged::Kept { peer, report, blobs } => view(json!({
            "merged": "kept",
            "peer": peer_view(&peer),
            "added": report.added,
            "matched": report.matched,
            "blobs": blobs,
        })),
        Merged::Replaced { peer, file, sent, blobs } => Reply {
            json: json!({"merged": "replaced", "peer": peer_view(&peer), "sent": sent, "blobs": blobs}).to_string(),
            secret: None,
            blobs: vec![file],
        },
    }
}

/// A merge about to touch the vault: asked from here, or accepted here.
enum MergeStart {
    Agreed(Agreed<TcpStream>),
    Pending(PendingMerge<TcpStream>),
}

/// Runs a merge both people agreed to, with the vault held only for the merge itself.
fn run_merge(with_service: &WithService<'_>, handle: u64, start: MergeStart, a: &LanArgs, now_ms: u64) -> Result<Reply, SessionError> {
    let mut own = FolderBlobs(a.blob_dir.clone().into());
    let mut new_home = FolderBlobs(a.new_blob_dir.clone().into());
    let mut start = Some(start);
    with_service(&mut |service| {
        let start = start.take().ok_or_else(|| failed("busy", "try that again"))?;
        let outcome = service.with_vault(handle, |vault| match start {
            MergeStart::Agreed(agreed) => agreed.run(vault, &mut own, &mut new_home, now_ms),
            MergeStart::Pending(pending) => pending.finish(true, vault, &mut own, &mut new_home, now_ms),
        })?;
        if matches!(outcome, Ok(Merged::Kept { .. })) {
            service.touch(handle)?;
        }
        Ok(outcome.map_or_else(failure_reply, merged_reply))
    })
}

fn sync_reply(report: &sync::LanReport) -> Reply {
    view(json!({
        "peer": report.peer.name,
        "key": hex(&report.peer.key),
        "sent": report.sent,
        "received": report.received,
        "blobs_sent": report.blobs_sent,
        "blobs_received": report.blobs_received,
        "converged": report.converged,
    }))
}

/// Attachment blobs in a folder. They are already sealed under their own keys.
struct FolderBlobs(std::path::PathBuf);

/// A blob name that stays inside the folder: no separators, and nothing starting with a dot,
/// so neither `..` nor a hidden file.
fn plain_blob_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.')
}

impl BlobStore for FolderBlobs {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        if !plain_blob_name(name) {
            return None;
        }
        std::fs::read(self.0.join(name)).ok()
    }

    fn has(&self, name: &str) -> bool {
        plain_blob_name(name) && self.0.join(name).is_file()
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), LanError> {
        // The name came from the other device.
        if !plain_blob_name(name) {
            return Err(LanError::Decode);
        }
        // Written aside and moved into place: a blob cut short by a crash or a full disk would
        // otherwise count as present, never be asked for again, and never open.
        let part = self.0.join(format!(".{name}.part"));
        let written = std::fs::write(&part, bytes).and_then(|()| std::fs::rename(&part, self.0.join(name)));
        if written.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        written.map_err(LanError::from)
    }
}

/// What a new device came away with: the device to trust as JSON, the sealed file as a blob.
fn joined_reply(joined: Joined) -> Reply {
    Reply {
        json: json!({"done": true, "peer": peer_view(&joined.peer), "blobs": joined.blobs}).to_string(),
        secret: None,
        blobs: vec![joined.file],
    }
}

type WithService<'a> = dyn Fn(&mut dyn FnMut(&mut Service) -> Result<Reply, SessionError>) -> Result<Reply, SessionError> + 'a;

/// The open vault, sealed as it would be saved, and the attachments it refers to.
fn vault_to_send(with_service: &WithService<'_>, handle: u64) -> Result<(Vec<u8>, Vec<String>), SessionError> {
    let mut out = None;
    with_service(&mut |service| {
        out = Some(service.with_vault(handle, |vault| Ok::<_, wmpass_core::Error>((vault.save()?, vault.referenced_blobs())))??);
        Ok(Reply::default())
    })?;
    out.ok_or_else(|| failed("invalid", "no vault is open"))
}

fn identity(secret: &[u8]) -> Result<Identity, SessionError> {
    let raw: [u8; 32] = secret.try_into().map_err(|_| failed("invalid", "bad device key"))?;
    Ok(Identity::from_secret(raw))
}

fn peer_view(peer: &Peer) -> Value {
    json!({"key": hex(&peer.key), "name": peer.name})
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn peers_from(list: &[PeerArg]) -> Vec<Peer> {
    list.iter()
        .filter_map(|p| {
            let key: [u8; 32] = unhex(&p.key)?.try_into().ok()?;
            Some(Peer { key, name: p.name.clone() })
        })
        .collect()
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct PeerArg {
    key: String,
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LanArgs {
    address: String,
    name: String,
    text: String,
    accepted: bool,
    peers: Vec<PeerArg>,
    blob_dir: String,
    port: u16,
    ms: u64,
    token: String,
    id: String,
    /// For `lan.serve`: wait on the background listener, with no time limit.
    background: bool,
    /// For `lan.merge`: whether this device's vault is the one kept.
    keep_mine: bool,
    /// For a merge: where the kept vault's attachments go, if this device takes it.
    new_blob_dir: String,
}

/// Waits for one connection, for up to two minutes, unless cancelled.
fn accept() -> Result<TcpStream, SessionError> {
    accept_on(false)
}

/// Waits on the screen's listener for two minutes, or on the background one for as long as it
/// runs.
fn accept_on(background: bool) -> Result<TcpStream, SessionError> {
    let listener = held(|h| {
        if background {
            h.background_cancelled = false;
            h.background.as_ref().map(|(l, _)| l.try_clone())
        } else {
            h.cancelled = false;
            h.listener.as_ref().map(TcpListener::try_clone)
        }
    })?
    .ok_or_else(|| failed("invalid", "not listening"))?
    .map_err(|e| failed("lan", e.to_string()))?;
    listener.set_nonblocking(true).map_err(|e| failed("lan", e.to_string()))?;
    let deadline = (!background).then(|| Instant::now() + Duration::from_secs(120));
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).map_err(|e| failed("lan", e.to_string()))?;
                wmpass_lan::prepare(&stream).map_err(lan_error)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let stopped = held(|h| if background { h.background_cancelled || h.background.is_none() } else { h.cancelled })?;
                if deadline.is_some_and(|d| Instant::now() > d) || stopped {
                    return Err(failed("cancelled", "stopped waiting for the other device"));
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(error) => return Err(failed("lan", error.to_string())),
        }
    }
}

fn connect(address: &str) -> Result<TcpStream, SessionError> {
    let target = address.parse().map_err(|_| failed("invalid", "that is not a device's address"))?;
    let stream = TcpStream::connect_timeout(&target, Duration::from_secs(10))
        .map_err(|_| failed("lan", "could not reach that device"))?;
    wmpass_lan::prepare(&stream).map_err(lan_error)?;
    Ok(stream)
}

/// Whether a call needs the local network.
pub fn is_lan_method(method: &str) -> bool {
    method.starts_with("lan.") || method.starts_with("share.")
}

/// One call on the local network.
#[allow(clippy::too_many_lines)]
pub fn call(service: &Mutex<Option<Service>>, method: &str, handle: u64, args: &str, secret: &[u8], now_ms: u64) -> Result<Reply, SessionError> {
    let with_service = |run: &mut dyn FnMut(&mut Service) -> Result<Reply, SessionError>| -> Result<Reply, SessionError> {
        let mut guard = crate::lock_service(service)?;
        let service = guard.as_mut().ok_or_else(|| failed("not_started", "the vault service has not started"))?;
        run(service)
    };
    match method {
        // ---- the local network ----
        "lan.new_identity" => {
            let fresh = Identity::generate().map_err(lan_error)?;
            Ok(Reply {
                json: json!({"public": hex(fresh.public())}).to_string(),
                secret: Some(Zeroizing::new(fresh.secret_bytes().to_vec())),
                blobs: Vec::new(),
            })
        }
        "lan.public" => Ok(view(json!({"public": hex(identity(secret)?.public())}))),
        "lan.listen" => {
            let listener = TcpListener::bind(("0.0.0.0", 0)).map_err(|e| failed("lan", e.to_string()))?;
            let port = listener.local_addr().map_err(|e| failed("lan", e.to_string()))?.port();
            held(|h| h.listener = Some(listener))?;
            Ok(view(json!({"port": port})))
        }
        "lan.advertise" => {
            let a: LanArgs = parse(args)?;
            let advert = discovery::advertise(a.port).map_err(lan_error)?;
            held(|h| h.advert = Some(advert))?;
            Ok(view(json!({})))
        }
        "lan.browse" => {
            let a: LanArgs = parse(args)?;
            let found = discovery::browse(Duration::from_millis(a.ms.clamp(200, 10_000))).map_err(lan_error)?;
            let rows: Vec<String> = found.iter().map(ToString::to_string).collect();
            Ok(view(json!(rows)))
        }
        "lan.stop" => {
            held(|h| {
                h.cancelled = true;
                h.listener = None;
                h.advert = None;
                h.qr = None;
                h.code = None;
                h.joining = None;
            })?;
            Ok(view(json!({})))
        }
        "lan.pair_show" => {
            let a: LanArgs = parse(args)?;
            let qr = PairingQr::new(&a.address, now_ms).map_err(lan_error)?;
            let text = qr.text();
            held(|h| h.qr = Some(qr))?;
            Ok(Reply { json: "{}".to_owned(), secret: Some(Zeroizing::new(text.into_bytes())), blobs: Vec::new() })
        }
        "lan.pair_wait" => {
            let a: LanArgs = parse(args)?;
            let me = identity(secret)?;
            let started = Instant::now();
            let stream = accept()?;
            let qr = held(|h| h.qr.clone())?.ok_or_else(|| failed("invalid", "no code on show"))?;
            let peer = qr.pair(stream, &me, &a.name, Role::Responder, now_after(now_ms, started)).map_err(lan_error)?;
            held(|h| h.qr = None)?;
            Ok(view(peer_view(&peer)))
        }
        "lan.pair_scan" => {
            let a: LanArgs = parse(args)?;
            let qr = PairingQr::parse(&a.text, now_ms).map_err(lan_error)?;
            let stream = connect(&qr.address)?;
            let peer = qr.pair(stream, &identity(secret)?, &a.name, Role::Initiator, now_ms).map_err(lan_error)?;
            Ok(view(json!({"peer": peer_view(&peer), "address": qr.address})))
        }
        "lan.code_wait" | "lan.code_connect" => {
            let a: LanArgs = parse(args)?;
            let (stream, role) = if method == "lan.code_wait" {
                (accept()?, Role::Responder)
            } else {
                (connect(&a.address)?, Role::Initiator)
            };
            let handle = second_handle(&stream)?;
            let pending = begin_code_pairing(stream, &identity(secret)?, &a.name, role).map_err(lan_error)?;
            let reply = json!({"code": pending.code(), "peer": peer_view(pending.peer())});
            held(|h| h.code = Some((pending, handle)))?;
            Ok(view(reply))
        }
        "lan.code_finish" => {
            let a: LanArgs = parse(args)?;
            let (pending, handle) = held(|h| h.code.take())?.ok_or_else(|| failed("invalid", "no pairing under way"))?;
            wait_for_person(&handle)?;
            let peer = pending.finish(a.accepted).map_err(lan_error)?;
            Ok(view(peer_view(&peer)))
        }
        "lan.listen_background" => {
            // Once per open vault; asking again while it runs changes nothing.
            if let Some(port) = held(|h| h.background.as_ref().and_then(|(l, _)| l.local_addr().ok()).map(|a| a.port()))? {
                return Ok(view(json!({"port": port})));
            }
            let listener = TcpListener::bind(("0.0.0.0", 0)).map_err(|e| failed("lan", e.to_string()))?;
            let port = listener.local_addr().map_err(|e| failed("lan", e.to_string()))?.port();
            let advert = discovery::advertise(port).map_err(lan_error)?;
            held(|h| {
                h.background = Some((listener, advert));
                h.background_cancelled = false;
            })?;
            Ok(view(json!({"port": port})))
        }
        "lan.stop_background" => {
            held(|h| {
                h.background = None;
                h.background_cancelled = true;
                h.merge = None;
            })?;
            Ok(view(json!({})))
        }
        // The handshake and the opening run before the vault is locked for use, so a slow or
        // silent device, or this computer's own background listener, holds nothing up.
        "lan.sync" => {
            let a: LanArgs = parse(args)?;
            let me = identity(secret)?;
            let peers = peers_from(&a.peers);
            let ready = match sync::connect(connect(&a.address)?, &me, &peers) {
                Ok(ready) => ready,
                Err(failure) => return Ok(failure_reply(failure)),
            };
            let mut blobs = FolderBlobs(a.blob_dir.clone().into());
            let mut ready = Some(ready);
            with_service(&mut |service| {
                let ready = ready.take().ok_or_else(|| failed("busy", "try that again"))?;
                Ok(match service.with_vault(handle, |vault| ready.sync(vault, &mut blobs))? {
                    Ok(report) => sync_reply(&report),
                    Err(failure) => failure_reply(failure),
                })
            })
        }
        "lan.serve" => {
            let a: LanArgs = parse(args)?;
            let me = identity(secret)?;
            let peers = peers_from(&a.peers);
            let stream = accept_on(a.background)?;
            let timeout = second_handle(&stream)?;
            let ready = match sync::answer(stream, &me, &peers) {
                Ok(Incoming::Sync(ready)) => ready,
                Ok(Incoming::Merge(pending)) => {
                    let reply = view(json!({"merge_asked": {"peer": peer_view(pending.peer()), "they_keep": pending.they_keep()}}));
                    // The person has a few minutes to answer before the asking device gives up.
                    let _ = wait_for_person(&timeout);
                    held(|h| h.merge = Some(pending))?;
                    return Ok(reply);
                }
                Err(failure) => return Ok(failure_reply(failure)),
            };
            let mut blobs = FolderBlobs(a.blob_dir.clone().into());
            if a.background {
                // Two devices that start syncing with each other at the same moment would each
                // hold their own vault while waiting on the other's. The background end gives
                // way after a few seconds instead, and the next try settles it.
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut guard = loop {
                    match service.try_lock() {
                        Ok(guard) => break guard,
                        Err(std::sync::TryLockError::Poisoned(_)) => return Err(failed("busy", "try that again")),
                        Err(std::sync::TryLockError::WouldBlock) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                        Err(std::sync::TryLockError::WouldBlock) => {
                            let key = ready.peer().key;
                            drop(ready);
                            return Ok(view(json!({"failed": {"code": "busy", "message": "busy with another sync", "key": hex(&key)}})));
                        }
                    }
                };
                let service = guard.as_mut().ok_or_else(|| failed("not_started", "the vault service has not started"))?;
                return Ok(match service.with_vault(handle, |vault| ready.sync(vault, &mut blobs))? {
                    Ok(report) => sync_reply(&report),
                    Err(failure) => failure_reply(failure),
                });
            }
            let mut ready = Some(ready);
            with_service(&mut |service| {
                let ready = ready.take().ok_or_else(|| failed("busy", "try that again"))?;
                Ok(match service.with_vault(handle, |vault| ready.sync(vault, &mut blobs))? {
                    Ok(report) => sync_reply(&report),
                    Err(failure) => failure_reply(failure),
                })
            })
        }
        "lan.merge" => {
            let a: LanArgs = parse(args)?;
            let me = identity(secret)?;
            let peers = peers_from(&a.peers);
            let stream = connect(&a.address)?;
            // The other device's person has to say yes first.
            wait_for_person(&stream)?;
            let agreed = match merge::request(stream, &me, &peers, a.keep_mine) {
                Ok(agreed) => agreed,
                Err(failure) => return Ok(failure_reply(failure)),
            };
            run_merge(&with_service, handle, MergeStart::Agreed(agreed), &a, now_ms)
        }
        "lan.merge_answer" => {
            let a: LanArgs = parse(args)?;
            let pending = held(|h| h.merge.take())?.ok_or_else(|| failed("invalid", "no device is asking any more"))?;
            if !a.accepted {
                pending.decline();
                return Ok(view(json!({})));
            }
            run_merge(&with_service, handle, MergeStart::Pending(pending), &a, now_ms)
        }
        // ---- bringing a new device into the vault ----
        // The new device listens and shows a code; the one with the vault connects.
        "lan.join_listen" => {
            let a: LanArgs = parse(args)?;
            let listener = TcpListener::bind(("0.0.0.0", 0)).map_err(|e| failed("lan", e.to_string()))?;
            let port = listener.local_addr().map_err(|e| failed("lan", e.to_string()))?.port();
            let advert = discovery::advertise_joining(port).map_err(lan_error)?;
            let qr = if a.address.is_empty() { None } else { Some(PairingQr::new(&format!("{}:{port}", a.address), now_ms).map_err(lan_error)?) };
            let text = qr.as_ref().map(PairingQr::text).unwrap_or_default();
            held(|h| {
                h.listener = Some(listener);
                h.advert = Some(advert);
                h.qr = qr;
            })?;
            Ok(Reply { json: json!({"port": port}).to_string(), secret: Some(Zeroizing::new(text.into_bytes())), blobs: Vec::new() })
        }
        "lan.join_wait" => {
            let a: LanArgs = parse(args)?;
            let me = identity(secret)?;
            let started = Instant::now();
            let stream = accept()?;
            let handle = second_handle(&stream)?;
            let qr = held(|h| h.qr.clone())?;
            let mut blobs = FolderBlobs(a.blob_dir.clone().into());
            match join::arrive(stream, &me, &a.name, qr.as_ref(), now_after(now_ms, started), &mut blobs).map_err(lan_error)? {
                Arrival::Done(joined) => Ok(joined_reply(joined)),
                Arrival::Code(pending) => {
                    let reply = json!({"code": pending.code(), "peer": peer_view(pending.peer())});
                    held(|h| h.joining = Some((pending, handle)))?;
                    Ok(view(reply))
                }
            }
        }
        "lan.join_finish" => {
            let a: LanArgs = parse(args)?;
            let (pending, handle) = held(|h| h.joining.take())?.ok_or_else(|| failed("invalid", "no device is waiting"))?;
            wait_for_person(&handle)?;
            let mut blobs = FolderBlobs(a.blob_dir.clone().into());
            Ok(joined_reply(pending.finish(a.accepted, &mut blobs).map_err(lan_error)?))
        }
        "lan.join_find" => {
            let a: LanArgs = parse(args)?;
            let found = discovery::browse_joining(Duration::from_millis(a.ms.clamp(200, 10_000))).map_err(lan_error)?;
            Ok(view(json!(found.iter().map(ToString::to_string).collect::<Vec<_>>())))
        }
        "lan.join_scan" => {
            let a: LanArgs = parse(args)?;
            let qr = PairingQr::parse(&a.text, now_ms).map_err(lan_error)?;
            let me = identity(secret)?;
            let (file, names) = vault_to_send(&with_service, handle)?;
            let stream = connect(&qr.address)?;
            let blobs = FolderBlobs(a.blob_dir.clone().into());
            let peer = join::send_by_qr(stream, &qr, &me, &a.name, now_ms, &file, &names, &blobs).map_err(lan_error)?;
            Ok(view(json!({"peer": peer_view(&peer)})))
        }
        "lan.join_connect" => {
            let a: LanArgs = parse(args)?;
            let stream = connect(&a.address)?;
            let handle = second_handle(&stream)?;
            let pending = join::begin_send_by_code(stream, &identity(secret)?, &a.name).map_err(lan_error)?;
            let reply = json!({"code": pending.code(), "peer": peer_view(pending.peer())});
            held(|h| h.code = Some((pending, handle)))?;
            Ok(view(reply))
        }
        "lan.join_send" => {
            let a: LanArgs = parse(args)?;
            let (pending, waiting) = held(|h| h.code.take())?.ok_or_else(|| failed("invalid", "no device is waiting"))?;
            let (file, names) = if a.accepted { vault_to_send(&with_service, handle)? } else { (Vec::new(), Vec::new()) };
            wait_for_person(&waiting)?;
            let blobs = FolderBlobs(a.blob_dir.clone().into());
            let peer = join::finish_send(pending, a.accepted, &file, &names, &blobs).map_err(lan_error)?;
            Ok(view(json!({"peer": peer_view(&peer)})))
        }
        // ---- one-time QR handover ----
        "share.offer" => {
            let receiver = ShareReceiver::new(now_ms).map_err(lan_error)?;
            let text = receiver.offer_text();
            let token: [u8; 8] = crypto::random_array()?;
            let token = hex(&token);
            held(|h| {
                h.offers.clear();
                h.offers.insert(token.clone(), receiver);
            })?;
            Ok(view(json!({"token": token, "text": text})))
        }
        "share.seal" => {
            let a: LanArgs = parse(args)?;
            with_service(&mut |service| {
                let sealed = service.with_vault(handle, |vault| {
                    let entry = vault.get(&a.id).ok_or_else(|| failed("invalid", "that entry is gone"))?;
                    share::seal_to_offer(&a.text, entry, now_ms).map_err(lan_error)
                })??;
                Ok(view(json!({"text": sealed})))
            })
        }
        "share.open" => {
            let a: LanArgs = parse(args)?;
            let receiver = held(|h| h.offers.remove(&a.token))?.ok_or_else(|| failed("invalid", "that offer has gone"))?;
            let entry = receiver.open(&a.text, now_ms).map_err(lan_error)?;
            with_service(&mut |service| {
                let entry = entry.clone();
                let id = service.with_vault(handle, |vault| vault.add(wmpass_core::model::Entry { id: String::new(), ..entry }, now_ms))??;
                service.touch(handle)?;
                Ok(view(json!({"id": id})))
            })
        }
        _ => Err(failed("invalid", format!("no such call: {method}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_for_a_person_reaches_the_connection_a_pairing_holds() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let near = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_far, _) = listener.accept().unwrap();
        wmpass_lan::prepare(&near).unwrap();
        assert_eq!(near.read_timeout().unwrap(), Some(Duration::from_secs(30)));
        // The pairing owns `near`; the session keeps only the second handle.
        let handle = second_handle(&near).unwrap();
        wait_for_person(&handle).unwrap();
        assert_eq!(near.read_timeout().unwrap(), Some(DECISION_WAIT));
    }

    #[test]
    fn expiry_is_judged_by_the_time_after_the_wait() {
        // A machine up for less than ten minutes cannot look that far back.
        let (Some(started), Some(long_ago)) = (Instant::now().checked_sub(Duration::from_secs(100)), Instant::now().checked_sub(Duration::from_secs(600))) else {
            return;
        };
        let later = now_after(1_000_000, started);
        assert!((1_100_000..1_160_000).contains(&later), "{later}");
        // A code shown at the start of a long wait has run out by the time someone connects.
        let qr = PairingQr::new("127.0.0.1:1", 0).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let me = Identity::generate().unwrap();
        let waited = now_after(0, long_ago);
        assert!(matches!(qr.pair(stream, &me, "me", Role::Responder, waited), Err(LanError::Expired)));
    }

    #[test]
    fn blob_names_stay_inside_the_folder_and_presence_needs_no_read() {
        let root = std::env::temp_dir().join(format!("wmpass-lan-blobs-{}", std::process::id()));
        let inner = root.join("blobs");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(root.join("outside"), b"not a blob").unwrap();
        let mut blobs = FolderBlobs(inner.clone());
        blobs.put("abc123", b"sealed").unwrap();
        assert!(blobs.has("abc123"));
        assert_eq!(blobs.get("abc123").as_deref(), Some(&b"sealed"[..]));
        assert!(!blobs.has("missing") && blobs.get("missing").is_none());
        for name in ["", ".", "..", "../outside", "..\\outside", "/etc/passwd", ".hidden"] {
            assert!(!blobs.has(name) && blobs.get(name).is_none(), "{name:?}");
            assert!(blobs.put(name, b"x").is_err(), "{name:?}");
        }
        // A folder of that name is not a blob.
        std::fs::create_dir_all(inner.join("dir")).unwrap();
        assert!(!blobs.has("dir"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_blob_is_there_only_once_it_is_whole() {
        let root = std::env::temp_dir().join(format!("wmpass-lan-whole-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut blobs = FolderBlobs(root.clone());
        // What a crash part way through a write leaves behind is not the blob.
        std::fs::write(root.join(".abc.part"), b"sea").unwrap();
        assert!(!blobs.has("abc"), "a torn write must be asked for again");
        blobs.put("abc", b"sealed").unwrap();
        assert_eq!(blobs.get("abc").as_deref(), Some(&b"sealed"[..]));
        let names: Vec<_> = std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("abc")], "nothing is left aside");
        // A write that cannot finish leaves no blob and nothing aside.
        std::fs::create_dir_all(root.join("taken").join("inside")).unwrap();
        assert!(blobs.put("taken", b"x").is_err());
        assert!(!blobs.has("taken") && !root.join(".taken.part").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
