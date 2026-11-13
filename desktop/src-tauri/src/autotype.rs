//! Types a login into the app behind this one. The password stays on this side of the bridge.

use std::time::Duration;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use zeroize::Zeroizing;

/// `first`, then Tab, then `second`, as if typed.
pub fn type_login(first: &str, second: &Zeroizing<String>) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    // Give the app behind a moment to take focus back.
    std::thread::sleep(Duration::from_millis(350));
    // Quick search types on Shift+Enter, and a Shift still held would turn the Tab into Shift+Tab,
    // which goes back a box and puts the password in the wrong one.
    enigo.key(Key::Shift, Direction::Release).map_err(|e| e.to_string())?;
    if !first.is_empty() {
        enigo.text(first).map_err(|e| e.to_string())?;
        enigo.key(Key::Tab, Direction::Click).map_err(|e| e.to_string())?;
    }
    if !second.is_empty() {
        enigo.text(second).map_err(|e| e.to_string())?;
    }
    Ok(())
}
