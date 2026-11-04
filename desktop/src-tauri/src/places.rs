//! Places outside this computer, through the shared sink code. The page says what to do; the
//! order backups and syncs do things in lives on its side, so every platform does the same.
//!
//! Which kind does what is `wmpass_sinks::places`, shared with the browser extension. What a
//! run changes about a place (a rotated token, a first host key or certificate) waits here
//! until the page collects it with `places_updates` and saves it with the place.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use wmpass_net::http::{NativeHttp, NetPolicy};
use wmpass_sinks::Stored;
use wmpass_sinks::http::{Blocking, Client, block_on};
use wmpass_sinks::places::{self, App, Apps, Done, Op};

/// A place as the page stores it. Kept as JSON: each kind reads its own fields.
#[derive(Deserialize, Clone)]
#[serde(transparent)]
pub struct Place(Value);

impl Place {
    fn id(&self) -> String {
        self.0.get("id").and_then(Value::as_str).unwrap_or_default().to_owned()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PlaceFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub modified_ms: u64,
}

impl From<Stored> for PlaceFile {
    fn from(file: Stored) -> Self {
        Self { id: file.id, name: file.name, size: file.size, modified_ms: 0 }
    }
}

impl From<&PlaceFile> for Stored {
    fn from(file: &PlaceFile) -> Self {
        Self { id: file.id.clone(), name: file.name.clone(), size: file.size }
    }
}

fn app(id: Option<&'static str>, secret: Option<&'static str>) -> App {
    App { id: id.unwrap_or_default(), secret: secret.unwrap_or_default() }
}

/// The ids this build was given (see build.rs). Google's desktop clients come with a secret
/// that Google requires even alongside PKCE.
pub fn apps() -> Apps {
    Apps {
        drive: app(option_env!("WMPASS_DRIVE_DESKTOP_CLIENT_ID"), option_env!("WMPASS_DRIVE_DESKTOP_SECRET")),
        dropbox: app(option_env!("WMPASS_DROPBOX_APP_KEY"), None),
        onedrive: app(option_env!("WMPASS_ONEDRIVE_CLIENT_ID"), None),
        boxcloud: app(option_env!("WMPASS_BOX_CLIENT_ID"), option_env!("WMPASS_BOX_CLIENT_SECRET")),
    }
}

static UPDATES: Mutex<Vec<(String, Map<String, Value>)>> = Mutex::new(Vec::new());

fn run(place: &Place, online: bool, op: Op<'_>) -> Result<Done, String> {
    let (done, update) = run_for(&place.0, online, op);
    if !update.is_empty()
        && let Ok(mut pending) = UPDATES.lock()
    {
        pending.push((place.id(), update));
    }
    done
}

/// One run, with what it changed about the place handed back rather than kept for this app's
/// page. The browser extension's places run here through its link, and their changes are its own.
pub fn run_for(place: &Value, online: bool, op: Op<'_>) -> (Result<Done, String>, Map<String, Value>) {
    let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
    let http = NativeHttp::default();
    let parsed = places::Place::from_json(place);
    let reach = places::Reach { client: || Client::new(Blocking(&http), policy), policy };
    let ran = block_on(places::run(&reach, &apps(), &parsed, op, crate::engine::now_ms()));
    (ran.done.map_err(|fault| fault.to_string()), ran.update)
}

pub fn list(place: &Place, online: bool) -> Result<Vec<PlaceFile>, String> {
    match run(place, online, Op::List)? {
        Done::Files(files) => Ok(files.into_iter().map(PlaceFile::from).collect()),
        _ => Ok(Vec::new()),
    }
}

pub fn read(place: &Place, online: bool, file: &PlaceFile) -> Result<Vec<u8>, String> {
    let stored = Stored::from(file);
    match run(place, online, Op::Read(&stored))? {
        Done::Bytes(bytes) => Ok(bytes),
        _ => Err("the place answered with no file".to_owned()),
    }
}

pub fn write(place: &Place, online: bool, name: &str, bytes: &[u8]) -> Result<(), String> {
    run(place, online, Op::Write(name, bytes)).map(|_| ())
}

pub fn remove(place: &Place, online: bool, file: &PlaceFile) -> Result<(), String> {
    let stored = Stored::from(file);
    run(place, online, Op::Delete(&stored)).map(|_| ())
}

/// What runs changed about places since the page last asked: `[[id, fields of extra]]`.
#[tauri::command]
pub fn places_updates() -> Vec<(String, Map<String, Value>)> {
    UPDATES.lock().map(|mut pending| std::mem::take(&mut *pending)).unwrap_or_default()
}

/// The kinds this build can reach. A browser sign-in needs an app id it was built with.
#[tauri::command]
pub fn places_kinds() -> Vec<&'static str> {
    let apps = apps();
    places::kinds()
        .into_iter()
        .filter(|kind| match *kind {
            "drive" => !apps.drive.id.is_empty(),
            "dropbox" => !apps.dropbox.id.is_empty(),
            "onedrive" => !apps.onedrive.id.is_empty(),
            "box" => !apps.boxcloud.id.is_empty(),
            _ => true,
        })
        .collect()
}

/// One step of signing in to an account place. See `signin`.
#[tauri::command]
pub async fn places_sign_in(place: Value, step: Value, online: bool) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || crate::signin::step(&place, &step, online))
        .await
        .map_err(|error| error.to_string())?
}
