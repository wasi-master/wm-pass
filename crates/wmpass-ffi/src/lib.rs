//! A C interface to the session, for shells that are neither JNI nor WebAssembly. Requests and
//! replies are the same frames every other shell uses, so secrets stay out of strings here too.
//!
//! A bug that panics is caught here and never unwinds into the caller. The service it panicked
//! in may be half changed, so it is dropped, which wipes the keys of every open vault, and a
//! fresh one takes its place: the call answers 3, and later calls on the old handles answer
//! `locked`, as after a lock. The files on disk are whole, since a save replaces a file at once.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};

use wmpass_session::{Service, wire};
use zeroize::Zeroize;

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);
static DEVICE: AtomicU32 = AtomicU32::new(0);

/// Starts the session. `device` tells this device's changes apart when merging.
#[unsafe(no_mangle)]
pub extern "C" fn wmpass_session_start(device: u32) {
    let _ = catch_unwind(|| {
        DEVICE.store(device, Ordering::SeqCst);
        *SERVICE.lock().unwrap_or_else(PoisonError::into_inner) = Some(Service::new(device));
        SERVICE.clear_poison();
    });
}

/// Runs one request frame and hands back the reply, which the caller returns with
/// [`wmpass_buffer_free`]. Returns 0 when it worked, 1 for a null pointer, 2 before
/// [`wmpass_session_start`], and 3 when an internal error stopped the call: every vault is
/// locked then, and the shell shows its unlock screen. Calls after that work again.
///
/// # Safety
/// `frame` must point to `length` readable bytes, and both out pointers must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmpass_session_call(frame: *const u8, length: usize, out: *mut *mut u8, out_length: *mut usize) -> i32 {
    if frame.is_null() || out.is_null() || out_length.is_null() {
        return 1;
    }
    // SAFETY: the caller promises this range is readable.
    let request = unsafe { std::slice::from_raw_parts(frame, length) };
    let reply = catch_unwind(AssertUnwindSafe(|| {
        // A lock a panic poisoned guards a service left half changed: it is never read.
        let Ok(mut guard) = SERVICE.lock() else {
            return Err(3);
        };
        let service = guard.as_mut().ok_or(2)?;
        Ok(wire::dispatch(service, request))
    }));
    let reply = match reply {
        Ok(Ok(reply)) => reply,
        Ok(Err(code)) => {
            if code == 3 {
                replace();
            }
            return code;
        }
        Err(_) => {
            replace();
            return 3;
        }
    };
    let bytes: Box<[u8]> = reply.to_vec().into_boxed_slice();
    let written = bytes.len();
    // SAFETY: the caller promises both pointers are writable.
    unsafe {
        *out = Box::into_raw(bytes).cast();
        *out_length = written;
    }
    0
}

/// Wipes and frees a reply.
///
/// # Safety
/// The pointer and length must be a pair [`wmpass_session_call`] returned, and only once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wmpass_buffer_free(buffer: *mut u8, length: usize) {
    if buffer.is_null() {
        return;
    }
    // SAFETY: this pair came from a boxed slice in `wmpass_session_call`.
    let mut bytes = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(buffer, length)) };
    bytes.zeroize();
}

/// Drops a service a panic may have left half changed, wiping its keys, and starts a fresh one.
fn replace() {
    let mut guard = SERVICE.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_some() {
        let old = guard.replace(Service::new(DEVICE.load(Ordering::SeqCst)));
        let _ = catch_unwind(AssertUnwindSafe(move || drop(old)));
    }
    drop(guard);
    SERVICE.clear_poison();
}

/// Locks every open vault.
#[unsafe(no_mangle)]
pub extern "C" fn wmpass_session_lock_all() {
    let _ = catch_unwind(|| match SERVICE.lock() {
        Ok(mut guard) => {
            if let Some(service) = guard.as_mut() {
                service.lock_all();
            }
        }
        Err(poisoned) => {
            drop(poisoned);
            replace();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, handle: u64, args: &str, secret: &[u8]) -> Vec<u8> {
        let head = format!(r#"{{"method":"{method}","handle":{handle},"args":{args},"now_ms":1}}"#);
        let mut frame = Vec::new();
        for run in [head.as_bytes(), secret] {
            frame.extend_from_slice(&u32::try_from(run.len()).unwrap_or(0).to_le_bytes());
            frame.extend_from_slice(run);
        }
        frame.extend_from_slice(&0u32.to_le_bytes());
        frame
    }

    /// The call's answer, and the reply's text when it had one.
    fn call(frame: &[u8]) -> (i32, String) {
        let (mut out, mut length) = (std::ptr::null_mut(), 0usize);
        // SAFETY: the pointers are to locals that outlive the call.
        let code = unsafe { wmpass_session_call(frame.as_ptr(), frame.len(), &raw mut out, &raw mut length) };
        if code != 0 {
            return (code, String::new());
        }
        // SAFETY: the pair the call just returned, read before it is freed.
        let text = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(out, length) }).into_owned();
        // SAFETY: the pair the call just returned.
        unsafe { wmpass_buffer_free(out, length) };
        (0, text)
    }

    #[test]
    fn each_failure_has_its_own_answer_and_a_panic_locks_without_stopping() {
        let (mut out, mut length) = (std::ptr::null_mut(), 0usize);
        // SAFETY: a null frame is refused before anything is read.
        assert_eq!(unsafe { wmpass_session_call(std::ptr::null(), 0, &raw mut out, &raw mut length) }, 1);
        assert_eq!(call(&request("kinds", 0, "{}", b"")).0, 2, "not started");
        wmpass_session_start(1);
        let (code, opened) = call(&request("vault.create", 0, r#"{"name":"Mine","kdf":{"m_kib":65536,"t":3,"p":1}}"#, b"correct horse"));
        assert_eq!(code, 0);
        assert!(opened.contains(r#""handle":1"#), "{opened}");
        assert!(!call(&request("vault.info", 1, "{}", b"")).1.contains("locked"));
        // A bug inside the call: debug.panic is only in debug builds and tests.
        assert_eq!(call(&request("debug.panic", 0, "{}", b"")).0, 3, "an internal error, and the process goes on");
        assert!(!SERVICE.is_poisoned());
        let fresh = SERVICE.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(Service::open_handles);
        assert_eq!(fresh, Some(Vec::new()), "the service went, and its keys with it");
        let (code, after) = call(&request("vault.info", 1, "{}", b""));
        assert_eq!(code, 0, "the session works again");
        assert!(after.contains(r#""code":"locked""#), "and the vault that was open is locked: {after}");
        // A lock poisoned some other way is treated the same.
        let _ = catch_unwind(|| {
            let _held = SERVICE.lock();
            panic!("mid-change");
        });
        assert_eq!(call(&request("kinds", 0, "{}", b"")).0, 3);
        assert_eq!(call(&request("kinds", 0, "{}", b"")).0, 0);
    }
}
