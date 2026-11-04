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
use wmpass_lan::sync::{self, BlobStore};
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
    code: Option<PendingCode<TcpStream>>,
    /// A new device waiting for its person to compare digits.
    joining: Option<PendingJoin<TcpStream>>,
    offers: HashMap<String, ShareReceiver>,
    cancelled: bool,
}

static HELD: Mutex<Option<Held>> = Mutex::new(None);

fn held<T>(run: impl FnOnce(&mut Held) -> T) -> Result<T, SessionError> {
    let mut guard = HELD.lock().map_err(|_| failed("busy", "try that again"))?;
    Ok(run(guard.get_or_insert_with(Held::default)))
}

fn lan_error(error: LanError) -> SessionError {
    failed("lan", error.to_string())
}

/// Attachment blobs in a folder. They are already sealed under their own keys.
struct FolderBlobs(std::path::PathBuf);

impl BlobStore for FolderBlobs {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        if name.contains(['/', '\\']) {
            return None;
        }
        std::fs::read(self.0.join(name)).ok()
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), LanError> {
        // The name came from the other device.
        if name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(LanError::Decode);
        }
        std::fs::write(self.0.join(name), bytes).map_err(LanError::from)
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
}

/// Waits for one connection, for up to two minutes, unless cancelled.
fn accept() -> Result<TcpStream, SessionError> {
    let listener = held(|h| {
        h.cancelled = false;
        h.listener.as_ref().map(TcpListener::try_clone)
    })?
    .ok_or_else(|| failed("invalid", "not listening"))?
    .map_err(|e| failed("lan", e.to_string()))?;
    listener.set_nonblocking(true).map_err(|e| failed("lan", e.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).map_err(|e| failed("lan", e.to_string()))?;
                wmpass_lan::prepare(&stream).map_err(lan_error)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline || held(|h| h.cancelled)? {
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
        let mut guard = service.lock().map_err(|_| failed("busy", "try that again"))?;
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
            let stream = accept()?;
            let qr = held(|h| h.qr.clone())?.ok_or_else(|| failed("invalid", "no code on show"))?;
            let peer = qr.pair(stream, &me, &a.name, Role::Responder, now_ms).map_err(lan_error)?;
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
            let pending = begin_code_pairing(stream, &identity(secret)?, &a.name, role).map_err(lan_error)?;
            let reply = json!({"code": pending.code(), "peer": peer_view(pending.peer())});
            held(|h| h.code = Some(pending))?;
            Ok(view(reply))
        }
        "lan.code_finish" => {
            let a: LanArgs = parse(args)?;
            let pending = held(|h| h.code.take())?.ok_or_else(|| failed("invalid", "no pairing under way"))?;
            let peer = pending.finish(a.accepted).map_err(lan_error)?;
            Ok(view(peer_view(&peer)))
        }
        "lan.sync" | "lan.serve" => {
            let a: LanArgs = parse(args)?;
            let me = identity(secret)?;
            let peers = peers_from(&a.peers);
            let (stream, role) = if method == "lan.serve" {
                (accept()?, Role::Responder)
            } else {
                (connect(&a.address)?, Role::Initiator)
            };
            let mut blobs = FolderBlobs(a.blob_dir.clone().into());
            with_service(&mut |service| {
                let stream = stream.try_clone().map_err(|e| failed("lan", e.to_string()))?;
                let report = service
                    .with_vault(handle, |vault| sync::sync(stream, &me, &peers, vault, &mut blobs, role))?
                    .map_err(lan_error)?;
                Ok(view(json!({
                    "peer": report.peer.name,
                    "sent": report.sent,
                    "received": report.received,
                    "blobs_sent": report.blobs_sent,
                    "blobs_received": report.blobs_received,
                    "converged": report.converged,
                })))
            })
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
            let stream = accept()?;
            let qr = held(|h| h.qr.clone())?;
            let mut blobs = FolderBlobs(a.blob_dir.clone().into());
            match join::arrive(stream, &me, &a.name, qr.as_ref(), now_ms, &mut blobs).map_err(lan_error)? {
                Arrival::Done(joined) => Ok(joined_reply(joined)),
                Arrival::Code(pending) => {
                    let reply = json!({"code": pending.code(), "peer": peer_view(pending.peer())});
                    held(|h| h.joining = Some(pending))?;
                    Ok(view(reply))
                }
            }
        }
        "lan.join_finish" => {
            let a: LanArgs = parse(args)?;
            let pending = held(|h| h.joining.take())?.ok_or_else(|| failed("invalid", "no device is waiting"))?;
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
            let pending = join::begin_send_by_code(connect(&a.address)?, &identity(secret)?, &a.name).map_err(lan_error)?;
            let reply = json!({"code": pending.code(), "peer": peer_view(pending.peer())});
            held(|h| h.code = Some(pending))?;
            Ok(view(reply))
        }
        "lan.join_send" => {
            let a: LanArgs = parse(args)?;
            let pending = held(|h| h.code.take())?.ok_or_else(|| failed("invalid", "no device is waiting"))?;
            let (file, names) = if a.accepted { vault_to_send(&with_service, handle)? } else { (Vec::new(), Vec::new()) };
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
