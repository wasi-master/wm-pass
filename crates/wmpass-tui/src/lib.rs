//! WM Pass in a terminal: a full-screen app for macOS, Linux and Windows terminals.
//!
//! It talks to the vault through the same session the phone, desktop and browser apps use,
//! so it sees the same views and never holds a secret it did not ask for. A revealed value
//! hides itself again after a few seconds, a copied one is cleared from the clipboard, and
//! the vault locks after a quiet spell. The alternate screen leaves nothing in scrollback.

mod app;
pub mod app_folder;
pub mod ask;
mod clip;
mod door;
mod draw;
mod editor;
mod engine;
mod generator;
mod glyphs;
mod input;
pub mod join;
mod link;
pub mod restore;
mod settings;
mod store;
mod theme;
mod vault;

use std::io::{IsTerminal, stdout};
use std::path::PathBuf;

use ratatui::crossterm::event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture};
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::execute;

/// Runs the app until it is quit. `vault` is a file to open, or `None` for the last one.
pub fn run(vault: Option<PathBuf>) -> Result<(), String> {
    if !is_terminal() {
        return Err("the terminal app needs a terminal; for scripts, use the subcommands (wmpass --help)".to_owned());
    }
    run_app(app::App::new(vault))
}

fn is_terminal() -> bool {
    stdout().is_terminal() && std::io::stdin().is_terminal()
}

/// Waits for a vault from another device, to keep at `vault`: full screen in a terminal, plain
/// lines otherwise. Once it is here, the app goes on to unlock it.
pub fn join(vault: PathBuf) -> Result<(), String> {
    if !is_terminal() {
        return join::receive(&vault);
    }
    run_app(app::App::linking(link::Role::Receive, vault))
}

/// Sends the vault in `vault` to a new device: full screen in a terminal, plain lines otherwise
/// or when the new device's code is given.
pub fn add_device(vault: PathBuf, code: Option<&str>) -> Result<(), String> {
    if code.is_some() || !is_terminal() {
        return join::send(&vault, code);
    }
    run_app(app::App::linking(link::Role::Send, vault))
}

fn run_app(mut app: app::App) -> Result<(), String> {
    let mut terminal = ratatui::try_init().map_err(|e| format!("could not take over the terminal: {e}"))?;
    // ratatui's own hook leaves raw mode and the alternate screen, but not the mouse, pasting
    // or a hidden cursor. A release build aborts on a panic, so nothing else would: the shell
    // would be left printing escape codes for every mouse move, with no cursor, and a copied
    // secret would stay on the clipboard. This hook runs first, then ratatui's.
    let theirs = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste, Show);
        clip::clear_after_panic();
        theirs(info);
    }));
    let _ = execute!(stdout(), EnableMouseCapture, EnableBracketedPaste);
    let result = app.run(&mut terminal, suspend);
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    let _ = std::panic::take_hook();
    result.map_err(|e| format!("the terminal stopped answering: {e}"))
}

/// Ctrl+Z: gives the terminal back to the shell as it found it, stops until `fg`, then takes
/// it again and draws everything afresh. In raw mode the key reaches the app rather than the
/// terminal stopping it, so without this it did nothing.
#[cfg(unix)]
fn suspend(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste, Show);
    ratatui::restore();
    signal_hook::low_level::raise(signal_hook::consts::SIGTSTP)?;
    // Here again once the shell continues it.
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    // A full redraw: the screen was the shell's meanwhile. The terminal's own clear would ask
    // where the cursor is, and wait on terminals that never say; this clears the screen, then
    // swaps in an empty "last frame", so the next one is drawn whole.
    ratatui::backend::Backend::clear(terminal.backend_mut())?;
    terminal.swap_buffers();
    Ok(())
}

#[cfg(not(unix))]
fn suspend(_terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
