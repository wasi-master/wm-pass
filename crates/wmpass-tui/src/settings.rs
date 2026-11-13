//! The Settings tab: every choice the terminal app has, in one list, grouped as the other apps
//! group them. Each one is changed in place with the arrows, or opened with Enter.

/// One setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Theme,
    Icons,
    LockAfter,
    ClipSeconds,
    RevealSeconds,
    Shelf,
    Password,
    VaultName,
    TrashDays,
    HistoryLimit,
    SyncSettings,
    SkipLocal,
    Import,
    Export,
    AddDevice,
    IntoApp,
}

/// How a setting is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// On or off.
    Toggle,
    /// One of a few values, with the arrows; Enter types one.
    Steps,
    /// Opens something.
    Action,
}

/// The groups, in order.
pub const GROUPS: [(&str, &[Setting]); 5] = [
    ("Look", &[Setting::Theme, Setting::Icons]),
    ("Security", &[Setting::LockAfter, Setting::ClipSeconds, Setting::RevealSeconds, Setting::Shelf, Setting::Password]),
    ("Vault", &[Setting::VaultName, Setting::TrashDays, Setting::HistoryLimit, Setting::SyncSettings]),
    ("Health", &[Setting::SkipLocal]),
    ("Move and share", &[Setting::Import, Setting::Export, Setting::AddDevice, Setting::IntoApp]),
];

/// Every setting, in the order listed.
pub fn all() -> Vec<Setting> {
    GROUPS.iter().flat_map(|(_, s)| s.iter().copied()).collect()
}

/// The group a setting starts, when it is the first of one.
pub fn heading_before(n: usize) -> Option<&'static str> {
    let mut at = 0;
    for (name, settings) in GROUPS {
        if at == n {
            return Some(name);
        }
        at += settings.len();
    }
    None
}

impl Setting {
    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Icons => "Nerd Font icons",
            Self::LockAfter => "Lock after",
            Self::ClipSeconds => "Clear the clipboard after",
            Self::RevealSeconds => "Hide a shown secret after",
            Self::Shelf => "Hidden shelf",
            Self::Password => "Change the vault password",
            Self::VaultName => "Vault name",
            Self::TrashDays => "Empty the trash after",
            Self::HistoryLimit => "Past values kept per field",
            Self::SyncSettings => "Settings follow the vault",
            Self::SkipLocal => "Leave out devices at home",
            Self::Import => "Import from another app",
            Self::Export => "Export",
            Self::AddDevice => "Add a new device",
            Self::IntoApp => "Keep in the WM Pass app",
        }
    }

    pub fn look(self) -> Look {
        match self {
            Self::Icons | Self::Shelf | Self::SyncSettings | Self::SkipLocal => Look::Toggle,
            Self::Theme | Self::LockAfter | Self::ClipSeconds | Self::RevealSeconds | Self::TrashDays | Self::HistoryLimit => Look::Steps,
            Self::Password | Self::VaultName | Self::Import | Self::Export | Self::AddDevice | Self::IntoApp => Look::Action,
        }
    }

    /// A sentence or two on what it does.
    pub fn about(self) -> &'static str {
        match self {
            Self::Theme => "The colors the terminal app is drawn in. Plain uses none, for terminals that show them badly.",
            Self::Icons => "Icons from a Nerd Font for kinds, tabs and the sidebar. Turn it off if they show as boxes: the terminal's font has none.",
            Self::LockAfter => "The vault locks after this long without a key press, and everything shown is cleared from the screen.",
            Self::ClipSeconds => "A copied password or code is wiped from the clipboard after this long, unless something else was copied since.",
            Self::RevealSeconds => "A password or other secret shown with r is hidden again after this long.",
            Self::Shelf => "Items put on the hidden shelf are left out of every list and search until it is opened with the vault password.",
            Self::Password => "The password that opens this vault. Every device takes the new one once the file reaches it.",
            Self::VaultName => "The name shown at the top and on the other devices.",
            Self::TrashDays => "Items in the trash are deleted for good after this many days.",
            Self::HistoryLimit => "How many earlier values each password and secret keeps, to look back at under Earlier values.",
            Self::SyncSettings => "Theme, icons, how long a secret stays shown and the Health choices travel in the vault to your other devices.",
            Self::SkipLocal => "Health leaves out logins only for your router, a NAS or other devices at home, which rarely have strong passwords.",
            Self::Import => "Bitwarden, 1Password, KeePass, Chrome, Aegis and more. You see what was found before anything is added.",
            Self::Export => "KeePass with a password of its own, or CSV, Bitwarden JSON or a page to print, which anyone with the file can read.",
            Self::AddDevice => "Sends this vault to a phone or computer nearby, with a code to type or scan.",
            Self::IntoApp => "Moves this vault into the folder the WM Pass desktop app keeps, so the app and this terminal share one vault.",
        }
    }
}

/// The step after or before `now` in `steps`, where 0 means never and sorts last.
pub fn step(steps: &[u32], now: u32, up: bool) -> u32 {
    let rank = |v: u32| if v == 0 { u32::MAX } else { v };
    if let Some(at) = steps.iter().position(|s| *s == now) {
        let next = if up { (at + 1).min(steps.len() - 1) } else { at.saturating_sub(1) };
        return steps[next];
    }
    if up {
        steps.iter().copied().find(|s| rank(*s) > rank(now)).unwrap_or(now)
    } else {
        steps.iter().rev().copied().find(|s| rank(*s) < rank(now)).unwrap_or(now)
    }
}

pub const LOCK_MINUTES: [u32; 9] = [1, 2, 5, 10, 15, 30, 60, 240, 0];
pub const CLIP_SECONDS: [u32; 9] = [10, 20, 30, 45, 60, 90, 120, 300, 0];
pub const REVEAL_SECONDS: [u32; 7] = [5, 10, 20, 30, 60, 120, 300];
pub const TRASH_DAYS: [u32; 8] = [7, 14, 30, 60, 90, 180, 365, 0];
pub const HISTORY: [u32; 7] = [1, 3, 5, 10, 20, 50, 0];

/// "5 min", "1 h", "never".
pub fn minutes(n: u32) -> String {
    match n {
        0 => "never".to_owned(),
        n if n >= 60 && n % 60 == 0 => format!("{} h", n / 60),
        n => format!("{n} min"),
    }
}

/// "45 s", "2 min", "never".
pub fn seconds(n: u32) -> String {
    match n {
        0 => "never".to_owned(),
        n if n >= 60 && n % 60 == 0 => format!("{} min", n / 60),
        n => format!("{n} s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_walk_and_never_sorts_last() {
        assert_eq!(step(&LOCK_MINUTES, 5, true), 10);
        assert_eq!(step(&LOCK_MINUTES, 5, false), 2);
        assert_eq!(step(&LOCK_MINUTES, 240, true), 0);
        assert_eq!(step(&LOCK_MINUTES, 0, true), 0);
        assert_eq!(step(&LOCK_MINUTES, 0, false), 240);
        assert_eq!(step(&LOCK_MINUTES, 7, true), 10, "a typed value steps to the next one");
        assert_eq!(step(&LOCK_MINUTES, 7, false), 5);
        assert_eq!(minutes(120), "2 h");
        assert_eq!(seconds(90), "90 s");
        assert_eq!(heading_before(2), Some("Security"));
        assert_eq!(all().len(), GROUPS.iter().map(|g| g.1.len()).sum::<usize>());
    }
}
