//! The session, behind one lock. A slow network call does not hold it.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wmpass_session::{Call, Reply, Service, SessionError, net, wire};
use zeroize::Zeroizing;

/// What the app shares between its commands.
#[derive(Clone)]
pub struct Engine {
    service: Arc<Mutex<Option<Service>>>,
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

    /// Whether the time is up, given when this last looked.
    fn due(&self, now: u64, last_tick: u64) -> bool {
        // The clock jumping far ahead of this loop's own pace means the machine was asleep.
        let slept = now.saturating_sub(last_tick) > SLEEP_GAP_MS;
        if slept && self.on_sleep.load(Ordering::SeqCst) {
            return true;
        }
        let after = self.after_seconds.load(Ordering::SeqCst);
        after >= 0 && now.saturating_sub(self.last_active_ms.load(Ordering::SeqCst)) > u64::try_from(after).unwrap_or(0) * 1000
    }
}

const SLEEP_GAP_MS: u64 = 30_000;

impl Engine {
    pub fn new(device: u32) -> Self {
        Self {
            service: Arc::new(Mutex::new(Some(Service::new(device)))),
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

    pub fn lock(&self) -> MutexGuard<'_, Option<Service>> {
        self.service.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn lock_all(&self) {
        if let Some(service) = self.lock().as_mut() {
            service.lock_all();
        }
        self.current.store(0, Ordering::SeqCst);
    }

    /// Runs one request as it came from the page.
    pub fn run(&self, frame: &[u8]) -> Result<Reply, SessionError> {
        let request = wire::parse_request(frame)?;
        if net::is_net_method(&request.method) {
            return net::call(&self.service, &request.method, request.handle, &request.args, request.secret, request.online, request.now_ms);
        }
        self.call(&request.method, request.handle, &request.args, request.secret, &request.blobs, request.now_ms)
    }

    /// One call on the session itself.
    pub fn call(&self, method: &str, handle: u64, args: &str, secret: &[u8], blobs: &[&[u8]], now_ms: u64) -> Result<Reply, SessionError> {
        let mut guard = self.lock();
        let service = guard.as_mut().ok_or(SessionError { code: "not_started", message: "the vault service has not started".to_owned() })?;
        service.call(method, Call { handle, args, secret, blobs, now_ms })
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
    fn activity_starts_it_again() {
        let g = guard(1, false, 0);
        g.active();
        assert!(!g.due(now_ms(), now_ms()));
    }
}
