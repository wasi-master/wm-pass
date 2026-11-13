//! Tells browsers where the small host program is, so the extension can reach this app. A manifest
//! names one program and the extensions allowed to start it, nothing more.

use std::fs;
use std::path::{Path, PathBuf};

const HOST_NAME: &str = "com.wasimaster.wmpass";
const CHROME_EXTENSION: &str = "chrome-extension://blpbdcnicidapbjjopepnaofggclabom/";
const FIREFOX_EXTENSION: &str = "wmpass@wasimaster.com";

struct Browser {
    name: &'static str,
    firefox: bool,
    /// Where the browser keeps its profile on this OS, which says it is installed.
    marker: PathBuf,
    /// Where its manifests go.
    folder: PathBuf,
}

fn chromium_manifest(host: &Path) -> String {
    serde_json::json!({
        "name": HOST_NAME,
        "description": "WM Pass",
        "path": host,
        "type": "stdio",
        "allowed_origins": [CHROME_EXTENSION],
    })
    .to_string()
}

fn firefox_manifest(host: &Path) -> String {
    serde_json::json!({
        "name": HOST_NAME,
        "description": "WM Pass",
        "path": host,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_EXTENSION],
    })
    .to_string()
}

#[cfg(target_os = "macos")]
fn browsers(home: &Path) -> Vec<Browser> {
    let support = home.join("Library/Application Support");
    let chromium = |name: &'static str, dir: &str| Browser { name, firefox: false, marker: support.join(dir), folder: support.join(dir).join("NativeMessagingHosts") };
    vec![
        chromium("Chrome", "Google/Chrome"),
        chromium("Chromium", "Chromium"),
        chromium("Edge", "Microsoft Edge"),
        chromium("Brave", "BraveSoftware/Brave-Browser"),
        chromium("Vivaldi", "Vivaldi"),
        Browser { name: "Firefox", firefox: true, marker: support.join("Firefox"), folder: support.join("Mozilla/NativeMessagingHosts") },
    ]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browsers(home: &Path) -> Vec<Browser> {
    let config = home.join(".config");
    let chromium = |name: &'static str, dir: &str| Browser { name, firefox: false, marker: config.join(dir), folder: config.join(dir).join("NativeMessagingHosts") };
    vec![
        chromium("Chrome", "google-chrome"),
        chromium("Chromium", "chromium"),
        chromium("Edge", "microsoft-edge"),
        chromium("Brave", "BraveSoftware/Brave-Browser"),
        chromium("Vivaldi", "vivaldi"),
        Browser { name: "Firefox", firefox: true, marker: home.join(".mozilla"), folder: home.join(".mozilla/native-messaging-hosts") },
    ]
}

#[cfg(windows)]
fn browsers(home: &Path) -> Vec<Browser> {
    // On Windows the registry says where a manifest is, and the file itself can live anywhere.
    let folder = home.join("AppData/Local/WM Pass/NativeMessagingHosts");
    let local = home.join("AppData/Local");
    let roaming = home.join("AppData/Roaming");
    let chromium = |name: &'static str, dir: &str| Browser { name, firefox: false, marker: local.join(dir), folder: folder.clone() };
    vec![
        chromium("Chrome", "Google/Chrome"),
        chromium("Chromium", "Chromium"),
        chromium("Edge", "Microsoft/Edge"),
        chromium("Brave", "BraveSoftware/Brave-Browser"),
        chromium("Vivaldi", "Vivaldi"),
        Browser { name: "Firefox", firefox: true, marker: roaming.join("Mozilla/Firefox"), folder: folder.clone() },
    ]
}

#[cfg(windows)]
fn registry_key(browser: &Browser) -> String {
    let key = match browser.name {
        "Chrome" => r"Software\Google\Chrome\NativeMessagingHosts",
        "Chromium" => r"Software\Chromium\NativeMessagingHosts",
        "Edge" => r"Software\Microsoft\Edge\NativeMessagingHosts",
        "Brave" => r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
        "Vivaldi" => r"Software\Vivaldi\NativeMessagingHosts",
        _ => r"Software\Mozilla\NativeMessagingHosts",
    };
    format!(r"HKCU\{key}\{HOST_NAME}")
}

#[cfg(windows)]
fn register(browser: &Browser, manifest: &Path) -> Result<(), String> {
    let status = std::process::Command::new("reg")
        .args(["add", &registry_key(browser), "/ve", "/t", "REG_SZ", "/d"])
        .arg(manifest)
        .arg("/f")
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("the registry refused".to_owned()) }
}

#[cfg(not(windows))]
fn register(_browser: &Browser, _manifest: &Path) -> Result<(), String> {
    Ok(())
}

/// Takes the browser's pointer to the manifest away, which on Windows outlives the file.
#[cfg(windows)]
fn unregister(browser: &Browser) {
    let _ = std::process::Command::new("reg").args(["delete", &registry_key(browser), "/f"]).output();
}

#[cfg(not(windows))]
fn unregister(_browser: &Browser) {}

fn home() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).ok_or_else(|| "this account has no home folder".to_owned())
}

/// The host sits beside this program, in every package.
fn host_path() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if cfg!(windows) { "wm-pass-host.exe" } else { "wm-pass-host" };
    let path = me.parent().ok_or("this program has no folder")?.join(name);
    if path.exists() { Ok(path) } else { Err("the browser host program is missing from this build".to_owned()) }
}

/// Writes a manifest for each browser found, and says which. One browser that cannot be set up
/// leaves the others still set up; only when none could be is the reason given.
pub fn install() -> Result<Vec<String>, String> {
    let host = host_path()?;
    let mut done = Vec::new();
    let mut failed = None;
    for browser in browsers(&home()?) {
        if !browser.marker.exists() {
            continue;
        }
        let suffix = if browser.firefox { "-firefox" } else { "" };
        let file = if cfg!(windows) { format!("{HOST_NAME}{suffix}.json") } else { format!("{HOST_NAME}.json") };
        let manifest = browser.folder.join(file);
        let text = if browser.firefox { firefox_manifest(&host) } else { chromium_manifest(&host) };
        let written = fs::create_dir_all(&browser.folder)
            .and_then(|()| fs::write(&manifest, text))
            .map_err(|e| e.to_string())
            .and_then(|()| register(&browser, &manifest));
        match written {
            Ok(()) => done.push(browser.name.to_owned()),
            Err(reason) => failed = Some(format!("{}: {reason}", browser.name)),
        }
    }
    match failed {
        Some(reason) if done.is_empty() => Err(reason),
        _ => Ok(done),
    }
}

pub fn remove() -> Result<(), String> {
    for browser in browsers(&home()?) {
        for file in [format!("{HOST_NAME}.json"), format!("{HOST_NAME}-firefox.json")] {
            let _ = fs::remove_file(browser.folder.join(file));
        }
        unregister(&browser);
    }
    Ok(())
}
