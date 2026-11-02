//! The session as the extension sees it: one call, bytes in and bytes out.
//!
//! The frame format is in `wmpass_session::wire`. Vault state stays in this module's memory,
//! so nothing a page script can reach ever holds a key.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wmpass_bridge::{Client, Keys};
use wmpass_session::{Service, wire};

thread_local! {
    static SERVICE: RefCell<Option<Service>> = const { RefCell::new(None) };
    static BRIDGE: RefCell<Option<Client>> = const { RefCell::new(None) };
}

/// Starts the service, with this browser's own number for breaking ties in a merge.
#[wasm_bindgen]
pub fn start(device: u32) {
    SERVICE.with(|cell| {
        let mut service = cell.borrow_mut();
        if service.is_none() {
            *service = Some(Service::new(device));
        }
    });
}

/// Runs one call. The caller fills its own copies with zeros when done with them.
#[wasm_bindgen]
pub fn call(frame: &[u8]) -> Vec<u8> {
    SERVICE.with(|cell| {
        let mut service = cell.borrow_mut();
        let reply = match service.as_mut() {
            Some(service) => wire::dispatch(service, frame),
            None => wire::reply_frame(&Err(wmpass_session::SessionError {
                code: "not_started",
                message: "the vault service has not started".to_owned(),
            })),
        };
        reply.to_vec()
    })
}

/// Forgets every key.
#[wasm_bindgen]
pub fn lock_all() {
    SERVICE.with(|cell| {
        if let Some(service) = cell.borrow_mut().as_mut() {
            service.lock_all();
        }
    });
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
    let secret: &[u8; 32] = secret.try_into().map_err(|_| JsError::new("bad key"))?;
    let pinned = <[u8; 32]>::try_from(pinned).ok();
    let (client, first) = Client::start(secret, pinned, name).map_err(bridge_failed)?;
    BRIDGE.with(|cell| *cell.borrow_mut() = Some(client));
    Ok(first)
}

/// Feeds one frame. Everything binary in the answer is hex, so it fits in JSON.
#[wasm_bindgen]
pub fn bridge_feed(frame: &[u8]) -> Result<String, JsError> {
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
pub fn bridge_seal(plain: &[u8]) -> Result<Vec<u8>, JsError> {
    BRIDGE.with(|cell| {
        let mut cell = cell.borrow_mut();
        cell.as_mut().ok_or_else(|| JsError::new("not connected"))?.seal(plain).map_err(bridge_failed)
    })
}

#[wasm_bindgen]
pub fn bridge_close() {
    BRIDGE.with(|cell| *cell.borrow_mut() = None);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
