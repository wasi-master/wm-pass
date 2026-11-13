//! WM Pass for the desktop. Everything secret stays in the session on this side, and crosses to the
//! page only as bytes the page wipes, or not at all: what goes to the clipboard never goes there.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod autotype;
mod bridge;
mod browsers;
mod clip;
mod engine;
mod hardware;
mod menu;
mod places;
mod signin;
mod probe;
mod sealed;
mod store;
mod widgets;
mod wifi;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::{Value, json};
use tauri::image::Image;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_updater::UpdaterExt;
use wmpass_session::{Reply, SessionError, wire};
use zeroize::Zeroizing;

use agent::Agent;
use bridge::Bridge;
use engine::{Engine, now_ms};
use store::Store;
#[cfg(target_os = "macos")]
use widgets::Link;
use widgets::Widgets;

struct Shell {
    close_to_tray: Mutex<bool>,
    shortcuts: Mutex<HashMap<String, String>>,
    /// The quick search chord registered with the system, or empty.
    global_chord: Mutex<String>,
    update: Mutex<Option<tauri_plugin_updater::Update>>,
    /// Places outside the app's folder the person chose in a dialog. Nothing else can be reached.
    outside: Mutex<Vec<String>>,
    /// Whether the tray icon is there to bring a hidden window back.
    has_tray: AtomicBool,
}

fn fail<E: ToString>(error: E) -> String {
    error.to_string()
}

/// The whole body of a raw call, in bytes that are wiped once used.
fn raw(request: &Request<'_>) -> Result<Zeroizing<Vec<u8>>, String> {
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(Zeroizing::new(bytes.clone())),
        InvokeBody::Json(_) => Err("that call needs bytes".to_owned()),
    }
}

/// A raw body made of a JSON head and bytes: `head_len(u32 LE) head bytes`.
fn split(body: &[u8]) -> Result<(Value, &[u8]), String> {
    let (length, rest) = body.split_first_chunk::<4>().ok_or("that call could not be read")?;
    let length = u32::from_le_bytes(*length) as usize;
    if length > rest.len() {
        return Err("that call could not be read".to_owned());
    }
    let (head, bytes) = rest.split_at(length);
    Ok((serde_json::from_slice(head).map_err(fail)?, bytes))
}

fn text_of<'a>(head: &'a Value, key: &str) -> &'a str {
    head.get(key).and_then(Value::as_str).unwrap_or("")
}

async fn blocking<T: Send + 'static>(run: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(run).await.map_err(fail)
}

// ---- the session ----

#[tauri::command]
async fn call(request: Request<'_>, engine: State<'_, Engine>) -> Result<Response, String> {
    let frame = raw(&request)?;
    let engine = engine.inner().clone();
    let reply = blocking(move || engine.run_frame(&frame)).await?;
    Ok(Response::new(reply.to_vec()))
}

/// Runs a call and puts the secret in its reply on the clipboard, so the page never holds it.
#[tauri::command]
async fn call_to_clipboard(request: Request<'_>, engine: State<'_, Engine>) -> Result<Response, String> {
    let body = raw(&request)?;
    let engine = engine.inner().clone();
    let reply = blocking(move || -> Result<Zeroizing<Vec<u8>>, String> {
        let (seconds, frame) = body.split_first_chunk::<4>().ok_or("that call could not be read")?;
        let seconds = u64::from(u32::from_le_bytes(*seconds));
        let result = engine.run(frame).and_then(|mut reply| {
            let secret = reply.secret.take().ok_or(SessionError { code: "invalid", message: "there is nothing to copy".to_owned() })?;
            let text = Zeroizing::new(String::from_utf8_lossy(&secret).into_owned());
            clip::copy(&text, true, seconds).map_err(|message| SessionError { code: "clipboard", message })?;
            Ok(reply)
        });
        Ok(wire::reply_frame(&result))
    })
    .await??;
    Ok(Response::new(reply.to_vec()))
}

#[tauri::command]
fn copy_text(text: String, sensitive: bool, clear_after_seconds: u64) -> Result<(), String> {
    let text = Zeroizing::new(text);
    clip::copy(&text, sensitive, clear_after_seconds)
}

#[tauri::command]
fn clear_clipboard() -> Result<(), String> {
    clip::clear()
}

/// Off the main thread: a long call, such as an unlock, holds the session until it is done.
#[tauri::command]
async fn lock_all(engine: State<'_, Engine>, widgets: State<'_, Arc<Widgets>>) -> Result<(), String> {
    let engine = engine.inner().clone();
    blocking(move || engine.lock_all()).await?;
    widgets.nudge();
    Ok(())
}

#[tauri::command]
fn state_changed(app: AppHandle, engine: State<'_, Engine>, shell: State<'_, Shell>, widgets: State<'_, Arc<Widgets>>, unlocked: bool, handle: u64) -> Result<(), String> {
    engine.current.store(if unlocked { handle } else { 0 }, Ordering::SeqCst);
    engine.guard.active();
    widgets.nudge();
    // The quick search, hidden, still holds the titles it last showed: a lock from the main
    // window takes them out of it too.
    if !unlocked {
        let _ = app.emit_to("palette", "command", "lock");
    }
    let shortcuts = shell.shortcuts.lock().map_err(fail)?.clone();
    set_app_menu(&app, menu::build(&app, &shortcuts, unlocked).map_err(fail)?).map_err(fail)?;
    tray_follows(&app, unlocked);
    Ok(())
}

/// Sets the menu bar. Off a Mac, Tauri gives the app's menu to every window that has none of its
/// own, and the quick search, a bare box with no frame, would grow a menu bar across its top.
fn set_app_menu(app: &AppHandle, menu: Menu<tauri::Wry>) -> tauri::Result<()> {
    app.set_menu(menu)?;
    #[cfg(not(target_os = "macos"))]
    if let Some(palette) = app.get_webview_window("palette") {
        palette.remove_menu()?;
    }
    Ok(())
}

#[tauri::command]
fn activity(engine: State<'_, Engine>) {
    engine.guard.active();
}

#[tauri::command]
fn set_lock_rule(engine: State<'_, Engine>, after_seconds: i64, on_sleep: bool) {
    engine.guard.set(after_seconds, on_sleep);
}

#[tauri::command]
fn current_handle(engine: State<'_, Engine>) -> u64 {
    engine.current.load(Ordering::SeqCst)
}

#[tauri::command]
fn set_shortcuts(app: AppHandle, engine: State<'_, Engine>, shell: State<'_, Shell>, shortcuts: HashMap<String, String>) -> Result<(), String> {
    let unlocked = engine.current.load(Ordering::SeqCst) != 0;
    set_app_menu(&app, menu::build(&app, &shortcuts, unlocked).map_err(fail)?).map_err(fail)?;
    *shell.shortcuts.lock().map_err(fail)? = shortcuts;
    Ok(())
}

// ---- widgets on a Mac ----

/// What the widgets may show, from the page. Nothing in it is secret; codes are fetched here.
#[tauri::command]
fn widgets_feed(widgets: State<'_, Arc<Widgets>>, feed: widgets::Feed) {
    widgets.feed(feed);
}

/// A wipe: what the widgets kept goes with the vault.
#[tauri::command]
fn widgets_forget(widgets: State<'_, Arc<Widgets>>) {
    widgets.forget();
}

/// A `wmpass://widget/...` link: from a click on a widget, or from anything else that can open a
/// link, so a panic lock and a copied password need the widgets' token.
#[cfg(target_os = "macos")]
fn open_link(app: &AppHandle, url: &str) {
    let widgets = app.state::<Arc<Widgets>>();
    let Some(link) = widgets::parse_link(url, &widgets.token) else { return };
    let shown = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false));
    match link {
        Link::Page(command) => {
            show(app, "main");
            let _ = app.emit("command", command);
        }
        Link::Item(id) => {
            show(app, "main");
            let _ = app.emit("command", format!("item:{id}"));
        }
        Link::Open => show(app, "main"),
        Link::Lock { token } => {
            let panic = token && widgets.current_feed().lock_action == "panic";
            app.state::<Engine>().lock_all();
            widgets.nudge();
            let _ = app.emit("command", if panic { "panic" } else { "lock" });
            if !shown {
                hide_app(app);
            }
        }
        Link::Generate { token } => {
            if token {
                let feed = widgets.current_feed();
                let engine = app.state::<Engine>().inner().clone();
                std::thread::spawn(move || {
                    let Ok(reply) = engine.call("gen.generate", 0, &feed.recipe.to_string(), &[], &[], now_ms()) else { return };
                    let Some(secret) = reply.secret else { return };
                    let text = Zeroizing::new(String::from_utf8_lossy(&secret).into_owned());
                    let _ = clip::copy(&text, true, feed.clipboard_seconds);
                });
            }
            if !shown {
                hide_app(app);
            }
        }
    }
}

/// Gives the front back to whatever had it, after a widget brought WM Pass forward to act.
#[cfg(target_os = "macos")]
fn hide_app(app: &AppHandle) {
    let _ = app.hide();
}

// ---- files in the app's folder ----

#[tauri::command]
fn list_files(store: State<'_, Store>, folder: String) -> Result<Vec<store::Listed>, String> {
    store.list(&folder)
}

#[tauri::command]
async fn read_file(store: State<'_, Store>, path: String) -> Result<Response, String> {
    let store = store.inner().clone();
    let bytes = blocking(move || store.read(&path)).await??;
    Ok(Response::new(bytes))
}

#[tauri::command]
async fn write_file(request: Request<'_>, store: State<'_, Store>) -> Result<(), String> {
    let body = raw(&request)?;
    let store = store.inner().clone();
    blocking(move || {
        let (head, bytes) = split(&body)?;
        store.write(text_of(&head, "path"), bytes)
    })
    .await?
}

#[tauri::command]
fn remove_file(store: State<'_, Store>, path: String) -> Result<(), String> {
    store.remove(&path)
}

#[tauri::command]
fn remove_folder(store: State<'_, Store>, path: String) -> Result<(), String> {
    store.remove_folder(&path)
}

#[tauri::command]
fn cover_file(store: State<'_, Store>, path: String) -> Result<(), String> {
    store.cover(&path)
}

#[tauri::command]
fn load_settings(store: State<'_, Store>, name: String) -> Result<Option<String>, String> {
    Ok(store.read(&format!("settings/{name}.json")).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

#[tauri::command]
fn save_settings(store: State<'_, Store>, name: String, text: String) -> Result<(), String> {
    store.write(&format!("settings/{name}.json"), text.as_bytes())
}

#[tauri::command]
async fn load_sealed(store: State<'_, Store>, name: String) -> Result<Option<String>, String> {
    let store = store.inner().clone();
    blocking(move || sealed::load(&store, &name)).await?
}

#[tauri::command]
async fn save_sealed(store: State<'_, Store>, name: String, text: String) -> Result<(), String> {
    let store = store.inner().clone();
    blocking(move || sealed::save(&store, &name, &text)).await?
}

// ---- the system's own dialogs ----

fn dialog(title: &str, extensions: &[String]) -> rfd::AsyncFileDialog {
    let mut picker = rfd::AsyncFileDialog::new().set_title(title);
    if !extensions.is_empty() {
        let list: Vec<&str> = extensions.iter().map(String::as_str).collect();
        picker = picker.add_filter("Files", &list);
    }
    picker
}

/// A file the person chose: `name_len(u32 LE) name bytes`, or empty if they cancelled.
#[tauri::command]
async fn pick_file(title: String, extensions: Vec<String>) -> Result<Response, String> {
    let Some(file) = dialog(&title, &extensions).pick_file().await else {
        return Ok(Response::new(Vec::new()));
    };
    let name = file.file_name();
    let bytes = file.read().await;
    let mut out = Vec::with_capacity(bytes.len() + name.len() + 4);
    out.extend_from_slice(&u32::try_from(name.len()).map_err(fail)?.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&bytes);
    Ok(Response::new(out))
}

#[tauri::command]
async fn save_file(request: Request<'_>) -> Result<bool, String> {
    let body = raw(&request)?;
    let (head, bytes) = split(&body)?;
    let chosen = rfd::AsyncFileDialog::new().set_title(text_of(&head, "title")).set_file_name(text_of(&head, "name")).save_file().await;
    let Some(chosen) = chosen else {
        return Ok(false);
    };
    store::write_atomically(chosen.path(), bytes)?;
    Ok(true)
}

fn remember_outside(shell: &Shell, store: &Store, path: &str) -> Result<(), String> {
    let mut list = shell.outside.lock().map_err(fail)?;
    if !list.iter().any(|known| known == path) {
        list.push(path.to_owned());
        store.write("settings/outside.json", serde_json::to_string(&*list).map_err(fail)?.as_bytes())?;
    }
    Ok(())
}

fn outside_known(shell: &Shell, path: &str) -> Result<(), String> {
    if shell.outside.lock().map_err(fail)?.iter().any(|known| known == path) {
        Ok(())
    } else {
        Err("that file was not chosen here".to_owned())
    }
}

#[tauri::command]
async fn outside_choose(shell: State<'_, Shell>, store: State<'_, Store>, title: String, extensions: Vec<String>) -> Result<Option<Value>, String> {
    let Some(file) = dialog(&title, &extensions).pick_file().await else {
        return Ok(None);
    };
    let path = file.path().to_string_lossy().into_owned();
    remember_outside(&shell, &store, &path)?;
    Ok(Some(json!({"path": path, "name": file.file_name()})))
}

#[tauri::command]
async fn outside_read(shell: State<'_, Shell>, path: String) -> Result<Response, String> {
    outside_known(&shell, &path)?;
    let bytes = blocking(move || std::fs::read(&path).map_err(|_| "that file could not be read".to_owned())).await??;
    Ok(Response::new(bytes))
}

#[tauri::command]
async fn outside_write(request: Request<'_>, shell: State<'_, Shell>) -> Result<(), String> {
    let body = raw(&request)?;
    let path = {
        let (head, _) = split(&body)?;
        text_of(&head, "path").to_owned()
    };
    outside_known(&shell, &path)?;
    blocking(move || {
        let (_, bytes) = split(&body)?;
        store::write_atomically(&PathBuf::from(&path), bytes)
    })
    .await?
}

pub fn open_in_browser(url: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(url).spawn();
}

/// Only web addresses, and mail ones for the mail program: a saved "site" comes from a vault that
/// may hold anything.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    // Saved addresses are sometimes written in capitals: `HTTPS://Example.com`.
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")) || url.chars().any(char::is_control) {
        return Err("that is not a web address".to_owned());
    }
    open_in_browser(&url);
    Ok(())
}

/// Reads one username page in a hidden window. `online` is the page's own switch, as on every
/// other call that reaches a network.
#[tauri::command]
async fn check_username_page(app: AppHandle, site: String, name: String, online: bool) -> Result<String, String> {
    if !online {
        return Err(wmpass_net::NetError::Blocked.to_string());
    }
    let site = probe::site(&site)?;
    let answer = blocking(move || probe::check(&app, site, &name)).await??;
    Ok(probe::word(answer).to_owned())
}

#[tauri::command]
fn memory_gib() -> f64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    #[allow(clippy::cast_precision_loss)]
    let bytes = system.total_memory() as f64;
    bytes / (1024.0 * 1024.0 * 1024.0)
}

/// A menu at the pointer. The answer comes back as an event, or never if it is dismissed.
#[tauri::command]
fn popup_menu(window: WebviewWindow, items: Vec<MenuEntry>) -> Result<(), String> {
    let app = window.app_handle();
    let menu = Menu::new(app).map_err(fail)?;
    for entry in &items {
        append_entry(app, &menu, entry).map_err(fail)?;
    }
    window.popup_menu(&menu).map_err(fail)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MenuEntry {
    id: Option<String>,
    label: Option<String>,
    disabled: bool,
    checked: Option<bool>,
    separator: bool,
    items: Option<Vec<MenuEntry>>,
}

fn append_entry(app: &AppHandle, menu: &Menu<tauri::Wry>, entry: &MenuEntry) -> tauri::Result<()> {
    if entry.separator {
        return menu.append(&PredefinedMenuItem::separator(app)?);
    }
    let label = entry.label.as_deref().unwrap_or("");
    if let Some(children) = &entry.items {
        let sub = tauri::menu::Submenu::new(app, label, !entry.disabled)?;
        for child in children {
            let id = format!("ctx:{}", child.id.as_deref().unwrap_or(""));
            if child.separator {
                sub.append(&PredefinedMenuItem::separator(app)?)?;
            } else if let Some(checked) = child.checked {
                sub.append(&CheckMenuItem::with_id(app, id, child.label.as_deref().unwrap_or(""), !child.disabled, checked, None::<&str>)?)?;
            } else {
                sub.append(&MenuItem::with_id(app, id, child.label.as_deref().unwrap_or(""), !child.disabled, None::<&str>)?)?;
            }
        }
        return menu.append(&sub);
    }
    let id = format!("ctx:{}", entry.id.as_deref().unwrap_or(""));
    if let Some(checked) = entry.checked {
        menu.append(&CheckMenuItem::with_id(app, id, label, !entry.disabled, checked, None::<&str>)?)
    } else {
        menu.append(&MenuItem::with_id(app, id, label, !entry.disabled, None::<&str>)?)
    }
}

// ---- places outside this computer ----

#[tauri::command]
async fn places_list(place: places::Place, online: bool) -> Result<Vec<places::PlaceFile>, String> {
    blocking(move || places::list(&place, online)).await?
}

#[tauri::command]
async fn places_read(place: places::Place, online: bool, file: places::PlaceFile) -> Result<Response, String> {
    let bytes = blocking(move || places::read(&place, online, &file)).await??;
    Ok(Response::new(bytes))
}

#[tauri::command]
async fn places_write(request: Request<'_>) -> Result<(), String> {
    let body = raw(&request)?;
    blocking(move || {
        let (head, bytes) = split(&body)?;
        let place: places::Place = serde_json::from_value(head.get("place").cloned().unwrap_or(Value::Null)).map_err(fail)?;
        let online = head.get("online").and_then(Value::as_bool).unwrap_or(false);
        places::write(&place, online, text_of(&head, "name"), bytes)
    })
    .await?
}

#[tauri::command]
async fn places_remove(place: places::Place, online: bool, file: places::PlaceFile) -> Result<(), String> {
    blocking(move || places::remove(&place, online, &file)).await?
}

#[tauri::command]
async fn places_choose_folder(shell: State<'_, Shell>, store: State<'_, Store>) -> Result<Option<String>, String> {
    let Some(folder) = rfd::AsyncFileDialog::new().set_title("Choose a folder").pick_folder().await else {
        return Ok(None);
    };
    let path = folder.path().to_string_lossy().into_owned();
    remember_outside(&shell, &store, &path)?;
    Ok(Some(path))
}

// ---- hardware keys ----

#[tauri::command]
async fn hardware_tap(kind: String, factor: Value) -> Result<Response, String> {
    let reply = blocking(move || wire::reply_frame(&hardware::tap(&kind, &factor))).await?;
    Ok(Response::new(reply.to_vec()))
}

// ---- saved Wi-Fi ----

/// The networks this computer has saved, without their passwords. Asks the person nothing.
#[tauri::command]
async fn saved_wifi() -> Result<Vec<wifi::Network>, String> {
    blocking(wifi::list).await?
}

/// One saved network's password, as bytes. A Mac asks for an administrator's password first.
#[tauri::command]
async fn saved_wifi_password(id: String) -> Result<Response, String> {
    let secret = blocking(move || wifi::password(&id)).await??;
    Ok(Response::new(secret.as_bytes().to_vec()))
}

// ---- the local network ----

#[tauri::command]
fn device_name() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "Computer".to_owned())
}

/// The address other devices would reach this one on. Connecting a UDP socket sends nothing.
#[tauri::command]
fn local_address() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback()).then(|| ip.to_string())
}

#[tauri::command]
fn blob_dir(store: State<'_, Store>, folder: String) -> Result<String, String> {
    let path = store.resolve(&folder)?;
    std::fs::create_dir_all(&path).map_err(fail)?;
    Ok(path.to_string_lossy().into_owned())
}

// ---- quick unlock ----

// The keyring answers over D-Bus, and the session may be busy, so none of these runs on the main thread.
#[tauri::command]
async fn quick_available() -> Result<bool, String> {
    blocking(sealed::quick_available).await
}

#[tauri::command]
async fn quick_enrolled(file: String) -> Result<bool, String> {
    blocking(move || sealed::quick_enrolled(&file)).await
}

#[tauri::command]
async fn quick_enroll(engine: State<'_, Engine>, handle: u64, file: String) -> Result<(), String> {
    let engine = engine.inner().clone();
    blocking(move || {
        let reply = engine.call("vault.key_for_wrapping", handle, "{}", &[], &[], now_ms()).map_err(|e| e.message)?;
        let key = reply.secret.ok_or("the vault gave no key to keep in the keyring")?;
        sealed::quick_store(&file, &key)
    })
    .await?
}

#[tauri::command]
async fn quick_forget(file: String) -> Result<(), String> {
    blocking(move || sealed::quick_forget(&file)).await?
}

/// Opens a vault with the key from the system's store, which never reaches the page.
#[tauri::command]
async fn quick_open(request: Request<'_>, engine: State<'_, Engine>) -> Result<Response, String> {
    let body = raw(&request)?;
    let engine = engine.inner().clone();
    let reply = blocking(move || -> Result<Zeroizing<Vec<u8>>, String> {
        let (head, sealed_file) = split(&body)?;
        let key = sealed::quick_read(text_of(&head, "file"))?;
        let result: Result<Reply, SessionError> = engine.call("vault.open_key", 0, "{}", &key, &[sealed_file], now_ms());
        Ok(wire::reply_frame(&result))
    })
    .await??;
    Ok(Response::new(reply.to_vec()))
}

// ---- the desktop itself ----

#[tauri::command]
fn set_screen_protection(app: AppHandle, on: bool) {
    for window in app.webview_windows().values() {
        let _ = window.set_content_protected(on);
    }
}

#[tauri::command]
fn show_main(app: AppHandle) {
    show(&app, "main");
}

#[tauri::command]
fn hide_palette(app: AppHandle) {
    if let Some(window) = app.get_webview_window("palette") {
        let _ = window.hide();
    }
}

#[tauri::command]
fn set_close_to_tray(shell: State<'_, Shell>, on: bool) -> Result<(), String> {
    *shell.close_to_tray.lock().map_err(fail)? = on;
    Ok(())
}

#[tauri::command]
fn autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(fail)
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<(), String> {
    if on { app.autolaunch().enable() } else { app.autolaunch().disable() }.map_err(fail)
}

/// `Mod+Shift+Space` in the form the system wants.
fn global_chord(chord: &str) -> String {
    menu::system_chord(chord, "CommandOrControl")
}

/// A chord something else holds is refused, and the one that worked before stays.
#[tauri::command]
fn set_global_shortcut(app: AppHandle, shell: State<'_, Shell>, chord: String) -> bool {
    let Ok(mut current) = shell.global_chord.lock() else { return false };
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    if chord.is_empty() || shortcuts.register(global_chord(&chord).as_str()).is_ok() {
        *current = chord;
        return true;
    }
    if !current.is_empty() && shortcuts.register(global_chord(&current).as_str()).is_err() {
        current.clear();
    }
    false
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// What the clipboard holds, for the debug self-test only.
#[cfg(debug_assertions)]
#[tauri::command]
fn test_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

#[cfg(not(debug_assertions))]
#[tauri::command]
fn test_clipboard() -> Option<String> {
    None
}

/// Ends the app, for the debug self-test.
#[cfg(debug_assertions)]
#[tauri::command]
fn test_exit(app: AppHandle) {
    app.exit(0);
}

#[cfg(not(debug_assertions))]
#[tauri::command]
fn test_exit() {}

#[tauri::command]
fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

// ---- browsers ----

#[tauri::command]
fn install_browser_host() -> Result<Vec<String>, String> {
    browsers::install()
}

#[tauri::command]
fn remove_browser_host() -> Result<(), String> {
    browsers::remove()
}

// ---- the SSH agent ----

#[tauri::command]
fn ssh_state(agent: State<'_, Agent>) -> Result<Value, String> {
    let state = agent.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(json!({"running": state.running, "socket": state.socket, "last": state.last, "stopped": state.stopped}))
}

// The bridge's key is sealed by the system's keychain, which may answer slowly or ask the
// person, so reading it runs off the main thread.
#[tauri::command]
async fn bridge_fingerprint(app: AppHandle) -> Result<String, String> {
    blocking(move || app.state::<Bridge>().fingerprint()).await?
}

#[tauri::command]
fn bridge_state(bridge: State<'_, Bridge>) -> bridge::Shown {
    bridge.shown()
}

#[tauri::command]
async fn bridge_set_enabled(app: AppHandle, bridge: State<'_, Bridge>, engine: State<'_, Engine>, on: bool, ask_sites: bool) -> Result<(), String> {
    bridge.ask_sites.store(ask_sites, Ordering::SeqCst);
    bridge.want(on);
    if !on {
        return Ok(());
    }
    let engine = engine.inner().clone();
    blocking(move || app.state::<Bridge>().start(engine, &agent::socket_folder()?)).await?
}

#[tauri::command]
fn bridge_forget(bridge: State<'_, Bridge>, id: String) -> Result<(), String> {
    bridge.forget(&id)
}

#[tauri::command]
fn ssh_set_enabled(agent: State<'_, Agent>, engine: State<'_, Engine>, on: bool, confirm: bool) -> Result<(), String> {
    agent.confirm.store(confirm, Ordering::SeqCst);
    if on {
        agent.start(engine.inner().clone(), &agent::socket_folder()?)
    } else {
        agent.stop();
        Ok(())
    }
}

// ---- auto-type ----

/// Types the login of an entry into whatever is in front. The password is read and typed here.
#[tauri::command]
async fn auto_type(app: AppHandle, engine: State<'_, Engine>, id: String) -> Result<(), String> {
    let engine = engine.inner().clone();
    let handle = engine.current.load(Ordering::SeqCst);
    // Read before any window goes: a locked vault or a deleted entry is said in the page, rather
    // than in a dialog over an app that then has no window to show.
    let (username, password) = blocking(move || -> Result<(String, Zeroizing<String>), String> {
        let args = json!({"id": id}).to_string();
        let detail = engine.call("entry.get", handle, &args, &[], &[], now_ms()).map_err(|e| e.message)?;
        let detail: Value = serde_json::from_str(&detail.json).map_err(|e| e.to_string())?;
        // A database item calls it user. Without it the password would go into the first box.
        let username = detail["fields"]
            .as_array()
            .and_then(|fields| fields.iter().find(|field| field["key"] == "username" || field["key"] == "user"))
            .and_then(|field| field["value"].as_str())
            .unwrap_or("")
            .to_owned();
        let args = json!({"id": id, "field": "password"}).to_string();
        let secret = engine.call("entry.reveal", handle, &args, &[], &[], now_ms()).map_err(|e| e.message)?.secret.unwrap_or_default();
        Ok((username, Zeroizing::new(String::from_utf8_lossy(&secret).into_owned())))
    })
    .await??;
    for window in app.webview_windows().values() {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.hide();
    let typed = blocking(move || autotype::type_login(&username, &password)).await?;
    // Every window is hidden by now, so an error sent back to the page would go unseen.
    if let Err(reason) = &typed {
        let hint = if cfg!(target_os = "macos") {
            "\n\nOn a Mac, WM Pass needs to be allowed under System Settings, Privacy & Security, Accessibility."
        } else {
            ""
        };
        rfd::AsyncMessageDialog::new()
            .set_title("WM Pass")
            .set_description(format!("WM Pass could not type into the app in front: {reason}{hint}"))
            .set_level(rfd::MessageLevel::Warning)
            .show()
            .await;
    }
    typed
}

// ---- updates ----

/// `online` is the network setting, as on every other call that reaches a network: with it off,
/// not even the check for a release leaves this computer.
#[tauri::command]
async fn check_update(app: AppHandle, shell: State<'_, Shell>, online: bool) -> Result<Option<Value>, String> {
    if !online {
        return Err(wmpass_net::NetError::Blocked.to_string());
    }
    let found = app.updater().map_err(fail)?.check().await.map_err(fail)?;
    let answer = found.as_ref().map(|update| json!({"version": update.version, "notes": update.body.clone().unwrap_or_default()}));
    *shell.update.lock().map_err(fail)? = found;
    Ok(answer)
}

/// The plugin refuses anything whose minisign signature does not match the key in this build.
#[tauri::command]
async fn install_update(app: AppHandle, shell: State<'_, Shell>) -> Result<(), String> {
    // Kept until it is installed: a download that fails, on a dropped connection say, can be tried
    // again without checking for the update afresh.
    let update = shell.update.lock().map_err(fail)?.clone().ok_or("there is no update to install")?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(fail)?;
    app.restart()
}

// ---- windows ----

fn show(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn toggle_palette(app: &AppHandle) {
    let Some(window) = app.get_webview_window("palette") else { return };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app.emit_to("palette", "palette-opened", ());
    }
}

/// The tray's menu. Lock is there only while there is something to lock.
fn tray_menu(app: &AppHandle, unlocked: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", "Open WM Pass", true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", "Quick Search", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "cmd:lock", "Lock", unlocked, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit WM Pass", true, None::<&str>)?;
    Menu::with_items(app, &[&open, &quick, &lock, &PredefinedMenuItem::separator(app)?, &quit])
}

/// After a lock or an unlock, from the page or from the timer here.
fn tray_follows(app: &AppHandle, unlocked: bool) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), tray_menu(app, unlocked)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn tray(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(include_bytes!("../icons/32x32.png"))?)
        .tooltip("WM Pass")
        .menu(&tray_menu(app, false)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show(app, "main"),
            "quick" => toggle_palette(app),
            id => menu::handle(app, id),
        })
        .on_tray_icon_event(|icon, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show(icon.app_handle(), "main");
            }
        })
        .build(app)?;
    Ok(())
}

/// A dump of this process holds whatever an open vault has in memory, so none is written
/// where the system lets the app say so. Someone with administrator rights can still read it.
#[cfg(unix)]
fn no_core_dumps() {
    use rustix::process::{Resource, Rlimit, setrlimit};
    let _ = setrlimit(Resource::Core, Rlimit { current: Some(0), maximum: Some(0) });
    // On Linux a crash file is only half of it: any program running as the same person could
    // read the memory through ptrace, gcore or /proc/<pid>/mem. A process that is not dumpable
    // is closed to all of those short of root. It ends at exec, so the web view's own
    // processes and anything the app starts are as they were.
    #[cfg(target_os = "linux")]
    let _ = rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable);
}

/// Windows Error Reporting puts the heap in the report it makes of a crash, and with it the
/// open vault. Reports are still made, without it.
#[cfg(windows)]
fn no_core_dumps() {
    use windows_sys::Win32::System::ErrorReporting::{WER_FAULT_REPORTING_FLAG_NOHEAP, WerSetFlags};
    // SAFETY: takes a flag and touches nothing of ours.
    let _ = unsafe { WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP) };
}

#[cfg(not(any(unix, windows)))]
fn no_core_dumps() {}

/// Debug builds can be pointed elsewhere, so a test never touches a real vault.
fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(folder) = std::env::var_os("WMPASS_DATA_DIR") {
        return Ok(PathBuf::from(folder));
    }
    app.path().app_data_dir()
}

fn device_number(store: &Store) -> u32 {
    if let Ok(raw) = store.read("settings/device.bin")
        && let Ok(found) = <[u8; 4]>::try_from(raw.as_slice())
    {
        // The same floor as when it was made, or a zero drawn then would read back as another device.
        return u32::from_le_bytes(found).max(1);
    }
    let made: [u8; 4] = wmpass_core::crypto::random_array().unwrap_or([1, 0, 0, 0]);
    let _ = store.write("settings/device.bin", &made);
    u32::from_le_bytes(made).max(1)
}

fn main() {
    no_core_dumps();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app, "main")))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        toggle_palette(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            call,
            call_to_clipboard,
            copy_text,
            clear_clipboard,
            lock_all,
            state_changed,
            current_handle,
            activity,
            set_lock_rule,
            set_shortcuts,
            list_files,
            read_file,
            write_file,
            remove_file,
            remove_folder,
            cover_file,
            load_settings,
            save_settings,
            load_sealed,
            save_sealed,
            pick_file,
            save_file,
            outside_choose,
            outside_read,
            outside_write,
            open_url,
            check_username_page,
            memory_gib,
            popup_menu,
            places_list,
            places_read,
            places_write,
            places_remove,
            places_choose_folder,
            places::places_updates,
            places::places_kinds,
            places::places_sign_in,
            hardware_tap,
            saved_wifi,
            saved_wifi_password,
            device_name,
            local_address,
            blob_dir,
            quick_available,
            quick_enrolled,
            quick_enroll,
            quick_forget,
            quick_open,
            set_screen_protection,
            show_main,
            hide_palette,
            set_close_to_tray,
            autostart,
            set_autostart,
            set_global_shortcut,
            app_version,
            os_name,
            test_exit,
            test_clipboard,
            install_browser_host,
            remove_browser_host,
            ssh_state,
            ssh_set_enabled,
            bridge_fingerprint,
            bridge_state,
            bridge_set_enabled,
            bridge_forget,
            auto_type,
            check_update,
            install_update,
            widgets_feed,
            widgets_forget,
        ])
        .on_menu_event(|app, event| menu::handle(app, event.id().as_ref()))
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                    let shell = app.state::<Shell>();
                    // With no tray icon, off a Mac, a hidden window could not be found again.
                    let reachable = cfg!(target_os = "macos") || shell.has_tray.load(Ordering::SeqCst);
                    let keep = reachable && shell.close_to_tray.lock().is_ok_and(|on| *on);
                    if keep {
                        api.prevent_close();
                        let _ = window.hide();
                        let _ = app.emit("command", "window-hidden");
                    } else {
                        // The hidden palette is a window too, so closing this one alone would leave
                        // the app running with the vault open and no window for the tray to show.
                        app.exit(0);
                    }
                }
                // Closed, as Close Window on a Mac or Alt+F4 does to whatever is in front, the
                // quick search would be gone and its shortcut would open nothing until a restart.
                tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "palette" => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                tauri::WindowEvent::Focused(false) if window.label() == "palette" => {
                    let _ = window.hide();
                }
                // In front, a widget's redraw costs nothing, so codes can follow each step.
                tauri::WindowEvent::Focused(focused) if window.label() == "main" => {
                    let widgets = app.state::<Arc<Widgets>>();
                    widgets.front.store(*focused, Ordering::SeqCst);
                    if *focused {
                        widgets.nudge();
                    }
                }
                _ => {}
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let store = Store::new(data_dir(&handle)?).map_err(std::io::Error::other)?;
            let outside = store.read("settings/outside.json").ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default();
            let engine = Engine::new(device_number(&store));
            let widgets = Arc::new(Widgets::new(store.clone()));
            let watcher = handle.clone();
            let locked = Arc::clone(&widgets);
            engine.watch(move || {
                let _ = watcher.emit("command", "lock");
                tray_follows(&watcher, false);
                locked.nudge();
            });
            // A bug in the session, met by the page, the extension's bridge, the SSH agent or the
            // widgets: the session was replaced, locked. The main window says so over whatever it
            // shows and then goes to its door; the quick search empties as for a lock.
            let told = handle.clone();
            let nudged = Arc::clone(&widgets);
            engine.on_internal(move || {
                let _ = told.emit("command", "internal");
                tray_follows(&told, false);
                nudged.nudge();
            });
            widgets.start(engine.clone());
            app.manage(widgets);
            app.manage(engine);
            app.manage(store);
            app.manage(Agent::new());
            app.manage(Bridge::new(app.state::<Store>().inner().clone(), app.handle().clone()));
            app.manage(Shell {
                close_to_tray: Mutex::new(true),
                shortcuts: Mutex::new(HashMap::new()),
                global_chord: Mutex::new(String::new()),
                update: Mutex::new(None),
                outside: Mutex::new(outside),
                has_tray: AtomicBool::new(false),
            });
            set_app_menu(&handle, menu::build(&handle, &HashMap::new(), false)?)?;
            // A Linux desktop without the indicator library has no tray to put the icon in. The
            // app runs without one rather than not at all; opening it again shows the window.
            match tray(&handle) {
                Ok(()) => app.state::<Shell>().has_tray.store(true, Ordering::SeqCst),
                Err(error) => eprintln!("WM Pass has no tray icon: {error}"),
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|error| {
            // A release build has no console, so a failure to start would otherwise be silent.
            rfd::MessageDialog::new()
                .set_title("WM Pass")
                .set_description(format!("WM Pass could not start: {error}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::exit(1);
        })
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                show(app, "main");
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &event {
                for url in urls {
                    open_link(app, url.as_str());
                }
            }
            // Quitting is a lock too: the widgets are left with nothing from the vault.
            if let tauri::RunEvent::Exit = &event {
                app.state::<Arc<Widgets>>().write_locked_now();
                // A copied secret's timer dies with the app, so it is cleared now rather than never.
                clip::clear_pending();
            }
        });
}
