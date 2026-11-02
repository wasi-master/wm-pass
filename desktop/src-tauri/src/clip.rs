//! The clipboard, on this side of the bridge, so a secret bound for it never reaches the page.
//! Marked to stay out of clipboard history, and cleared on a timer only if it is still ours.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use arboard::Clipboard;
use zeroize::Zeroizing;

/// On Linux the contents last only as long as this object does.
static BOARD: Mutex<Option<Clipboard>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn with_board<T>(run: impl FnOnce(&mut Clipboard) -> Result<T, arboard::Error>) -> Result<T, String> {
    let mut guard = BOARD.lock().map_err(|_| "the clipboard is busy".to_owned())?;
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
    with_board(|board| if sensitive { set_hidden(board, text) } else { board.set_text(text) })?;
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if clear_after_seconds > 0 {
        let fingerprint = blake3::hash(text.as_bytes());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(clear_after_seconds));
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            // Only if it is still what was put there: never wipe something copied since.
            let still_ours = with_board(|board| board.get_text()).map(Zeroizing::new).is_ok_and(|now| blake3::hash(now.as_bytes()) == fingerprint);
            if still_ours {
                let _ = clear();
            }
        });
    }
    Ok(())
}

pub fn clear() -> Result<(), String> {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    with_board(Clipboard::clear)
}
