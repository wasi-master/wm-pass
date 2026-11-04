//! Widgets on a Mac: the desktop and Notification Centre.
//!
//! The extension (`packaging/macos/widgets`) is sandboxed and asks this app for nothing: it cannot
//! reach the vault, the session or this process. This side writes what the widgets may show into
//! the extension's own folder, `~/Library/Containers/<extension>/Data/snapshot.json`, and asks
//! WidgetKit to redraw. An app group would be the usual way, but a widget reading one from an
//! unsigned app is refused by the system with no prompt, which Corvene found first.
//!
//! What the file may hold follows one rule: nothing from inside the vault unless the vault is open
//! and Show on widgets is on. Then it holds codes (never the secrets behind them) for at most the
//! time left before the vault would lock by itself, ten minutes at most, and the names of
//! favorites. Every way of locking writes a file without them at once, and so does starting, which
//! clears anything left behind by a crash. The extension also stops showing them at `hide_after`
//! by itself, in case no newer file reaches it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::{Engine, now_ms};
use crate::store::Store;

/// `packaging/macos/widgets/Info.plist`
pub const BUNDLE_ID: &str = "com.wasimaster.wmpass.desktop.widgets";
/// The widgets as `WMPassWidgets.swift` names them, for reloading one at a time.
pub const KINDS: [&str; 7] = ["quick", "status", "health", "backups", "generator", "codes", "favorites"];
/// The quick bar's buttons, as on the phone.
pub const BUTTONS: [&str; 6] = ["search", "add", "codes", "generator", "health", "lock"];
/// How far ahead codes go, at most.
pub const WINDOW_MS: u64 = 10 * 60 * 1000;
/// While vault content is shown, the file is written again this often, inside the window.
const REWRITE_MS: u64 = 8 * 60 * 1000;
const IDLE_MS: u64 = 30 * 60 * 1000;
const MAX_CODES: usize = 8;
const MAX_FAVORITES: usize = 12;
const FEED_FILE: &str = "settings/widgets.json";
const TOKEN_FILE: &str = "settings/widget-token.bin";

/// What the page tells this side. Nothing in it is secret: codes are fetched here, from the
/// session, and never pass through the page.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Feed {
    /// The gallery offers the widgets (`desktop_widgets`).
    pub enabled: bool,
    /// `none`, `locked` or `open`, as the page sees it. Open also needs an open handle here.
    pub state: String,
    pub vault: Option<String>,
    pub health: Option<Health>,
    pub show_score: bool,
    pub backups: Vec<BackupRow>,
    pub every_days: u32,
    pub buttons: Vec<String>,
    /// `lock` or `panic`.
    pub lock_action: String,
    /// Show on widgets (`widget_access`).
    pub access: bool,
    /// `upcoming` or `current`.
    pub codes_mode: String,
    /// The items whose codes the Codes widget shows, already worked out by the page.
    pub code_ids: Vec<String>,
    pub favorites: Vec<Favorite>,
    /// `password`, `passphrase` or `PIN`, for the Generator widget's words.
    pub generator: String,
    /// What `gen.generate` takes.
    pub recipe: Value,
    pub clipboard_seconds: u64,
}

impl Default for Feed {
    fn default() -> Self {
        Self {
            enabled: true,
            state: "locked".to_owned(),
            vault: None,
            health: None,
            show_score: true,
            backups: Vec::new(),
            every_days: 0,
            buttons: ["search", "add", "codes", "lock"].map(str::to_owned).to_vec(),
            lock_action: "lock".to_owned(),
            access: false,
            codes_mode: "upcoming".to_owned(),
            code_ids: Vec::new(),
            favorites: Vec::new(),
            generator: "password".to_owned(),
            recipe: json!({"Password": {"length": 20, "lower": true, "upper": true, "digits": true, "symbols": true, "exclude_ambiguous": false}}),
            clipboard_seconds: 30,
        }
    }
}

impl Feed {
    /// What is kept on disk between runs: no favorites and no vault name, which are the vault's.
    fn kept(&self) -> Self {
        Self { state: "locked".to_owned(), vault: None, favorites: Vec::new(), code_ids: Vec::new(), ..self.clone() }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Health {
    pub score: u32,
    pub weak: u32,
    pub reused: u32,
    pub breached: u32,
    /// When the counts were taken, in ms.
    pub at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BackupRow {
    pub label: String,
    pub last: Option<u64>,
    pub failed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Favorite {
    pub id: String,
    pub title: String,
    /// `#rrggbb`
    pub color: String,
}

/// One code for one stretch of time.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Step {
    pub from: u64,
    pub until: u64,
    pub code: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CodeRow {
    pub id: String,
    pub title: String,
    pub steps: Vec<Step>,
    #[serde(skip)]
    period_ms: u64,
}

/// What the extension reads (`Snapshot` in `WidgetViews.swift`).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Snapshot {
    pub generated_at: u64,
    /// Carried by the widget's own lock and generate links. See [`Widgets::token`].
    pub token: String,
    /// `none`, `locked` or `open`.
    pub state: String,
    pub vault: Option<String>,
    /// When codes and favorites must be gone if no newer file has come.
    pub hide_after: Option<u64>,
    pub buttons: Vec<String>,
    pub lock_action: String,
    pub access: bool,
    pub health: Option<Health>,
    pub show_score: bool,
    pub backups: Vec<BackupRow>,
    pub every_days: u32,
    pub generator: String,
    pub codes_mode: String,
    pub codes: Vec<CodeRow>,
    pub favorites: Vec<Favorite>,
}

/// The file for one moment. With the vault not open, or Show on widgets off, it holds no code,
/// no favorite and no vault name, whatever the feed says.
pub fn build(feed: &Feed, open: bool, codes: Vec<CodeRow>, hide_after: u64, token: &str, now: u64) -> Snapshot {
    let shows = open && feed.access;
    let state = if open {
        "open"
    } else if feed.state == "none" {
        "none"
    } else {
        "locked"
    };
    Snapshot {
        generated_at: now,
        token: token.to_owned(),
        state: state.to_owned(),
        vault: if open { feed.vault.clone() } else { None },
        hide_after: open.then_some(hide_after),
        buttons: feed.buttons.iter().filter(|b| BUTTONS.contains(&b.as_str())).cloned().collect(),
        lock_action: if feed.lock_action == "panic" { "panic" } else { "lock" }.to_owned(),
        access: feed.access,
        health: feed.health.clone(),
        show_score: feed.show_score,
        backups: feed.backups.clone(),
        every_days: feed.every_days,
        generator: feed.generator.clone(),
        codes_mode: if feed.codes_mode == "current" { "current" } else { "upcoming" }.to_owned(),
        codes: if shows { codes } else { Vec::new() },
        favorites: if shows { feed.favorites.iter().take(MAX_FAVORITES).cloned().collect() } else { Vec::new() },
    }
}

/// Each widget's part of the file, so a change reloads only the widgets it touches. A widget's
/// redraws are rationed by the system, and codes change far more often than the rest.
fn parts(s: &Snapshot) -> [(&'static str, String); 7] {
    let text = |v: Value| v.to_string();
    [
        ("quick", text(json!([s.state, s.buttons, s.token]))),
        ("status", text(json!([s.state, s.vault, s.lock_action, s.token, s.hide_after]))),
        ("health", text(json!([s.health, s.show_score]))),
        ("backups", text(json!([s.backups, s.every_days]))),
        ("generator", text(json!([s.generator, s.token]))),
        ("codes", text(json!([s.state, s.access, s.codes, s.codes_mode, s.hide_after]))),
        ("favorites", text(json!([s.state, s.access, s.favorites, s.hide_after]))),
    ]
}

/// The end of the time codes are given for: the auto-lock deadline, ten minutes at most.
pub fn window_end(now: u64, deadline: Option<u64>) -> u64 {
    let cap = now.saturating_add(WINDOW_MS);
    deadline.map_or(cap, |d| d.clamp(now, cap))
}

/// The codes for [`ids`], in that order, from the current step until `until`. Counter-based codes
/// are left out, since showing one uses it up. `until <= now` gives the current step only.
pub fn fetch_codes(engine: &Engine, handle: u64, ids: &[String], now: u64, until: u64) -> Result<Vec<CodeRow>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let ask = |at: u64, ids: &[&str]| -> Result<Vec<Value>, String> {
        let args = json!({"ids": ids}).to_string();
        let reply = engine.call("otp.codes", handle, &args, &[], &[], at).map_err(|e| e.message)?;
        serde_json::from_str::<Vec<Value>>(&reply.json).map_err(|e| e.to_string())
    };
    let wanted: Vec<&str> = ids.iter().take(MAX_CODES).map(String::as_str).collect();
    let first = ask(now, &wanted)?;
    let mut rows: Vec<CodeRow> = Vec::new();
    for id in &wanted {
        let Some(row) = first.iter().find(|r| r["id"].as_str() == Some(id)) else { continue };
        if row["otp"]["kind"].as_str() == Some("hotp") {
            continue;
        }
        let Some(code) = row["code"].as_str() else { continue };
        let period_ms = row["otp"]["period"].as_u64().filter(|p| *p > 0).unwrap_or(30) * 1000;
        let from = now - now % period_ms;
        rows.push(CodeRow {
            id: (*id).to_owned(),
            title: row["title"].as_str().unwrap_or("").to_owned(),
            steps: vec![Step { from, until: from + period_ms, code: code.to_owned() }],
            period_ms,
        });
    }
    // Later steps, one call per step for all the codes that share a period.
    let mut periods: Vec<u64> = rows.iter().map(|r| r.period_ms).collect();
    periods.sort_unstable();
    periods.dedup();
    for period in periods {
        let group: Vec<String> = rows.iter().filter(|r| r.period_ms == period).map(|r| r.id.clone()).collect();
        let group: Vec<&str> = group.iter().map(String::as_str).collect();
        let mut at = now - now % period + period;
        while at < until {
            let found = ask(at, &group)?;
            for row in rows.iter_mut().filter(|r| r.period_ms == period) {
                if let Some(code) = found.iter().find(|f| f["id"].as_str() == Some(row.id.as_str())).and_then(|f| f["code"].as_str()) {
                    row.steps.push(Step { from: at, until: at + period, code: code.to_owned() });
                }
            }
            at += period;
        }
    }
    Ok(rows)
}

struct Inner {
    feed: Feed,
    dirty: bool,
    /// The parts last written, to reload only what changed.
    written: Option<[(&'static str, String); 7]>,
    /// Whether the extension was last switched on or off.
    offered: Option<bool>,
}

/// The widgets' side of the app: the feed, the writer and the token.
pub struct Widgets {
    inner: Mutex<Inner>,
    wake: Condvar,
    store: Store,
    /// A random value, kept between runs, written into the file and carried by the widget's own lock
    /// and generate links. A link without it (from a web page, say) can lock, but never starts a
    /// panic lock and never copies a password.
    pub token: String,
    /// WM Pass is the front app, when a redraw costs nothing.
    pub front: AtomicBool,
}

impl Widgets {
    pub fn new(store: Store) -> Self {
        let feed = store.read(FEED_FILE).ok().and_then(|raw| serde_json::from_slice::<Feed>(&raw).ok()).unwrap_or_default();
        let token = match store.read(TOKEN_FILE) {
            Ok(raw) if raw.len() == 16 => hex(&raw),
            _ => {
                let made: [u8; 16] = wmpass_core::crypto::random_array().unwrap_or([7; 16]);
                let _ = store.write(TOKEN_FILE, &made);
                hex(&made)
            }
        };
        Self {
            inner: Mutex::new(Inner { feed: feed.kept(), dirty: true, written: None, offered: None }),
            wake: Condvar::new(),
            store,
            token,
            front: AtomicBool::new(false),
        }
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The page's latest word. Saved, without the vault's parts, for the next start.
    pub fn feed(&self, feed: Feed) {
        let mut inner = self.inner();
        if inner.feed.kept() != feed.kept()
            && let Ok(text) = serde_json::to_vec(&feed.kept())
        {
            let _ = self.store.write(FEED_FILE, &text);
        }
        inner.feed = feed;
        inner.dirty = true;
        self.wake.notify_all();
    }

    pub fn current_feed(&self) -> Feed {
        self.inner().feed.clone()
    }

    /// Something changed: a lock, an unlock, the window coming to the front. Writes at once.
    pub fn nudge(&self) {
        self.inner().dirty = true;
        self.wake.notify_all();
    }

    /// A wipe: the counts and places go with the vault.
    pub fn forget(&self) {
        let _ = self.store.remove(FEED_FILE);
        let mut inner = self.inner();
        inner.feed = Feed { enabled: inner.feed.enabled, ..Feed::default() };
        inner.dirty = true;
        self.wake.notify_all();
    }

    /// Writes a file with nothing from the vault, now, on this thread. For quitting.
    pub fn write_locked_now(&self) {
        let feed = self.inner().feed.clone();
        if !feed.enabled {
            return;
        }
        let snapshot = build(&feed, false, Vec::new(), 0, &self.token, now_ms());
        if write(&snapshot).is_ok() {
            reload(None);
        }
    }

    /// Runs for as long as the app does.
    pub fn start(self: &Arc<Self>, engine: Engine) {
        let widgets = Arc::clone(self);
        std::thread::spawn(move || widgets.run(&engine));
    }

    fn run(&self, engine: &Engine) {
        loop {
            let (feed, offer) = {
                let mut inner = self.inner();
                inner.dirty = false;
                let offer = (inner.offered != Some(inner.feed.enabled)).then_some(inner.feed.enabled);
                inner.offered = Some(inner.feed.enabled);
                (inner.feed.clone(), offer)
            };
            if let Some(on) = offer {
                set_extension_enabled(on);
                if !on {
                    if let Some(dir) = container() {
                        let _ = std::fs::remove_file(dir.join("snapshot.json"));
                    }
                    self.inner().written = None;
                }
            }
            let wait = if feed.enabled { self.write_once(engine, &feed) } else { IDLE_MS };
            let inner = self.inner();
            if !inner.dirty {
                let _ = self.wake.wait_timeout_while(inner, Duration::from_millis(wait.max(1000)), |i| !i.dirty);
            }
        }
    }

    /// Writes the file if any part changed, and says how long until it should be written again.
    fn write_once(&self, engine: &Engine, feed: &Feed) -> u64 {
        let now = now_ms();
        let handle = engine.current.load(Ordering::SeqCst);
        let open = handle != 0 && feed.state == "open";
        let hide_after = window_end(now, engine.guard.deadline_ms());
        let shows = open && feed.access;
        let until = if feed.codes_mode == "current" { now } else { hide_after };
        let codes = if shows { fetch_codes(engine, handle, &feed.code_ids, now, until).unwrap_or_default() } else { Vec::new() };
        let snapshot = build(feed, open, codes, hide_after, &self.token, now);
        let parts = parts(&snapshot);
        let changed: Vec<&'static str> = {
            let inner = self.inner();
            match &inner.written {
                Some(old) => parts.iter().zip(old.iter()).filter(|(a, b)| a.1 != b.1).map(|(a, _)| a.0).collect(),
                None => KINDS.to_vec(),
            }
        };
        if !changed.is_empty() && write(&snapshot).is_ok() {
            self.inner().written = Some(parts);
            if changed.len() == KINDS.len() {
                reload(None);
            } else {
                for kind in changed {
                    reload(Some(kind));
                }
            }
        }
        next_wait(&snapshot, self.front.load(Ordering::SeqCst), now)
    }
}

/// How long until the file should be written again.
pub fn next_wait(s: &Snapshot, front: bool, now: u64) -> u64 {
    let shows_vault = !s.codes.is_empty() || !s.favorites.is_empty();
    if !shows_vault {
        return IDLE_MS;
    }
    // In front, a redraw is free, so "only the current code" can follow each code as it changes.
    if s.codes_mode == "current" && front {
        let next = s.codes.iter().filter_map(|c| c.steps.first()).map(|step| step.until).min();
        if let Some(next) = next {
            return next.saturating_sub(now) + 50;
        }
    }
    let before_end = s.hide_after.map_or(REWRITE_MS, |end| end.saturating_sub(now).saturating_sub(30_000));
    REWRITE_MS.min(before_end).max(1000)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `~/Library/Containers/<extension>/Data`, which the system makes the first time the widget
/// runs. Not ours to create.
fn container() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let dir = PathBuf::from(std::env::var_os("HOME")?).join("Library/Containers").join(BUNDLE_ID).join("Data");
    dir.is_dir().then_some(dir)
}

/// Written whole or not at all, readable by this user only.
fn write(snapshot: &Snapshot) -> Result<(), String> {
    let dir = container().ok_or("no widget folder yet")?;
    let text = serde_json::to_vec(snapshot).map_err(|e| e.to_string())?;
    write_private(&dir, &text)
}

fn write_private(dir: &Path, text: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let tmp = dir.join("snapshot.json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
    file.write_all(text).and_then(|()| file.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join("snapshot.json")).map_err(|e| e.to_string())
}

/// `wmpass_reload_widgets` of the library in `Contents/Frameworks`, which only a bundle built with
/// the extension has. One widget by its kind, or all of them.
#[cfg(target_os = "macos")]
fn reload(kind: Option<&str>) {
    use std::sync::OnceLock;
    static LIBRARY: OnceLock<Option<libloading::Library>> = OnceLock::new();
    let library = LIBRARY.get_or_init(|| {
        let path = std::env::current_exe().ok()?.parent()?.parent()?.join("Frameworks/libWMPassWidgetCenter.dylib");
        if !path.exists() {
            return None;
        }
        // SAFETY: our own library from our own bundle; loading it runs no initialiser of ours.
        unsafe { libloading::Library::new(&path).ok() }
    });
    let Some(library) = library else { return };
    let name = kind.and_then(|k| std::ffi::CString::new(k).ok());
    // SAFETY: the one function the library exports takes a C string or null and returns nothing.
    unsafe {
        if let Ok(run) = library.get::<unsafe extern "C" fn(*const std::ffi::c_char)>(b"wmpass_reload_widgets\0") {
            run(name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()));
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn reload(_kind: Option<&str>) {}

/// `pluginkit -e use|ignore`: whether the gallery offers the widgets. Only in a bundle that has them.
fn set_extension_enabled(on: bool) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let in_bundle = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("PlugIns/WMPassWidgets.appex")))
        .is_some_and(|p| p.exists());
    if in_bundle {
        let _ = std::process::Command::new("/usr/bin/pluginkit").args(["-e", if on { "use" } else { "ignore" }, "-i", BUNDLE_ID]).status();
    }
}

/// What a `wmpass://widget/...` link asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Link {
    /// Bring the window forward and run this page command.
    Page(&'static str),
    Item(String),
    Open,
    /// Lock; a panic lock only with the token.
    Lock { token: bool },
    /// Copy a new password; only with the token.
    Generate { token: bool },
}

/// Reads a link. Anything not made by the widgets is `None`.
pub fn parse_link(url: &str, token: &str) -> Option<Link> {
    let url = tauri::Url::parse(url).ok()?;
    if url.scheme() != "wmpass" || url.host_str() != Some("widget") {
        return None;
    }
    let mut id = None;
    let mut given = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "id" => id = Some(value.into_owned()),
            "t" => given = Some(value.into_owned()),
            _ => {}
        }
    }
    let token_ok = given.is_some_and(|g| !token.is_empty() && g == token);
    Some(match url.path().trim_start_matches('/') {
        "search" => Link::Page("search"),
        "add" => Link::Page("add"),
        "codes" => Link::Page("codes"),
        "generator" => Link::Page("generator"),
        "health" => Link::Page("health"),
        "backups" => Link::Page("backups"),
        "open" => Link::Open,
        // An item id names nothing by itself, but it is still checked to be one.
        "item" => {
            let id = id.filter(|id| !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))?;
            Link::Item(id)
        }
        "lock" => Link::Lock { token: token_ok },
        "generate" => Link::Generate { token: token_ok },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with_code(seed: &str) -> (Engine, u64, String) {
        let engine = Engine::new(1);
        let create = engine
            .call("vault.create", 0, r#"{"name":"Test","kdf":{"m_kib":65536,"t":3,"p":1}}"#, b"master", &[], 1000)
            .expect("vault");
        let handle = serde_json::from_str::<Value>(&create.json).expect("json")["handle"].as_u64().expect("handle");
        let entry = engine
            .call("entry.create", handle, &json!({"kind": "login", "title": "Mail"}).to_string(), &[], &[], 1000)
            .expect("entry");
        let id = serde_json::from_str::<Value>(&entry.json).expect("json")["id"].as_str().expect("id").to_owned();
        engine
            .call("entry.otp_set", handle, &json!({"id": id, "issuer": "Mail"}).to_string(), seed.as_bytes(), &[], 1000)
            .expect("otp");
        (engine, handle, id)
    }

    fn feed() -> Feed {
        Feed {
            state: "open".to_owned(),
            vault: Some("Personal".to_owned()),
            access: true,
            favorites: vec![Favorite { id: "f".to_owned(), title: "Bank".to_owned(), color: "#336699".to_owned() }],
            ..Feed::default()
        }
    }

    fn code(id: &str, steps: &[(u64, u64)]) -> CodeRow {
        CodeRow { id: id.to_owned(), title: id.to_owned(), steps: steps.iter().map(|(from, until)| Step { from: *from, until: *until, code: "123456".to_owned() }).collect(), period_ms: 30_000 }
    }

    /// RFC 6238's SHA-1 key, "12345678901234567890", and its published codes cut to six digits.
    const RFC_SEED: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn upcoming_codes_are_the_rfc_codes() {
        let (engine, handle, id) = engine_with_code(RFC_SEED);
        let now = 1_111_111_100_000;
        let rows = fetch_codes(&engine, handle, std::slice::from_ref(&id), now, now + 40_000).expect("codes");
        assert_eq!(rows.len(), 1);
        let steps: Vec<(u64, &str)> = rows[0].steps.iter().map(|s| (s.from, s.code.as_str())).collect();
        // T = 1111111109 and T = 1111111111 in the RFC's table.
        assert_eq!(steps, [(1_111_111_080_000, "081804"), (1_111_111_110_000, "050471")]);
        let now = 1_234_567_890_000;
        let rows = fetch_codes(&engine, handle, &[id], now, now).expect("codes");
        assert_eq!(rows[0].steps.len(), 1, "until <= now is the current step only");
        assert_eq!(rows[0].steps[0].code, "005924");
    }

    #[test]
    fn the_window_never_passes_the_lock_or_ten_minutes() {
        let now = 1_000_000;
        assert_eq!(window_end(now, None), now + WINDOW_MS);
        assert_eq!(window_end(now, Some(now + 60_000)), now + 60_000);
        assert_eq!(window_end(now, Some(now + 3 * WINDOW_MS)), now + WINDOW_MS);
        assert_eq!(window_end(now, Some(now - 5)), now, "a deadline already passed gives nothing ahead");
        let (engine, handle, id) = engine_with_code(RFC_SEED);
        let start = 1_700_000_000_000;
        let rows = fetch_codes(&engine, handle, &[id], start, window_end(start, Some(start + 95_000))).expect("codes");
        let last = rows[0].steps.last().expect("a step");
        assert!(last.from < start + 95_000, "no step starts after the deadline");
        assert!(rows[0].steps.len() <= 5);
    }

    #[test]
    fn locked_or_not_allowed_shows_nothing_from_the_vault() {
        let codes = vec![code("a", &[(0, 30_000)])];
        let locked = build(&feed(), false, codes.clone(), 50, "t", 1);
        assert!(locked.codes.is_empty() && locked.favorites.is_empty());
        assert_eq!((locked.state.as_str(), locked.vault.as_deref(), locked.hide_after), ("locked", None, None));
        let off = build(&Feed { access: false, ..feed() }, true, codes.clone(), 50, "t", 1);
        assert!(off.codes.is_empty() && off.favorites.is_empty());
        let open = build(&feed(), true, codes, 50, "t", 1);
        assert_eq!((open.codes.len(), open.favorites.len(), open.hide_after), (1, 1, Some(50)));
        let text = serde_json::to_string(&locked).expect("json");
        assert!(!text.contains("Bank") && !text.contains("Personal") && !text.contains("123456"));
    }

    #[test]
    fn only_known_buttons_reach_the_widget() {
        let snapshot = build(&Feed { buttons: vec!["lock".into(), "rm -rf".into(), "codes".into()], ..feed() }, true, Vec::new(), 0, "t", 1);
        assert_eq!(snapshot.buttons, ["lock", "codes"]);
    }

    #[test]
    fn a_code_change_reloads_only_the_codes() {
        let one = build(&feed(), true, vec![code("a", &[(0, 30_000)])], 50, "t", 1);
        let two = build(&feed(), true, vec![code("a", &[(30_000, 60_000)])], 50, "t", 2);
        let changed: Vec<&str> = parts(&one).iter().zip(parts(&two).iter()).filter(|(a, b)| a.1 != b.1).map(|(a, _)| a.0).collect();
        assert_eq!(changed, ["codes"]);
    }

    #[test]
    fn rewrites_come_before_the_window_closes() {
        let now = 1_000_000;
        let mut s = build(&feed(), true, vec![code("a", &[(now, now + 30_000)])], now + WINDOW_MS, "t", now);
        assert_eq!(next_wait(&s, false, now), REWRITE_MS);
        s.hide_after = Some(now + 120_000);
        assert_eq!(next_wait(&s, false, now), 90_000);
        s.codes_mode = "current".to_owned();
        assert_eq!(next_wait(&s, true, now), 30_050, "in front, each code is followed");
        let quiet = build(&feed(), false, Vec::new(), 0, "t", now);
        assert_eq!(next_wait(&quiet, true, now), IDLE_MS);
    }

    /// The field names the extension decodes (`WidgetViews.swift`).
    #[test]
    fn the_snapshot_keeps_its_field_names() {
        let json = serde_json::to_value(build(&feed(), true, vec![code("a", &[(0, 1)])], 9, "t", 1)).expect("json");
        for key in [
            "generated_at", "token", "state", "vault", "hide_after", "buttons", "lock_action", "access", "health", "show_score", "backups",
            "every_days", "generator", "codes_mode", "codes", "favorites",
        ] {
            assert!(json.get(key).is_some(), "{key}");
        }
        assert!(json["codes"][0].get("period_ms").is_none());
        for key in ["id", "title", "steps"] {
            assert!(json["codes"][0].get(key).is_some(), "{key}");
        }
        // The fixture the Swift decoder is checked against: `packaging/macos/widgets/check.sh`.
        if let Some(out) = std::env::var_os("WMPASS_WIDGET_FIXTURE") {
            std::fs::write(out, serde_json::to_vec_pretty(&json).expect("json")).expect("fixture");
        }
    }

    #[test]
    fn links_need_the_token_for_what_a_page_must_not_start() {
        assert_eq!(parse_link("wmpass://widget/codes", "abc"), Some(Link::Page("codes")));
        assert_eq!(parse_link("wmpass://widget/lock?t=abc", "abc"), Some(Link::Lock { token: true }));
        assert_eq!(parse_link("wmpass://widget/lock", "abc"), Some(Link::Lock { token: false }));
        assert_eq!(parse_link("wmpass://widget/lock?t=abd", "abc"), Some(Link::Lock { token: false }));
        assert_eq!(parse_link("wmpass://widget/generate?t=", ""), Some(Link::Generate { token: false }));
        assert_eq!(parse_link("wmpass://widget/item?id=ab-12", "abc"), Some(Link::Item("ab-12".into())));
        assert_eq!(parse_link("wmpass://widget/item?id=../../x", "abc"), None);
        assert_eq!(parse_link("wmpass://widget/delete", "abc"), None);
        assert_eq!(parse_link("wmpass://oauth/codes", "abc"), None);
        assert_eq!(parse_link("https://widget/codes", "abc"), None);
    }

    #[test]
    fn the_file_is_written_whole_and_private() {
        let dir = std::env::temp_dir().join(format!("wmpass-widgets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        write_private(&dir, b"{}").expect("write");
        assert!(!dir.join("snapshot.json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("snapshot.json")).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
