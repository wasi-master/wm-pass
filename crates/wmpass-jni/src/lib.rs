//! The Android side of [`wmpass_session`]: one call with bytes for secrets.
//!
//! `extern "system"` functions abort on a panic rather than unwind into the JVM, and
//! the release profile aborts anyway. The process dying with the vault in it is the
//! right way for a bug here to fail.

use std::sync::Mutex;

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString};
use jni::sys::{jint, jlong, jobjectArray};
use wmpass_session::{Call, Reply, Service, SessionError};
use zeroize::Zeroizing;

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

fn with_service<T>(run: impl FnOnce(&mut Service) -> T) -> Option<T> {
    // A poisoned lock means a panic got here under a debug profile. Nothing that
    // panicked mid-change should be used again.
    let mut guard = SERVICE.lock().ok()?;
    guard.as_mut().map(run)
}

fn throw(env: &mut JNIEnv<'_>, code: &str, message: &str) {
    let text = format!("{code}\n{message}");
    let _ = env.throw_new("com/wasimaster/wmpass/core/NativeFailure", text);
}

/// Copies a Java byte array. The copy is wiped on drop; the caller wipes the original.
fn bytes(env: &JNIEnv<'_>, array: &JByteArray<'_>) -> jni::errors::Result<Zeroizing<Vec<u8>>> {
    if array.as_raw().is_null() {
        return Ok(Zeroizing::new(Vec::new()));
    }
    env.convert_byte_array(array).map(Zeroizing::new)
}

fn read_blobs(env: &mut JNIEnv<'_>, blobs: &JObjectArray<'_>) -> jni::errors::Result<Vec<Zeroizing<Vec<u8>>>> {
    if blobs.as_raw().is_null() {
        return Ok(Vec::new());
    }
    let count = env.get_array_length(blobs)?;
    let mut out = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
    for index in 0..count {
        let item = env.get_object_array_element(blobs, index)?;
        let array = JByteArray::from(item);
        out.push(bytes(env, &array)?);
        env.delete_local_ref(array)?;
    }
    Ok(out)
}

fn write_reply(env: &mut JNIEnv<'_>, reply: &Reply) -> jni::errors::Result<jobjectArray> {
    let object = env.find_class("java/lang/Object")?;
    let out = env.new_object_array(3, object, JObject::null())?;
    let json = env.new_string(&reply.json)?;
    env.set_object_array_element(&out, 0, json)?;
    if let Some(secret) = &reply.secret {
        let array = env.byte_array_from_slice(secret)?;
        env.set_object_array_element(&out, 1, array)?;
    }
    let byte_array = env.find_class("[B")?;
    let count = i32::try_from(reply.blobs.len()).unwrap_or(0);
    let blobs = env.new_object_array(count, byte_array, JObject::null())?;
    for (index, blob) in reply.blobs.iter().enumerate() {
        let array = env.byte_array_from_slice(blob)?;
        env.set_object_array_element(&blobs, i32::try_from(index).unwrap_or(0), &array)?;
        env.delete_local_ref(array)?;
    }
    env.set_object_array_element(&out, 2, blobs)?;
    Ok(out.into_raw())
}

/// `init(device)`: starts the service once per process.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_init(_env: JNIEnv<'_>, _class: JClass<'_>, device: jint) {
    if let Ok(mut guard) = SERVICE.lock()
        && guard.is_none()
    {
        *guard = Some(Service::new(device.cast_unsigned()));
    }
}

/// `lockAll()`: drops every open vault, wiping its keys.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_lockAll(_env: JNIEnv<'_>, _class: JClass<'_>) {
    with_service(Service::lock_all);
}

/// `call(method, handle, args, secret, blobs, nowMs)`: returns `{json, secret, blobs}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_call<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    method: JString<'local>,
    handle: jlong,
    args: JString<'local>,
    secret: JByteArray<'local>,
    blobs: JObjectArray<'local>,
    now_ms: jlong,
) -> jobjectArray {
    let result = run_call(&mut env, &method, handle, &args, &secret, &blobs, now_ms);
    match result {
        Ok(Ok(reply)) => match write_reply(&mut env, &reply) {
            Ok(array) => array,
            Err(_) => {
                throw(&mut env, "jni", "the reply could not be passed back");
                std::ptr::null_mut()
            }
        },
        Ok(Err(error)) => {
            throw(&mut env, error.code, &error.message);
            std::ptr::null_mut()
        }
        Err(_) => {
            if !env.exception_check().unwrap_or(false) {
                throw(&mut env, "jni", "the call could not be read");
            }
            std::ptr::null_mut()
        }
    }
}

fn run_call(
    env: &mut JNIEnv<'_>,
    method: &JString<'_>,
    handle: jlong,
    args: &JString<'_>,
    secret: &JByteArray<'_>,
    blobs: &JObjectArray<'_>,
    now_ms: jlong,
) -> jni::errors::Result<Result<Reply, SessionError>> {
    let method: String = env.get_string(method)?.into();
    let args: String = env.get_string(args)?.into();
    let secret = bytes(env, secret)?;
    let blobs = read_blobs(env, blobs)?;
    let views: Vec<&[u8]> = blobs.iter().map(|b| b.as_slice()).collect();
    let call = Call {
        handle: handle.cast_unsigned(),
        args: &args,
        secret: &secret,
        blobs: &views,
        now_ms: now_ms.cast_unsigned(),
    };
    Ok(with_service(|service| service.call(&method, call)).unwrap_or_else(|| {
        Err(SessionError {
            code: "not_started",
            message: "the vault service has not started".to_owned(),
        })
    }))
}
