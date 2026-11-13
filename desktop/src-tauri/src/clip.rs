//! The clipboard, on this side of the bridge, so a secret bound for it never reaches the page.
//! Marked to stay out of clipboard history, and cleared on a timer only if it is still ours.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use arboard::Clipboard;
use zeroize::Zeroizing;

/// On Linux the contents last only as long as this object does.
static BOARD: Mutex<Option<Clipboard>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The copy a timer is still waiting to clear, by its count and fingerprint, so quitting can clear
/// it: the timer's thread does not outlive the app. Only touched while the clipboard is held.
static PENDING: Mutex<Option<(u64, blake3::Hash)>> = Mutex::new(None);

fn with_board<T>(run: impl FnOnce(&mut Clipboard) -> Result<T, arboard::Error>) -> Result<T, String> {
    // A panic while it was held leaves nothing half-done here, and a poisoned lock would otherwise
    // stop every later copy and every timed clear.
    let mut guard = BOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.is_none() {
        *guard = Some(Clipboard::new().map_err(|e| e.to_string())?);
    }
    let board = guard.as_mut().ok_or("the clipboard is not available")?;
    run(board).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn set_hidden(board: &mut Clipboard, text: &str) -> Result<(), arboard::Error> {
    use arboard::SetExtApple;
    board.set().exclude_from_history().text(text)
}

#[cfg(target_os = "windows")]
fn set_hidden(board: &mut Clipboard, text: &str) -> Result<(), arboard::Error> {
    use arboard::SetExtWindows;
    board.set().exclude_from_cloud().exclude_from_history().exclude_from_monitoring().text(text)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn set_hidden(board: &mut Clipboard, text: &str) -> Result<(), arboard::Error> {
    use arboard::SetExtLinux;
    board.set().exclude_from_history().text(text)
}

/// Puts text on the clipboard. A sensitive value stays out of history and goes after the timer.
pub fn copy(text: &str, sensitive: bool, clear_after_seconds: u64) -> Result<(), String> {
    // Counted while the clipboard is held, so a timer that wakes now sees either the old text with
    // the old count or the new text with the new one, never the new text with the old count.
    let fingerprint = blake3::hash(text.as_bytes());
    let generation = with_board(|board| {
        if sensitive { set_hidden(board, text) } else { board.set_text(text) }?;
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        *pending() = (clear_after_seconds > 0).then_some((generation, fingerprint));
        Ok(generation)
    })?;
    if clear_after_seconds > 0 {
        std::thread::spawn(move || {
            wait_until_due(generation, clear_after_seconds);
            // Checked and cleared in one hold of the clipboard, so a copy made between the two
            // is never the one wiped. Only if it is still what was put there: never something
            // copied since.
            let _ = with_board(|board| clear_if_ours(board, generation, fingerprint));
        });
    }
    Ok(())
}

/// Waits out a clear timer by the wall clock. A plain sleep counts only time the machine is awake
/// (on a Mac the clock under it stops while asleep), so a password copied before closing the lid
/// would stay on the clipboard for the rest of the wait after waking. Short naps, each followed by
/// a look at the clock, let the clear run within a second of waking. The awake time counts as well,
/// so a wall clock set back cannot hold the clear off. Returns early once something else was copied.
fn wait_until_due(generation: u64, seconds: u64) {
    let wait = Duration::from_secs(seconds);
    let deadline = SystemTime::now() + wait;
    let started = Instant::now();
    loop {
        if GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let awake = started.elapsed();
        let left_by_wall = deadline.duration_since(SystemTime::now()).unwrap_or(Duration::ZERO);
        let left = left_by_wall.min(wait.saturating_sub(awake));
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_secs(1)));
    }
}

/// Clears the clipboard if it still holds the copy counted `generation`, with this fingerprint.
fn clear_if_ours(board: &mut Clipboard, generation: u64, fingerprint: blake3::Hash) -> Result<(), arboard::Error> {
    if GENERATION.load(Ordering::SeqCst) != generation {
        return Ok(());
    }
    let still_ours = board.get_text().map(Zeroizing::new).is_ok_and(|now| blake3::hash(now.as_bytes()) == fingerprint);
    if still_ours {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        *pending() = None;
        board.clear()?;
    }
    Ok(())
}

pub fn clear() -> Result<(), String> {
    with_board(|board| {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        *pending() = None;
        board.clear()
    })
}

/// For quitting: clears a copy whose timer has not run yet, if it is still on the clipboard.
/// Anything copied since, here or elsewhere, is left alone, as the timer would.
pub fn clear_pending() {
    let _ = with_board(|board| {
        let Some((generation, fingerprint)) = pending().take() else { return Ok(()) };
        clear_if_ours(board, generation, fingerprint)
    });
}

fn pending() -> std::sync::MutexGuard<'static, Option<(u64, blake3::Hash)>> {
    PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
