//! A C interface to the session, for shells that are neither JNI nor WebAssembly. Requests and
//! replies are the same frames every other shell uses, so secrets stay out of strings here too.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use wmpass_session::{Service, wire};
use zeroize::Zeroize;

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

/// Starts the session. `device` tells this device's changes apart when merging.
#[unsafe(no_mangle)]
pub extern "C" fn wmpass_session_start(device: u32) {
    let _ = catch_unwind(|| {
        if let Ok(mut service) = SERVICE.lock() {
            *service = Some(Service::new(device));
        }
    });
}

/// Runs one request frame and hands back the reply, which the caller returns with
/// [`wmpass_buffer_free`]. Returns 0 when it worked.
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
        let mut guard = SERVICE.lock().ok()?;
        let service = guard.as_mut()?;
        Some(wire::dispatch(service, request))
    }));
    let Ok(Some(reply)) = reply else { return 2 };
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

/// Locks every open vault.
#[unsafe(no_mangle)]
pub extern "C" fn wmpass_session_lock_all() {
    let _ = catch_unwind(|| {
        if let Ok(mut guard) = SERVICE.lock()
            && let Some(service) = guard.as_mut()
        {
            service.lock_all();
        }
    });
}
