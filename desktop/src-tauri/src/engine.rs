//! The session, behind one lock. A slow network call does not hold it.
//!
//! Every way into the session goes through here, from the page's calls to the extension's
//! bridge, the SSH agent, auto-type and the widgets, and each runs under `catch_unwind`. A bug
//! that panics in the session leaves it possibly half changed, so it is never used again: it is
//! dropped, which wipes the keys of every open vault, and a fresh one takes its place. The call
//! answers `internal`, the windows are told, and the app goes on, locked. The vault files are
//! whole, as a save replaces a file in one step.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use wmpass_session::{Call, INTERNAL, Reply, Service, SessionError, internal_error, net, wire};
use zeroize::Zeroizing;

type Told = Box<dyn Fn() + Send + Sync>;

/// What the app shares between its commands.
#[derive(Clone)]
pub struct Engine {
    service: Arc<Mutex<Option<Service>>>,
    device: u32,
    /// Called after an internal error replaced the session, from whichever thread met it.
    on_internal: Arc<OnceLock<Told>>,
    /// The vault open in the main window, or zero. The SSH agent and the palette use it.
    pub current: Arc<AtomicU64>,
    pub guard: Arc<LockGuard>,
}

/// When to lock by itself. This runs here and not in the page, because a page behind other windows
/// can be suspended, and a vault must not stay open because its timer stopped.
#[derive(Debug)]
pub struct LockGuard {
    last_active_ms: AtomicU64,
    /// Seconds without activity, or a negative number for never.
    after_seconds: AtomicI64,
    on_sleep: AtomicBool,
}

impl LockGuard {
    pub fn active(&self) {
        self.last_active_ms.store(now_ms(), Ordering::SeqCst);
    }

    pub fn set(&self, after_seconds: i64, on_sleep: bool) {
        self.after_seconds.store(after_seconds, Ordering::SeqCst);
        self.on_sleep.store(on_sleep, Ordering::SeqCst);
    }

    /// When the vault will lock by itself if nothing happens before then, or `None` for never.
    pub fn deadline_ms(&self) -> Option<u64> {
        let after = u64::try_from(self.after_seconds.load(Ordering::SeqCst)).ok()?;
        Some(self.last_active_ms.load(Ordering::SeqCst).saturating_add(after.saturating_mul(1000)))
    }

    /// Whether the time is up, given when this last looked.
    fn due(&self, now: u64, last_tick: u64) -> bool {
        // The clock jumping far ahead of this loop's own pace means the machine was asleep.
        let slept = now.saturating_sub(last_tick) > SLEEP_GAP_MS;
        if slept && self.on_sleep.load(Ordering::SeqCst) {
            return true;
        }
        let after = self.after_seconds.load(Ordering::SeqCst);
        after >= 0 && now.saturating_sub(self.last_active_ms.load(Ordering::SeqCst)) > u64::try_from(after).unwrap_or(0).saturating_mul(1000)
    }
}

const SLEEP_GAP_MS: u64 = 30_000;

impl Engine {
    pub fn new(device: u32) -> Self {
        Self {
            service: Arc::new(Mutex::new(Some(Service::new(device)))),
            device,
            on_internal: Arc::new(OnceLock::new()),
            current: Arc::new(AtomicU64::new(0)),
            guard: Arc::new(LockGuard { last_active_ms: AtomicU64::new(now_ms()), after_seconds: AtomicI64::new(300), on_sleep: AtomicBool::new(true) }),
        }
    }

    /// Watches the clock for as long as the app runs, and calls `locked` after locking.
    pub fn watch(&self, locked: impl Fn() + Send + 'static) {
        let engine = self.clone();
        std::thread::spawn(move || {
            let mut last_tick = now_ms();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let now = now_ms();
                if engine.current.load(Ordering::SeqCst) != 0 && engine.guard.due(now, last_tick) {
                    engine.lock_all();
                    locked();
                }
                last_tick = now;
            }
        });
    }

    /// What to do once an internal error has replaced the session: tell the windows, the tray
    /// and the widgets. Set once, when the app starts.
    pub fn on_internal(&self, told: impl Fn() + Send + Sync + 'static) {
        let _ = self.on_internal.set(Box::new(told));
    }

    /// The session's lock. One a panic poisoned guards a session that may be half changed: it is
    /// treated as locked, the session is replaced, and the caller hears `internal`.
    fn service(&self) -> Result<MutexGuard<'_, Option<Service>>, SessionError> {
        match self.service.lock() {
            Ok(guard) => Ok(guard),
            Err(poisoned) => {
                drop(poisoned);
                self.internal();
                Err(internal_error())
            }
        }
    }

    /// Throws the session away after a panic and starts a fresh one. Dropping the old one wipes
    /// every key it held; nothing in it is read on the way. Then the app is told.
    fn internal(&self) {
        {
            let mut guard = self.service.lock().unwrap_or_else(PoisonError::into_inner);
            let old = guard.replace(Service::new(self.device));
            // A drop that panics as well still leaves the fresh session in place.
            let _ = catch_unwind(AssertUnwindSafe(move || drop(old)));
        }
        self.service.clear_poison();
        self.current.store(0, Ordering::SeqCst);
        if let Some(told) = self.on_internal.get() {
            let _ = catch_unwind(AssertUnwindSafe(told));
        }
    }

    /// Runs `run` so that a panic in it costs the session, not the app.
    fn guarded<T>(&self, run: impl FnOnce() -> Result<T, SessionError>) -> Result<T, SessionError> {
        match catch_unwind(AssertUnwindSafe(run)) {
            Ok(result) => {
                // A call on the network that found the lock poisoned by another thread's panic:
                // whichever reaches here first replaces the session.
                if matches!(&result, Err(failure) if failure.code == INTERNAL) && self.service.is_poisoned() {
                    self.internal();
                }
                result
            }
            Err(_) => {
                self.internal();
                Err(internal_error())
            }
        }
    }

    pub fn lock_all(&self) {
        let _ = self.guarded(|| {
            if let Some(service) = self.service()?.as_mut() {
                service.lock_all();
            }
            Ok(())
        });
        self.current.store(0, Ordering::SeqCst);
    }

    /// Runs one request as it came from the page.
    pub fn run(&self, frame: &[u8]) -> Result<Reply, SessionError> {
        self.guarded(|| {
            let request = wire::parse_request(frame)?;
            if net::is_net_method(&request.method) {
                return net::call(&self.service, &request.method, request.handle, &request.args, request.secret, request.online, request.now_ms);
            }
            self.call_once(&request.method, request.handle, &request.args, request.secret, &request.blobs, request.now_ms)
        })
    }

    /// One call on the session itself.
    pub fn call(&self, method: &str, handle: u64, args: &str, secret: &[u8], blobs: &[&[u8]], now_ms: u64) -> Result<Reply, SessionError> {
        self.guarded(|| self.call_once(method, handle, args, secret, blobs, now_ms))
    }

    fn call_once(&self, method: &str, handle: u64, args: &str, secret: &[u8], blobs: &[&[u8]], now_ms: u64) -> Result<Reply, SessionError> {
        let mut guard = self.service()?;
        let service = guard.as_mut().ok_or(SessionError { code: "not_started", message: "the vault service has not started".to_owned() })?;
        service.call(method, Call { handle, args, secret, blobs, now_ms })
    }

    /// Handles still open, for the tests.
    #[cfg(test)]
    fn open_handles(&self) -> Vec<u64> {
        self.service.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(Service::open_handles).unwrap_or_default()
    }

    pub fn run_frame(&self, frame: &[u8]) -> Zeroizing<Vec<u8>> {
        wire::reply_frame(&self.run(frame))
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(after: i64, on_sleep: bool, last_active_ms: u64) -> LockGuard {
        LockGuard { last_active_ms: AtomicU64::new(last_active_ms), after_seconds: AtomicI64::new(after), on_sleep: AtomicBool::new(on_sleep) }
    }

    #[test]
    fn the_timer_runs_out() {
        let g = guard(300, true, 1_000_000);
        assert!(!g.due(1_000_000 + 299_000, 1_000_000 + 298_000));
        assert!(g.due(1_000_000 + 301_000, 1_000_000 + 300_000));
    }

    #[test]
    fn never_means_never_but_sleep_still_counts() {
        let g = guard(-1, true, 0);
        assert!(!g.due(100_000_000, 100_000_000 - 1000));
        assert!(g.due(100_000_000, 100_000_000 - 3_600_000));
        let g = guard(-1, false, 0);
        assert!(!g.due(100_000_000, 100_000_000 - 3_600_000));
    }

    #[test]
    fn a_huge_timeout_waits_instead_of_overflowing() {
        let g = guard(i64::MAX, false, 1_000);
        assert_eq!(g.deadline_ms(), Some(u64::MAX));
        assert!(!g.due(2_000, 1_500));
    }

    fn frame(method: &str, handle: u64, args: &str, secret: &[u8]) -> Vec<u8> {
        let head = format!(r#"{{"method":"{method}","handle":{handle},"args":{args},"now_ms":1}}"#);
        let mut out = Vec::new();
        for run in [head.as_bytes(), secret] {
            out.extend_from_slice(&u32::try_from(run.len()).unwrap_or(0).to_le_bytes());
            out.extend_from_slice(run);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    #[test]
    fn a_panic_in_the_session_locks_it_and_the_app_goes_on() {
        let engine = Engine::new(1);
        let told = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&told);
        engine.on_internal(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        let created = engine.run(&frame("vault.create", 0, r#"{"name":"Mine","kdf":{"m_kib":65536,"t":3,"p":1}}"#, b"correct horse"));
        let Ok(created) = created else { panic!("the vault opens") };
        let handle = serde_json::from_str::<serde_json::Value>(&created.json).unwrap_or_default()["handle"].as_u64().unwrap_or(0);
        engine.current.store(handle, Ordering::SeqCst);
        assert!(engine.call("vault.info", handle, "{}", &[], &[], 1).is_ok());
        // debug.panic is in debug builds and tests only.
        let failure = engine.run(&frame("debug.panic", 0, "{}", b"")).err().map(|failure| failure.code);
        assert_eq!(failure, Some(INTERNAL), "the call is told, and nothing unwinds out of it");
        assert_eq!(told.load(Ordering::SeqCst), 1, "the windows are told");
        assert_eq!(engine.current.load(Ordering::SeqCst), 0, "nothing is open for the agent or the palette");
        assert!(engine.open_handles().is_empty(), "the old session went, and its keys with it");
        let after = engine.call("vault.info", handle, "{}", &[], &[], 1).err().map(|failure| failure.code);
        assert_eq!(after, Some("locked"), "the vault that was open is locked");
        assert!(engine.run(&frame("vault.create", 0, r#"{"name":"Again","kdf":{"m_kib":65536,"t":3,"p":1}}"#, b"x")).is_ok(), "and the session works again");
    }

    #[test]
    fn a_lock_poisoned_elsewhere_is_treated_as_locked() {
        let engine = Engine::new(1);
        let service = Arc::clone(&engine.service);
        let _ = std::thread::spawn(move || {
            let _held = service.lock();
            panic!("mid-change");
        })
        .join();
        assert!(engine.service.is_poisoned());
        assert_eq!(engine.call("kinds", 0, "{}", &[], &[], 1).err().map(|failure| failure.code), Some(INTERNAL));
        assert!(!engine.service.is_poisoned());
        assert!(engine.call("kinds", 0, "{}", &[], &[], 1).is_ok());
        // A call on the network meets the same.
        let service = Arc::clone(&engine.service);
        let _ = std::thread::spawn(move || {
            let _held = service.lock();
            panic!("mid-change");
        })
        .join();
        assert_eq!(engine.run(&frame("remote.sync", 0, "{}", b"")).err().map(|failure| failure.code), Some(INTERNAL));
        assert!(!engine.service.is_poisoned());
    }

    #[test]
    fn activity_starts_it_again() {
        let g = guard(1, false, 0);
        g.active();
        assert!(!g.due(now_ms(), now_ms()));
    }
}
