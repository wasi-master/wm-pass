//! An SSH agent over the keys in the open vault. It speaks the small part of the protocol that
//! clients need: list keys, and sign. When the vault locks, it has nothing to offer.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Stream};
use serde::Deserialize;

use crate::engine::{Engine, now_ms};

const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
const LARGEST_MESSAGE: usize = 256 * 1024;

#[derive(Deserialize)]
struct KeyView {
    title: String,
    comment: String,
    blob: String,
}

#[derive(Default)]
pub struct AgentState {
    pub running: bool,
    pub socket: Option<String>,
    pub last: Option<String>,
    /// Why the agent stopped by itself, until it starts again.
    pub stopped: Option<String>,
}

pub struct Agent {
    pub state: Arc<Mutex<AgentState>>,
    stop: Arc<AtomicBool>,
    pub confirm: Arc<AtomicBool>,
}

impl Agent {
    pub fn new() -> Self {
        Self { state: Arc::default(), stop: Arc::new(AtomicBool::new(false)), confirm: Arc::new(AtomicBool::new(true)) }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Starts listening, unless it already is.
    pub fn start(&self, engine: Engine, socket_dir: &std::path::Path) -> Result<(), String> {
        {
            let state = self.state.lock().map_err(|_| "busy")?;
            if state.running {
                // A stop that the listening thread has not acted on yet is simply taken back.
                // Under the state's lock, which that thread holds to decide whether to leave.
                self.stop.store(false, Ordering::SeqCst);
                return Ok(());
            }
        }
        let (name, shown) = socket_name(socket_dir)?;
        let options = ListenerOptions::new().name(name).nonblocking(ListenerNonblockingMode::Accept).try_overwrite(true);
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
            options.security_descriptor(owner_only()?)
        };
        let listener = options
            .create_sync()
            .map_err(|e| format!("the agent could not start: {e}"))?;
        restrict(&shown);
        self.stop.store(false, Ordering::SeqCst);
        {
            let mut state = self.state.lock().map_err(|_| "busy")?;
            state.running = true;
            state.socket = Some(shown);
            state.stopped = None;
        }
        let stop = Arc::clone(&self.stop);
        let state = Arc::clone(&self.state);
        let confirm = Arc::clone(&self.confirm);
        std::thread::spawn(move || {
            // A bug that panics in here starts the listening again, a few times, rather than leave
            // the agent shown as running with nothing answering. Calls into the session are
            // guarded by the engine itself.
            let mut restarts = 0;
            loop {
                let accepted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    while !stop.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok(stream) => {
                                // On BSD and macOS it inherits the listener's mode, and a client that
                                // pauses before its first write would otherwise look like it had gone.
                                if stream.set_nonblocking(false).is_err() {
                                    continue;
                                }
                                let engine = engine.clone();
                                let state = Arc::clone(&state);
                                let confirm = Arc::clone(&confirm);
                                std::thread::spawn(move || {
                                    // A panic costs this one client its answer. The state it may have
                                    // held is a few plain fields, still whole.
                                    let served = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve(stream, &engine, &state, &confirm)));
                                    if served.is_err() {
                                        state.clear_poison();
                                    }
                                });
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(100)),
                            Err(_) => return true,
                        }
                    }
                    false
                }));
                let broken = match accepted {
                    Ok(broken) => broken,
                    Err(_) => {
                        state.clear_poison();
                        restarts += 1;
                        if restarts <= 3 {
                            continue;
                        }
                        true
                    }
                };
                let mut held = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                // A start since the stop was seen found this running and took the stop back.
                if !broken && !stop.load(Ordering::SeqCst) {
                    continue;
                }
                held.running = false;
                held.socket = None;
                if broken {
                    held.last = None;
                    held.stopped = Some("The SSH agent stopped after an internal error. Turn it off and on again to restart it.".to_owned());
                }
                return;
            }
        });
        Ok(())
    }
}

/// A socket's whole path has to fit in about a hundred bytes, so this cannot live under a long
/// application folder. It sits in a folder only this user can enter.
#[cfg(unix)]
pub fn socket_folder() -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    #[cfg(debug_assertions)]
    let test = std::env::var_os("WMPASS_RUN_DIR");
    #[cfg(not(debug_assertions))]
    let test: Option<std::ffi::OsString> = None;
    let folder = match (test, std::env::var_os("XDG_RUNTIME_DIR"), std::env::var_os("HOME")) {
        (Some(folder), _, _) => std::path::PathBuf::from(folder),
        (_, Some(runtime), _) => std::path::PathBuf::from(runtime).join("wm-pass"),
        (_, _, Some(home)) => std::path::PathBuf::from(home).join(".wm-pass"),
        _ => return Err("this account has no home folder".to_owned()),
    };
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    Ok(folder)
}

#[cfg(windows)]
pub fn socket_folder() -> Result<std::path::PathBuf, String> {
    Ok(std::path::PathBuf::new())
}

#[cfg(unix)]
fn socket_name(dir: &std::path::Path) -> Result<(interprocess::local_socket::Name<'static>, String), String> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("ssh-agent.sock");
    let shown = path.to_string_lossy().into_owned();
    let name = shown.clone().to_fs_name::<GenericFilePath>().map_err(|e| e.to_string())?;
    Ok((name, shown))
}

#[cfg(windows)]
fn socket_name(_dir: &std::path::Path) -> Result<(interprocess::local_socket::Name<'static>, String), String> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    let name = "wmpass-ssh-agent".to_ns_name::<GenericNamespaced>().map_err(|e| e.to_string())?;
    Ok((name, r"\\.\pipe\wmpass-ssh-agent".to_owned()))
}

#[cfg(unix)]
fn restrict(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(windows)]
fn restrict(_path: &str) {}

/// A pipe only this account and the system can open. Windows would otherwise let every account
/// on the computer read from it. The pipe's first instance is made with
/// `FILE_FLAG_FIRST_PIPE_INSTANCE` by interprocess, so a pipe of that name made first by another
/// program is refused rather than shared.
#[cfg(windows)]
pub(crate) fn owner_only() -> Result<interprocess::os::windows::security_descriptor::SecurityDescriptor, String> {
    // Protected, so nothing is inherited: full access for the system and for the pipe's owner.
    let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;SY)(A;;GA;;;OW)").map_err(|e| e.to_string())?;
    interprocess::os::windows::security_descriptor::SecurityDescriptor::deserialize(&sddl).map_err(|e| format!("the pipe's access could not be set: {e}"))
}

fn read_message(stream: &mut Stream) -> io::Result<Vec<u8>> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > LARGEST_MESSAGE {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok(body)
}

fn write_message(stream: &mut Stream, body: &[u8]) -> io::Result<()> {
    let length = u32::try_from(body.len()).map_err(|_| io::ErrorKind::InvalidData)?;
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn put_string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&u32::try_from(value.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(value);
}

fn take_string<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let (head, rest) = input.split_first_chunk::<4>()?;
    let length = u32::from_be_bytes(*head) as usize;
    if length > rest.len() {
        return None;
    }
    let (value, rest) = rest.split_at(length);
    *input = rest;
    Some(value)
}

fn keys(engine: &Engine) -> Vec<KeyView> {
    let handle = engine.current.load(Ordering::SeqCst);
    if handle == 0 {
        return Vec::new();
    }
    engine
        .call("ssh.agent_keys", handle, "{}", &[], &[], now_ms())
        .ok()
        .and_then(|reply| serde_json::from_str(&reply.json).ok())
        .unwrap_or_default()
}

fn answer_list(engine: &Engine) -> Vec<u8> {
    let found = keys(engine);
    let mut out = vec![IDENTITIES_ANSWER];
    let usable: Vec<(Vec<u8>, &KeyView)> = found.iter().filter_map(|key| Some((B64.decode(&key.blob).ok()?, key))).collect();
    out.extend_from_slice(&u32::try_from(usable.len()).unwrap_or(0).to_be_bytes());
    for (blob, key) in &usable {
        put_string(&mut out, blob);
        put_string(&mut out, key.comment.as_bytes());
    }
    out
}

fn ask(title: &str) -> bool {
    let answer = tauri::async_runtime::block_on(
        rfd::AsyncMessageDialog::new()
            .set_title("WM Pass")
            .set_description(format!("A program wants to use the SSH key “{title}”."))
            .set_buttons(rfd::MessageButtons::OkCancelCustom("Allow".to_owned(), "Refuse".to_owned()))
            .set_level(rfd::MessageLevel::Warning)
            .show(),
    );
    matches!(answer, rfd::MessageDialogResult::Custom(ref text) if text == "Allow") || matches!(answer, rfd::MessageDialogResult::Ok)
}

fn answer_sign(request: &[u8], engine: &Engine, state: &Mutex<AgentState>, confirm: &AtomicBool) -> Option<Vec<u8>> {
    let mut rest = request;
    let blob = take_string(&mut rest)?;
    let data = take_string(&mut rest)?;
    let flags = rest.split_first_chunk::<4>().map_or(0, |(head, _)| u32::from_be_bytes(*head));
    let handle = engine.current.load(Ordering::SeqCst);
    if handle == 0 {
        return None;
    }
    let wanted = B64.encode(blob);
    let found = keys(engine);
    let key = found.iter().find(|key| key.blob == wanted)?;
    if confirm.load(Ordering::SeqCst) && !ask(&key.title) {
        return None;
    }
    let args = serde_json::json!({"blob": wanted, "data": B64.encode(data), "flags": flags}).to_string();
    let reply = engine.call("ssh.agent_sign", handle, &args, &[], &[], now_ms()).ok()?;
    if let Ok(mut state) = state.lock() {
        state.last = Some(key.title.clone());
    }
    let signature = reply.secret?;
    let mut out = vec![SIGN_RESPONSE];
    put_string(&mut out, &signature);
    Some(out)
}

fn serve(mut stream: Stream, engine: &Engine, state: &Mutex<AgentState>, confirm: &AtomicBool) -> io::Result<()> {
    loop {
        let message = read_message(&mut stream)?;
        let reply = match message.split_first() {
            Some((&REQUEST_IDENTITIES, _)) => answer_list(engine),
            Some((&SIGN_REQUEST, rest)) => answer_sign(rest, engine, state, confirm).unwrap_or_else(|| vec![FAILURE]),
            _ => vec![FAILURE],
        };
        write_message(&mut stream, &reply)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_read_back_and_lies_are_refused() {
        let mut out = Vec::new();
        put_string(&mut out, b"abc");
        let mut rest = out.as_slice();
        assert_eq!(take_string(&mut rest), Some(&b"abc"[..]));
        assert!(rest.is_empty());
        let mut lying: &[u8] = &[0, 0, 0, 9, 1];
        assert_eq!(take_string(&mut lying), None);
    }
}
