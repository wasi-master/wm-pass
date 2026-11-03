//! Signing in to an account place from the desktop. Google Drive, Dropbox, OneDrive and Box
//! go through the service's own page in the default browser, which comes back to a listener
//! on this computer's loopback address, so WM Pass never sees that password. The rest sign
//! in with a password here, in steps when the service asks for a code.
//!
//! App ids are build-time facts, read from the environment when this is compiled (see
//! build.rs and `places::apps`). A build without one does not offer that service.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_net::http::{NativeHttp, NetPolicy};
use wmpass_sinks::http::{Blocking, Client, block_on};
use wmpass_sinks::oauth::{self, Pkce, Service};
use wmpass_sinks::places::{self, App};

/// The fixed loopback port, registered as the redirect in each service's console:
/// `http://127.0.0.1:53683/oauth`. Fixed because Dropbox and Box match the whole address.
const PORT: u16 = 53683;
const REDIRECT: &str = "http://127.0.0.1:53683/oauth";
const WAIT: Duration = Duration::from_secs(300);

fn service(kind: &str, place: &Value) -> Option<(Service, App)> {
    let apps = crate::places::apps();
    let (service, app) = match kind {
        "dropbox" => (oauth::DROPBOX, apps.dropbox),
        "onedrive" => (oauth::ONEDRIVE, apps.onedrive),
        "box" => (oauth::BOX, apps.boxcloud),
        "drive" => {
            let visible = place.pointer("/extra/drive_space").and_then(Value::as_str) == Some("folder");
            let scope = if visible { wmpass_sinks::drive::SCOPE_FILE } else { wmpass_sinks::drive::SCOPE_APP_DATA };
            (Service { scope, ..oauth::DRIVE }, apps.drive)
        }
        _ => return None,
    };
    (!app.id.is_empty()).then_some((service, app))
}

/// One step: a browser sign-in for the four that have one, else a password and maybe a code.
/// Answers `{ extra }` to keep with the place, or `{ needs, pending }` to ask for more.
pub fn step(place: &Value, step: &Value, online: bool) -> Result<Value, String> {
    if !online {
        return Err(wmpass_net::NetError::Blocked.to_string());
    }
    let kind = place.get("kind").and_then(Value::as_str).unwrap_or_default();
    if let Some((service, app)) = service(kind, place) {
        return browser(service, app, kind);
    }
    let http = NativeHttp::default();
    let reach = places::Reach { client: || Client::new(Blocking(&http), NetPolicy::Any), policy: NetPolicy::Any };
    block_on(places::sign_in(&reach, &places::Place::from_json(place), step, crate::engine::now_ms())).map_err(|fault| fault.to_string())
}

/// Opens the service's page and waits for it to come back with a code. Answers the
/// `extra` to keep: the refresh token.
fn browser(service: Service, app: App, kind: &str) -> Result<Value, String> {
    let listener = TcpListener::bind(("127.0.0.1", PORT)).map_err(|_| "another program is using the sign-in port 53683; close it and try again")?;
    let pkce = oauth::pkce().map_err(|fault| fault.to_string())?;
    let mut url = oauth::authorize_url(service, app.id, REDIRECT, &pkce);
    if kind == "box" {
        // Box has no scopes, and refuses an empty one.
        url = url.replace("&scope=&", "&");
    }
    crate::open_in_browser(&url);
    let code = wait_for_code(&listener, &pkce)?;
    let http = NativeHttp::default();
    let client = Client::new(Blocking(&http), NetPolicy::Any);
    let refresh = block_on(places::exchange(&client, service, app, REDIRECT, code.expose(), pkce.verifier.expose())).map_err(|fault| fault.to_string())?;
    Ok(json!({ "extra": { "refresh_token": refresh.expose() } }))
}

fn wait_for_code(listener: &TcpListener, pkce: &Pkce) -> Result<Secret, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(answer) = answer(stream, pkce) {
                    return answer;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("the sign-in did not come back in five minutes".to_owned())
}

/// Reads one request. Anything but the redirect (a favicon, a probe) gets a 404 and the wait
/// goes on.
fn answer(mut stream: TcpStream, pkce: &Pkce) -> Option<Result<Secret, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = [0u8; 8192];
    let read = stream.read(&mut head).ok()?;
    let text = String::from_utf8_lossy(&head[..read]);
    let target = text.lines().next()?.split(' ').nth(1)?.to_owned();
    let Some(query) = target.strip_prefix("/oauth?") else {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        return None;
    };
    let field = |name: &str| {
        query.split('&').find_map(|pair| pair.strip_prefix(name).and_then(|v| v.strip_prefix('='))).map(wmpass_core::otp::percent_decode)
    };
    let outcome = match (field("code"), field("state")) {
        // The state ties the redirect to this request: a page that sends the browser here with a
        // code of its own choosing is refused.
        (Some(code), Some(state)) if state == pkce.state => Ok(Secret::from(code)),
        (Some(_), _) => Err("the sign-in came back with the wrong state, so it was refused".to_owned()),
        (None, _) => Err("the sign-in was cancelled".to_owned()),
    };
    let words = if outcome.is_ok() { "Signed in. You can close this tab and go back to WM Pass." } else { "The sign-in did not finish. Go back to WM Pass and try again." };
    let page = format!("<!doctype html><meta charset=utf-8><title>WM Pass</title><body style=\"font:16px system-ui;text-align:center;padding:48px\">{words}</body>");
    let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}", page.len());
    Some(outcome)
}
