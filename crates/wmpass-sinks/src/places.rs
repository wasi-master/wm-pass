//! A place of any kind, described the way the desktop app and the extension store it, and one
//! operation run against it. Both shells come through here, so a kind is wired once.
//!
//! The common fields sit at the top of [`Place`]; everything else a kind needs is in
//! [`Place::extra`] under the names the settings page writes. What changes during a run (a
//! rotated refresh token, a first host key, a NAS certificate, new session cookies) comes back
//! as [`Ran::update`], to be merged into `extra` and saved.

use serde_json::{Map, Value};
use wmpass_core::Secret;

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

/// A place as stored.
#[derive(Clone, Default)]
#[allow(missing_docs)]
pub struct Place {
    pub kind: String,
    pub path: String,
    pub url: String,
    pub user: String,
    pub password: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
    pub extra: Map<String, Value>,
}

impl std::fmt::Debug for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Addresses, names, passwords and tokens all stay out of logs.
        f.debug_struct("Place").field("kind", &self.kind).finish_non_exhaustive()
    }
}

impl Place {
    /// Reads a place as a page stored it. Missing fields are empty.
    pub fn from_json(value: &Value) -> Self {
        let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        Self {
            kind: text("kind"),
            path: text("path"),
            url: text("url"),
            user: text("user"),
            password: text("password"),
            endpoint: text("endpoint"),
            region: text("region"),
            bucket: text("bucket"),
            prefix: text("prefix"),
            access_key: text("access_key"),
            secret_key: text("secret_key"),
            extra: value.get("extra").and_then(Value::as_object).cloned().unwrap_or_default(),
        }
    }

    /// A text field of `extra`, empty when absent.
    pub fn text(&self, key: &str) -> &str {
        self.extra.get(key).and_then(Value::as_str).unwrap_or_default()
    }

    /// A yes/no field of `extra`, `default` when absent.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        self.extra.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    /// A number field of `extra`, which a page may have stored as text.
    pub fn number(&self, key: &str) -> Option<u64> {
        match self.extra.get(key)? {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// The folder inside an account, never another app's default.
    pub fn folder(&self) -> String {
        let folder = self.text("folder").trim();
        if folder.is_empty() { "WM Pass".to_owned() } else { folder.to_owned() }
    }
}

/// One app registration: its id, and a secret where the service insists on one anyway.
#[derive(Clone, Copy, Debug, Default)]
pub struct App {
    /// Client id.
    pub id: &'static str,
    /// Client secret, empty for a PKCE-only service.
    pub secret: &'static str,
}

/// The ids this build was given for the services that sign in through a browser.
#[derive(Clone, Copy, Debug, Default)]
#[allow(missing_docs)]
pub struct Apps {
    pub drive: App,
    pub dropbox: App,
    pub onedrive: App,
    pub boxcloud: App,
}

/// What to do.
#[derive(Debug)]
pub enum Op<'a> {
    /// Whether a write could succeed now.
    Readiness,
    /// Writes a file whole.
    Write(&'a str, &'a [u8]),
    /// Files there.
    List,
    /// Reads a file.
    Read(&'a Stored),
    /// Deletes a file.
    Delete(&'a Stored),
}

/// What an operation gave back.
#[derive(Debug)]
pub enum Done {
    /// Readiness or a delete.
    Nothing,
    /// A write.
    Stored(Stored),
    /// A listing.
    Files(Vec<Stored>),
    /// A read.
    Bytes(Vec<u8>),
}

/// An operation's outcome, and what changed about the place on the way.
#[derive(Debug)]
pub struct Ran {
    /// The outcome.
    pub done: Result<Done>,
    /// Fields of `extra` to replace. Empty most of the time.
    pub update: Map<String, Value>,
}

async fn apply<S: Sink>(sink: &S, op: Op<'_>) -> Result<Done> {
    Ok(match op {
        Op::Readiness => {
            sink.readiness().await?;
            Done::Nothing
        }
        Op::Write(name, bytes) => Done::Stored(sink.write(name, bytes).await?),
        Op::List => Done::Files(sink.list().await?),
        Op::Read(file) => Done::Bytes(sink.read(file).await?),
        Op::Delete(file) => {
            sink.delete(file).await?;
            Done::Nothing
        }
    })
}

/// Which kinds this target can reach. A browser has no raw connections.
pub fn kinds() -> Vec<&'static str> {
    let mut kinds = vec!["webdav", "s3", "drive", "dropbox", "onedrive"];
    if cfg!(not(target_arch = "wasm32")) {
        kinds.insert(0, "folder");
    }
    kinds
}

/// Box's addresses. It takes its client secret rather than PKCE, and has no scopes.
pub const BOX: crate::oauth::Service = crate::oauth::Service {
    authorize: "https://account.box.com/api/oauth2/authorize",
    token: "https://api.box.com/oauth2/token",
    scope: "",
    extra: "",
};

fn form(fields: &[(&str, &str)]) -> Vec<u8> {
    fields
        .iter()
        .map(|(key, value)| format!("{}={}", wmpass_core::otp::percent_encode(key), wmpass_core::otp::percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

/// Trades the code a browser sign-in came back with for a refresh token.
pub async fn exchange<H: AsyncHttp>(client: &Client<H>, service: crate::oauth::Service, app: App, redirect: &str, code: &str, verifier: &str) -> Result<Secret> {
    let mut fields = vec![
        ("grant_type", "authorization_code"),
        ("client_id", app.id),
        ("redirect_uri", redirect),
        ("code", code),
        ("code_verifier", verifier),
    ];
    if !app.secret.is_empty() {
        fields.push(("client_secret", app.secret));
    }
    let mut request = wmpass_net::Request::verb("POST", service.token);
    request.body = Some(("application/x-www-form-urlencoded", form(&fields)));
    let root = client.ok(request).await?.json().map_err(|_| Fault::Io)?;
    root.get("refresh_token").and_then(Value::as_str).map(Secret::new).ok_or(Fault::PermissionLost)
}

/// Trades a stored refresh token for an access token, noting a replacement refresh token.
async fn access<H: AsyncHttp>(client: &Client<H>, service: crate::oauth::Service, app: App, place: &Place, update: &mut Map<String, Value>) -> Result<Secret> {
    if app.id.is_empty() {
        return Err(Fault::NotConfigured);
    }
    let refresh = place.text("refresh_token");
    if refresh.is_empty() {
        return Err(Fault::NotConfigured);
    }
    let mut fields = vec![("grant_type", "refresh_token"), ("client_id", app.id), ("refresh_token", refresh)];
    if !app.secret.is_empty() {
        fields.push(("client_secret", app.secret));
    }
    if service.token == crate::oauth::ONEDRIVE.token {
        // Microsoft wants the scope again on a refresh.
        fields.push(("scope", service.scope));
    }
    let mut request = wmpass_net::Request::verb("POST", service.token);
    request.body = Some(("application/x-www-form-urlencoded", form(&fields)));
    let response = client.send(request).await?;
    match response.status {
        200..=299 => {}
        // A refused refresh token is a sign-in to renew, not a network failure.
        400 | 401 => return Err(Fault::PermissionLost),
        status => return Err(crate::status_fault(status)),
    }
    let root = response.json().map_err(|_| Fault::Io)?;
    if let Some(rotated) = root.get("refresh_token").and_then(Value::as_str).filter(|t| *t != refresh) {
        update.insert("refresh_token".into(), Value::String(rotated.to_owned()));
    }
    root.get("access_token").and_then(Value::as_str).map(Secret::new).ok_or(Fault::Io)
}

/// Runs `op` against `place`. `client` makes a policy-checked client each time one is needed.
pub async fn run<H: AsyncHttp>(client: impl Fn() -> Client<H>, apps: &Apps, place: &Place, op: Op<'_>, now_ms: u64) -> Ran {
    let mut update = Map::new();
    let done = dispatch(&client, apps, place, op, now_ms, &mut update).await;
    Ran { done, update }
}

async fn dispatch<H: AsyncHttp>(client: &impl Fn() -> Client<H>, apps: &Apps, place: &Place, op: Op<'_>, now_ms: u64, update: &mut Map<String, Value>) -> Result<Done> {
    match place.kind.as_str() {
        #[cfg(not(target_arch = "wasm32"))]
        "folder" => apply(&crate::folder::Folder::new(&place.path), op).await,
        "webdav" => apply(&crate::webdav::WebDav::new(client(), &place.url, &place.user, &Secret::new(place.password.as_str())), op).await,
        "s3" => {
            let config = crate::s3::S3Config {
                endpoint: place.endpoint.clone(),
                region: place.region.clone(),
                bucket: place.bucket.clone(),
                prefix: place.prefix.clone(),
                access_key: place.access_key.clone(),
                // A place saved before this choice existed was always path style.
                virtual_host: place.extra.get("path_style").and_then(Value::as_bool) == Some(false),
            };
            apply(&crate::s3::S3::new(client(), config, Secret::new(place.secret_key.as_str()), now_ms), op).await
        }
        "drive" => {
            let space = if place.text("drive_space") == "folder" { crate::drive::Space::Folder(place.folder()) } else { crate::drive::Space::AppData };
            let service = crate::oauth::Service { scope: space.scope(), ..crate::oauth::DRIVE };
            let token = access(&client(), service, apps.drive, place, update).await?;
            apply(&crate::drive::Drive::in_space(client(), token, space), op).await
        }
        "dropbox" => {
            let token = access(&client(), crate::oauth::DROPBOX, apps.dropbox, place, update).await?;
            apply(&crate::dropbox::Dropbox::new(client(), token), op).await
        }
        "onedrive" => {
            let token = access(&client(), crate::oauth::ONEDRIVE, apps.onedrive, place, update).await?;
            apply(&crate::onedrive::OneDrive::new(client(), token), op).await
        }
        _ => Err(Fault::NotConfigured),
    }
}
