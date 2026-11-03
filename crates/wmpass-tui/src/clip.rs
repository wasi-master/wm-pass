//! The clipboard. A secret is marked to stay out of clipboard history and cleared on a timer,
//! only if it is still what was put there. Over SSH, with no clipboard to reach, the copy goes
//! to the terminal as an OSC 52 sequence, which most terminals hand to the local clipboard.

use std::io::Write;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use base64::Engine;
use zeroize::Zeroizing;

/// How a copy was made, which decides how it is cleared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The system clipboard.
    System,
    /// An escape sequence to the terminal.
    Terminal,
}

/// A secret waiting to be cleared.
struct Pending {
    due: Instant,
    fingerprint: blake3::Hash,
    route: Route,
}

/// One clipboard for the whole run. On Linux what was copied lasts only as long as this does.
pub struct Clip {
    board: Option<Clipboard>,
    pending: Option<Pending>,
}

impl std::fmt::Debug for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clip").field("system", &self.board.is_some()).finish_non_exhaustive()
    }
}

#[cfg(target_os = "macos")]
fn set_hidden(board: &mut Clipboard, text: &str) -> Result<(), arboard::Error> {
    use arboard::SetExtApple;
    board.set().exclude_from_history().text(text)
}

#[cfg(target_os = "windows")]
fn set_hidden(board: &mut Clipboard, text: &str) -> Result<(), arboard::Error> {
    use arboard::SetExtWindows;
    board
        .set()
        .exclude_from_cloud()
        .exclude_from_history()
        .exclude_from_monitoring()
        .text(text)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn set_hidden(board: &mut Clipboard, text: &str) -> Result<(), arboard::Error> {
    use arboard::SetExtLinux;
    board.set().exclude_from_history().text(text)
}

fn osc52(text: &str) -> std::io::Result<()> {
    let encoded = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(text));
    let mut out = std::io::stdout();
    write!(out, "\x1b]52;c;{}\x07", encoded.as_str())?;
    out.flush()
}

impl Clip {
    /// Opens the clipboard. Over SSH or with no display there is none, and copies go to the
    /// terminal instead.
    pub fn new() -> Self {
        let remote = std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
        let board = if remote { None } else { Clipboard::new().ok() };
        Self { board, pending: None }
    }

    /// A clipboard that copies nowhere, for tests.
    #[cfg(test)]
    pub fn none() -> Self {
        Self { board: None, pending: None }
    }

    /// Puts text on the clipboard. A secret is cleared after `clear_after`, if not zero.
    pub fn copy(&mut self, text: &str, secret: bool, clear_after: Duration) -> Result<Route, String> {
        let route = match self.board.as_mut() {
            Some(board) => {
                let done = if secret { set_hidden(board, text) } else { board.set_text(text) };
                match done {
                    Ok(()) => Route::System,
                    Err(_) => {
                        osc52(text).map_err(|e| e.to_string())?;
                        Route::Terminal
                    }
                }
            }
            None => {
                osc52(text).map_err(|e| format!("no clipboard here: {e}"))?;
                Route::Terminal
            }
        };
        self.pending = (secret && !clear_after.is_zero()).then(|| Pending {
            due: Instant::now() + clear_after,
            fingerprint: blake3::hash(text.as_bytes()),
            route,
        });
        Ok(route)
    }

    /// Seconds until a copied secret goes, for the status bar.
    pub fn seconds_left(&self) -> Option<u64> {
        self.pending
            .as_ref()
            .map(|p| p.due.saturating_duration_since(Instant::now()).as_secs() + 1)
    }

    /// Clears a secret whose time is up. Returns whether one went.
    pub fn tick(&mut self) -> bool {
        match &self.pending {
            Some(pending) if Instant::now() >= pending.due => {
                self.clear_now();
                true
            }
            _ => false,
        }
    }

    /// Clears a pending secret now, as on lock or quit, unless something else was copied since.
    pub fn clear_now(&mut self) {
        let Some(pending) = self.pending.take() else { return };
        match pending.route {
            Route::System => {
                if let Some(board) = self.board.as_mut() {
                    let still_ours = board
                        .get_text()
                        .map(Zeroizing::new)
                        .is_ok_and(|now| blake3::hash(now.as_bytes()) == pending.fingerprint);
                    if still_ours {
                        let _ = board.clear();
                    }
                }
            }
            // A terminal cannot be asked what it holds, so it is always cleared.
            Route::Terminal => {
                let _ = osc52("");
            }
        }
    }
}
