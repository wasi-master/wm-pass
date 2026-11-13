//! A password asked for on the terminal, for the subcommands.
//!
//! The usual way turns echo off and reads a line; Ctrl+C then kills the program with echo
//! still off, and the shell after it shows nothing typed. Here the terminal is in raw mode
//! instead, so Ctrl+C is a key like any other: the terminal is put back and the question is
//! answered with "cancelled".

use std::io::Write;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::terminal;
use zeroize::Zeroizing;

/// Why no password came back.
#[derive(Debug, PartialEq, Eq)]
pub enum NoAnswer {
    /// Ctrl+C, or Ctrl+D with nothing typed.
    Cancelled,
    /// No terminal to ask on, as in a script or a pipe with no terminal behind it.
    NoTerminal,
    /// The terminal failed part way.
    Failed(String),
}

impl std::fmt::Display for NoAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::NoTerminal => f.write_str("no terminal to ask for the password on"),
            Self::Failed(why) => write!(f, "could not read the password: {why}"),
        }
    }
}

/// Puts the terminal back however the question ends.
struct Raw;

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Where the question is written: the terminal itself where there is one to open, so the
/// prompt shows even with the output redirected, as `wmpass show --reveal > file` has it.
fn screen() -> Box<dyn Write> {
    #[cfg(unix)]
    if let Ok(tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        return Box::new(tty);
    }
    Box::new(std::io::stderr())
}

/// Asks for a secret, showing nothing as it is typed.
pub fn secret(prompt: &str) -> Result<Zeroizing<String>, NoAnswer> {
    let mut out = screen();
    let raw = terminal::enable_raw_mode().map(|()| Raw).map_err(|_| NoAnswer::NoTerminal)?;
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
    // Room enough that typing never moves the text, which would leave a copy behind.
    let mut typed = Zeroizing::new(String::with_capacity(512));
    let answer = loop {
        let key = match event::read() {
            Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => key,
            Ok(_) => continue,
            Err(e) => break Err(NoAnswer::Failed(e.to_string())),
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => break Ok(()),
            KeyCode::Char('c') if ctrl => break Err(NoAnswer::Cancelled),
            KeyCode::Char('d') if ctrl && typed.is_empty() => break Err(NoAnswer::Cancelled),
            KeyCode::Char('u') if ctrl => typed.clear(),
            KeyCode::Backspace => {
                typed.pop();
            }
            KeyCode::Char(ch) if !ctrl => typed.push(ch),
            _ => {}
        }
    };
    drop(raw);
    let _ = writeln!(out);
    let _ = out.flush();
    answer.map(|()| typed)
}
