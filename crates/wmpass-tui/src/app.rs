//! The event loop, and what each key does.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use serde_json::json;
use zeroize::Zeroizing;

use crate::clip::{Clip, Route};
use crate::door::{self, Ask, Door};
use crate::editor::{Editor, Slot};
use crate::engine::{self, ImportPreview, Secret, Unlock};
use crate::draw::Hit;
use crate::input::Input;
use crate::link::{LinkUi, Role, Stage};
use crate::store::{self, Prefs};
use crate::theme::Theme;
use crate::vault::{HEALTH_GROUPS, Line, Pane, Tab, VaultUi};
use crate::{draw, vault};

/// A short message at the bottom.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub level: Level,
    pub at: Instant,
}

/// How a message reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Good,
    Warn,
    Bad,
}

/// What the whole screen is.
#[derive(Debug)]
pub enum Screen {
    Door(Box<Door>),
    Vault(Box<VaultUi>),
}

/// What a prompt's answer is for.
#[derive(Debug, Clone)]
pub enum PromptFor {
    NewFolder,
    AddTag(String),
    ImportPath,
    ImportPassword(PathBuf),
    ExportPath(String),
    ExportPassword(String, PathBuf),
    AttachAdd(String),
    AttachSave { id: String, blob: String, name: String },
    ShelfPassword,
    LockMinutes,
}

/// What a confirmation does on yes.
#[derive(Debug, Clone)]
pub enum Act {
    Trash(String),
    Purge(String),
    Export(String, PathBuf),
    Import,
}

/// What a pick is for.
#[derive(Debug, Clone)]
pub enum PickFor {
    NewKind,
    MoveFolder(String),
    RemoveTag(String),
    ExportFormat,
}

/// A palette command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    New,
    Edit,
    Trash,
    Duplicate,
    Favourite,
    Pin,
    MoveFolder,
    AddTag,
    RemoveTag,
    NewFolder,
    CopyPassword,
    CopyUsername,
    CopyCode,
    Import,
    Export,
    Attach,
    Shelf,
    Theme,
    Icons,
    SyncSettings,
    SkipLocal,
    AddDevice,
    IntoApp,
    LockAfter,
    Items,
    Codes,
    Generator,
    Health,
    Help,
    Lock,
    Quit,
}

/// The terminal's settings that follow the vault, by the key they travel under. Its themes are
/// not the apps' themes, so they travel under names of their own; the reveal time and the
/// Health choices are shared with every app.
const SYNCED: [&str; 5] = ["tui_theme", "tui_icons", "reveal_seconds", "health_ignored", "health_skip_local"];

/// Every palette command, with its words and key.
pub const COMMANDS: [(Command, &str, &str); 31] = [
    (Command::New, "New item", "n"),
    (Command::Edit, "Edit item", "e"),
    (Command::CopyPassword, "Copy password", "c"),
    (Command::CopyUsername, "Copy username", "u"),
    (Command::CopyCode, "Copy one-time code", "t"),
    (Command::Favourite, "Favourite or not", "f"),
    (Command::Pin, "Pin to the top or not", "p"),
    (Command::MoveFolder, "Move to folder", "m"),
    (Command::AddTag, "Add a tag", "+"),
    (Command::RemoveTag, "Remove a tag", "-"),
    (Command::Duplicate, "Duplicate item", "ctrl+d"),
    (Command::Attach, "Attach a file", "a"),
    (Command::Trash, "Move to the trash", "d"),
    (Command::NewFolder, "New folder", ""),
    (Command::Import, "Import from another app", ""),
    (Command::Export, "Export", ""),
    (Command::Shelf, "Open or close the hidden shelf", ""),
    (Command::Theme, "Next theme", "ctrl+t"),
    (Command::Icons, "Nerd Font icons on or off", ""),
    (Command::SyncSettings, "Sync settings with the vault on or off", ""),
    (Command::SkipLocal, "Leave local addresses out of Health or not", ""),
    (Command::AddDevice, "Add a new device: send this vault to it", ""),
    (Command::IntoApp, "Keep this vault in the WM Pass app on this computer", ""),
    (Command::LockAfter, "Lock after…", ""),
    (Command::Items, "Go to Items", "1"),
    (Command::Codes, "Go to Codes", "2"),
    (Command::Generator, "Go to Generator", "3"),
    (Command::Health, "Go to Health", "4"),
    (Command::Help, "Keys", "?"),
    (Command::Lock, "Lock now", "ctrl+l"),
    (Command::Quit, "Quit", "q"),
];

/// Something drawn over the screen.
#[derive(Debug)]
pub enum Overlay {
    Help,
    Palette { input: Input, at: usize },
    Confirm { title: String, body: String, act: Act, danger: bool },
    Prompt { title: String, hint: String, input: Input, secret: bool, then: PromptFor },
    Pick { title: String, items: Vec<(String, String)>, at: usize, then: PickFor },
    Editor(Box<Editor>),
    Import(ImportPreview),
    /// Moving the vault to or from another device.
    Link(Box<LinkUi>),
}

/// The palette's matches for what was typed, best first.
pub fn palette_matches(text: &str) -> Vec<(Command, &'static str, &'static str)> {
    let wanted = text.to_lowercase();
    let mut scored: Vec<(usize, (Command, &str, &str))> = COMMANDS
        .iter()
        .filter_map(|c| fuzzy(&c.1.to_lowercase(), &wanted).map(|score| (score, *c)))
        .collect();
    scored.sort_by_key(|(score, _)| *score);
    scored.into_iter().map(|(_, c)| c).collect()
}

/// A small fuzzy match: every typed character in order, scored by how spread out they are.
fn fuzzy(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if let Some(at) = haystack.find(needle) {
        return Some(at);
    }
    let mut chars = haystack.char_indices();
    let (mut first, mut last) = (None, 0);
    for wanted in needle.chars() {
        let (at, _) = chars.find(|(_, c)| *c == wanted)?;
        first.get_or_insert(at);
        last = at;
    }
    Some(100 + last - first.unwrap_or(0))
}

/// The application.
#[derive(Debug)]
pub struct App {
    pub prefs: Prefs,
    pub theme: Theme,
    pub clip: Clip,
    pub screen: Screen,
    pub overlay: Option<Overlay>,
    pub toast: Option<Toast>,
    last_input: Instant,
    quit: bool,
    /// Work to do once the next frame shows that it is happening.
    deferred: Option<Ask>,
    /// What the last frame drew where, for the mouse.
    pub hits: Vec<(Rect, Hit)>,
    /// Where the overlay's part of `hits` begins, when one is open.
    pub overlay_from: Option<usize>,
    /// The last left click, to tell a double click.
    last_click: Option<(Position, Instant)>,
    /// Whether the last QR code drawn used big cells, for flipping its size.
    pub link_drew_big: bool,
}

impl App {
    pub fn new(path: Option<PathBuf>) -> Self {
        let prefs = Prefs::load();
        let mut theme = Theme::pick(&prefs.theme);
        theme.g = crate::glyphs::Glyphs::pick(&prefs.icons);
        let mut door = Door::new(path.clone(), &prefs.recent);
        if let Some(path) = &path
            && !path.exists()
        {
            door.toggle_mode();
            door.path.set(&path.to_string_lossy());
            door.notice = Some("No vault there yet. Make one?".to_owned());
        }
        Self {
            prefs,
            theme,
            clip: Clip::new(),
            screen: Screen::Door(Box::new(door)),
            overlay: None,
            toast: None,
            last_input: Instant::now(),
            quit: false,
            deferred: None,
            hits: Vec::new(),
            overlay_from: None,
            last_click: None,
            link_drew_big: false,
        }
    }

    /// Opens straight onto moving a vault: waiting for it at `path`, or sending the one there.
    pub fn linking(role: Role, path: PathBuf) -> Self {
        let mut app = Self::new(if role == Role::Send { Some(path.clone()) } else { None });
        app.overlay = Some(Overlay::Link(Box::new(match role {
            Role::Receive => LinkUi::receive(path),
            Role::Send => LinkUi::send(path),
        })));
        app
    }

    /// For tests: an app over an open vault, with no clipboard and no saved preferences.
    #[cfg(test)]
    pub fn over(engine: Box<engine::Engine>) -> Self {
        Self {
            prefs: Prefs::default(),
            theme: crate::theme::NIGHT,
            clip: Clip::none(),
            screen: Screen::Vault(Box::new(VaultUi::new(engine))),
            overlay: None,
            toast: None,
            last_input: Instant::now(),
            quit: false,
            deferred: None,
            hits: Vec::new(),
            overlay_from: None,
            last_click: None,
            link_drew_big: false,
        }
    }

    /// Time left before the vault locks itself, if it will.
    pub fn lock_in(&self) -> Option<Duration> {
        let minutes = self.prefs.lock_minutes;
        (minutes > 0).then(|| Duration::from_secs(u64::from(minutes) * 60).saturating_sub(self.last_input.elapsed()))
    }

    /// For tests: a mouse event, as if it happened.
    #[cfg(test)]
    pub fn mouse_for_test(&mut self, mouse: MouseEvent) {
        self.mouse(mouse);
    }

    /// For tests: one key, as if pressed.
    #[cfg(test)]
    pub fn key_for_test(&mut self, key: KeyEvent) {
        self.key(key);
    }

    pub fn say(&mut self, level: Level, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), level, at: Instant::now() });
    }

    /// Runs until quit.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        let mut dirty = true;
        while !self.quit {
            if dirty {
                terminal.draw(|frame| draw::draw(frame, self))?;
                dirty = false;
            }
            if let Some(ask) = self.deferred.take() {
                self.finish_door(ask);
                dirty = true;
                continue;
            }
            if event::poll(Duration::from_millis(250))? {
                match event::read()? {
                    // Windows reports releases too; only presses do anything.
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        self.last_input = Instant::now();
                        self.key(key);
                    }
                    Event::Mouse(mouse) => {
                        self.last_input = Instant::now();
                        self.mouse(mouse);
                    }
                    Event::Paste(text) => {
                        self.last_input = Instant::now();
                        let text = Zeroizing::new(text);
                        for ch in text.chars().filter(|c| *c != '\r') {
                            let code = if ch == '\n' { KeyCode::Enter } else { KeyCode::Char(ch) };
                            if code == KeyCode::Enter && !self.typing_multiline() {
                                continue;
                            }
                            self.key(KeyEvent::new(code, KeyModifiers::NONE));
                        }
                    }
                    _ => {}
                }
                dirty = true;
            }
            dirty |= self.tick();
        }
        self.clip.clear_now();
        if let Screen::Vault(_) = self.screen {
            self.lock("");
        }
        Ok(())
    }

    fn typing_multiline(&self) -> bool {
        match &self.overlay {
            Some(Overlay::Editor(editor)) => editor.rows.get(editor.at).is_some_and(|r| r.value.multiline),
            _ => false,
        }
    }

    /// Timers. Returns whether anything on screen changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        if self.clip.tick() {
            self.say(Level::Info, "Clipboard cleared");
            changed = true;
        }
        if let Some(Overlay::Link(link)) = &mut self.overlay {
            link.tick();
            // The spinner and the countdown move.
            changed = true;
        }
        if self.toast.as_ref().is_some_and(|t| t.at.elapsed() > Duration::from_secs(6)) {
            self.toast = None;
            changed = true;
        }
        if let Screen::Vault(ui) = &mut self.screen {
            let minutes = self.prefs.lock_minutes;
            if minutes > 0 && self.last_input.elapsed() >= Duration::from_secs(u64::from(minutes) * 60) {
                self.lock(&format!("Locked after {minutes} minutes without a key press"));
                return true;
            }
            let (redraw, news) = ui.tick(Duration::from_secs(u64::from(self.prefs.reveal_seconds.max(5))));
            changed |= redraw;
            if std::mem::take(&mut ui.merged) {
                self.pull_settings();
            }
            if let Some(text) = news {
                let level = if text.contains("older") { Level::Warn } else { Level::Info };
                self.say(level, text);
                changed = true;
            }
        }
        // The countdowns in the status bar move every second.
        changed || self.clip.seconds_left().is_some() || matches!(self.screen, Screen::Vault(_))
    }

    /// The desktop app's vault follows the app's own timers, which stay on this computer.
    fn follow_app_timers(&mut self, root: &std::path::Path) {
        let (lock, clip, reveal) = crate::app_folder::app_timers(root);
        if let Some(seconds) = lock {
            self.prefs.lock_minutes = u32::try_from(seconds.div_ceil(60)).unwrap_or(u32::MAX).min(24 * 60);
        }
        if let Some(seconds) = clip {
            self.prefs.clip_seconds = u32::try_from(seconds).unwrap_or(u32::MAX).min(3600);
        }
        if let Some(seconds) = reveal {
            self.prefs.reveal_seconds = u32::try_from(seconds).unwrap_or(u32::MAX).min(3600);
        }
    }

    // ---- settings that follow the vault ----

    fn syncs(&self, key: &str) -> bool {
        self.prefs.sync_settings && !self.prefs.sync_skip.iter().any(|skipped| skipped == key)
    }

    fn synced_value(&self, key: &str) -> serde_json::Value {
        match key {
            "tui_theme" => json!(self.prefs.theme),
            "tui_icons" => json!(self.prefs.icons),
            "health_ignored" => json!(self.prefs.ignored_text()),
            "health_skip_local" => json!(self.prefs.health_skip_local),
            _ => json!(self.prefs.reveal_seconds),
        }
    }

    /// Takes the settings other devices left in the vault, then leaves any this terminal has
    /// changed that the vault has never held.
    fn pull_settings(&mut self) {
        if !self.prefs.sync_settings {
            return;
        }
        let Screen::Vault(ui) = &mut self.screen else { return };
        let Ok(held) = ui.engine.synced_settings() else { return };
        let defaults = Prefs::default();
        let mut health_changed = false;
        let mut missing = serde_json::Map::new();
        for key in SYNCED {
            if !self.syncs(key) {
                continue;
            }
            let Some(value) = held.get(key) else {
                let fresh = match key {
                    "tui_theme" => self.prefs.theme == defaults.theme,
                    "tui_icons" => self.prefs.icons == defaults.icons,
                    "health_ignored" => self.prefs.health_ignored == defaults.health_ignored,
                    "health_skip_local" => self.prefs.health_skip_local == defaults.health_skip_local,
                    _ => self.prefs.reveal_seconds == defaults.reveal_seconds,
                };
                if !fresh {
                    missing.insert(key.to_owned(), self.synced_value(key));
                }
                continue;
            };
            match key {
                "tui_theme" => {
                    if let Some(name) = value.as_str().filter(|name| *name != self.prefs.theme) {
                        name.clone_into(&mut self.prefs.theme);
                        self.theme = Theme { g: self.theme.g, ..Theme::pick(name) };
                    }
                }
                "tui_icons" => {
                    if let Some(name) = value.as_str().filter(|name| *name != self.prefs.icons) {
                        name.clone_into(&mut self.prefs.icons);
                        self.theme.g = crate::glyphs::Glyphs::pick(name);
                    }
                }
                "health_ignored" => {
                    if let Some(text) = value.as_str().filter(|text| *text != self.prefs.ignored_text()) {
                        self.prefs.set_ignored_text(text);
                        health_changed = true;
                    }
                }
                "health_skip_local" => {
                    if let Some(on) = value.as_bool().filter(|on| *on != self.prefs.health_skip_local) {
                        self.prefs.health_skip_local = on;
                        health_changed = true;
                    }
                }
                _ => {
                    if let Some(seconds) = value.as_u64().filter(|s| *s <= 3600).and_then(|s| u32::try_from(s).ok()) {
                        self.prefs.reveal_seconds = seconds;
                    }
                }
            }
        }
        self.prefs.save();
        if health_changed {
            self.refresh_health();
        }
        if let Screen::Vault(ui) = &mut self.screen {
            let _ = ui.engine.sync_settings(missing);
        }
    }

    /// Works the health report out again, with what was ignored and whether local addresses
    /// count.
    fn refresh_health(&mut self) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        ui.health = ui.engine.health(&self.prefs.ignored_checks(), self.prefs.health_skip_local).ok();
        ui.health_ignored.clone_from(&self.prefs.health_ignored);
        ui.health_group = ui.health_group.min(ui.health_groups() - 1);
        let len = ui.health_ids(ui.health_group).len();
        ui.health_at = ui.health_at.min(len.saturating_sub(1));
    }

    /// Leaves one setting in the vault for the other devices, if this terminal lets it go.
    fn push_setting(&mut self, key: &str) {
        if !self.syncs(key) {
            return;
        }
        let mut values = serde_json::Map::new();
        values.insert(key.to_owned(), self.synced_value(key));
        if let Screen::Vault(ui) = &mut self.screen {
            let _ = ui.engine.sync_settings(values);
        }
    }

    /// Locks, back to the door.
    pub fn lock(&mut self, why: &str) {
        self.clip.clear_now();
        self.overlay = None;
        let door = Door::new(None, &self.prefs.recent);
        if let Screen::Vault(ui) = std::mem::replace(&mut self.screen, Screen::Door(Box::new(door))) {
            let path = ui.engine.path().to_owned();
            ui.engine.lock();
            if let Screen::Door(door) = &mut self.screen {
                door.path.set(&path.to_string_lossy());
                door.focus = door::Field::Password;
                if !why.is_empty() {
                    door.notice = Some(why.to_owned());
                }
            }
        }
    }

    fn key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.overlay.is_some() {
            self.overlay_key(key);
            return;
        }
        match &mut self.screen {
            Screen::Door(door) => {
                if door.busy.is_some() {
                    return;
                }
                match door.key(key, &self.prefs.recent) {
                    Ask::Quit => self.quit = true,
                    Ask::Nothing => {}
                    Ask::Join => {
                        let path = door::join_target(if door.mode == door::Mode::Create { door.path.text() } else { "" });
                        self.overlay = Some(Overlay::Link(Box::new(LinkUi::receive(path))));
                    }
                    ask => {
                        door.busy = Some(if ask == Ask::Create { "Making your vault…" } else { "Unlocking…" });
                        door.error = None;
                        self.deferred = Some(ask);
                    }
                }
            }
            Screen::Vault(_) => self.vault_key(key),
        }
    }

    /// Runs the slow part of an unlock or create, after a frame saying so.
    fn finish_door(&mut self, ask: Ask) {
        let Screen::Door(door) = &mut self.screen else { return };
        door.busy = None;
        let path = store::expand(door.path.text());
        let device = self.prefs.device;
        let result = match ask {
            Ask::Create => {
                if door.password.text() != door.confirm.text() {
                    door.error = Some("Those passwords do not match".into());
                    door.confirm.clear();
                    door.focus = door::Field::Confirm;
                    return;
                }
                if door.password.text().chars().count() < 8 {
                    door.error = Some("Use at least 8 characters; a few random words work well".into());
                    return;
                }
                let name = if door.name.is_empty() { "Personal".to_owned() } else { door.name.text().to_owned() };
                engine::create(&path, &name, door.password.text().as_bytes(), device).map(|e| (Box::new(e), None))
            }
            Ask::Unlock => {
                let key = if door.wants_key && !door.key_file.is_empty() {
                    match store::read(&store::expand(door.key_file.text())) {
                        Ok(bytes) => Some(Zeroizing::new(bytes)),
                        Err(e) => {
                            door.error = Some(e);
                            return;
                        }
                    }
                } else {
                    None
                };
                match engine::unlock(&path, door.password.text().as_bytes(), key.as_deref().map(Vec::as_slice), device) {
                    Ok(Unlock::Opened(engine, rollback)) => Ok((engine, rollback)),
                    Ok(Unlock::Wrong) => Err(if door.wants_key { "Wrong password or key file" } else { "Wrong password" }.to_owned()),
                    Ok(Unlock::NeedsKeyFile) => {
                        door.wants_key = true;
                        door.focus = door::Field::KeyFile;
                        door.error = Some("This vault also needs its key file".into());
                        return;
                    }
                    Ok(Unlock::NeedsHardware) => Err("This vault needs a hardware key. Open it in the desktop or phone app".into()),
                    Ok(Unlock::Waiting(at)) => {
                        let minutes = at.saturating_sub(engine::now_ms()).div_ceil(60_000);
                        Err(format!("Too many wrong passwords. Try again in {minutes} min"))
                    }
                    Err(e) => Err(e),
                }
            }
            _ => return,
        };
        match result {
            Ok((engine, rollback)) => {
                door.wipe();
                self.prefs.remember(&path);
                let name = engine.name.clone();
                if let Some(root) = engine.app_folder() {
                    self.follow_app_timers(root);
                }
                self.screen = Screen::Vault(Box::new(VaultUi::new(engine)));
                self.pull_settings();
                match rollback {
                    Some((seen, file)) => self.say(
                        Level::Warn,
                        format!("This copy is older than one seen here (revision {file}, seen {seen}). Check your sync"),
                    ),
                    None => self.say(Level::Good, format!("{name} unlocked")),
                }
            }
            Err(message) => {
                door.wipe();
                door.focus = door::Field::Password;
                door.error = Some(message);
            }
        }
    }

    fn vault_key(&mut self, key: KeyEvent) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Typing in a search box takes every key but the few that leave it.
        if ui.searching {
            match key.code {
                KeyCode::Esc => {
                    ui.searching = false;
                    ui.search.clear();
                    ui.refresh_list(None);
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Tab => {
                    ui.searching = false;
                    ui.pane = Pane::List;
                }
                _ => {
                    if ui.search.key(key) == crate::input::Edit::Changed {
                        ui.list.select(Some(0));
                        ui.refresh_list(None);
                    }
                }
            }
            return;
        }
        if ui.filtering_codes {
            match key.code {
                KeyCode::Esc => {
                    ui.filtering_codes = false;
                    ui.code_filter.clear();
                    ui.refresh_codes();
                }
                KeyCode::Enter | KeyCode::Down => ui.filtering_codes = false,
                _ => {
                    if ui.code_filter.key(key) == crate::input::Edit::Changed {
                        ui.refresh_codes();
                    }
                }
            }
            return;
        }

        match key.code {
            KeyCode::Char('l') if ctrl => return self.lock("Locked"),
            KeyCode::Char('k' | 'p') if ctrl => return self.open_palette(),
            KeyCode::Char(':') => return self.open_palette(),
            KeyCode::Char('t') if ctrl => return self.command(Command::Theme),
            KeyCode::Char('d') if ctrl => return self.command(Command::Duplicate),
            KeyCode::Char('?') => return self.command(Command::Help),
            KeyCode::Char('q') => return self.command(Command::Quit),
            KeyCode::Char(n @ '1'..='4') => {
                let tab = Tab::ALL[(n as usize) - ('1' as usize)];
                return self.switch_tab(tab);
            }
            KeyCode::Tab if ui.tab != Tab::Items => {
                let at = Tab::ALL.iter().position(|t| *t == ui.tab).unwrap_or(0);
                return self.switch_tab(Tab::ALL[(at + 1) % Tab::ALL.len()]);
            }
            _ => {}
        }
        match ui.tab {
            Tab::Items => self.items_key(key),
            Tab::Codes => self.codes_key(key),
            Tab::Generator => self.generator_key(key),
            Tab::Health => self.health_key(key),
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        ui.tab = tab;
        match tab {
            Tab::Codes => ui.refresh_codes(),
            Tab::Generator if ui.generator.value.is_empty() => self.regenerate(),
            Tab::Health if ui.health.is_none() => self.refresh_health(),
            _ => {}
        }
    }

    fn items_key(&mut self, key: KeyEvent) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let page = (ui.areas.list.height.saturating_sub(2) / 2).max(1) as isize;
        match key.code {
            KeyCode::Char('/') => {
                ui.searching = true;
                return;
            }
            KeyCode::Esc if !ui.search.is_empty() => {
                ui.search.clear();
                ui.refresh_list(None);
                return;
            }
            KeyCode::Esc if ui.pane == Pane::Detail => {
                ui.pane = Pane::List;
                return;
            }
            KeyCode::Tab => {
                ui.pane = match ui.pane {
                    Pane::Sidebar => Pane::List,
                    Pane::List => Pane::Detail,
                    Pane::Detail => Pane::Sidebar,
                };
                return;
            }
            KeyCode::BackTab => {
                ui.pane = match ui.pane {
                    Pane::Sidebar => Pane::Detail,
                    Pane::List => Pane::Sidebar,
                    Pane::Detail => Pane::List,
                };
                return;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                ui.pane = match ui.pane {
                    Pane::Detail => Pane::List,
                    _ => Pane::Sidebar,
                };
                return;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                ui.pane = match ui.pane {
                    Pane::Sidebar => Pane::List,
                    _ => Pane::Detail,
                };
                return;
            }
            KeyCode::Char('s') => {
                ui.sort = ui.sort.next();
                let sort = ui.sort.name();
                ui.refresh_list(None);
                self.say(Level::Info, format!("Sorted by {sort}"));
                return;
            }
            _ => {}
        }
        match ui.pane {
            Pane::Sidebar => match key.code {
                KeyCode::Down | KeyCode::Char('j') => ui.move_section(1),
                KeyCode::Up | KeyCode::Char('k') => ui.move_section(-1),
                KeyCode::Enter => ui.pane = Pane::List,
                _ => self.item_action(key),
            },
            Pane::List => match key.code {
                KeyCode::Down | KeyCode::Char('j') => ui.move_list(1),
                KeyCode::Up | KeyCode::Char('k') => ui.move_list(-1),
                KeyCode::PageDown => ui.move_list(page),
                KeyCode::PageUp => ui.move_list(-page),
                KeyCode::Home | KeyCode::Char('g') => ui.move_list(-isize::MAX / 2),
                KeyCode::End | KeyCode::Char('G') => ui.move_list(isize::MAX / 2),
                KeyCode::Enter => ui.pane = Pane::Detail,
                _ => self.item_action(key),
            },
            Pane::Detail => match key.code {
                KeyCode::Down | KeyCode::Char('j') => ui.move_line(1),
                KeyCode::Up | KeyCode::Char('k') => ui.move_line(-1),
                KeyCode::Enter | KeyCode::Char('y') => self.copy_line(),
                KeyCode::Char('r') | KeyCode::Char(' ') => self.reveal_line(),
                KeyCode::Char('o') => self.save_attachment(),
                _ => self.item_action(key),
            },
        }
    }

    /// Keys that act on the picked item from any pane.
    fn item_action(&mut self, key: KeyEvent) {
        let Screen::Vault(ui) = &self.screen else { return };
        let trash = ui.in_trash();
        let command = match key.code {
            KeyCode::Char('n') => Command::New,
            KeyCode::Char('e') if !trash => Command::Edit,
            KeyCode::Char('c') => Command::CopyPassword,
            KeyCode::Char('u') if !trash => Command::CopyUsername,
            KeyCode::Char('t') => Command::CopyCode,
            KeyCode::Char('f') if !trash => Command::Favourite,
            KeyCode::Char('p') if !trash => Command::Pin,
            KeyCode::Char('m') if !trash => Command::MoveFolder,
            KeyCode::Char('+') if !trash => Command::AddTag,
            KeyCode::Char('-') if !trash => Command::RemoveTag,
            KeyCode::Char('a') if !trash => Command::Attach,
            KeyCode::Char('d') | KeyCode::Delete if !trash => Command::Trash,
            KeyCode::Char('u') if trash => return self.restore(),
            KeyCode::Char('D') | KeyCode::Delete if trash => return self.ask_purge(),
            KeyCode::Char('r') => return self.reveal_main(),
            _ => return,
        };
        self.command(command);
    }

    fn open_palette(&mut self) {
        self.overlay = Some(Overlay::Palette { input: Input::new(), at: 0 });
    }

    /// Runs a command.
    pub fn command(&mut self, command: Command) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let picked = ui.current_id();
        let need = |app: &mut Self| -> Option<String> {
            if picked.is_none() {
                app.say(Level::Info, "Pick an item first");
            }
            picked.clone()
        };
        match command {
            Command::New => {
                let glyphs = self.theme.g;
                let items = vault::KINDS.iter().map(|(k, name)| ((*k).to_owned(), format!("{}  {name}", glyphs.kind(k)))).collect();
                self.overlay = Some(Overlay::Pick { title: "New item".into(), items, at: 0, then: PickFor::NewKind });
            }
            Command::Edit => {
                if let Some(detail) = ui.detail.clone() {
                    self.overlay = Some(Overlay::Editor(Box::new(Editor::edit(&detail))));
                } else {
                    self.say(Level::Info, "Pick an item first");
                }
            }
            Command::Trash => {
                if let Some(row) = ui.current().cloned() {
                    self.overlay = Some(Overlay::Confirm {
                        title: "Move to the trash?".into(),
                        body: format!("{} stays in the trash for a while and can be brought back.", row.title),
                        act: Act::Trash(row.id),
                        danger: false,
                    });
                }
            }
            Command::Duplicate => {
                let Some(id) = need(self) else { return };
                let Screen::Vault(ui) = &mut self.screen else { return };
                match ui.engine.duplicate(&id) {
                    Ok(copy) => {
                        ui.reload();
                        ui.refresh_list(Some(copy));
                        self.say(Level::Good, "Duplicated");
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            Command::Favourite | Command::Pin => {
                let Some(row) = ui.current().cloned() else { return };
                let (key, now) = if command == Command::Favourite { ("favourite", !row.favourite) } else { ("pinned", !row.pinned) };
                match ui.engine.set_common(&row.id, json!({ key: now })) {
                    Ok(()) => {
                        ui.reload();
                        let what = match (command, now) {
                            (Command::Favourite, true) => "Added to favourites",
                            (Command::Favourite, false) => "Taken out of favourites",
                            (_, true) => "Pinned",
                            (_, false) => "Unpinned",
                        };
                        self.say(Level::Good, what);
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            Command::MoveFolder => {
                let Some(id) = need(self) else { return };
                let Screen::Vault(ui) = &self.screen else { return };
                let mut items = vec![(String::new(), "—  No folder".to_owned())];
                let mut folders = ui.folders.clone();
                folders.sort_by_key(|f| f.name.to_lowercase());
                items.extend(folders.into_iter().map(|f| (f.id, format!("▸  {}", f.name))));
                items.push(("+".into(), "+  New folder…".into()));
                self.overlay = Some(Overlay::Pick { title: "Move to folder".into(), items, at: 0, then: PickFor::MoveFolder(id) });
            }
            Command::AddTag => {
                let Some(id) = need(self) else { return };
                self.prompt("Add a tag", "One word or a few, such as work", false, PromptFor::AddTag(id));
            }
            Command::RemoveTag => {
                let Some(row) = ui.current().cloned() else { return };
                if row.tags.is_empty() {
                    return self.say(Level::Info, "It has no tags");
                }
                let items = row.tags.iter().map(|t| (t.clone(), format!("#  {t}"))).collect();
                self.overlay = Some(Overlay::Pick { title: "Remove a tag".into(), items, at: 0, then: PickFor::RemoveTag(row.id) });
            }
            Command::NewFolder => self.prompt("New folder", "Its name", false, PromptFor::NewFolder),
            Command::CopyPassword => self.copy_main(),
            Command::CopyUsername => self.copy_username(),
            Command::CopyCode => self.copy_code(),
            Command::Import => self.prompt(
                "Import",
                "Path of a file from another app: Bitwarden, 1Password, KeePass, Chrome, Aegis and more",
                false,
                PromptFor::ImportPath,
            ),
            Command::Export => {
                let items = vec![
                    ("kdbx".into(), "KeePass (.kdbx), with a password of its own".into()),
                    ("csv".into(), "CSV, readable by anyone who has the file".into()),
                    ("bitwarden".into(), "Bitwarden JSON, readable by anyone who has the file".into()),
                    ("html".into(), "A page to print, readable by anyone who has the file".into()),
                ];
                self.overlay = Some(Overlay::Pick { title: "Export".into(), items, at: 0, then: PickFor::ExportFormat });
            }
            Command::Attach => {
                let Some(id) = need(self) else { return };
                self.prompt("Attach a file", "Its path. It is sealed with the vault's key", false, PromptFor::AttachAdd(id));
            }
            Command::Shelf => {
                if ui.shelf_open {
                    let _ = ui.engine.close_shelf();
                    ui.shelf_open = false;
                    ui.reload();
                    self.say(Level::Info, "Hidden shelf closed");
                } else {
                    self.prompt("Open the hidden shelf", "Your vault password", true, PromptFor::ShelfPassword);
                }
            }
            Command::Theme => {
                self.theme = self.theme.next();
                self.prefs.theme = self.theme.name.to_owned();
                self.prefs.save();
                self.push_setting("tui_theme");
                let name = self.theme.name;
                self.say(Level::Info, format!("Theme: {name}"));
            }
            Command::Icons => {
                self.theme.g.nerd = !self.theme.g.nerd;
                self.prefs.icons = self.theme.g.name().to_owned();
                self.prefs.save();
                self.push_setting("tui_icons");
                let text = if self.theme.g.nerd {
                    "Nerd Font icons on. Boxes instead of icons mean the terminal's font has none"
                } else {
                    "Plain icons"
                };
                self.say(Level::Info, text);
            }
            Command::SyncSettings => {
                self.prefs.sync_settings = !self.prefs.sync_settings;
                self.prefs.save();
                if self.prefs.sync_settings {
                    self.pull_settings();
                    self.say(Level::Info, "Theme, icons, reveal time and Health choices now follow the vault");
                } else {
                    self.say(Level::Info, "Settings stay on this computer");
                }
            }
            Command::SkipLocal => {
                self.prefs.health_skip_local = !self.prefs.health_skip_local;
                self.prefs.save();
                self.push_setting("health_skip_local");
                self.refresh_health();
                let text = if self.prefs.health_skip_local {
                    "Health leaves out logins only for your router, NAS or other devices at home"
                } else {
                    "Health checks logins for devices at home too"
                };
                self.say(Level::Info, text);
            }
            Command::LockAfter => {
                let mut input = Input::with(&self.prefs.lock_minutes.to_string());
                input.multiline = false;
                self.overlay = Some(Overlay::Prompt {
                    title: "Lock after".into(),
                    hint: "Minutes without a key press. 0 never locks".into(),
                    input,
                    secret: false,
                    then: PromptFor::LockMinutes,
                });
            }
            Command::Items => self.switch_tab(Tab::Items),
            Command::Codes => self.switch_tab(Tab::Codes),
            Command::Generator => self.switch_tab(Tab::Generator),
            Command::Health => self.switch_tab(Tab::Health),
            Command::Help => self.overlay = Some(Overlay::Help),
            Command::Lock => self.lock("Locked"),
            Command::IntoApp => {
                if ui.engine.app_folder().is_some() {
                    return self.say(Level::Info, "This is already the WM Pass app's vault");
                }
                let Some(root) = crate::app_folder::default_root() else {
                    return self.say(Level::Bad, "No WM Pass app folder on this computer");
                };
                let old = ui.engine.path().to_owned();
                match ui.engine.move_into(&root) {
                    Ok(()) => {
                        self.prefs.recent.retain(|p| *p != old && std::fs::canonicalize(&old).ok().as_ref() != Some(p));
                        self.prefs.remember(&root);
                        self.follow_app_timers(&root);
                        self.say(Level::Good, format!("The app and this terminal now share it. {} is no longer used; delete it once you are sure", old.display()));
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            Command::AddDevice => {
                if let Screen::Vault(ui) = &self.screen {
                    self.overlay = Some(Overlay::Link(Box::new(LinkUi::send(ui.engine.path().to_owned()))));
                }
            }
            Command::Quit => self.quit = true,
        }
    }

    fn prompt(&mut self, title: &str, hint: &str, secret: bool, then: PromptFor) {
        self.overlay = Some(Overlay::Prompt { title: title.into(), hint: hint.into(), input: Input::new(), secret, then });
    }

    fn clip_for(&self) -> Duration {
        Duration::from_secs(u64::from(self.prefs.clip_seconds))
    }

    fn copied(&mut self, what: &str, route: Result<Route, String>, secret: bool) {
        match route {
            Ok(route) => {
                let tail = match (secret, self.prefs.clip_seconds) {
                    (true, n) if n > 0 => format!(", cleared in {n} s"),
                    _ => String::new(),
                };
                let via = if route == Route::Terminal { " through the terminal" } else { "" };
                self.say(Level::Good, format!("{what} copied{via}{tail}"));
            }
            Err(e) => self.say(Level::Bad, e),
        }
    }

    fn copy_secret(&mut self, what: &str, secret: &Secret) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let Some(id) = ui.current_id() else { return };
        let value = match ui.engine.reveal(secret, &id) {
            Ok(value) => value,
            Err(e) => return self.say(Level::Bad, e),
        };
        let wait = self.clip_for();
        let route = self.clip.copy(&value, true, wait);
        self.copied(what, route, true);
    }

    /// Copies the item's main secret: a password, a card number, a key.
    fn copy_main(&mut self) {
        let Screen::Vault(ui) = &self.screen else { return };
        let Some(detail) = &ui.detail else { return self.say(Level::Info, "Pick an item first") };
        let main = detail.fields.iter().find(|f| f.secret && f.has_value).map(|f| (f.label.clone(), f.key.clone()));
        let has_code = detail.otp.is_some();
        match main {
            Some((label, key)) => self.copy_secret(&label, &Secret::Field(key)),
            None if has_code => self.copy_code(),
            None => self.say(Level::Info, "Nothing secret to copy here"),
        }
    }

    fn copy_username(&mut self) {
        let Screen::Vault(ui) = &self.screen else { return };
        let Some(detail) = &ui.detail else { return };
        let user = detail
            .fields
            .iter()
            .find(|f| matches!(f.key.as_str(), "username" | "email" | "holder" | "user" | "ssid") && f.has_value)
            .and_then(|f| f.value.as_ref()?.as_str().map(str::to_owned));
        match user {
            Some(user) => {
                let route = self.clip.copy(&user, false, Duration::ZERO);
                self.copied("Username", route, false);
            }
            None => self.say(Level::Info, "No username here"),
        }
    }

    fn copy_code(&mut self) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let Some(id) = ui.current_id() else { return };
        let hotp = ui.detail.as_ref().and_then(|d| d.otp.as_ref()).is_some_and(|o| o.kind == "hotp");
        if ui.detail.as_ref().is_some_and(|d| d.otp.is_none()) {
            return self.say(Level::Info, "No one-time code here");
        }
        let code = if hotp { ui.engine.hotp_next(&id) } else { ui.engine.reveal(&Secret::Code, &id) };
        match code {
            Ok(code) => {
                let wait = self.clip_for();
                let route = self.clip.copy(&code, true, wait);
                self.copied("Code", route, true);
            }
            Err(e) => self.say(Level::Bad, e),
        }
    }

    fn reveal_main(&mut self) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let Some(at) = ui.lines.iter().position(|l| matches!(l, Line::Value { secret: Some(_), has_value: true, .. })) else {
            return;
        };
        ui.line = at;
        self.reveal_line();
    }

    fn reveal_line(&mut self) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let Some(line) = ui.lines.get(ui.line).cloned() else { return };
        match line {
            Line::Value { secret: Some(secret), has_value: true, .. } => {
                if let Err(e) = ui.toggle_reveal(secret) {
                    self.say(Level::Bad, e);
                }
            }
            Line::Past { index, .. } => {
                if let Err(e) = ui.toggle_reveal(Secret::History(index)) {
                    self.say(Level::Bad, e);
                }
            }
            _ => {}
        }
    }

    fn copy_line(&mut self) {
        let Screen::Vault(ui) = &self.screen else { return };
        let Some(line) = ui.lines.get(ui.line).cloned() else { return };
        match line {
            Line::Value { secret: Some(secret), label, has_value: true, .. } => self.copy_secret(&label, &secret),
            Line::Value { text: Some(text), label, .. } => {
                let route = self.clip.copy(&text, false, Duration::ZERO);
                let label = if label.is_empty() { "Value".to_owned() } else { label };
                self.copied(&label, route, false);
            }
            Line::Code => self.copy_code(),
            Line::Past { index, field, .. } => self.copy_secret(&format!("Earlier {field}"), &Secret::History(index)),
            Line::File(_) => self.save_attachment(),
            _ => {}
        }
    }

    fn save_attachment(&mut self) {
        let Screen::Vault(ui) = &self.screen else { return };
        let (Some(Line::File(file)), Some(id)) = (ui.lines.get(ui.line), ui.current_id()) else {
            return self.say(Level::Info, "Pick a file in the item first");
        };
        let file = file.clone();
        let mut input = Input::new();
        let dir = std::env::current_dir().unwrap_or_default();
        input.set(&dir.join(&file.name).to_string_lossy());
        self.overlay = Some(Overlay::Prompt {
            title: format!("Save {}", file.name),
            hint: "Where to write it, unencrypted".into(),
            input,
            secret: false,
            then: PromptFor::AttachSave { id, blob: file.blob, name: file.name },
        });
    }

    fn restore(&mut self) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let Some(id) = ui.current_id() else { return };
        match ui.engine.restore(&id) {
            Ok(()) => {
                ui.reload();
                self.say(Level::Good, "Brought back");
            }
            Err(e) => self.say(Level::Bad, e),
        }
    }

    fn ask_purge(&mut self) {
        let Screen::Vault(ui) = &self.screen else { return };
        let Some(row) = ui.current().cloned() else { return };
        self.overlay = Some(Overlay::Confirm {
            title: "Delete for good?".into(),
            body: format!("{} and its files go now, on this device and, at the next sync, everywhere else. This cannot be undone.", row.title),
            act: Act::Purge(row.id),
            danger: true,
        });
    }

    fn codes_key(&mut self, key: KeyEvent) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let cols = usize::from(ui.areas.code_cols.max(1));
        let last = ui.codes.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('/') => ui.filtering_codes = true,
            KeyCode::Right | KeyCode::Char('l') => ui.code_at = (ui.code_at + 1).min(last),
            KeyCode::Left | KeyCode::Char('h') => ui.code_at = ui.code_at.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => ui.code_at = (ui.code_at + cols).min(last),
            KeyCode::Up | KeyCode::Char('k') => ui.code_at = ui.code_at.saturating_sub(cols),
            KeyCode::Enter | KeyCode::Char('c' | 'y') => {
                let Some(code) = ui.codes.get(ui.code_at) else { return };
                let (id, hotp, title) = (code.id.clone(), code.otp.kind == "hotp", code.title.clone());
                let value = if hotp { ui.engine.hotp_next(&id) } else { ui.engine.reveal(&Secret::Code, &id) };
                match value {
                    Ok(value) => {
                        let wait = self.clip_for();
                        let route = self.clip.copy(&value, true, wait);
                        self.copied(&format!("Code for {title}"), route, true);
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            KeyCode::Char('o') => {
                if let Some(id) = ui.codes.get(ui.code_at).map(|c| c.id.clone()) {
                    ui.show_entry(&id);
                }
            }
            _ => {}
        }
    }

    fn regenerate(&mut self) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let recipe = ui.generator.recipe();
        match ui.engine.generate(&recipe) {
            Ok((value, bits)) => {
                ui.generator.strength = ui.engine.strength(&value).ok();
                ui.generator.value = value;
                ui.generator.bits = bits;
            }
            Err(e) => self.say(Level::Bad, e),
        }
    }

    fn generator_key(&mut self, key: KeyEvent) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let generator = &mut ui.generator;
        let knobs = generator.knobs().len();
        let fast = if key.modifiers.contains(KeyModifiers::SHIFT) { 4 } else { 1 };
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => generator.at = (generator.at + 1).min(knobs - 1),
            KeyCode::Up | KeyCode::Char('k') => generator.at = generator.at.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l' | 'L') => {
                generator.adjust(fast);
                self.regenerate();
            }
            KeyCode::Left | KeyCode::Char('h' | 'H') => {
                generator.adjust(-fast);
                self.regenerate();
            }
            KeyCode::Char(' ') => {
                generator.adjust(1);
                self.regenerate();
            }
            KeyCode::Enter | KeyCode::Char('g' | 'r') => self.regenerate(),
            KeyCode::Char('c' | 'y') => {
                let value = ui.generator.value.clone();
                if !value.is_empty() {
                    let wait = self.clip_for();
                    let route = self.clip.copy(&value, true, wait);
                    self.copied("Generated value", route, true);
                }
            }
            _ => {}
        }
    }

    fn health_key(&mut self, key: KeyEvent) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let items = ui.health_items(ui.health_group);
        let ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                ui.health_group = ui.health_group.saturating_sub(1);
                ui.health_at = 0;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                ui.health_group = (ui.health_group + 1).min(ui.health_groups() - 1);
                ui.health_at = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => ui.health_at = (ui.health_at + 1).min(ids.len().saturating_sub(1)),
            KeyCode::Up | KeyCode::Char('k') => ui.health_at = ui.health_at.saturating_sub(1),
            KeyCode::Char('r') => {
                self.refresh_health();
                self.say(Level::Info, "Checked again");
            }
            KeyCode::Char('i') => {
                let group = ui.health_group;
                let Some((id, ignored_for)) = items.get(ui.health_at).cloned() else { return };
                let title = ui.titles.get(&id).map_or_else(|| "A deleted item".to_owned(), |(title, _)| title.clone());
                let (check, ignore) = match ignored_for {
                    Some(check) => (check, false),
                    None => (HEALTH_GROUPS[group].2.to_owned(), true),
                };
                if check.is_empty() {
                    return;
                }
                self.prefs.ignore_health(&check, &id, ignore);
                self.prefs.save();
                self.push_setting("health_ignored");
                self.refresh_health();
                let label = crate::vault::check_label(&check);
                let text = if ignore {
                    format!("{title} no longer counts under {label}. It is listed under Ignored")
                } else {
                    format!("{title} counts under {label} again")
                };
                self.say(Level::Info, text);
            }
            KeyCode::Enter => {
                if let Some(id) = ids.get(ui.health_at) {
                    let id = id.clone();
                    ui.show_entry(&id);
                }
            }
            _ => {}
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) {
        let Some(mut overlay) = self.overlay.take() else { return };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let mut save = false;
        let keep = match &mut overlay {
            Overlay::Help => !matches!(key.code, KeyCode::Esc | KeyCode::Char('?' | 'q') | KeyCode::Enter),
            Overlay::Palette { input, at } => match key.code {
                KeyCode::Esc => false,
                KeyCode::Down => {
                    *at = (*at + 1).min(palette_matches(input.text()).len().saturating_sub(1));
                    true
                }
                KeyCode::Up => {
                    *at = at.saturating_sub(1);
                    true
                }
                KeyCode::Enter => {
                    if let Some((command, ..)) = palette_matches(input.text()).get(*at) {
                        self.command(*command);
                    }
                    return;
                }
                _ => {
                    if input.key(key) == crate::input::Edit::Changed {
                        *at = 0;
                    }
                    true
                }
            },
            Overlay::Confirm { act, .. } => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                    let act = act.clone();
                    self.confirmed(act);
                    return;
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => false,
                _ => true,
            },
            Overlay::Prompt { input, then, .. } => match key.code {
                KeyCode::Esc => false,
                KeyCode::Enter => {
                    let answer = input.take();
                    let then = then.clone();
                    self.answered(then, answer);
                    return;
                }
                _ => {
                    input.key(key);
                    true
                }
            },
            Overlay::Pick { items, at, then, .. } => match key.code {
                KeyCode::Esc => false,
                KeyCode::Down | KeyCode::Char('j') => {
                    *at = (*at + 1).min(items.len().saturating_sub(1));
                    true
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *at = at.saturating_sub(1);
                    true
                }
                KeyCode::Enter => {
                    let choice = items.get(*at).map(|i| i.0.clone()).unwrap_or_default();
                    let then = then.clone();
                    self.picked(then, choice);
                    return;
                }
                _ => true,
            },
            Overlay::Import(_) => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                    self.confirmed(Act::Import);
                    return;
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    if let Screen::Vault(ui) = &mut self.screen {
                        ui.engine.import_cancel();
                    }
                    false
                }
                _ => true,
            },
            Overlay::Link(link) => match self.link_key(link, key) {
                Some(keep) => keep,
                None => return,
            },
            Overlay::Editor(editor) => {
                let row_multiline = editor.rows.get(editor.at).is_some_and(|r| r.value.multiline);
                match key.code {
                    KeyCode::Esc => false,
                    KeyCode::Char('s') if ctrl => {
                        save = true;
                        true
                    }
                    KeyCode::Enter if !row_multiline && editor.at + 1 == editor.rows.len() => {
                        save = true;
                        true
                    }
                    KeyCode::Tab | KeyCode::Down => {
                        editor.at = (editor.at + 1).min(editor.rows.len() - 1);
                        true
                    }
                    KeyCode::Enter if !row_multiline => {
                        editor.at = (editor.at + 1).min(editor.rows.len() - 1);
                        true
                    }
                    KeyCode::BackTab | KeyCode::Up => {
                        editor.at = editor.at.saturating_sub(1);
                        true
                    }
                    KeyCode::Char('r') if ctrl => {
                        editor.show = !editor.show;
                        if editor.show
                            && let Screen::Vault(ui) = &mut self.screen
                            && let Err(e) = editor.load_secret(&mut ui.engine)
                        {
                            editor.error = Some(e);
                        }
                        true
                    }
                    KeyCode::Char('g') if ctrl => {
                        let row = editor.row();
                        if row.input == "password"
                            && let Screen::Vault(ui) = &mut self.screen
                        {
                            let recipe = ui.generator.recipe();
                            if let Ok((value, _)) = ui.engine.generate(&recipe) {
                                row.value.set(&value);
                                editor.show = true;
                            }
                        }
                        true
                    }
                    _ => {
                        let row = editor.row();
                        match row.input.as_str() {
                            "toggle" if matches!(key.code, KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right) => {
                                row.flag = !row.flag;
                            }
                            "choice" if matches!(key.code, KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right) => {
                                let at = row.choices.iter().position(|c| c == row.value.text()).unwrap_or(0);
                                let len = row.choices.len().max(1);
                                let next = if key.code == KeyCode::Left { (at + len - 1) % len } else { (at + 1) % len };
                                if let Some(choice) = row.choices.get(next).cloned() {
                                    row.value.set(&choice);
                                }
                            }
                            "toggle" | "choice" => {}
                            _ => {
                                row.value.key(key);
                            }
                        }
                        editor.error = None;
                        true
                    }
                }
            }
        };
        if save
            && let Overlay::Editor(editor) = &mut overlay
            && self.save_editor(editor)
        {
            return;
        }
        if keep && self.overlay.is_none() {
            self.overlay = Some(overlay);
        }
    }

    /// Saves the editor. Returns whether it is done with.
    fn save_editor(&mut self, editor: &mut Editor) -> bool {
        let Screen::Vault(ui) = &mut self.screen else { return true };
        let new = editor.id.is_none();
        match editor.save(&mut ui.engine) {
            Ok(id) => {
                ui.reload();
                ui.refresh_list(Some(id));
                self.say(Level::Good, if new { "Saved" } else { "Changes saved" });
                true
            }
            Err(e) => {
                editor.error = Some(e);
                false
            }
        }
    }

    fn confirmed(&mut self, act: Act) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let result = match act {
            Act::Trash(id) => ui.engine.trash(&id).map(|()| "Moved to the trash. Restore it from Trash with u".to_owned()),
            Act::Purge(id) => ui.engine.purge(&id).map(|()| "Deleted for good".to_owned()),
            Act::Import => ui
                .engine
                .import_apply()
                .map(|(added, dupes)| format!("Imported {added}; {dupes} were already here")),
            Act::Export(format, path) => {
                let password = Zeroizing::new(String::new());
                Self::write_export(ui, &format, &path, &password)
            }
        };
        ui.reload();
        match result {
            Ok(text) => self.say(Level::Good, text),
            Err(e) => self.say(Level::Bad, e),
        }
    }

    fn write_export(ui: &mut VaultUi, format: &str, path: &std::path::Path, password: &str) -> Result<String, String> {
        let bytes = ui.engine.export(format, password.as_bytes())?;
        store::write_new(path, &bytes)?;
        Ok(if format == "kdbx" {
            format!("Wrote {}", path.display())
        } else {
            format!("Wrote {}. It holds your secrets in the clear: delete it when done", path.display())
        })
    }

    fn picked(&mut self, then: PickFor, choice: String) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        match then {
            PickFor::NewKind => {
                let fields: Vec<engine::FieldRow> = wmpass_session::fields::fields_of(&choice)
                    .iter()
                    .map(|f| engine::FieldRow {
                        key: f.key.to_owned(),
                        label: f.label.to_owned(),
                        input: serde_json::to_value(f.input).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default(),
                        secret: f.secret,
                        choices: f.choices.iter().map(|c| (*c).to_owned()).collect(),
                        has_value: false,
                        value: None,
                    })
                    .collect();
                let mut editor = Editor::new(&choice, &fields);
                if let Some(tag) = ui.sections.get(ui.section).filter(|s| s.query.starts_with("tag:"))
                    && let Some(row) = editor.rows.iter_mut().find(|r| r.slot == Slot::Tags)
                {
                    row.value.set(&tag.label);
                }
                self.overlay = Some(Overlay::Editor(Box::new(editor)));
            }
            PickFor::MoveFolder(_) if choice == "+" => {
                self.prompt("New folder", "Its name. The item moves into it", false, PromptFor::NewFolder);
            }
            PickFor::MoveFolder(id) => {
                let folder = (!choice.is_empty()).then_some(choice);
                match ui.engine.set_common(&id, json!({"folder": folder})) {
                    Ok(()) => {
                        ui.reload();
                        self.say(Level::Good, "Moved");
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PickFor::RemoveTag(id) => {
                let tags: Vec<String> = ui.current().map(|r| r.tags.iter().filter(|t| **t != choice).cloned().collect()).unwrap_or_default();
                match ui.engine.set_common(&id, json!({"tags": tags})) {
                    Ok(()) => {
                        ui.reload();
                        self.say(Level::Good, format!("Removed #{choice}"));
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PickFor::ExportFormat => {
                let name = ui.engine.path().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "vault".into());
                let ext = match choice.as_str() {
                    "bitwarden" => "json",
                    other => other,
                };
                let dir = std::env::current_dir().unwrap_or_default();
                let mut input = Input::new();
                input.set(&dir.join(format!("{name}-export.{ext}")).to_string_lossy());
                self.overlay = Some(Overlay::Prompt {
                    title: "Export to".into(),
                    hint: "A new file. An existing one is never replaced".into(),
                    input,
                    secret: false,
                    then: PromptFor::ExportPath(choice),
                });
            }
        }
    }

    fn answered(&mut self, then: PromptFor, answer: Zeroizing<String>) {
        let Screen::Vault(ui) = &mut self.screen else { return };
        let text = answer.trim();
        match then {
            PromptFor::NewFolder => {
                if text.is_empty() {
                    return;
                }
                let picked = ui.current_id();
                match ui.engine.add_folder(text) {
                    Ok(folder) => {
                        if let Some(id) = picked {
                            let _ = ui.engine.set_common(&id, json!({"folder": folder}));
                        }
                        ui.reload();
                        self.say(Level::Good, format!("Folder {text} made"));
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PromptFor::AddTag(id) => {
                if text.is_empty() {
                    return;
                }
                let mut tags = ui.current().map(|r| r.tags.clone()).unwrap_or_default();
                let tag = text.trim_start_matches('#').to_owned();
                if !tags.contains(&tag) {
                    tags.push(tag.clone());
                }
                match ui.engine.set_common(&id, json!({"tags": tags})) {
                    Ok(()) => {
                        ui.reload();
                        self.say(Level::Good, format!("Tagged #{tag}"));
                    }
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PromptFor::ImportPath => {
                let path = store::expand(text);
                let data = match store::read(&path) {
                    Ok(data) => Zeroizing::new(data),
                    Err(e) => return self.say(Level::Bad, e),
                };
                if wmpass_core::io::needs_password(&data) {
                    return self.prompt("File password", "The password of the file being imported", true, PromptFor::ImportPassword(path));
                }
                match ui.engine.import_read(&data, &[]) {
                    Ok(preview) => self.overlay = Some(Overlay::Import(preview)),
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PromptFor::ImportPassword(path) => {
                let data = match store::read(&path) {
                    Ok(data) => Zeroizing::new(data),
                    Err(e) => return self.say(Level::Bad, e),
                };
                match ui.engine.import_read(&data, answer.as_bytes()) {
                    Ok(preview) => self.overlay = Some(Overlay::Import(preview)),
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PromptFor::ExportPath(format) => {
                let path = store::expand(text);
                if path.exists() {
                    return self.say(Level::Bad, format!("{} already exists", path.display()));
                }
                if format == "kdbx" {
                    self.prompt("Password for the export", "KeePass will ask for it", true, PromptFor::ExportPassword(format, path));
                } else {
                    self.overlay = Some(Overlay::Confirm {
                        title: "Write every secret in the clear?".into(),
                        body: format!(
                            "{} will hold every password and note unencrypted. Anyone or anything that can read the file can read them.",
                            path.display()
                        ),
                        act: Act::Export(format, path),
                        danger: true,
                    });
                }
            }
            PromptFor::ExportPassword(format, path) => {
                if answer.chars().count() < 8 {
                    return self.say(Level::Bad, "Use at least 8 characters for the export's password");
                }
                let result = Self::write_export(ui, &format, &path, &answer);
                match result {
                    Ok(text) => self.say(Level::Good, text),
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PromptFor::AttachAdd(id) => match ui.engine.add_attachment(&id, &store::expand(text)) {
                Ok(()) => {
                    ui.reload();
                    self.say(Level::Good, "Attached");
                }
                Err(e) => self.say(Level::Bad, e),
            },
            PromptFor::AttachSave { id, blob, name } => {
                let path = store::expand(text);
                let result = ui.engine.open_attachment(&id, &blob).and_then(|plain| store::write_new(&path, &plain));
                match result {
                    Ok(()) => self.say(Level::Good, format!("Saved {name}")),
                    Err(e) => self.say(Level::Bad, e),
                }
            }
            PromptFor::ShelfPassword => match ui.engine.open_shelf(answer.as_bytes()) {
                Ok(()) => {
                    ui.shelf_open = true;
                    ui.reload();
                    if let Some(at) = ui.sections.iter().position(|s| s.query == "hidden") {
                        ui.section = at;
                        ui.refresh_list(None);
                    }
                    self.say(Level::Good, "Hidden shelf open until you lock");
                }
                Err(e) => self.say(Level::Bad, e),
            },
            PromptFor::LockMinutes => match text.parse::<u32>() {
                Ok(minutes) => {
                    self.prefs.lock_minutes = minutes.min(24 * 60);
                    self.prefs.save();
                    let text = if minutes == 0 { "Never locks on its own".to_owned() } else { format!("Locks after {minutes} minutes") };
                    self.say(Level::Good, text);
                }
                Err(_) => self.say(Level::Bad, "That is not a number of minutes"),
            },
        }
    }

    /// A key on the screen that moves a vault. Returns whether the screen stays, or `None` when
    /// this has already decided what is open.
    fn link_key(&mut self, link: &mut LinkUi, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (&link.stage, key.code) {
            (Stage::Received { path, .. }, KeyCode::Enter | KeyCode::Esc) => {
                // A vault that went into the app's folder opens as the app's.
                let path = crate::app_folder::root_of(path).unwrap_or_else(|| path.clone());
                if let Screen::Door(door) = &mut self.screen {
                    if door.mode == door::Mode::Create {
                        door.toggle_mode();
                    }
                    door.path.set(&path.to_string_lossy());
                    door.focus = door::Field::Password;
                    door.notice = Some("Your vault is here. Unlock it with its master password".into());
                }
                Some(false)
            }
            (Stage::Sent { .. }, KeyCode::Enter | KeyCode::Esc) => {
                self.say(Level::Good, "Sent. Unlock it on the other device with the master password");
                Some(false)
            }
            (Stage::Failed(_), KeyCode::Char('r') | KeyCode::Enter) => {
                link.restart();
                Some(true)
            }
            (Stage::Digits { .. }, KeyCode::Char('y' | 'Y') | KeyCode::Enter) => {
                link.answer(true);
                Some(true)
            }
            (Stage::Digits { .. }, KeyCode::Char('n' | 'N')) => {
                link.answer(false);
                Some(true)
            }
            (Stage::Waiting { code: Some(code), .. }, KeyCode::Char('c' | 'y')) if !ctrl => {
                let code = code.clone();
                let wait = self.clip_for();
                let route = self.clip.copy(&code, true, wait);
                self.copied("Code", route, true);
                Some(true)
            }
            (Stage::Waiting { .. }, KeyCode::Char('z')) => {
                // Flip whatever was drawn last; with no choice yet, that was what fitted.
                let big = !link.big.unwrap_or(self.link_drew_big);
                link.big = Some(big);
                Some(true)
            }
            (Stage::Searching, KeyCode::Up) => {
                link.at = link.at.saturating_sub(1);
                Some(true)
            }
            (Stage::Searching, KeyCode::Down) => {
                link.at = (link.at + 1).min(link.found.len().saturating_sub(1));
                Some(true)
            }
            (Stage::Searching, KeyCode::Enter) => {
                if link.code.is_empty() {
                    link.send_to_picked();
                } else {
                    link.send_to_code();
                }
                Some(true)
            }
            (Stage::Searching, KeyCode::Esc) if !link.code.is_empty() => {
                link.code.clear();
                Some(true)
            }
            (_, KeyCode::Esc) => Some(false),
            (Stage::Searching, _) => {
                link.code.key(key);
                Some(true)
            }
            _ => Some(true),
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        let at = Position::new(mouse.column, mouse.row);
        let landed = crate::draw::hit_at(&self.hits, self.overlay_from, at);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let double = self
                    .last_click
                    .is_some_and(|(where_, when)| where_ == at && when.elapsed() < Duration::from_millis(400));
                self.last_click = Some((at, Instant::now()));
                match landed {
                    // Outside an open overlay: the light ones close, the rest stay.
                    Some(None) => {
                        if matches!(self.overlay, Some(Overlay::Help | Overlay::Palette { .. } | Overlay::Pick { .. })) {
                            self.overlay = None;
                        }
                    }
                    Some(Some(hit)) => self.click(hit, double),
                    None => {}
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let by: isize = if mouse.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                let landed = landed.flatten();
                self.scroll(by, landed, at);
            }
            _ => {}
        }
    }

    /// A left click on something drawn.
    fn click(&mut self, hit: Hit, double: bool) {
        let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
        match hit {
            Hit::Cover => {
                if matches!(self.overlay, Some(Overlay::Help)) {
                    self.overlay = None;
                }
            }
            Hit::Key(key) => self.key(key),
            Hit::DoorField(field) => {
                if let Screen::Door(door) = &mut self.screen {
                    door.focus = field;
                }
            }
            Hit::Recent(n) => {
                if let Screen::Door(door) = &mut self.screen {
                    door.recent = Some(n);
                }
                self.key(plain(KeyCode::Enter));
            }
            Hit::PaletteRow(n) => {
                if let Some(Overlay::Palette { at, .. }) = &mut self.overlay {
                    *at = n;
                }
                self.key(plain(KeyCode::Enter));
            }
            Hit::PickRow(n) => {
                if let Some(Overlay::Pick { at, .. }) = &mut self.overlay {
                    *at = n;
                }
                self.key(plain(KeyCode::Enter));
            }
            Hit::EditorRow(n) => {
                if let Some(Overlay::Editor(editor)) = &mut self.overlay {
                    editor.at = n.min(editor.rows.len().saturating_sub(1));
                    let row = editor.row();
                    if double && matches!(row.input.as_str(), "toggle" | "choice") {
                        self.key(plain(KeyCode::Char(' ')));
                    }
                }
            }
            Hit::LinkDevice(n) => {
                if let Some(Overlay::Link(link)) = &mut self.overlay {
                    link.at = n;
                    link.send_to_picked();
                }
            }
            Hit::Section(n) => {
                let Screen::Vault(ui) = &mut self.screen else { return };
                ui.pane = Pane::Sidebar;
                if n != ui.section {
                    ui.section = n;
                    ui.list.select(Some(0));
                    ui.refresh_list(None);
                }
            }
            Hit::Row(n) => {
                let Screen::Vault(ui) = &mut self.screen else { return };
                ui.pane = Pane::List;
                if ui.list.selected() != Some(n) {
                    ui.list.select(Some(n));
                    ui.load_detail();
                }
                if double {
                    ui.pane = Pane::Detail;
                }
            }
            Hit::Line(n) => {
                let Screen::Vault(ui) = &mut self.screen else { return };
                ui.pane = Pane::Detail;
                ui.line = n;
                if double {
                    self.copy_line();
                }
            }
            Hit::Code(n) => {
                let Screen::Vault(ui) = &mut self.screen else { return };
                ui.code_at = n;
                if double {
                    self.codes_key(plain(KeyCode::Enter));
                }
            }
            Hit::Knob(n) => {
                let Screen::Vault(ui) = &mut self.screen else { return };
                let same = ui.generator.at == n;
                ui.generator.at = n;
                if same || double {
                    self.generator_key(plain(KeyCode::Char(' ')));
                }
            }
            Hit::HealthGroup(n) => {
                let Screen::Vault(ui) = &mut self.screen else { return };
                ui.health_group = n;
                ui.health_at = 0;
            }
            Hit::HealthEntry(n) => {
                let Screen::Vault(ui) = &mut self.screen else { return };
                ui.health_at = n;
                if double {
                    self.health_key(plain(KeyCode::Enter));
                }
            }
        }
    }

    /// The wheel, over whatever is under the pointer.
    fn scroll(&mut self, by: isize, over: Option<Hit>, at: Position) {
        let step = |n: usize, by: isize, len: usize| n.saturating_add_signed(by).min(len.saturating_sub(1));
        match &mut self.overlay {
            Some(Overlay::Palette { input, at: picked }) => {
                *picked = step(*picked, by, palette_matches(input.text()).len());
                return;
            }
            Some(Overlay::Pick { items, at: picked, .. }) => {
                *picked = step(*picked, by, items.len());
                return;
            }
            Some(Overlay::Editor(editor)) => {
                editor.at = step(editor.at, by, editor.rows.len());
                return;
            }
            Some(Overlay::Link(link)) => {
                link.at = step(link.at, by, link.found.len());
                return;
            }
            Some(_) => return,
            None => {}
        }
        let Screen::Vault(ui) = &mut self.screen else { return };
        match ui.tab {
            Tab::Items => {
                if matches!(over, Some(Hit::Line(_))) || ui.areas.detail.contains(at) {
                    ui.move_line(by);
                } else if matches!(over, Some(Hit::Section(_))) || ui.areas.sidebar.contains(at) {
                    ui.move_section(by);
                } else {
                    ui.move_list(by);
                }
            }
            Tab::Codes => {
                let cols = ui.areas.code_cols.max(1) as isize;
                ui.code_at = step(ui.code_at, by * cols, ui.codes.len());
            }
            Tab::Generator => {
                // Over a setting, the wheel changes it; elsewhere it moves between them.
                if let Some(Hit::Knob(n)) = over {
                    ui.generator.at = n;
                    ui.generator.adjust(-by.signum() as i32);
                    self.regenerate();
                } else {
                    let len = ui.generator.knobs().len();
                    ui.generator.at = step(ui.generator.at, by, len);
                }
            }
            Tab::Health => {
                if let Some(Hit::HealthGroup(_)) = over {
                    ui.health_group = step(ui.health_group, by, ui.health_groups());
                    ui.health_at = 0;
                } else {
                    let len = ui.health_ids(ui.health_group).len();
                    ui.health_at = step(ui.health_at, by, len);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_close_matches() {
        let found = palette_matches("copy pa");
        assert_eq!(found[0].0, Command::CopyPassword);
        let found = palette_matches("gen");
        assert_eq!(found[0].0, Command::Generator);
        assert!(palette_matches("zzzz").is_empty());
        assert_eq!(palette_matches("").len(), COMMANDS.len());
    }
}
