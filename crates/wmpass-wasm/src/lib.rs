//! The session as the extension sees it: one call, bytes in and bytes out.
//!
//! The frame format is in `wmpass_session::wire`. Vault state stays in this module's memory,
//! so nothing a page script can reach ever holds a key.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wmpass_session::{Service, wire};

thread_local! {
    static SERVICE: RefCell<Option<Service>> = const { RefCell::new(None) };
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
