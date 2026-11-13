//! The session as the extension and the web vault see it: one call, bytes in and bytes out.
//!
//! The frame format is in `wmpass_session::wire`. Vault state stays in this module's memory,
//! so nothing a page script can reach ever holds a key.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};

mod places;

use js_sys::Function;
use wasm_bindgen::prelude::*;
use wmpass_bridge::{Client, Keys};
use wmpass_session::{Service, SessionError, net, wire};
use zeroize::{Zeroize, Zeroizing};

// A Mutex rather than a RefCell: a call that waits on the network borrows the service between
// its requests, and other calls run in the meantime. Nothing holds it across a wait.
static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

thread_local! {
    static BRIDGE: RefCell<Option<Client>> = const { RefCell::new(None) };
}

// A panic here aborts, as stable wasm32 cannot unwind: the call traps and JavaScript sees a
// `RuntimeError`. Nothing unwinds, so a lock or borrow held at that moment is never given back,
// and every later call would trap again with nothing to say why. The hook notes the panic, and
// later calls answer with an `internal` error, as the desktop app and the phone do. Unlike
// theirs, this module cannot start a fresh service: what it held stays in its memory until the
// page wipes that memory and loads the module again (ui/src/hosts/wasm.ts, the extension's
// background).
static PANICKED: AtomicBool = AtomicBool::new(false);

fn watch_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICKED.store(true, Ordering::SeqCst);
            previous(info);
        }));
    });
}

fn panicked() -> bool {
    PANICKED.load(Ordering::SeqCst)
}

fn stopped() -> SessionError {
    // Keys the service holds go with it, where its lock was not held when the panic hit.
    if let Ok(mut held) = SERVICE.try_lock() {
        drop(held.take());
    }
    wmpass_session::internal_error()
}

/// Whether a panic has stopped this module, for a page that saw a call trap.
#[wasm_bindgen]
pub fn stopped_by_panic() -> bool {
    panicked()
}

fn stopped_js() -> JsError {
    JsError::new(&stopped().message)
}

fn service() -> MutexGuard<'static, Option<Service>> {
    SERVICE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_started() -> SessionError {
    SessionError { code: "not_started", message: "the vault service has not started".to_owned() }
}

/// Starts the service, with this browser's own number for breaking ties in a merge.
#[wasm_bindgen]
pub fn start(device: u32) {
    watch_panics();
    if panicked() {
        return;
    }
    let mut service = service();
    if service.is_none() {
        *service = Some(Service::new(device));
    }
}

// What crosses to JavaScript, and who wipes it.
//
// A request comes in as a `Vec<u8>`: the glue copies the caller's Uint8Array into memory it
// allocates in this module and hands that allocation over, so Rust owns it and wipes it,
// spare capacity included, before it is freed. A `&[u8]` would have the glue free its copy
// unwiped after the call, leaving master passwords in freed memory for the next allocation.
//
// A reply goes out as a Uint8Array that JavaScript owns, copied out of a buffer Rust then
// wipes. Returning a `Vec<u8>` would have the glue copy it and free Rust's copy unwiped.
//
// What Rust cannot wipe: the caller's own Uint8Array of the request and the Uint8Array of the
// reply, both in the JavaScript heap, which the caller fills with zeros when done; and any
// copy the JavaScript engine makes of those on its own, which nothing here can reach.

/// Answers one frame, then fills it with zeros.
fn answer(frame: &mut [u8]) -> Zeroizing<Vec<u8>> {
    let reply = if panicked() {
        wire::reply_frame(&Err(stopped()))
    } else {
        match service().as_mut() {
            Some(service) => wire::dispatch(service, frame),
            None => wire::reply_frame(&Err(not_started())),
        }
    };
    frame.zeroize();
    reply
}

/// A reply handed to JavaScript, copied out of a buffer that is wiped once it is.
fn to_js(reply: &[u8]) -> js_sys::Uint8Array {
    js_sys::Uint8Array::from(reply)
}

/// Runs one call. The caller fills its own copies with zeros when done with them.
#[wasm_bindgen]
pub fn call(frame: Vec<u8>) -> js_sys::Uint8Array {
    let mut frame = Zeroizing::new(frame);
    to_js(&answer(&mut frame))
}

/// Runs one call, including those that reach a network: breach checks, aliases, inboxes, icons,
/// Bitwarden. Their requests go out through `fetch(method, url, headers, body, timeoutMs)`, which
/// the page supplies; the policy check and the reading of answers stay here.
#[wasm_bindgen]
pub async fn call_async(frame: Vec<u8>, fetch: Function) -> js_sys::Uint8Array {
    let mut frame = Zeroizing::new(frame);
    if panicked() {
        return to_js(&wire::reply_frame(&Err(stopped())));
    }
    let request = match wire::parse_request(&frame) {
        Ok(request) => request,
        Err(error) => return to_js(&wire::reply_frame(&Err(error))),
    };
    if !net::is_net_method(&request.method) {
        return to_js(&answer(&mut frame));
    }
    let http = places::JsFetch(fetch);
    let result = net::call_over(&http, &SERVICE, &request.method, request.handle, &request.args, request.secret, request.online, request.now_ms).await;
    to_js(&wire::reply_frame(&result))
}

/// Forgets every key.
#[wasm_bindgen]
pub fn lock_all() {
    if panicked() {
        stopped();
        return;
    }
    if let Some(service) = service().as_mut() {
        service.lock_all();
    }
}

fn bridge_failed(error: wmpass_bridge::BridgeError) -> JsError {
    JsError::new(&error.to_string())
}

/// A new key pair for talking to the desktop app: the secret half, then the public half.
#[wasm_bindgen]
pub fn bridge_new_keys() -> Result<Vec<u8>, JsError> {
    let keys = Keys::generate().map_err(bridge_failed)?;
    let mut out = keys.secret.to_vec();
    out.extend_from_slice(&keys.public);
    Ok(out)
}

/// Starts a conversation. `pinned` is the app's key if it is already known, otherwise empty.
/// Returns the first frame to send.
#[wasm_bindgen]
pub fn bridge_start(secret: &[u8], pinned: &[u8], name: &str) -> Result<Vec<u8>, JsError> {
    if panicked() {
        return Err(stopped_js());
    }
    let secret: &[u8; 32] = secret.try_into().map_err(|_| JsError::new("bad key"))?;
    let pinned = <[u8; 32]>::try_from(pinned).ok();
    let (client, first) = Client::start(secret, pinned, name).map_err(bridge_failed)?;
    BRIDGE.with(|cell| *cell.borrow_mut() = Some(client));
    Ok(first)
}

/// Feeds one frame. Everything binary in the answer is hex, so it fits in JSON.
#[wasm_bindgen]
pub fn bridge_feed(frame: &[u8]) -> Result<String, JsError> {
    if panicked() {
        return Err(stopped_js());
    }
    BRIDGE.with(|cell| {
        let mut cell = cell.borrow_mut();
        let client = cell.as_mut().ok_or_else(|| JsError::new("not connected"))?;
        let step = client.feed(frame).map_err(bridge_failed)?;
        let send: Vec<String> = step.send.iter().map(|f| hex(f)).collect();
        Ok(serde_json::json!({
            "send": send,
            "compare": step.compare,
            "ready": step.ready,
            "declined": step.declined,
            "message": step.message.map(|m| hex(&m)),
            "remote": client.remote().map(|r| hex(&r)),
        })
        .to_string())
    })
}

#[wasm_bindgen]
pub fn bridge_seal(plain: Vec<u8>) -> Result<Vec<u8>, JsError> {
    // A request on its way to the desktop app, master password and all: owned, so it is
    // wiped like `call`'s. The sealed bytes going back are safe to leave.
    let plain = Zeroizing::new(plain);
    if panicked() {
        return Err(stopped_js());
    }
    BRIDGE.with(|cell| {
        let mut cell = cell.borrow_mut();
        cell.as_mut().ok_or_else(|| JsError::new("not connected"))?.seal(&plain).map_err(bridge_failed)
    })
}

#[wasm_bindgen]
pub fn bridge_close() {
    BRIDGE.with(|cell| {
        // After a panic the cell may still be borrowed; then there is nothing to close safely.
        if let Ok(mut cell) = cell.try_borrow_mut() {
            *cell = None;
        }
    });
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_wiped_once_answered() {
        start(1);
        // head, secret, no blobs; lengths are little-endian u32.
        let head = br#"{"method":"unlock","handle":1,"args":{}}"#;
        let secret = b"correct horse battery";
        let mut frame = Vec::new();
        for run in [&head[..], &secret[..]] {
            frame.extend_from_slice(&(run.len() as u32).to_le_bytes());
            frame.extend_from_slice(run);
        }
        frame.extend_from_slice(&0u32.to_le_bytes());
        assert!(wire::parse_request(&frame).is_ok(), "a frame the session reads");
        assert!(frame.windows(7).any(|w| w == b"correct"));
        let reply = answer(&mut frame);
        assert!(!reply.is_empty(), "it was answered");
        assert!(frame.iter().all(|&b| b == 0), "and the password in it is gone");
    }

    #[test]
    fn after_a_panic_calls_answer_with_an_error_that_says_so() {
        start(1);
        let ok = answer(&mut []);
        assert!(!String::from_utf8_lossy(&ok).contains("\"internal\""));
        let _ = std::panic::catch_unwind(|| panic!("mid-call"));
        assert!(panicked(), "the hook noticed");
        let reply = answer(&mut []);
        assert!(String::from_utf8_lossy(&reply).contains("\"internal\""), "a reply frame naming the internal error");
        assert!(stopped_by_panic());
        assert!(SERVICE.lock().unwrap_or_else(PoisonError::into_inner).is_none(), "and the keys went with the service");
        PANICKED.store(false, Ordering::SeqCst);
    }
}
