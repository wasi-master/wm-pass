//! The door: unlock a vault, or make a new one.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::Input;

/// Unlocking or creating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Opening a file.
    Unlock,
    /// Making one.
    Create,
}

/// A box on the door.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Path,
    Name,
    Password,
    Confirm,
    KeyFile,
}

/// The door's state.
#[derive(Debug)]
pub struct Door {
    pub mode: Mode,
    pub focus: Field,
    pub path: Input,
    pub name: Input,
    pub password: Input,
    pub confirm: Input,
    pub key_file: Input,
    /// Whether the vault asked for a key file.
    pub wants_key: bool,
    /// Which recent vault is picked, when the list has the keyboard.
    pub recent: Option<usize>,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Shown while the key is derived, which takes a second on purpose.
    pub busy: Option<&'static str>,
    /// How strong a new password looks, 0 to 4.
    pub strength: Option<(u8, String)>,
    /// The vault file named before a new one was started, to go back to.
    unlock_path: Option<String>,
}

/// What the door wants done.
#[derive(Debug, PartialEq, Eq)]
pub enum Ask {
    Nothing,
    Unlock,
    Create,
    Quit,
    /// Bring the vault over from another device.
    Join,
    /// Restore it from a place its backups are kept.
    Restore,
}

impl Door {
    /// A door for a file, or for the most recent one.
    pub fn new(path: Option<PathBuf>, recent: &[PathBuf]) -> Self {
        // The desktop app's vault comes first, so both open the same one.
        let path = path.or_else(crate::app_folder::with_vaults).or_else(|| recent.first().cloned());
        let mut door = Self {
            mode: Mode::Unlock,
            focus: Field::Password,
            path: Input::new(),
            name: Input::with("Personal"),
            password: Input::new(),
            confirm: Input::new(),
            key_file: Input::new(),
            wants_key: std::env::var_os("WMPASS_KEY_FILE").is_some(),
            recent: None,
            error: None,
            notice: None,
            busy: None,
            strength: None,
            unlock_path: None,
        };
        match path {
            Some(path) => door.path.set(&path.to_string_lossy()),
            None => door.focus = Field::Path,
        }
        if let Some(key) = std::env::var_os("WMPASS_KEY_FILE") {
            door.key_file.set(&key.to_string_lossy());
        }
        door
    }

    /// The boxes shown, in order.
    pub fn fields(&self) -> Vec<Field> {
        let mut out = match self.mode {
            Mode::Unlock => vec![Field::Path, Field::Password],
            Mode::Create => vec![Field::Path, Field::Name, Field::Password, Field::Confirm],
        };
        if self.wants_key && self.mode == Mode::Unlock {
            out.push(Field::KeyFile);
        }
        out
    }

    fn input(&mut self, field: Field) -> &mut Input {
        match field {
            Field::Path => &mut self.path,
            Field::Name => &mut self.name,
            Field::Password => &mut self.password,
            Field::Confirm => &mut self.confirm,
            Field::KeyFile => &mut self.key_file,
        }
    }

    fn step(&mut self, by: isize) {
        let fields = self.fields();
        let at = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        let len = fields.len().cast_signed();
        self.focus = fields[(at.cast_signed() + by).rem_euclid(len).cast_unsigned()];
    }

    /// Switches between unlocking and creating.
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Unlock => Mode::Create,
            Mode::Create => Mode::Unlock,
        };
        self.password.clear();
        self.confirm.clear();
        self.error = None;
        // "Locked" and the like were about the vault being left, not the one being made.
        self.notice = None;
        self.strength = None;
        if self.mode == Mode::Create {
            self.unlock_path = Some(self.path.text().to_owned());
            self.path.set(&default_new_path().to_string_lossy());
            self.focus = Field::Name;
        } else {
            // Going back from a new vault reopens the one that was named before.
            if let Some(path) = self.unlock_path.take() {
                self.path.set(&path);
            }
            self.focus = if self.path.is_empty() { Field::Path } else { Field::Password };
        }
    }

    /// Wipes every password box, as after a failed or finished attempt.
    pub fn wipe(&mut self) {
        self.password.clear();
        self.confirm.clear();
    }

    /// Applies a key.
    pub fn key(&mut self, key: KeyEvent, recent: &[PathBuf]) -> Ask {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(at) = self.recent {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.recent = Some(at.saturating_sub(1)),
                KeyCode::Down | KeyCode::Char('j') => self.recent = Some((at + 1).min(recent.len().saturating_sub(1))),
                KeyCode::Enter => {
                    if let Some(path) = recent.get(at) {
                        self.path.set(&path.to_string_lossy());
                    }
                    self.recent = None;
                    self.focus = Field::Password;
                }
                KeyCode::Esc | KeyCode::Char('r') => self.recent = None,
                _ => {}
            }
            return Ask::Nothing;
        }
        match key.code {
            KeyCode::Char('c' | 'q') if ctrl => return Ask::Quit,
            KeyCode::Esc if self.mode == Mode::Create => self.toggle_mode(),
            KeyCode::Esc => return Ask::Quit,
            KeyCode::Char('n') if ctrl => self.toggle_mode(),
            KeyCode::Char('g') if ctrl => return Ask::Join,
            KeyCode::Char('b') if ctrl => return Ask::Restore,
            KeyCode::Char('r') if ctrl && !recent.is_empty() && self.mode == Mode::Unlock => self.recent = Some(0),
            KeyCode::Char('f') if ctrl && self.mode == Mode::Unlock => {
                self.wants_key = !self.wants_key;
                if self.wants_key {
                    self.focus = Field::KeyFile;
                }
            }
            KeyCode::Tab | KeyCode::Down => self.step(1),
            KeyCode::BackTab | KeyCode::Up => self.step(-1),
            KeyCode::Enter => {
                let fields = self.fields();
                let last = fields.last().copied() == Some(self.focus);
                let ready = !self.password.is_empty() && !self.path.is_empty();
                // An empty box is never sent: an empty password would only cost a second of
                // key stretching and count as a wrong guess against the lockout.
                if last && self.mode == Mode::Unlock && !ready {
                    let (focus, error) = if self.path.is_empty() {
                        (Field::Path, "Type where the vault file is")
                    } else {
                        (Field::Password, "Type the vault's password")
                    };
                    self.focus = focus;
                    self.error = Some(error.to_owned());
                    return Ask::Nothing;
                }
                if last || (ready && self.mode == Mode::Unlock) {
                    return match self.mode {
                        Mode::Unlock => Ask::Unlock,
                        Mode::Create => Ask::Create,
                    };
                }
                self.step(1);
            }
            _ => {
                let focus = self.focus;
                let changed = self.input(focus).key(key) == crate::input::Edit::Changed;
                if changed {
                    self.error = None;
                    if self.mode == Mode::Create && focus == Field::Password {
                        self.strength = (!self.password.is_empty()).then(|| {
                            let s = wmpass_core::strength::assess(self.password.text(), &[]);
                            (s.score, s.crack_slow_hash)
                        });
                    }
                }
            }
        }
        Ask::Nothing
    }
}

/// Where a first vault goes when nobody says: the desktop app's folder when the app is on this
/// computer, so both use it, else the home folder.
pub fn default_new_path() -> PathBuf {
    if let Some(root) = crate::app_folder::default_root().filter(|r| r.is_dir()) {
        return root;
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    home.join("Personal.wmvault")
}

/// Where a vault coming from another device is kept: what was typed if that is free, else a
/// new file in the desktop app's folder when the app is here, else beside the home folder.
pub fn join_target(typed: &str) -> PathBuf {
    let typed = crate::store::expand(typed);
    if !typed.as_os_str().is_empty() && !typed.exists() {
        return typed;
    }
    let base = default_new_path();
    if base.is_dir()
        && let Ok(name) = crate::app_folder::new_name()
    {
        return crate::app_folder::vault_path(&base, &name);
    }
    base
}

/// How the door shows a vault's place: the desktop app's folder by name, a file by its path.
pub fn place_label(path: &str) -> Option<&'static str> {
    let path = crate::store::expand(path);
    (crate::app_folder::is_app_folder(&path) || crate::app_folder::root_of(&path).is_some()).then_some("The WM Pass app's vault on this computer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backing_out_of_a_new_vault_keeps_the_one_named_before() {
        let mut door = Door::new(Some(PathBuf::from("/somewhere/mine.wmvault")), &[]);
        door.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL), &[]);
        assert_eq!(door.mode, Mode::Create);
        assert_ne!(door.path.text(), "/somewhere/mine.wmvault");
        door.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &[]);
        assert_eq!(door.mode, Mode::Unlock);
        assert_eq!(door.path.text(), "/somewhere/mine.wmvault");
        assert_eq!(door.focus, Field::Password);
    }

    #[test]
    fn enter_on_an_empty_password_asks_for_one_rather_than_trying_it() {
        let mut door = Door::new(Some(PathBuf::from("/somewhere/mine.wmvault")), &[]);
        assert_eq!(door.focus, Field::Password);
        assert_eq!(door.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &[]), Ask::Nothing);
        assert_eq!(door.error.as_deref(), Some("Type the vault's password"));
        door.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), &[]);
        assert!(door.error.is_none());
        assert_eq!(door.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &[]), Ask::Unlock);
    }
}
