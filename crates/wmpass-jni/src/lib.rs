//! The Android side of [`wmpass_session`]: one call with bytes for secrets.
//!
//! Every exported function runs its body under `catch_unwind`, so a bug that panics comes back
//! to Kotlin as a `NativeFailure` with the code `internal` rather than unwinding into the JVM or
//! ending the process. The service it panicked in may be half changed, so it is never used
//! again: it is dropped, which wipes the keys of every open vault, and a fresh one takes its
//! place. Later calls on the old handles answer `locked`, and the vault opens again from its
//! file, which a save only ever replaces whole. A call that reaches the lock a panic poisoned,
//! before that, answers `internal` and does the same.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JFloatArray, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jobjectArray};
use wmpass_cardscan::{Crop, Frame, Month, Reader, Tally, TextLine};
use wmpass_core::format::{FactorKind, FactorOutput};
use wmpass_hwkey::HwError;
use wmpass_hwkey::otp::OtpHid;
use wmpass_hwkey::transport::{ApduDevice, CtapHid, CtapNfc, HidDevice};
use wmpass_session::{Call, FactorView, INTERNAL, Reply, Service, SessionError, factor_of, factor_view, internal_error};
use zeroize::Zeroizing;

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);
/// The device number `init` was given, for the fresh service that replaces one a panic stopped.
static DEVICE: AtomicU32 = AtomicU32::new(0);

type HwResult<T> = std::result::Result<T, HwError>;

fn not_started() -> SessionError {
    SessionError {
        code: "not_started",
        message: "the vault service has not started".to_owned(),
    }
}

/// Throws away a service a panic may have left half changed, wiping its keys as it drops, and
/// starts a fresh one in its place. Nothing of the old one is read on the way.
fn replace_service() {
    let mut guard = SERVICE.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_some() {
        let old = guard.replace(Service::new(DEVICE.load(Ordering::SeqCst)));
        // A drop that panics too still leaves the fresh service in place.
        let _ = catch_unwind(AssertUnwindSafe(move || drop(old)));
    }
    drop(guard);
    SERVICE.clear_poison();
}

/// The service's lock. A poisoned one is treated as locked: the service is replaced, and the
/// caller is told with `internal`.
fn lock_service() -> Result<MutexGuard<'static, Option<Service>>, SessionError> {
    match SERVICE.lock() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => {
            drop(poisoned);
            replace_service();
            Err(internal_error())
        }
    }
}

fn with_service<T>(run: impl FnOnce(&mut Service) -> T) -> Result<T, SessionError> {
    let mut guard = lock_service()?;
    guard.as_mut().map(run).ok_or_else(not_started)
}

fn throw(env: &mut JNIEnv<'_>, code: &str, message: &str) {
    // A failed JNI call can leave a Java exception pending, and nothing else may be thrown
    // on top of one.
    if env.exception_check().unwrap_or(true) {
        let _ = env.exception_clear();
    }
    let text = format!("{code}\n{message}");
    let _ = env.throw_new("com/wasimaster/wmpass/core/NativeFailure", text);
}

/// Runs an exported function's body. A panic replaces the service, becomes a `NativeFailure`
/// with the code `internal`, and `fallback` is returned, so nothing unwinds into the JVM.
fn guarded<'local, T>(env: &mut JNIEnv<'local>, fallback: T, run: impl FnOnce(&mut JNIEnv<'local>) -> T) -> T {
    match catch_unwind(AssertUnwindSafe(|| run(env))) {
        Ok(value) => value,
        Err(_) => {
            replace_service();
            let failure = internal_error();
            throw(env, failure.code, &failure.message);
            fallback
        }
    }
}

/// Runs a card reader export. A panic there touches no vault: it drops the scan, and Kotlin hears
/// `card`, as for a scan that would not read.
fn guarded_card<'local, T>(env: &mut JNIEnv<'local>, fallback: T, run: impl FnOnce(&mut JNIEnv<'local>) -> T) -> T {
    match catch_unwind(AssertUnwindSafe(|| run(env))) {
        Ok(value) => value,
        Err(_) => {
            drop_cards();
            throw(env, "card", CARD_FAILED);
            fallback
        }
    }
}

const CARD_FAILED: &str = "the card reader hit an internal error. Close it and scan again";

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
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_init(mut env: JNIEnv<'_>, _class: JClass<'_>, device: jint) {
    guarded(&mut env, (), |_| {
        DEVICE.store(device.cast_unsigned(), Ordering::SeqCst);
        if let Ok(mut guard) = lock_service()
            && guard.is_none()
        {
            *guard = Some(Service::new(device.cast_unsigned()));
        }
    });
}

/// `lockAll()`: drops every open vault, wiping its keys.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_lockAll(mut env: JNIEnv<'_>, _class: JClass<'_>) {
    // A poisoned lock replaces the service, which locks every vault too.
    guarded(&mut env, (), |_| {
        if let Ok(mut guard) = lock_service()
            && let Some(service) = guard.as_mut()
        {
            service.lock_all();
        }
    });
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
    guarded(&mut env, std::ptr::null_mut(), |env| {
        match run_call(env, &method, handle, &args, &secret, &blobs, now_ms) {
            Ok(result) => finish(env, result),
            Err(_) => {
                if !env.exception_check().unwrap_or(false) {
                    throw(env, "jni", "the call could not be read");
                }
                std::ptr::null_mut()
            }
        }
    })
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
    Ok(with_service(|service| service.call(&method, call)).and_then(|result| result))
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
    guarded(&mut env, std::ptr::null_mut(), |env| {
        let read = (|| -> jni::errors::Result<(String, String, Zeroizing<Vec<u8>>)> {
            Ok((env.get_string(&method)?.into(), env.get_string(&args)?.into(), bytes(env, &secret)?))
        })();
        let Ok((method, args, secret)) = read else {
            throw(env, "jni", "the call could not be read");
            return std::ptr::null_mut();
        };
        // The network calls take the lock as they go and answer `internal` on a poisoned one,
        // leaving it to this boundary to replace the service.
        if let Err(failure) = with_service(|_| ())
            && failure.code == INTERNAL
        {
            return finish(env, Err(failure));
        }
        let result = wmpass_session::net::call(
            &SERVICE,
            &method,
            handle.cast_unsigned(),
            &args,
            &secret,
            online != 0,
            now_ms.cast_unsigned(),
        );
        // Another thread's panic poisoned the lock while this call waited on the network.
        if SERVICE.is_poisoned() {
            replace_service();
        }
        finish(env, result)
    })
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
        exchange_with(self, method, input)
    }
}

/// The JNI steps of one exchange, apart from the order and the clean-up, so that a test can
/// check without a JVM that every local reference made is deleted on every path. A key
/// session makes hundreds of exchanges inside one native call, where nothing frees local
/// references until it returns.
trait Locals {
    type Array;
    fn new_array(&mut self, bytes: &[u8]) -> HwResult<Self::Array>;
    /// The method's answer; None when it answered null.
    fn call_device(&mut self, method: &str, input: &Self::Array) -> HwResult<Option<Self::Array>>;
    fn read_array(&mut self, array: &Self::Array) -> HwResult<Vec<u8>>;
    fn delete_array(&mut self, array: Self::Array);
}

/// Sends `input` to the device's `method` and returns its answer, deleting both arrays
/// whatever happens.
fn exchange_with<L: Locals>(jni: &mut L, method: &str, input: &[u8]) -> HwResult<Vec<u8>> {
    let array = jni.new_array(input)?;
    let answer = jni.call_device(method, &array);
    jni.delete_array(array);
    let out = answer?.ok_or(HwError::Transport)?;
    let bytes = jni.read_array(&out);
    jni.delete_array(out);
    bytes
}

impl<'local> Locals for Bridge<'_, 'local> {
    type Array = JByteArray<'local>;

    fn new_array(&mut self, bytes: &[u8]) -> HwResult<Self::Array> {
        self.env.byte_array_from_slice(bytes).map_err(|_| HwError::Transport)
    }

    fn call_device(&mut self, method: &str, input: &Self::Array) -> HwResult<Option<Self::Array>> {
        let value = self
            .env
            .call_method(&self.device, method, "([B)[B", &[JValue::Object(input)])
            .map_err(|_| {
                // A Kotlin exception here is a dropped key. It must not stay pending in the JVM.
                let _ = self.env.exception_clear();
                HwError::Transport
            })?
            // Not an object is not a reference either: nothing to delete.
            .l()
            .map_err(|_| HwError::Transport)?;
        Ok((!value.as_raw().is_null()).then(|| JByteArray::from(value)))
    }

    fn read_array(&mut self, array: &Self::Array) -> HwResult<Vec<u8>> {
        self.env.convert_byte_array(array).map_err(|_| HwError::Transport)
    }

    fn delete_array(&mut self, array: Self::Array) {
        let _ = self.env.delete_local_ref(array);
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
        HwError::PinBlocked => "pin_blocked",
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
    guarded(&mut env, std::ptr::null_mut(), |env| {
        let read = (|| -> jni::errors::Result<(String, String)> { Ok((env.get_string(&method)?.into(), env.get_string(&factor)?.into())) })();
        let Ok((method, factor)) = read else {
            throw(env, "jni", "the call could not be read");
            return std::ptr::null_mut();
        };
        let result = run_hardware(env, device, &method, &factor);
        finish(env, result)
    })
}

/// The card reader while its scanner is open: the model when frames are read here, and what
/// the frames so far read.
struct CardScan {
    reader: Option<Reader>,
    tally: Tally,
}

static CARDS: Mutex<Option<CardScan>> = Mutex::new(None);

/// `cardOpen(model)`: starts a card scan and forgets earlier reads. With the model's bytes,
/// frames are read here (`cardFrame`); with null, a text recognizer on the Kotlin side reads
/// them and passes its lines (`cardText`). False when the model will not load.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_cardOpen<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    model: JByteArray<'local>,
) -> jboolean {
    guarded_card(&mut env, 0, |env| {
        let reader = if model.as_raw().is_null() {
            None
        } else {
            let Ok(model) = env.convert_byte_array(&model) else { return 0 };
            let Ok(reader) = Reader::new(&model) else { return 0 };
            Some(reader)
        };
        // A new scan replaces everything, so a scan that panicked leaves nothing to distrust.
        *CARDS.lock().unwrap_or_else(PoisonError::into_inner) = Some(CardScan { reader, tally: Tally::default() });
        CARDS.clear_poison();
        1
    })
}

/// The open card scan, or a `NativeFailure` when an earlier frame panicked mid-read.
fn with_cards(env: &mut JNIEnv<'_>, run: impl FnOnce(&mut JNIEnv<'_>, &mut CardScan) -> jobjectArray) -> jobjectArray {
    let Ok(mut guard) = CARDS.lock() else {
        throw(env, "card", CARD_FAILED);
        return std::ptr::null_mut();
    };
    match guard.as_mut() {
        Some(scan) => run(env, scan),
        None => std::ptr::null_mut(),
    }
}

/// What the frames agree on so far: null until a number, then `{ByteArray number, String?
/// expiry, String? name}`. The caller wipes the number's array once it has used it.
fn card_reply(env: &mut JNIEnv<'_>, tally: &Tally) -> jobjectArray {
    let Some(number) = tally.number() else { return std::ptr::null_mut() };
    let expiry = tally.expiry().map(|e| e.to_string());
    let reply = (|| -> jni::errors::Result<jobjectArray> {
        let object = env.find_class("java/lang/Object")?;
        let out = env.new_object_array(3, object, JObject::null())?;
        let digits = env.byte_array_from_slice(number.as_bytes())?;
        env.set_object_array_element(&out, 0, digits)?;
        if let Some(expiry) = expiry {
            let text = env.new_string(expiry)?;
            env.set_object_array_element(&out, 1, text)?;
        }
        if let Some(name) = tally.name() {
            let text = env.new_string(name)?;
            env.set_object_array_element(&out, 2, text)?;
        }
        Ok(out.into_raw())
    })();
    reply.unwrap_or(std::ptr::null_mut())
}

/// `cardFrame(pixels, width, height, rowStride, rotation, crop, nowMs)`: reads one RGBA frame
/// with the model. Returns what [`card_reply`] does.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_cardFrame<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    pixels: JByteArray<'local>,
    width: jint,
    height: jint,
    row_stride: jint,
    rotation: jint,
    crop: JFloatArray<'local>,
    now_ms: jlong,
) -> jobjectArray {
    guarded_card(&mut env, std::ptr::null_mut(), |env| {
        // The frame shows the card, so the copy is wiped like a secret.
        let read = (|| -> jni::errors::Result<(Zeroizing<Vec<u8>>, [f32; 4])> {
            let mut edges = [0f32; 4];
            env.get_float_array_region(&crop, 0, &mut edges)?;
            Ok((Zeroizing::new(env.convert_byte_array(&pixels)?), edges))
        })();
        let Ok((pixels, [left, top, right, bottom])) = read else {
            throw(env, "jni", "the frame could not be read");
            return std::ptr::null_mut();
        };
        let (Ok(width), Ok(height), Ok(row_stride), Ok(rotation)) =
            (usize::try_from(width), usize::try_from(height), usize::try_from(row_stride), u16::try_from(rotation))
        else {
            return std::ptr::null_mut();
        };
        let frame = Frame { pixels: &pixels, width, height, row_stride, rotation };
        let crop = Crop { left, top, right, bottom };
        with_cards(env, |env, scan| {
            let Some(reader) = &scan.reader else { return std::ptr::null_mut() };
            // A frame that will not run is only a frame missed; the next one may.
            if let Ok(card) = reader.read(&frame, crop, Month::at(now_ms.cast_unsigned())) {
                scan.tally.add(card);
            }
            card_reply(env, &scan.tally)
        })
    })
}

/// `cardText(lines, boxes, nowMs)`: one frame as a text recognizer read it. `boxes` holds each
/// line's left and top, as fractions of the card's window. Returns what [`card_reply`] does.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_cardText<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    lines: JObjectArray<'local>,
    boxes: JFloatArray<'local>,
    now_ms: jlong,
) -> jobjectArray {
    guarded_card(&mut env, std::ptr::null_mut(), |env| card_text(env, &lines, &boxes, now_ms))
}

fn card_text(env: &mut JNIEnv<'_>, lines: &JObjectArray<'_>, boxes: &JFloatArray<'_>, now_ms: jlong) -> jobjectArray {
    let read = (|| -> jni::errors::Result<Vec<TextLine>> {
        let count = env.get_array_length(lines)?;
        let mut corners = vec![0f32; usize::try_from(count).unwrap_or(0) * 2];
        env.get_float_array_region(boxes, 0, &mut corners)?;
        let mut out = Vec::with_capacity(corners.len() / 2);
        for index in 0..count {
            let item = JString::from(env.get_object_array_element(lines, index)?);
            let text: String = env.get_string(&item)?.into();
            env.delete_local_ref(item)?;
            let at = usize::try_from(index).unwrap_or(0) * 2;
            out.push(TextLine { text, left: corners[at], top: corners[at + 1] });
        }
        Ok(out)
    })();
    let Ok(lines) = read else {
        throw(env, "jni", "the text could not be read");
        return std::ptr::null_mut();
    };
    with_cards(env, |env, scan| {
        scan.tally.add(wmpass_cardscan::card::read_text(&lines, Month::at(now_ms.cast_unsigned())));
        card_reply(env, &scan.tally)
    })
}

/// `cardClose()`: drops the card reader, wiping every number it read.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wasimaster_wmpass_core_Native_cardClose(mut env: JNIEnv<'_>, _class: JClass<'_>) {
    guarded_card(&mut env, (), |_| {
        drop_cards();
    });
}

/// Drops the card reader and every number it read, even after a panic mid-read.
fn drop_cards() {
    *CARDS.lock().unwrap_or_else(PoisonError::into_inner) = None;
    CARDS.clear_poison();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local references as a JVM keeps them: counted, and each step can be made to fail.
    #[derive(Default)]
    struct Counted {
        live: usize,
        fail_new: bool,
        fail_call: bool,
        null_answer: bool,
        fail_read: bool,
    }

    impl Locals for Counted {
        type Array = Vec<u8>;

        fn new_array(&mut self, bytes: &[u8]) -> HwResult<Vec<u8>> {
            if self.fail_new {
                return Err(HwError::Transport);
            }
            self.live += 1;
            Ok(bytes.to_vec())
        }

        fn call_device(&mut self, _: &str, input: &Vec<u8>) -> HwResult<Option<Vec<u8>>> {
            if self.fail_call {
                return Err(HwError::Transport);
            }
            if self.null_answer {
                return Ok(None);
            }
            self.live += 1;
            Ok(Some(input.iter().rev().copied().collect()))
        }

        fn read_array(&mut self, array: &Vec<u8>) -> HwResult<Vec<u8>> {
            if self.fail_read { Err(HwError::Transport) } else { Ok(array.clone()) }
        }

        fn delete_array(&mut self, _: Vec<u8>) {
            self.live -= 1;
        }
    }

    #[test]
    fn an_exchange_deletes_its_local_references_on_every_path() {
        let mut ok = Counted::default();
        assert_eq!(exchange_with(&mut ok, "transceive", &[1, 2, 3]), Ok(vec![3, 2, 1]));
        assert_eq!(ok.live, 0);
        for mut failing in [
            Counted { fail_new: true, ..Counted::default() },
            Counted { fail_call: true, ..Counted::default() },
            Counted { null_answer: true, ..Counted::default() },
            Counted { fail_read: true, ..Counted::default() },
        ] {
            assert_eq!(exchange_with(&mut failing, "transceive", &[1]), Err(HwError::Transport));
            assert_eq!(failing.live, 0, "a reference outlived a failed exchange");
        }
    }

    #[test]
    fn a_blocked_pin_reaches_kotlin_as_its_own_code() {
        let failure = hw_failure(HwError::PinBlocked);
        assert_eq!(failure.code, "pin_blocked");
        assert!(failure.message.contains("too many wrong PINs"));
        assert_eq!(hw_failure(HwError::PinRequired).code, "pin_required");
    }

    /// Every test here that starts or panics the one service takes its turn.
    static TURN: Mutex<()> = Mutex::new(());

    fn open_vault() -> u64 {
        let args = r#"{"name":"Mine","kdf":{"m_kib":65536,"t":3,"p":1}}"#;
        let reply = with_service(|service| service.call("vault.create", Call { args, secret: b"correct horse", now_ms: 1, ..Call::default() }));
        let Ok(Ok(reply)) = reply else { panic!("the vault opens") };
        let opened: serde_json::Value = serde_json::from_str(&reply.json).unwrap_or_default();
        opened["handle"].as_u64().unwrap_or(0)
    }

    fn info(handle: u64) -> Result<(), &'static str> {
        match with_service(|service| service.call("vault.info", Call { handle, ..Call::default() })) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(failure)) | Err(failure) => Err(failure.code),
        }
    }

    #[test]
    fn a_panic_in_a_call_locks_every_vault_and_the_service_goes_on() {
        let _turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
        DEVICE.store(1, Ordering::SeqCst);
        *SERVICE.lock().unwrap_or_else(PoisonError::into_inner) = Some(Service::new(1));
        SERVICE.clear_poison();
        let handle = open_vault();
        assert_eq!(info(handle), Ok(()));
        // As `guarded` meets one: a panic inside a call, with the service's lock held.
        let caught = catch_unwind(AssertUnwindSafe(|| with_service(|service| service.call("debug.panic", Call::default()))));
        assert!(caught.is_err(), "debug.panic panics in a test build");
        assert!(SERVICE.is_poisoned());
        // The first call to reach the poisoned lock is told, and replaces the service.
        assert_eq!(info(handle), Err(INTERNAL));
        assert!(!SERVICE.is_poisoned());
        let fresh = SERVICE.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(Service::open_handles);
        assert_eq!(fresh, Some(Vec::new()), "a fresh service: the old one went, and its keys with it");
        assert_eq!(info(handle), Err("locked"), "the old handle is locked");
        let again = open_vault();
        assert_eq!(info(again), Ok(()), "and a vault opens again");
    }

    #[test]
    fn the_boundary_replaces_the_service_itself() {
        let _turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
        DEVICE.store(7, Ordering::SeqCst);
        *SERVICE.lock().unwrap_or_else(PoisonError::into_inner) = Some(Service::new(7));
        SERVICE.clear_poison();
        let handle = open_vault();
        let _ = catch_unwind(AssertUnwindSafe(|| with_service(|_| panic!("mid-change"))));
        // What `guarded` does once it has caught the panic, before it throws `internal`.
        replace_service();
        assert!(!SERVICE.is_poisoned());
        assert_eq!(info(handle), Err("locked"));
    }
}
