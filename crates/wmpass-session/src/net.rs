//! Everything that waits on a network. These calls run outside the lock on [`Service`], so a slow
//! server leaves the vault usable. They hold no vault state of their own: a shell reveals
//! what a call needs, such as an API key, and passes it in as bytes.

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wmpass_core::crypto;
use wmpass_core::Secret;
use wmpass_lan::discovery::{self, Advert};
use wmpass_lan::pair::{PairingQr, PendingCode, begin_code_pairing};
use wmpass_lan::share::{self, ShareReceiver};
use wmpass_lan::sync::{self, BlobStore};
use wmpass_lan::{Identity, LanError, Peer, Role};
use wmpass_net::aliases::{self, Alias, Provider};
use wmpass_net::breach;
use wmpass_net::http::{Call as NetCall, NativeHttp, NetError, NetPolicy};
use wmpass_net::inbox;
use wmpass_net::usernames::{self, Availability};
use zeroize::Zeroizing;

use crate::service::{Reply, Service, SessionError};

/// What outlives a single call: open ports and half-finished pairings.
#[derive(Default)]
struct Held {
    listener: Option<TcpListener>,
    advert: Option<Advert>,
    qr: Option<PairingQr>,
    code: Option<PendingCode<TcpStream>>,
    offers: HashMap<String, ShareReceiver>,
    cancelled: bool,
}

static HELD: Mutex<Option<Held>> = Mutex::new(None);

fn held<T>(run: impl FnOnce(&mut Held) -> T) -> Result<T, SessionError> {
    let mut guard = HELD.lock().map_err(|_| failed("busy", "try that again"))?;
    Ok(run(guard.get_or_insert_with(Held::default)))
}

fn failed(code: &'static str, message: impl Into<String>) -> SessionError {
    SessionError { code, message: message.into() }
}

fn net_error(error: NetError) -> SessionError {
    failed("network", error.to_string())
}

fn lan_error(error: LanError) -> SessionError {
    failed("lan", error.to_string())
}

fn parse<T: DeserializeOwned>(args: &str) -> Result<T, SessionError> {
    serde_json::from_str(if args.is_empty() { "{}" } else { args })
        .map_err(|e| failed("invalid", format!("bad arguments: {e}")))
}

fn secret_text(secret: &[u8]) -> Result<Secret, SessionError> {
    Ok(Secret::new(std::str::from_utf8(secret).map_err(|_| failed("invalid", "not text"))?))
}

fn run<T>(call: NetCall<T>, online: bool) -> Result<T, SessionError> {
    let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
    call.run(&NativeHttp::default(), policy).map_err(net_error)
}

fn view(value: Value) -> Reply {
    Reply { json: value.to_string(), ..Reply::default() }
}

fn alias_view(alias: &Alias) -> Value {
    json!({"id": alias.id, "address": alias.address, "enabled": alias.enabled, "note": alias.note})
}

fn provider(args: &AliasArgs) -> Result<Provider, SessionError> {
    Ok(match args.provider.as_str() {
        "simplelogin" => match &args.base {
            Some(base) => Provider::SimpleLogin { base: base.clone() },
            None => Provider::simplelogin(),
        },
        "addy" => match (&args.base, &args.domain) {
            (None, None) => Provider::addy(),
            _ => Provider::Addy {
                base: args.base.clone().unwrap_or_else(|| "https://app.addy.io".to_owned()),
                domain: args.domain.clone().unwrap_or_else(|| "anonaddy.me".to_owned()),
            },
        },
        "firefox-relay" => Provider::FirefoxRelay,
        "duckduckgo" => Provider::DuckDuckGo,
        _ => return Err(failed("invalid", "unknown alias service")),
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AliasArgs {
    provider: String,
    base: Option<String>,
    domain: Option<String>,
    note: String,
    id: String,
    enabled: bool,
    address: String,
    tag: String,
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

fn identity(secret: &[u8]) -> Result<Identity, SessionError> {
    let raw: [u8; 32] = secret.try_into().map_err(|_| failed("invalid", "bad device key"))?;
    Ok(Identity::from_secret(raw))
}

fn peer_view(peer: &Peer) -> Value {
    json!({"key": hex(&peer.key), "name": peer.name})
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

/// One call. `service` is locked only for the moments vault state is needed.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn call(
    service: &Mutex<Option<Service>>,
    method: &str,
    handle: u64,
    args: &str,
    secret: &[u8],
    online: bool,
    now_ms: u64,
) -> Result<Reply, SessionError> {
    let with_service = |run: &mut dyn FnMut(&mut Service) -> Result<Reply, SessionError>| -> Result<Reply, SessionError> {
        let mut guard = service.lock().map_err(|_| failed("busy", "try that again"))?;
        let service = guard.as_mut().ok_or_else(|| failed("not_started", "the vault service has not started"))?;
        run(service)
    };
    match method {
        // ---- breaches ----
        "breach.password" => {
            let count = run(breach::password_count(&secret_text(secret)?), online)?;
            Ok(view(json!({"count": count})))
        }
        "breach.account" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                source: String,
                email: String,
            }
            let a: A = parse(args)?;
            let found = match a.source.as_str() {
                "hibp" => run(breach::hibp_account(&a.email, &secret_text(secret)?), online)?,
                "leakcheck" => run(breach::leakcheck_public(&a.email), online)?,
                "leakcheck_pro" => run(breach::leakcheck_pro(&a.email, &secret_text(secret)?), online)?,
                _ => return Err(failed("invalid", "unknown source")),
            };
            let rows: Vec<Value> = found.iter().map(|b| json!({"name": b.name, "date": b.date, "data": b.data})).collect();
            Ok(view(json!(rows)))
        }
        "breach.comb" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                email: String,
            }
            let a: A = parse(args)?;
            // Only an address and passwords from the open vault, whatever a shell passes in.
            let mut prepared = None;
            with_service(&mut |service| {
                prepared = Some(service.comb_inputs(handle, &a.email)?);
                Ok(Reply::default())
            })?;
            let (email, candidates, ids) = prepared.ok_or_else(|| failed("invalid", "nothing to check"))?;
            let result = run(breach::proxynova_check(&email, candidates), online)?;
            let matched: Vec<&String> = result.matched.iter().filter_map(|i| ids.get(*i)).collect();
            Ok(view(json!({"lines": result.lines, "matched": matched})))
        }
        // ---- aliases ----
        "alias.create" => {
            let a: AliasArgs = parse(args)?;
            let made = run(provider(&a)?.create(&secret_text(secret)?, &a.note), online)?;
            Ok(view(alias_view(&made)))
        }
        "alias.list" => {
            let a: AliasArgs = parse(args)?;
            let list = run(provider(&a)?.list(&secret_text(secret)?).map_err(net_error)?, online)?;
            Ok(view(json!(list.iter().map(alias_view).collect::<Vec<_>>())))
        }
        "alias.set_enabled" => {
            let a: AliasArgs = parse(args)?;
            run(provider(&a)?.set_enabled(&secret_text(secret)?, &a.id, a.enabled).map_err(net_error)?, online)?;
            Ok(view(json!({})))
        }
        "alias.delete" => {
            let a: AliasArgs = parse(args)?;
            run(provider(&a)?.delete(&secret_text(secret)?, &a.id).map_err(net_error)?, online)?;
            Ok(view(json!({})))
        }
        "alias.plus" => {
            let a: AliasArgs = parse(args)?;
            Ok(view(json!({"address": aliases::plus_address(&a.address, &a.tag).map_err(net_error)?})))
        }
        "alias.dots" => {
            let a: AliasArgs = parse(args)?;
            Ok(view(json!({"address": aliases::gmail_dots(&a.address).map_err(net_error)?})))
        }
        // ---- usernames ----
        "usernames.sites" => {
            let names: Vec<&str> = usernames::SITES.iter().map(|s| s.name).collect();
            Ok(view(json!(names)))
        }
        "usernames.check" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                site: String,
                name: String,
            }
            let a: A = parse(args)?;
            let site = usernames::SITES.iter().find(|s| s.name == a.site).ok_or_else(|| failed("invalid", "unknown site"))?;
            let answer = run(usernames::check(*site, &a.name).map_err(net_error)?, online)?;
            Ok(view(json!({"answer": match answer {
                Availability::Available => "free",
                Availability::Taken => "taken",
                Availability::Unknown => "unknown",
            }})))
        }
        // ---- disposable inboxes ----
        "inbox.new" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                service: String,
            }
            let a: A = parse(args)?;
            match a.service.as_str() {
                "mailtm" => {
                    let domains = run(inbox::mailtm_domains(), online)?;
                    let domain = domains.first().ok_or_else(|| failed("network", "the service offered no addresses"))?;
                    let local: [u8; 8] = crypto::random_array()?;
                    let address = format!("{}@{domain}", hex(&local));
                    let password: [u8; 18] = crypto::random_array()?;
                    let password = Secret::from(hex(&password));
                    let account = run(inbox::mailtm_create(&address, &password), online)?;
                    let token = run(inbox::mailtm_token(&address, &password), online)?;
                    Ok(Reply {
                        json: json!({"address": address, "service": "mailtm", "account": account}).to_string(),
                        secret: Some(Zeroizing::new(token.expose().as_bytes().to_vec())),
                        blobs: Vec::new(),
                    })
                }
                "guerrilla" => {
                    let session = run(inbox::guerrilla_new(), online)?;
                    Ok(Reply {
                        json: json!({"address": session.address, "service": "guerrilla"}).to_string(),
                        secret: Some(Zeroizing::new(session.token.expose().as_bytes().to_vec())),
                        blobs: Vec::new(),
                    })
                }
                _ => Err(failed("invalid", "unknown inbox service")),
            }
        }
        "inbox.messages" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                service: String,
                address: String,
            }
            let a: A = parse(args)?;
            let token = secret_text(secret)?;
            let list = if a.service == "mailtm" {
                run(inbox::mailtm_messages(&token), online)?
            } else {
                run(inbox::guerrilla_messages(&inbox::GuerrillaSession { address: a.address, token }), online)?
            };
            let rows: Vec<Value> = list.iter().map(|m| json!({"id": m.id, "from": m.from, "subject": m.subject, "preview": m.preview})).collect();
            Ok(view(json!(rows)))
        }
        "inbox.body" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                id: String,
            }
            let a: A = parse(args)?;
            let text = run(inbox::mailtm_body(&secret_text(secret)?, &a.id), online)?;
            Ok(view(json!({"text": text})))
        }
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
        // ---- accounts held elsewhere ----
        "bitwarden.login" | "bitwarden.open_cached" | "bitwarden.register" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                server: Option<String>,
                email: String,
                name: String,
                device: String,
                cache: String,
                code: Option<String>,
                provider: Option<u32>,
                region: Option<String>,
            }
            use wmpass_bw::account::Account;
            use wmpass_bw::api::{Device, Endpoints, SecondFactor};
            let a: A = parse(args)?;
            let endpoints = match (&a.server, a.region.as_deref()) {
                (Some(base), _) if !base.is_empty() => Endpoints::self_hosted(base),
                (_, Some("eu")) => Endpoints::official_eu(),
                _ => Endpoints::official(),
            };
            let http = NativeHttp::default();
            let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
            let bw_error = |e: wmpass_bw::BwError| match e {
                wmpass_bw::BwError::TwoFactor(providers) => failed("two_factor", format!("{providers:?}")),
                other => failed("bitwarden", other.to_string()),
            };
            if method == "bitwarden.register" {
                wmpass_bw::account::register(
                    &http,
                    policy,
                    &endpoints,
                    &a.email,
                    &a.name,
                    secret,
                    wmpass_bw::crypto::Kdf::Argon2id { iterations: 3, memory_mib: 64, parallelism: 4 },
                )
                .map_err(bw_error)?;
                return Ok(view(json!({})));
            }
            let account = if method == "bitwarden.login" {
                let device = Device { identifier: a.device.clone(), name: "WM Pass".to_owned(), kind: 0 };
                let factor = a.code.map(|code| SecondFactor { provider: a.provider.unwrap_or(0), code });
                Account::login(&http, policy, endpoints, &a.email, secret, &device, factor.as_ref()).map_err(bw_error)?
            } else {
                let cached = crate::service::base64_decode(&a.cache)?;
                Account::unlock_offline(endpoints, &cached, secret).map_err(bw_error)?
            };
            let name = account.email().to_owned();
            let unreadable = account.unreadable();
            let mut slot = Some(Box::new(account));
            with_service(&mut |service| {
                let account = slot.take().ok_or_else(|| failed("busy", "try that again"))?;
                service.adopt_bitwarden(&name, account, unreadable, now_ms)
            })
        }
        "remote.sync" => {
            let http = NativeHttp::default();
            let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
            let mut taken = None;
            with_service(&mut |service| {
                taken = Some(service.take_bitwarden(handle, now_ms)?);
                Ok(Reply::default())
            })?;
            let mut account = taken.ok_or_else(|| failed("invalid", "nothing to sync"))?;
            let pushed = account.push(&http, policy).and_then(|()| account.pull(&http, policy));
            let mut account_slot = Some(account);
            with_service(&mut |service| {
                if let Some(account) = account_slot.take() {
                    service.return_bitwarden(handle, account, now_ms)?;
                }
                Ok(Reply::default())
            })?;
            pushed.map_err(|e| failed("bitwarden", e.to_string()))?;
            Ok(view(json!({})))
        }
        _ => Err(failed("invalid", format!("no such call: {method}"))),
    }
}
