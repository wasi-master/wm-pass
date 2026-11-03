//! WM Pass in a terminal: a full-screen app for macOS, Linux and Windows terminals.
//!
//! It talks to the vault through the same session the phone, desktop and browser apps use,
//! so it sees the same views and never holds a secret it did not ask for. A revealed value
//! hides itself again after a few seconds, a copied one is cleared from the clipboard, and
//! the vault locks after a quiet spell. The alternate screen leaves nothing in scrollback.

mod app;
mod clip;
mod door;
mod draw;
mod editor;
mod engine;
mod generator;
mod glyphs;
mod input;
mod store;
mod theme;
mod vault;

use std::io::{IsTerminal, stdout};
use std::path::PathBuf;

use ratatui::crossterm::event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture};
use ratatui::crossterm::execute;

/// Runs the app until it is quit. `vault` is a file to open, or `None` for the last one.
pub fn run(vault: Option<PathBuf>) -> Result<(), String> {
    if !stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err("the terminal app needs a terminal; for scripts, use the subcommands (wmpass --help)".to_owned());
    }
    let mut app = app::App::new(vault);
    let mut terminal = ratatui::init();
    let _ = execute!(stdout(), EnableMouseCapture, EnableBracketedPaste);
    let result = app.run(&mut terminal);
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result.map_err(|e| format!("the terminal stopped answering: {e}"))
}

#[cfg(test)]
mod tests;
