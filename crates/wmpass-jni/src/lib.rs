//! The Android side of [`wmpass_session`]: one call with bytes for secrets.
//!
//! `extern "system"` functions abort on a panic rather than unwind into the JVM, and
//! the release profile aborts anyway. The process dying with the vault in it is the
//! right way for a bug here to fail.

use std::sync::Mutex;

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jobjectArray};
use wmpass_core::format::{FactorKind, FactorOutput};
use wmpass_hwkey::HwError;
use wmpass_hwkey::otp::OtpHid;
use wmpass_hwkey::transport::{ApduDevice, CtapHid, CtapNfc, HidDevice};
use wmpass_session::{Call, FactorView, Reply, Service, SessionError, factor_of, factor_view};
use zeroize::Zeroizing;

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

type HwResult<T> = std::result::Result<T, HwError>;

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

/// `net(method, handle, args, secret, online, nowMs)`: anything that waits on a network. Runs
/// outside the lock on the service, so a slow server leaves the vault usable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_net<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    method: JString<'local>,
    handle: jlong,
    args: JString<'local>,
    secret: JByteArray<'local>,
    online: jboolean,
    now_ms: jlong,
) -> jobjectArray {
    let read = (|| -> jni::errors::Result<(String, String, Zeroizing<Vec<u8>>)> {
        Ok((env.get_string(&method)?.into(), env.get_string(&args)?.into(), bytes(&env, &secret)?))
    })();
    let Ok((method, args, secret)) = read else {
        throw(&mut env, "jni", "the call could not be read");
        return std::ptr::null_mut();
    };
    let result = wmpass_session::net::call(
        &SERVICE,
        &method,
        handle.cast_unsigned(),
        &args,
        &secret,
        online != 0,
        now_ms.cast_unsigned(),
    );
    finish(&mut env, result)
}

fn finish(env: &mut JNIEnv<'_>, result: Result<Reply, SessionError>) -> jobjectArray {
    match result {
        Ok(reply) => match write_reply(env, &reply) {
            Ok(array) => array,
            Err(_) => {
                throw(env, "jni", "the reply could not be passed back");
                std::ptr::null_mut()
            }
        },
        Err(error) => {
            throw(env, error.code, &error.message);
            std::ptr::null_mut()
        }
    }
}

/// Bytes through a Kotlin object, for a hardware key on USB or NFC.
struct Bridge<'a, 'local> {
    env: &'a mut JNIEnv<'local>,
    device: JObject<'local>,
}

impl Bridge<'_, '_> {
    fn exchange(&mut self, method: &str, input: &[u8]) -> HwResult<Vec<u8>> {
        let array = self.env.byte_array_from_slice(input).map_err(|_| HwError::Transport)?;
        let value = self
            .env
            .call_method(&self.device, method, "([B)[B", &[JValue::Object(&array)])
            .map_err(|_| {
                // A Kotlin exception here is a dropped key. It must not stay pending in the JVM.
                let _ = self.env.exception_clear();
                HwError::Transport
            })?
            .l()
            .map_err(|_| HwError::Transport)?;
        let out = JByteArray::from(value);
        if out.as_raw().is_null() {
            return Err(HwError::Transport);
        }
        let bytes = self.env.convert_byte_array(&out).map_err(|_| HwError::Transport)?;
        let _ = self.env.delete_local_ref(out);
        let _ = self.env.delete_local_ref(array);
        Ok(bytes)
    }
}

impl ApduDevice for Bridge<'_, '_> {
    fn transceive(&mut self, apdu: &[u8]) -> HwResult<Vec<u8>> {
        self.exchange("transceive", apdu)
    }
}

impl HidDevice for Bridge<'_, '_> {
    fn write(&mut self, report: &[u8; 64]) -> HwResult<()> {
        self.exchange("hidWrite", report).map(|_| ())
    }

    fn read(&mut self) -> HwResult<[u8; 64]> {
        let out = self.exchange("hidRead", &[])?;
        out.as_slice().try_into().map_err(|_| HwError::Transport)
    }
}

impl OtpHid for Bridge<'_, '_> {
    fn get_report(&mut self) -> HwResult<[u8; 8]> {
        let out = self.exchange("otpGet", &[])?;
        out.as_slice().try_into().map_err(|_| HwError::Transport)
    }

    fn set_report(&mut self, report: &[u8; 8]) -> HwResult<()> {
        self.exchange("otpSet", report).map(|_| ())
    }

    fn pause(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn hw_failure(error: HwError) -> SessionError {
    let code = match error {
        HwError::WrongKey => "wrong_key",
        HwError::NotConfirmed => "not_confirmed",
        HwError::Unsupported => "unsupported",
        HwError::PinRequired => "pin_required",
        _ => "hardware",
    };
    SessionError { code, message: error.to_string() }
}

/// What a hardware key operation is, and over what.
fn run_hardware(env: &mut JNIEnv<'_>, device: JObject<'_>, method: &str, factor: &str) -> Result<Reply, SessionError> {
    // SAFETY: the bridge is used only on this thread, for the length of this call.
    let mut local = unsafe { env.unsafe_clone() };
    let mut bridge = Bridge { env: &mut local, device };
    let wanted: Option<FactorView> = if factor.is_empty() {
        None
    } else {
        Some(serde_json::from_str(factor).map_err(|_| SessionError { code: "invalid", message: "bad factor".to_owned() })?)
    };
    let existing = wanted.as_ref().map(factor_of).transpose()?;
    let nonce = || wmpass_core::crypto::random_array::<8>().map_err(|_| HwError::Crypto);
    let run = |bridge: &mut Bridge<'_, '_>| -> HwResult<(FactorKind, FactorOutput)> {
        match method {
            "fido_nfc" | "fido_usb" => {
                if method == "fido_nfc" {
                    let mut transport = CtapNfc::open(BorrowedDevice(bridge))?;
                    let kind = match existing.clone() {
                        Some(kind) => kind,
                        None => wmpass_hwkey::ctap::enroll(&mut transport)?,
                    };
                    let output = wmpass_hwkey::ctap::derive(&mut transport, &kind)?;
                    Ok((kind, output))
                } else {
                    let mut transport = CtapHid::open(BorrowedDevice(bridge), nonce()?)?;
                    let kind = match existing.clone() {
                        Some(kind) => kind,
                        None => wmpass_hwkey::ctap::enroll(&mut transport)?,
                    };
                    let output = wmpass_hwkey::ctap::derive(&mut transport, &kind)?;
                    Ok((kind, output))
                }
            }
            "otp_nfc" | "otp_usb" => {
                let kind = match existing.clone() {
                    Some(kind) => kind,
                    None => wmpass_hwkey::otp::enroll(2)?,
                };
                let output = if method == "otp_nfc" {
                    wmpass_hwkey::otp::derive_nfc(bridge, &kind)?
                } else {
                    wmpass_hwkey::otp::derive_usb(bridge, &kind, 150)?
                };
                Ok((kind, output))
            }
            _ => Err(HwError::Protocol),
        }
    };
    let (kind, output) = run(&mut bridge).map_err(hw_failure)?;
    Ok(Reply {
        json: serde_json::to_string(&factor_view(&kind)).unwrap_or_default(),
        secret: Some(Zeroizing::new(output.to_vec())),
        blobs: Vec::new(),
    })
}

/// The transports take their device by value; this lends them the bridge.
struct BorrowedDevice<'b, 'a, 'local>(&'b mut Bridge<'a, 'local>);

impl ApduDevice for BorrowedDevice<'_, '_, '_> {
    fn transceive(&mut self, apdu: &[u8]) -> HwResult<Vec<u8>> {
        self.0.transceive(apdu)
    }
}

impl HidDevice for BorrowedDevice<'_, '_, '_> {
    fn write(&mut self, report: &[u8; 64]) -> HwResult<()> {
        HidDevice::write(self.0, report)
    }

    fn read(&mut self) -> HwResult<[u8; 64]> {
        HidDevice::read(self.0)
    }
}

/// `hardware(method, factorJson, device)`: enrols a new factor if `factorJson` is empty, and
/// returns its view and output. The device object is a `HwDevice` on the Kotlin side.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_hardware<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    method: JString<'local>,
    factor: JString<'local>,
    device: JObject<'local>,
) -> jobjectArray {
    let read = (|| -> jni::errors::Result<(String, String)> { Ok((env.get_string(&method)?.into(), env.get_string(&factor)?.into())) })();
    let Ok((method, factor)) = read else {
        throw(&mut env, "jni", "the call could not be read");
        return std::ptr::null_mut();
    };
    let result = run_hardware(&mut env, device, &method, &factor);
    finish(&mut env, result)
}
