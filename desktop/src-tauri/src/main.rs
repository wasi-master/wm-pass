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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

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

struct Shell {
    close_to_tray: Mutex<bool>,
    shortcuts: Mutex<HashMap<String, String>>,
    update: Mutex<Option<tauri_plugin_updater::Update>>,
    /// Places outside the app's folder the person chose in a dialog. Nothing else can be reached.
    outside: Mutex<Vec<String>>,
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
    clip::copy(&text, sensitive, clear_after_seconds)
}

#[tauri::command]
fn clear_clipboard() -> Result<(), String> {
    clip::clear()
}

#[tauri::command]
fn lock_all(engine: State<'_, Engine>) {
    engine.lock_all();
}

#[tauri::command]
fn state_changed(app: AppHandle, engine: State<'_, Engine>, shell: State<'_, Shell>, unlocked: bool, handle: u64) -> Result<(), String> {
    engine.current.store(if unlocked { handle } else { 0 }, Ordering::SeqCst);
    engine.guard.active();
    let shortcuts = shell.shortcuts.lock().map_err(fail)?.clone();
    app.set_menu(menu::build(&app, &shortcuts, unlocked).map_err(fail)?).map_err(fail)?;
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
    app.set_menu(menu::build(&app, &shortcuts, unlocked).map_err(fail)?).map_err(fail)?;
    *shell.shortcuts.lock().map_err(fail)? = shortcuts;
    Ok(())
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

/// Only web addresses: a saved "site" comes from a vault that may hold anything.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.chars().any(char::is_control) {
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

#[tauri::command]
fn quick_available() -> bool {
    sealed::quick_available()
}

#[tauri::command]
fn quick_enrolled(file: String) -> bool {
    sealed::quick_enrolled(&file)
}

#[tauri::command]
fn quick_enroll(engine: State<'_, Engine>, handle: u64, file: String) -> Result<(), String> {
    let reply = engine.call("vault.key_for_wrapping", handle, "{}", &[], &[], now_ms()).map_err(|e| e.message)?;
    let key = reply.secret.ok_or("no key")?;
    sealed::quick_store(&file, &key)
}

#[tauri::command]
fn quick_forget(file: String) -> Result<(), String> {
    sealed::quick_forget(&file)
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
    chord
        .split('+')
        .map(|part| match part {
            "Mod" => "CommandOrControl",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[tauri::command]
fn set_global_shortcut(app: AppHandle, chord: String) -> bool {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    chord.is_empty() || shortcuts.register(global_chord(&chord).as_str()).is_ok()
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
    let state = agent.state.lock().map_err(fail)?;
    Ok(json!({"running": state.running, "socket": state.socket, "last": state.last}))
}

#[tauri::command]
fn bridge_fingerprint(bridge: State<'_, Bridge>) -> Result<String, String> {
    bridge.fingerprint()
}

#[tauri::command]
fn bridge_state(bridge: State<'_, Bridge>) -> bridge::Shown {
    bridge.shown()
}

#[tauri::command]
fn bridge_set_enabled(bridge: State<'_, Bridge>, engine: State<'_, Engine>, on: bool, ask_sites: bool) -> Result<(), String> {
    bridge.ask_sites.store(ask_sites, Ordering::SeqCst);
    if on {
        bridge.start(engine.inner().clone(), &agent::socket_folder()?)
    } else {
        bridge.stop();
        Ok(())
    }
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
    for window in app.webview_windows().values() {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.hide();
    blocking(move || -> Result<(), String> {
        let args = json!({"id": id}).to_string();
        let detail = engine.call("entry.get", handle, &args, &[], &[], now_ms()).map_err(|e| e.message)?;
        let detail: Value = serde_json::from_str(&detail.json).map_err(fail)?;
        let username = detail["fields"]
            .as_array()
            .and_then(|fields| fields.iter().find(|field| field["key"] == "username"))
            .and_then(|field| field["value"].as_str())
            .unwrap_or("")
            .to_owned();
        let args = json!({"id": id, "field": "password"}).to_string();
        let secret = engine.call("entry.reveal", handle, &args, &[], &[], now_ms()).map_err(|e| e.message)?.secret.unwrap_or_default();
        let password = Zeroizing::new(String::from_utf8_lossy(&secret).into_owned());
        autotype::type_login(&username, &password)
    })
    .await?
}

// ---- updates ----

#[tauri::command]
async fn check_update(app: AppHandle, shell: State<'_, Shell>) -> Result<Option<Value>, String> {
    let found = app.updater().map_err(fail)?.check().await.map_err(fail)?;
    let answer = found.as_ref().map(|update| json!({"version": update.version, "notes": update.body.clone().unwrap_or_default()}));
    *shell.update.lock().map_err(fail)? = found;
    Ok(answer)
}

/// The plugin refuses anything whose minisign signature does not match the key in this build.
#[tauri::command]
async fn install_update(app: AppHandle, shell: State<'_, Shell>) -> Result<(), String> {
    let update = shell.update.lock().map_err(fail)?.take().ok_or("there is no update to install")?;
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

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open WM Pass", true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", "Quick Search", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "cmd:lock", "Lock", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit WM Pass", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quick, &lock, &PredefinedMenuItem::separator(app)?, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(include_bytes!("../icons/32x32.png"))?)
        .tooltip("WM Pass")
        .menu(&menu)
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

/// A core dump would hold whatever was in memory.
#[cfg(unix)]
fn no_core_dumps() {
    use rustix::process::{Resource, Rlimit, setrlimit};
    let _ = setrlimit(Resource::Core, Rlimit { current: Some(0), maximum: Some(0) });
}

#[cfg(not(unix))]
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
        return u32::from_le_bytes(found);
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
        ])
        .on_menu_event(|app, event| menu::handle(app, event.id().as_ref()))
        .on_window_event(|window, event| {
            let app = window.app_handle();
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                    let keep = app.state::<Shell>().close_to_tray.lock().is_ok_and(|on| *on);
                    if keep {
                        api.prevent_close();
                        let _ = window.hide();
                        let _ = app.emit("command", "window-hidden");
                    }
                }
                tauri::WindowEvent::Focused(false) if window.label() == "palette" => {
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let store = Store::new(data_dir(&handle)?).map_err(std::io::Error::other)?;
            let outside = store.read("settings/outside.json").ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default();
            let engine = Engine::new(device_number(&store));
            let watcher = handle.clone();
            engine.watch(move || {
                let _ = watcher.emit("command", "lock");
            });
            app.manage(engine);
            app.manage(store);
            app.manage(Agent::new());
            app.manage(Bridge::new(app.state::<Store>().inner().clone(), app.handle().clone()));
            app.manage(Shell {
                close_to_tray: Mutex::new(true),
                shortcuts: Mutex::new(HashMap::new()),
                update: Mutex::new(None),
                outside: Mutex::new(outside),
            });
            handle.set_menu(menu::build(&handle, &HashMap::new(), false)?)?;
            tray(&handle)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("WM Pass could not start")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &_event {
                show(_app, "main");
            }
        });
}
