//! Places outside the browser, through the same sink code the desktop app runs. Requests go out
//! through a `fetch` the extension page hands in, which omits cookies and does not follow
//! redirects, so the policy and the rules stay the Rust ones.

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use serde_json::Value;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wmpass_net::http::{NetError, NetPolicy, Request, Response};
use wmpass_sinks::http::{AsyncHttp, Client};
use wmpass_sinks::places::{self, App, Apps, Done, Op, Place};
use wmpass_sinks::Stored;

/// `fetch(method, url, headers, body, timeoutMs)` on the page, answering
/// `{ status, headers: [[name, value]], body: Uint8Array }`.
#[derive(Clone)]
struct JsFetch(Function);

impl std::fmt::Debug for JsFetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JsFetch")
    }
}

impl AsyncHttp for JsFetch {
    async fn send(&self, request: &Request) -> Result<Response, NetError> {
        let headers = Array::new();
        for (name, value) in &request.headers {
            headers.push(&Array::of2(&JsValue::from_str(name), &JsValue::from_str(value)));
        }
        let body = match &request.body {
            Some((kind, bytes)) => {
                headers.push(&Array::of2(&JsValue::from_str("content-type"), &JsValue::from_str(kind)));
                Uint8Array::from(bytes.as_slice()).into()
            }
            None => JsValue::NULL,
        };
        let args = Array::of5(
            &JsValue::from_str(request.method),
            &JsValue::from_str(&request.url),
            &headers,
            &body,
            &JsValue::from_f64(request.timeout_ms as f64),
        );
        let promise = self.0.apply(&JsValue::NULL, &args).map_err(|_| NetError::Unreachable)?;
        let answer = JsFuture::from(Promise::from(promise)).await.map_err(|_| NetError::Unreachable)?;
        let field = |name: &str| Reflect::get(&answer, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED);
        let status = field("status").as_f64().ok_or(NetError::BadAnswer)? as u16;
        let body = field("body").dyn_into::<Uint8Array>().map(|bytes| bytes.to_vec()).unwrap_or_default();
        let headers = Array::from(&field("headers"))
            .iter()
            .filter_map(|pair| {
                let pair = Array::from(&pair);
                Some((pair.get(0).as_string()?.to_ascii_lowercase(), pair.get(1).as_string()?))
            })
            .collect();
        let retry_after = field("retry_after").as_f64().map(|s| s as u64);
        Ok(Response { status, retry_after, body, headers })
    }
}

fn app(id: Option<&'static str>, secret: Option<&'static str>) -> App {
    App { id: id.unwrap_or_default(), secret: secret.unwrap_or_default() }
}

/// The ids this build was given. The extension signs in to Google with a web client, which
/// always has a secret.
fn apps() -> Apps {
    Apps {
        drive: app(option_env!("WMPASS_DRIVE_WEB_CLIENT_ID"), option_env!("WMPASS_DRIVE_WEB_SECRET")),
        dropbox: app(option_env!("WMPASS_DROPBOX_APP_KEY"), None),
        onedrive: app(option_env!("WMPASS_ONEDRIVE_CLIENT_ID"), None),
        boxcloud: app(option_env!("WMPASS_BOX_CLIENT_ID"), option_env!("WMPASS_BOX_CLIENT_SECRET")),
    }
}

/// The kinds this build can reach, and the browser sign-in ids the page needs to start one.
#[wasm_bindgen]
pub fn place_kinds() -> String {
    let apps = apps();
    let offered: Vec<&str> = places::kinds()
        .into_iter()
        .filter(|kind| match *kind {
            "drive" => !apps.drive.id.is_empty(),
            "dropbox" => !apps.dropbox.id.is_empty(),
            "onedrive" => !apps.onedrive.id.is_empty(),
            "box" => !apps.boxcloud.id.is_empty(),
            _ => true,
        })
        .collect();
    serde_json::json!({
        "kinds": offered,
        "ids": { "drive": apps.drive.id, "dropbox": apps.dropbox.id, "onedrive": apps.onedrive.id, "box": apps.boxcloud.id },
    })
    .to_string()
}

fn stored(value: Option<&Value>) -> Result<Stored, JsError> {
    let value = value.ok_or_else(|| JsError::new("which file?"))?;
    let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    Ok(Stored { id: text("id"), name: text("name"), size: value.get("size").and_then(Value::as_u64).unwrap_or(0) })
}

/// Runs one operation. `request` is `{ op, name?, file?, online, now_ms }`, where `op` is
/// `readiness`, `write`, `list`, `read` or `delete`. Answers `{ value, update }` or
/// `{ problem, update }`; `update` holds fields of the place's `extra` to save.
#[wasm_bindgen]
pub async fn place_run(place: String, request: String, bytes: Vec<u8>, fetch: Function) -> Result<JsValue, JsError> {
    let place = Place::from_json(&serde_json::from_str(&place).map_err(|_| JsError::new("that place is not valid"))?);
    let request: Value = serde_json::from_str(&request).map_err(|_| JsError::new("that request is not valid"))?;
    let op = request.get("op").and_then(Value::as_str).unwrap_or_default();
    let name = request.get("name").and_then(Value::as_str).unwrap_or_default();
    let online = request.get("online").and_then(Value::as_bool).unwrap_or(false);
    let now_ms = request.get("now_ms").and_then(Value::as_u64).unwrap_or(0);
    let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
    let http = JsFetch(fetch);
    let file = if op == "read" || op == "delete" { Some(stored(request.get("file"))?) } else { None };
    let operation = match (op, &file) {
        ("readiness", _) => Op::Readiness,
        ("write", _) => Op::Write(name, &bytes),
        ("list", _) => Op::List,
        ("read", Some(file)) => Op::Read(file),
        ("delete", Some(file)) => Op::Delete(file),
        _ => return Err(JsError::new("unknown operation")),
    };
    let ran = places::run(|| Client::new(http.clone(), policy), &apps(), &place, operation, now_ms).await;
    let out = Object::new();
    let set = |key: &str, value: &JsValue| Reflect::set(&out, &JsValue::from_str(key), value).map(|_| ()).map_err(|_| JsError::new("could not answer"));
    set("update", &JsValue::from_str(&Value::Object(ran.update).to_string()))?;
    match ran.done {
        Ok(Done::Nothing | Done::Stored(_)) => set("value", &JsValue::NULL)?,
        Ok(Done::Files(files)) => {
            let list: Vec<Value> = files.into_iter().map(|f| serde_json::json!({ "id": f.id, "name": f.name, "size": f.size, "modified_ms": 0 })).collect();
            set("value", &JsValue::from_str(&Value::Array(list).to_string()))?;
        }
        Ok(Done::Bytes(bytes)) => set("value", &Uint8Array::from(bytes.as_slice()).into())?,
        Err(fault) => set("problem", &JsValue::from_str(&fault.to_string()))?,
    }
    Ok(out.into())
}

fn service(kind: &str, place: &Place) -> Option<(wmpass_sinks::oauth::Service, App)> {
    let apps = apps();
    Some(match kind {
        "dropbox" => (wmpass_sinks::oauth::DROPBOX, apps.dropbox),
        "onedrive" => (wmpass_sinks::oauth::ONEDRIVE, apps.onedrive),
        "box" => (wmpass_sinks::places::BOX, apps.boxcloud),
        "drive" => {
            let scope = if place.text("drive_space") == "folder" { wmpass_sinks::drive::SCOPE_FILE } else { wmpass_sinks::drive::SCOPE_APP_DATA };
            (wmpass_sinks::oauth::Service { scope, ..wmpass_sinks::oauth::DRIVE }, apps.drive)
        }
        _ => return None,
    })
    .filter(|(_, app)| !app.id.is_empty())
}

/// Starts a browser sign-in: `{ url, verifier, state }`. The page opens `url`, checks the state
/// that comes back, and hands the code and verifier to [`place_exchange`].
#[wasm_bindgen]
pub fn place_authorize(place: String, redirect: String) -> Result<String, JsError> {
    let place = Place::from_json(&serde_json::from_str(&place).map_err(|_| JsError::new("that place is not valid"))?);
    let (service, app) = service(&place.kind, &place).ok_or_else(|| JsError::new("this build has no sign-in for that service"))?;
    let pkce = wmpass_sinks::oauth::pkce().map_err(|fault| JsError::new(&fault.to_string()))?;
    let mut url = wmpass_sinks::oauth::authorize_url(service, app.id, &redirect, &pkce);
    if service.scope.is_empty() {
        url = url.replace("&scope=&", "&");
    }
    Ok(serde_json::json!({ "url": url, "verifier": pkce.verifier.expose(), "state": pkce.state }).to_string())
}

/// Trades the code from a browser sign-in for the refresh token the place keeps.
#[wasm_bindgen]
pub async fn place_exchange(place: String, code: String, verifier: String, redirect: String, fetch: Function) -> Result<String, JsError> {
    let place = Place::from_json(&serde_json::from_str(&place).map_err(|_| JsError::new("that place is not valid"))?);
    let (service, app) = service(&place.kind, &place).ok_or_else(|| JsError::new("this build has no sign-in for that service"))?;
    let client = Client::new(JsFetch(fetch), NetPolicy::Any);
    let refresh = wmpass_sinks::places::exchange(&client, service, app, &redirect, &code, &verifier)
        .await
        .map_err(|fault| JsError::new(&fault.to_string()))?;
    Ok(refresh.expose().to_owned())
}
