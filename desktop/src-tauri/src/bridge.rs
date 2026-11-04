//! The app's end of the conversation with the browser extension. Requests arrive sealed through
//! the relay program, and only the few named here are answered. An extension can ask for what
//! matches a site and fill an entry that matches it; it cannot read the vault at large. It can
//! also have this app do what a browser cannot, for the extension's own vault: check a username
//! on a site that only answers a browser, reach a backup place over a raw connection, and ask a
//! security key over USB. Each place and the keys need a person's yes once, here.

use std::collections::HashMap;
use std::io::{self, Read, Write};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wmpass_bridge::{BridgeError, Keys, Server, fingerprint};
use tauri::AppHandle;
use wmpass_core::matching;
use wmpass_net::http::NativeHttp;
use wmpass_net::{NetError, NetPolicy, usernames};
use wmpass_sinks::Stored;
use wmpass_sinks::places::{Done, Op};
use zeroize::Zeroizing;

use crate::engine::{Engine, now_ms};
use crate::places::PlaceFile;
use crate::probe;
use crate::sealed;
use crate::store::Store;

const LARGEST_FRAME: usize = 1024 * 1024;
const MOST_CONNECTIONS: usize = 4;
const STATE_FILE: &str = "settings/bridge.json";
const KEY_NAME: &str = "bridge-key";
/// The page's settings, where its network switch is kept.
const PREFS_FILE: &str = "settings/prefs.json";
/// A menu with more than this is not one anybody would read.
const MOST_MATCHES: usize = 50;
/// File bytes cross in parts, since the browser refuses a message from the relay over 1 MiB.
const PART: usize = 384 * 1024;
/// The most one connection may have in flight, so an extension cannot fill this app's memory.
const MOST_HELD: usize = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Paired {
    id: String,
    name: String,
    key: String,
    /// Sites a person has allowed this extension to fill.
    sites: Vec<String>,
    /// Backup places, and security keys, a person has let this extension use through the app.
    allowed: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Saved {
    paired: Vec<Paired>,
}

/// What the settings page draws.
#[derive(Serialize)]
pub struct Shown {
    running: bool,
    paired: Vec<ShownPaired>,
    pending: Option<String>,
}

#[derive(Serialize)]
struct ShownPaired {
    id: String,
    name: String,
}

pub struct Bridge {
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    pub ask_sites: Arc<AtomicBool>,
    connections: Arc<AtomicUsize>,
    saved: Arc<Mutex<Saved>>,
    store: Store,
    app: AppHandle,
}

impl Bridge {
    pub fn new(store: Store, app: AppHandle) -> Self {
        let saved = store.read(STATE_FILE).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self {
            running: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            ask_sites: Arc::new(AtomicBool::new(true)),
            connections: Arc::new(AtomicUsize::new(0)),
            saved: Arc::new(Mutex::new(saved)),
            store,
            app,
        }
    }

    pub fn shown(&self) -> Shown {
        let saved = self.saved.lock().map(|s| s.paired.clone()).unwrap_or_default();
        Shown {
            running: self.running.load(Ordering::SeqCst),
            paired: saved.into_iter().map(|p| ShownPaired { id: p.id, name: p.name }).collect(),
            pending: None,
        }
    }

    pub fn fingerprint(&self) -> Result<String, String> {
        Ok(fingerprint(&keys(&self.store)?.public))
    }

    pub fn forget(&self, id: &str) -> Result<(), String> {
        let mut saved = self.saved.lock().map_err(|_| "busy")?;
        saved.paired.retain(|p| p.id != id);
        persist(&self.store, &saved)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn start(&self, engine: Engine, socket_dir: &std::path::Path) -> Result<(), String> {
        // A stop that the listening thread has not noticed yet is simply taken back.
        self.stop.store(false, Ordering::SeqCst);
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let started = self.listen(engine, socket_dir);
        if started.is_err() {
            self.running.store(false, Ordering::SeqCst);
        }
        started
    }

    fn listen(&self, engine: Engine, socket_dir: &std::path::Path) -> Result<(), String> {
        let keys = keys(&self.store)?;
        let (name, shown) = socket_name(socket_dir)?;
        let listener = ListenerOptions::new()
            .name(name)
            .nonblocking(ListenerNonblockingMode::Accept)
            .try_overwrite(true)
            .create_sync()
            .map_err(|e| format!("the connection could not start: {e}"))?;
        restrict(&shown);
        self.stop.store(false, Ordering::SeqCst);
        let conversation = Conversation {
            engine,
            secret: Arc::new(keys.secret),
            saved: Arc::clone(&self.saved),
            store: self.store.clone(),
            app: self.app.clone(),
            ask_sites: Arc::clone(&self.ask_sites),
            stop: Arc::clone(&self.stop),
        };
        let (stop, running, connections) = (Arc::clone(&self.stop), Arc::clone(&self.running), Arc::clone(&self.connections));
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok(stream) => {
                        if stream.set_nonblocking(false).is_err() || connections.load(Ordering::SeqCst) >= MOST_CONNECTIONS {
                            continue;
                        }
                        connections.fetch_add(1, Ordering::SeqCst);
                        let conversation = conversation.clone();
                        let connections = Arc::clone(&connections);
                        std::thread::spawn(move || {
                            let _ = conversation.serve(stream);
                            connections.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(120)),
                    Err(_) => break,
                }
            }
            running.store(false, Ordering::SeqCst);
        });
        Ok(())
    }
}

/// The app's key, made the first time it is needed. It lives sealed like other device secrets.
fn keys(store: &Store) -> Result<Keys, String> {
    if let Some(text) = sealed::load(store, KEY_NAME)? {
        let secret: Zeroizing<Vec<u8>> = Zeroizing::new(unhex(&text).ok_or("the connection key is damaged")?);
        let secret: [u8; 32] = secret.as_slice().try_into().map_err(|_| "the connection key is damaged")?;
        let public = Server::public_of(&secret).map_err(|e| e.to_string())?;
        return Ok(Keys { public, secret: Zeroizing::new(secret) });
    }
    let made = Keys::generate().map_err(|e| e.to_string())?;
    sealed::save(store, KEY_NAME, &hex(made.secret.as_ref()))?;
    Ok(made)
}

fn persist(store: &Store, saved: &Saved) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(saved).map_err(|e| e.to_string())?;
    store.write(STATE_FILE, &bytes)
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

#[cfg(unix)]
fn socket_name(dir: &std::path::Path) -> Result<(interprocess::local_socket::Name<'static>, String), String> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let shown = dir.join("bridge.sock").to_string_lossy().into_owned();
    let name = shown.clone().to_fs_name::<GenericFilePath>().map_err(|e| e.to_string())?;
    Ok((name, shown))
}

#[cfg(windows)]
fn socket_name(_dir: &std::path::Path) -> Result<(interprocess::local_socket::Name<'static>, String), String> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    let name = "wmpass-bridge".to_ns_name::<GenericNamespaced>().map_err(|e| e.to_string())?;
    Ok((name, r"\\.\pipe\wmpass-bridge".to_owned()))
}

#[cfg(unix)]
fn restrict(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(windows)]
fn restrict(_path: &str) {}

fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > LARGEST_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok(body)
}

fn write_frame(stream: &mut impl Write, frame: &[u8]) -> io::Result<()> {
    let length = u32::try_from(frame.len()).map_err(|_| io::ErrorKind::InvalidData)?;
    stream.write_all(&length.to_le_bytes())?;
    stream.write_all(frame)?;
    stream.flush()
}

/// Asks a person, in a window a web page cannot draw over or answer.
fn ask(description: String, yes: &str, no: &str) -> bool {
    #[cfg(debug_assertions)]
    if let Some(answer) = std::env::var_os("WMPASS_TEST_ANSWER") {
        return answer == "yes";
    }
    let answer = tauri::async_runtime::block_on(
        rfd::AsyncMessageDialog::new()
            .set_title("WM Pass")
            .set_description(description)
            .set_buttons(rfd::MessageButtons::OkCancelCustom(yes.to_owned(), no.to_owned()))
            .set_level(rfd::MessageLevel::Warning)
            .show(),
    );
    matches!(answer, rfd::MessageDialogResult::Custom(ref text) if text == yes) || matches!(answer, rfd::MessageDialogResult::Ok)
}

fn broken(_: BridgeError) -> io::Error {
    io::ErrorKind::InvalidData.into()
}

#[derive(Deserialize)]
struct Request {
    id: u64,
    method: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    entry: String,
    #[serde(default)]
    site: String,
    #[serde(default)]
    name: String,
    /// A backup place as the extension keeps it, and what to do there.
    #[serde(default)]
    place: Value,
    #[serde(default)]
    op: String,
    #[serde(default)]
    file: Option<PlaceFile>,
    #[serde(default)]
    step: Value,
    /// Bytes crossing in parts: which transfer, the part, and where a read part starts.
    #[serde(default)]
    token: u64,
    #[serde(default)]
    part: String,
    #[serde(default)]
    offset: usize,
    /// A security key: `fido2` or `yubikey`, and the factor a vault slot holds, if any.
    #[serde(default)]
    kind: String,
    #[serde(default)]
    factor: Value,
}

/// File bytes on their way through one connection, in parts.
#[derive(Default)]
struct Transfers {
    next: u64,
    held: HashMap<u64, Zeroizing<Vec<u8>>>,
}

impl Transfers {
    fn room(&self, more: usize) -> Result<(), String> {
        if self.held.values().map(|bytes| bytes.len()).sum::<usize>() + more > MOST_HELD {
            return Err("too much at once".to_owned());
        }
        Ok(())
    }

    /// Adds a part to a transfer, starting one for token zero. Answers the token.
    fn append(&mut self, token: u64, part: &[u8]) -> Result<u64, String> {
        self.room(part.len())?;
        let token = if token == 0 {
            self.next += 1;
            self.held.insert(self.next, Zeroizing::new(Vec::new()));
            self.next
        } else {
            token
        };
        self.held.get_mut(&token).ok_or("that transfer is gone")?.extend_from_slice(part);
        Ok(token)
    }

    fn keep(&mut self, bytes: Vec<u8>) -> Result<u64, String> {
        self.room(bytes.len())?;
        self.next += 1;
        self.held.insert(self.next, Zeroizing::new(bytes));
        Ok(self.next)
    }

    fn take(&mut self, token: u64) -> Result<Zeroizing<Vec<u8>>, String> {
        self.held.remove(&token).ok_or_else(|| "that transfer is gone".to_owned())
    }

    /// One part of a read, from `offset`. The last part ends the transfer.
    fn part(&mut self, token: u64, offset: usize) -> Result<(String, bool), String> {
        let bytes = self.held.get(&token).ok_or("that transfer is gone")?;
        let end = offset.saturating_add(PART).min(bytes.len());
        let part = B64.encode(bytes.get(offset..end).ok_or("past the end")?);
        let done = end == bytes.len();
        if done {
            self.held.remove(&token);
        }
        Ok((part, done))
    }
}

/// How a person would know a place: its kind, where it is, and as whom.
fn place_label(place: &Value) -> (String, String) {
    let text = |value: Option<&Value>| value.and_then(Value::as_str).unwrap_or_default().trim().to_owned();
    let host_of = |value: Option<&Value>| {
        let address = text(value);
        let rest = address.split_once("://").map_or("", |(_, rest)| rest);
        rest.split(['/', '?', '#']).next().unwrap_or_default().rsplit('@').next().unwrap_or_default().to_owned()
    };
    let kind = text(place.get("kind"));
    let extra = place.get("extra");
    let at = [text(extra.and_then(|x| x.get("host"))), host_of(place.get("url")), host_of(place.get("endpoint")), text(place.get("bucket")), text(extra.and_then(|x| x.get("server")))]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    let user = text(place.get("user"));
    let shown = format!(
        "{}{}{}",
        kind.to_uppercase(),
        if at.is_empty() { String::new() } else { format!(" at {at}") },
        if user.is_empty() { String::new() } else { format!(" as {user}") }
    );
    (format!("place:{kind}:{at}:{user}").to_lowercase(), shown)
}

#[derive(Clone)]
struct Conversation {
    engine: Engine,
    secret: Arc<Zeroizing<[u8; 32]>>,
    saved: Arc<Mutex<Saved>>,
    store: Store,
    app: AppHandle,
    ask_sites: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Conversation {
    fn knows(&self, key: &[u8; 32]) -> bool {
        let wanted = hex(key);
        self.saved.lock().is_ok_and(|s| s.paired.iter().any(|p| p.key == wanted))
    }

    fn serve(&self, mut stream: Stream) -> io::Result<()> {
        // The relay says which extension started it. Anything local could claim the same, so it
        // is only ever shown as a claim.
        let claim = String::from_utf8_lossy(&read_frame(&mut stream)?).chars().filter(|c| !c.is_control()).take(120).collect::<String>();
        let mut server = Server::start(&self.secret).map_err(broken)?;
        let mut transfers = Transfers::default();
        let mut ready = false;
        let mut wanted_name = String::new();
        while !self.stop.load(Ordering::SeqCst) {
            let frame = read_frame(&mut stream)?;
            if self.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let step = server.feed(&frame, |key| self.knows(key)).map_err(broken)?;
            for out in &step.send {
                write_frame(&mut stream, out)?;
            }
            if let Some(name) = step.wants_pairing {
                wanted_name = name;
            }
            ready |= step.ready;
            if let Some(code) = step.compare {
                let accepted = ask(
                    format!(
                        "A browser extension wants to connect to WM Pass.\n\nIt calls itself {wanted_name}, from {claim}.\n\nCheck that it shows this code:\n\n{code}\n\nIf you did not start this, or the code differs, refuse."
                    ),
                    "They match",
                    "Refuse",
                );
                write_frame(&mut stream, &server.answer(accepted).map_err(broken)?)?;
                if !accepted {
                    return Ok(());
                }
                self.pair(&server, &wanted_name)?;
                ready = true;
            }
            if let Some(message) = step.message {
                if !ready {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                let key = server.remote().ok_or(io::ErrorKind::InvalidData)?;
                // Forgetting an extension also ends a conversation it already has open.
                if !self.knows(&key) {
                    return Ok(());
                }
                let reply = self.answer(&message, &key, &mut transfers);
                write_frame(&mut stream, &server.seal(&reply).map_err(broken)?)?;
            }
        }
        Ok(())
    }

    fn pair(&self, server: &Server, name: &str) -> io::Result<()> {
        let key = server.remote().ok_or(io::ErrorKind::InvalidData)?;
        let mut saved = self.saved.lock().map_err(|_| io::ErrorKind::Other)?;
        let wanted = hex(&key);
        saved.paired.retain(|p| p.key != wanted);
        let shown = if name.is_empty() { "Browser extension" } else { name };
        saved.paired.push(Paired { id: fingerprint(&key), name: shown.to_owned(), key: wanted, ..Paired::default() });
        persist(&self.store, &saved).map_err(io::Error::other)
    }

    fn answer(&self, message: &[u8], key: &[u8; 32], transfers: &mut Transfers) -> Zeroizing<Vec<u8>> {
        let Ok(request) = serde_json::from_slice::<Request>(message) else {
            return reply_bytes(&json!({"id": 0, "error": "unreadable"}));
        };
        let id = request.id;
        match self.handle(&request, key, transfers) {
            Ok(reply) => reply,
            Err(error) => reply_bytes(&json!({"id": id, "error": error})),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle(&self, request: &Request, key: &[u8; 32], transfers: &mut Transfers) -> Result<Zeroizing<Vec<u8>>, String> {
        let id = request.id;
        let handle = self.engine.current.load(Ordering::SeqCst);
        match request.method.as_str() {
            "status" => Ok(reply_bytes(&json!({"id": id, "unlocked": handle != 0}))),
            "lock" => {
                self.engine.lock_all();
                Ok(reply_bytes(&json!({"id": id})))
            }
            "match" => {
                if handle == 0 {
                    return Ok(reply_bytes(&json!({"id": id, "locked": true})));
                }
                let reply = self.engine.call("match.web", handle, &json!({"url": request.url}).to_string(), &[], &[], now_ms()).map_err(|e| e.message)?;
                let rows: Vec<Value> = serde_json::from_str(&reply.json).map_err(|e| e.to_string())?;
                let rows: Vec<Value> = rows.into_iter().take(MOST_MATCHES).map(row).collect();
                Ok(reply_bytes(&json!({"id": id, "entries": rows})))
            }
            "fill" => {
                if handle == 0 {
                    return Ok(reply_bytes(&json!({"id": id, "locked": true})));
                }
                // Nobody should be asked about a site the entry was never for.
                let args = json!({"id": request.entry, "url": request.url}).to_string();
                self.engine.call("match.fill", handle, &args, &[], &[], now_ms()).map_err(|e| e.message)?;
                self.approve(&request.url, key)?;
                let reply = self.engine.call("match.fill", handle, &args, &[], &[], now_ms()).map_err(|e| e.message)?;
                self.engine.guard.active();
                let secret = reply.secret.ok_or("nothing to fill")?;
                let mut out = Zeroizing::new(format!("{{\"id\":{id},\"fill\":").into_bytes());
                out.extend_from_slice(&secret);
                out.push(b'}');
                Ok(out)
            }
            "usernames.sites" => {
                let sites: Vec<&str> = usernames::SITES.iter().map(|s| s.name).collect();
                let pages: Vec<&str> = usernames::PAGE_SITES.iter().map(|s| s.name).collect();
                Ok(reply_bytes(&json!({"id": id, "sites": sites, "pages": pages})))
            }
            "usernames.check" => {
                if !self.network_allowed() {
                    return Err("Network features are off in the desktop app.".to_owned());
                }
                // A page check holds this conversation for up to its whole wait, as a fill would.
                let answer = match usernames::SITES.iter().find(|s| s.name == request.site) {
                    Some(site) => usernames::check(*site, &request.name).and_then(|call| call.run(&NativeHttp::default(), NetPolicy::Any)).map_err(|e: NetError| e.to_string())?,
                    None => probe::check(&self.app, probe::site(&request.site)?, &request.name)?,
                };
                Ok(reply_bytes(&json!({"id": id, "answer": probe::word(answer)})))
            }
            "usernames.links" => {
                let links = usernames::LINK_SITES
                    .iter()
                    .map(|site| Ok(json!({"site": site.name, "url": usernames::link_url(site, &request.name)?})))
                    .collect::<Result<Vec<_>, NetError>>()
                    .map_err(|e| e.to_string())?;
                Ok(reply_bytes(&json!({"id": id, "links": links})))
            }
            // ---- backup places the browser cannot reach, for the extension's own vault ----
            "places.kinds" => Ok(reply_bytes(&json!({"id": id, "kinds": crate::places::places_kinds()}))),
            "places.put" => {
                let part = Zeroizing::new(B64.decode(&request.part).map_err(|_| "an unreadable part")?);
                Ok(reply_bytes(&json!({"id": id, "token": transfers.append(request.token, &part)?})))
            }
            "places.get" => {
                let (part, done) = transfers.part(request.token, request.offset)?;
                Ok(reply_bytes(&json!({"id": id, "part": part, "done": done})))
            }
            "places.run" => {
                self.approve_place(&request.place, key)?;
                let stored = request.file.as_ref().map(Stored::from);
                let written = if request.op == "write" { transfers.take(request.token)? } else { Zeroizing::new(Vec::new()) };
                let op = match (request.op.as_str(), &stored) {
                    ("readiness", _) => Op::Readiness,
                    ("list", _) => Op::List,
                    ("write", _) => Op::Write(&request.name, &written),
                    ("read", Some(file)) => Op::Read(file),
                    ("delete", Some(file)) => Op::Delete(file),
                    _ => return Err("unknown operation".to_owned()),
                };
                // The network switch here holds for the extension's runs too.
                let (done, update) = crate::places::run_for(&request.place, self.network_allowed(), op);
                let value = match done? {
                    Done::Files(files) => json!(files.into_iter().map(PlaceFile::from).collect::<Vec<_>>()),
                    Done::Bytes(bytes) => json!({"token": transfers.keep(bytes)?}),
                    Done::Nothing | Done::Stored(_) => Value::Null,
                };
                Ok(reply_bytes(&json!({"id": id, "value": value, "update": update})))
            }
            "places.sign_in" => {
                self.approve_place(&request.place, key)?;
                let answer = crate::signin::step(&request.place, &request.step, self.network_allowed())?;
                Ok(reply_bytes(&json!({"id": id, "answer": answer})))
            }
            // ---- security keys over USB: a YubiKey's challenge-response slot, and keys set up
            // for this app before browsers could reach them ----
            "hardware.tap" => {
                if request.kind != "fido2" && request.kind != "yubikey" {
                    return Err("unknown kind of key".to_owned());
                }
                self.approve_once(key, "keys", |name| {
                    format!("Let {name} use your security keys through WM Pass?\n\nIt asks them to open its own vault. Each key still needs your touch. WM Pass will not ask again.")
                })?;
                let reply = crate::hardware::tap(&request.kind, &request.factor).map_err(|e| e.message)?;
                let output = reply.secret.ok_or("the key answered with nothing")?;
                let mut out = Zeroizing::new(format!("{{\"id\":{id},\"factor\":{},\"output\":\"", reply.json).into_bytes());
                out.extend_from_slice(Zeroizing::new(B64.encode(&*output)).as_bytes());
                out.extend_from_slice(b"\"}");
                Ok(out)
            }
            _ => Err("unknown request".to_owned()),
        }
    }

    /// The same switch the page shows, read fresh each time so turning it off takes hold at once.
    fn network_allowed(&self) -> bool {
        self.store
            .read(PREFS_FILE)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|prefs| prefs.get("network_allowed").and_then(Value::as_bool))
            .unwrap_or(false)
    }

    /// A person says yes once per place and extension. The app then connects where the extension
    /// says, with the details it sends, and keeps none of them.
    fn approve_place(&self, place: &Value, key: &[u8; 32]) -> Result<(), String> {
        let (item, shown) = place_label(place);
        self.approve_once(key, &item, |name| {
            format!("Let {name} keep copies of its vault at {shown}, through WM Pass?\n\nWM Pass connects there with the details the extension sends, and keeps none of them. It will not ask again for this place.")
        })
    }

    /// Asks once for `item`, in this app's own window, and remembers a yes for this extension.
    fn approve_once(&self, key: &[u8; 32], item: &str, question: impl FnOnce(&str) -> String) -> Result<(), String> {
        let wanted = hex(key);
        let (already, name) = {
            let saved = self.saved.lock().map_err(|_| "busy")?;
            let paired = saved.paired.iter().find(|p| p.key == wanted).ok_or("this extension is not connected")?;
            (paired.allowed.iter().any(|a| a == item), paired.name.clone())
        };
        if already {
            return Ok(());
        }
        if !ask(question(&name), "Allow", "Refuse") {
            return Err("refused".to_owned());
        }
        let mut saved = self.saved.lock().map_err(|_| "busy")?;
        if let Some(paired) = saved.paired.iter_mut().find(|p| p.key == wanted) {
            paired.allowed.push(item.to_owned());
        }
        persist(&self.store, &saved)
    }

    /// A person says yes once per site and extension, unless they turned the question off.
    fn approve(&self, url: &str, key: &[u8; 32]) -> Result<(), String> {
        if !self.ask_sites.load(Ordering::SeqCst) {
            return Ok(());
        }
        let host = matching::host_of(url).ok_or("that page has no address to match")?;
        let site = matching::registrable_domain(&host);
        let wanted = hex(key);
        let (already, name) = {
            let saved = self.saved.lock().map_err(|_| "busy")?;
            let paired = saved.paired.iter().find(|p| p.key == wanted).ok_or("this extension is not connected")?;
            (paired.sites.contains(&site), paired.name.clone())
        };
        if already {
            return Ok(());
        }
        if !ask(format!("Fill a saved login on {site}?\n\nAsked for by {name}. WM Pass will not ask again for this site."), "Fill", "Refuse") {
            return Err("refused".to_owned());
        }
        let mut saved = self.saved.lock().map_err(|_| "busy")?;
        if let Some(paired) = saved.paired.iter_mut().find(|p| p.key == wanted) {
            paired.sites.push(site);
        }
        persist(&self.store, &saved)
    }
}

/// Only what a menu shows. Neither passwords nor notes are in a summary, and this narrows it further.
fn row(value: Value) -> Value {
    let entry = value.get("entry").cloned().unwrap_or_default();
    let mut shown = HashMap::new();
    for key in ["id", "title", "subtitle", "icon", "color", "site", "has_password", "has_otp"] {
        if let Some(field) = entry.get(key) {
            shown.insert(key, field.clone());
        }
    }
    json!({"entry": shown, "level": value.get("level")})
}

fn reply_bytes(value: &Value) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(value.to_string().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_crosses_in_parts_and_is_gone_once_read() {
        let mut transfers = Transfers::default();
        let big: Vec<u8> = (0..PART * 2 + 7).map(|i| (i % 251) as u8).collect();
        let token = transfers.append(0, &big[..PART]).unwrap();
        assert_eq!(transfers.append(token, &big[PART..]).unwrap(), token);
        let written = transfers.take(token).unwrap();
        assert_eq!(written.as_slice(), big.as_slice());

        let token = transfers.keep(big.clone()).unwrap();
        let mut back = Vec::new();
        loop {
            let (part, done) = transfers.part(token, back.len()).unwrap();
            back.extend(B64.decode(part).unwrap());
            if done {
                break;
            }
        }
        assert_eq!(back, big);
        assert!(transfers.part(token, 0).is_err(), "a finished read is not kept");
    }

    #[test]
    fn an_empty_file_still_makes_a_transfer() {
        let mut transfers = Transfers::default();
        let token = transfers.append(0, &[]).unwrap();
        assert!(transfers.take(token).unwrap().is_empty());
    }

    #[test]
    fn an_extension_cannot_hold_without_limit() {
        let mut transfers = Transfers::default();
        transfers.keep(vec![0; 1024]).unwrap();
        assert!(transfers.room(MOST_HELD - 1024).is_ok());
        assert!(transfers.room(MOST_HELD - 1023).is_err());
    }

    #[test]
    fn a_place_is_asked_about_by_where_it_is_and_as_whom() {
        let sftp = json!({"kind": "sftp", "user": "Wasi", "extra": {"host": "nas.local", "port": 22}});
        assert_eq!(place_label(&sftp), ("place:sftp:nas.local:wasi".to_owned(), "SFTP at nas.local as Wasi".to_owned()));
        let webdav = json!({"kind": "webdav", "url": "https://me@dav.example.com/remote.php/webdav/", "user": ""});
        assert_eq!(place_label(&webdav).1, "WEBDAV at dav.example.com");
        // Another account on the same server is another place.
        let other = json!({"kind": "sftp", "user": "someone", "extra": {"host": "nas.local"}});
        assert_ne!(place_label(&sftp).0, place_label(&other).0);
    }
}
