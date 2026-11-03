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

/// Which kinds this target can reach. A browser has no raw connections, and cannot accept a
/// NAS's own certificate either, though a NAS with a public one works.
pub fn kinds() -> Vec<&'static str> {
    let mut kinds = vec!["webdav", "s3", "drive", "dropbox", "onedrive", "box", "pcloud", "mega", "proton", "synology", "qnap", "git"];
    if cfg!(not(target_arch = "wasm32")) {
        kinds.insert(0, "folder");
        // iCloud needs cookies and an Origin header, which a page's fetch may not set.
        kinds.extend(["icloud", "sftp", "ftp", "smb", "nfs", "imap"]);
    }
    kinds
}

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

/// How a run reaches the network: a client for HTTP, with the policy it was built with,
/// which raw connections check for themselves.
#[derive(Debug)]
pub struct Reach<F> {
    /// Makes a policy-checked HTTP client each time one is needed.
    pub client: F,
    /// The same policy, for SFTP, FTP, SMB, NFS and IMAP.
    pub policy: wmpass_net::NetPolicy,
}

/// What the sinks' callbacks report during a run, gathered for [`Ran::update`].
type Changes = std::sync::Arc<std::sync::Mutex<Map<String, Value>>>;

fn note(changes: &Changes, key: &str, value: impl Into<Value>) {
    if let Ok(mut map) = changes.lock() {
        map.insert(key.to_owned(), value.into());
    }
}

/// Runs `op` against `place`.
pub async fn run<H: AsyncHttp, F: Fn() -> Client<H>>(reach: &Reach<F>, apps: &Apps, place: &Place, op: Op<'_>, now_ms: u64) -> Ran {
    let changes: Changes = std::sync::Arc::default();
    let mut update = Map::new();
    let done = dispatch(reach, apps, place, op, now_ms, &mut update, &changes).await;
    if let Ok(mut map) = changes.lock() {
        update.append(&mut map);
    }
    Ran { done, update }
}

fn secret(place: &Place, key: &str) -> Secret {
    Secret::new(place.text(key))
}

fn nas_config(place: &Place) -> crate::nas::NasConfig {
    crate::nas::NasConfig {
        server: place.text("server").to_owned(),
        user: place.user.clone(),
        password: Secret::new(place.password.as_str()),
        path: place.path.clone(),
        device_token: secret(place, "device_token"),
        own_certificate: place.flag("own_certificate", false),
        cert_pin: place.text("cert_pin").to_owned(),
    }
}

/// Runs `body` with a client for a NAS: one that holds its own certificate to the pin where
/// the owner allowed that, the ordinary one otherwise.
macro_rules! with_nas_client {
    ($reach:expr, $config:expr, $changes:expr, |$client:ident| $body:expr) => {{
        #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
        {
            let changes = $changes.clone();
            let http = crate::nas::nas_http(&$config, Box::new(move |pin| note(&changes, "cert_pin", pin)))?;
            let $client = Client::new(crate::http::Blocking(http.as_ref()), $reach.policy);
            $body
        }
        #[cfg(not(all(feature = "native", not(target_arch = "wasm32"))))]
        {
            let _ = &$changes;
            let $client = ($reach.client)();
            $body
        }
    }};
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
async fn dispatch<H: AsyncHttp, F: Fn() -> Client<H>>(
    reach: &Reach<F>,
    apps: &Apps,
    place: &Place,
    op: Op<'_>,
    now_ms: u64,
    update: &mut Map<String, Value>,
    changes: &Changes,
) -> Result<Done> {
    let client = &reach.client;
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
        "box" => {
            if apps.boxcloud.id.is_empty() {
                return Err(Fault::NotConfigured);
            }
            let credentials = crate::boxcloud::BoxCredentials {
                client_id: apps.boxcloud.id.to_owned(),
                client_secret: Secret::new(apps.boxcloud.secret),
                refresh_token: secret(place, "refresh_token"),
            };
            let changes = changes.clone();
            let rotated: crate::boxcloud::OnRotated = Box::new(move |token| note(&changes, "refresh_token", token.expose()));
            apply(&crate::boxcloud::BoxCloud::new(client(), crate::boxcloud::BoxConfig { folder: place.folder() }, credentials, rotated), op).await
        }
        "pcloud" => {
            let config = crate::pcloud::PCloudConfig {
                region: if place.text("region") == "eu" { crate::pcloud::PCloudRegion::Eu } else { crate::pcloud::PCloudRegion::Us },
                email: place.user.clone(),
                auth: secret(place, "auth"),
                folder: place.folder(),
            };
            apply(&crate::pcloud::PCloud::new(client(), config), op).await
        }
        "mega" => {
            let config = crate::mega::MegaConfig {
                email: place.user.clone(),
                session_id: secret(place, "session_id"),
                master_key: secret(place, "master_key"),
                folder: place.folder(),
            };
            apply(&crate::mega::Mega::new(client(), config), op).await
        }
        "proton" => {
            let config = crate::proton::ProtonConfig {
                user: place.user.clone(),
                uid: place.text("uid").to_owned(),
                access_token: secret(place, "access_token"),
                refresh_token: secret(place, "refresh_token"),
                salted_key_pass: secret(place, "salted_key_pass"),
                folder: place.folder(),
            };
            let changes = changes.clone();
            let on_tokens = Box::new(move |tokens: &crate::proton::ProtonTokens| {
                note(&changes, "access_token", tokens.access.expose());
                note(&changes, "refresh_token", tokens.refresh.expose());
            });
            apply(&crate::proton::Proton::new(client(), config, now_ms, on_tokens), op).await
        }
        "icloud" => {
            let config = crate::icloud::ICloudConfig {
                apple_id: place.user.clone(),
                password: Secret::new(place.password.as_str()),
                trust_token: secret(place, "trust_token"),
                cookies: secret(place, "cookies"),
                client_id: place.text("client_id").to_owned(),
                trusted_at_ms: place.number("trusted_at_ms").unwrap_or(0),
                folder: place.folder(),
            };
            let changes = changes.clone();
            let on_session = Box::new(move |session: &crate::icloud::ICloudSessionUpdate| {
                note(&changes, "cookies", session.cookies.expose());
                if !session.trust_token.is_empty() {
                    note(&changes, "trust_token", session.trust_token.expose());
                }
            });
            apply(&crate::icloud::ICloud::new(client(), config, now_ms, on_session)?, op).await
        }
        "synology" => {
            let config = nas_config(place);
            with_nas_client!(reach, config, changes, |nas| apply(&crate::nas::Synology::new(nas, config), op).await)
        }
        "qnap" => {
            let config = nas_config(place);
            with_nas_client!(reach, config, changes, |nas| apply(&crate::nas::Qnap::new(nas, config), op).await)
        }
        "git" => {
            let config = crate::git::GitConfig {
                provider: match place.text("provider") {
                    "gitlab" => crate::git::GitProvider::GitLab,
                    "gitea" => crate::git::GitProvider::Gitea,
                    _ => crate::git::GitProvider::GitHub,
                },
                server: place.text("server").to_owned(),
                repository: place.text("repository").trim_matches('/').to_owned(),
                branch: place.text("branch").to_owned(),
                path: place.path.clone(),
                token: secret(place, "token"),
                author_name: "WM Pass".to_owned(),
                allow_public: place.flag("allow_public", false),
                ..crate::git::GitConfig::default()
            };
            apply(&crate::git::Git::new(client(), config, place.text("device"), now_ms), op).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        "sftp" => {
            let config = crate::sftp::SftpConfig {
                host: place.text("host").to_owned(),
                port: port(place, 22),
                user: place.user.clone(),
                password: Secret::new(place.password.as_str()),
                private_key: secret(place, "private_key"),
                key_passphrase: secret(place, "key_passphrase"),
                path: place.path.clone(),
                host_key: place.text("host_key").to_owned(),
                legacy_algorithms: place.flag("legacy_algorithms", false),
            };
            let changes = changes.clone();
            apply(&crate::sftp::Sftp::new(reach.policy, config, Box::new(move |key| note(&changes, "host_key", key))), op).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        "ftp" => {
            let config = crate::ftp::FtpConfig {
                host: place.text("host").to_owned(),
                port: port(place, 21),
                user: place.user.clone(),
                password: Secret::new(place.password.as_str()),
                path: place.path.clone(),
                security: match place.text("security") {
                    "implicit" => crate::ftp::FtpSecurity::Implicit,
                    _ if !place.flag("secure", true) => crate::ftp::FtpSecurity::None,
                    _ => crate::ftp::FtpSecurity::Explicit,
                },
            };
            apply(&crate::ftp::Ftp::new(reach.policy, config), op).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        "smb" => {
            let config = crate::smb::SmbConfig {
                host: place.text("host").to_owned(),
                port: port(place, 445),
                share: place.text("share").to_owned(),
                path: place.path.clone(),
                domain: place.text("domain").to_owned(),
                user: place.user.clone(),
                password: Secret::new(place.password.as_str()),
                encrypt: place.flag("encrypt", true),
            };
            apply(&crate::smb::Smb::new(reach.policy, config), op).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        "nfs" => {
            let id = |key: &str| place.number(key).and_then(|n| u32::try_from(n).ok()).unwrap_or(1000);
            let config = crate::nfs::NfsConfig {
                host: place.text("host").to_owned(),
                export: place.text("export").to_owned(),
                path: place.path.clone(),
                uid: id("uid"),
                gid: id("gid"),
                port: 0,
                mount_port: 0,
            };
            apply(&crate::nfs::Nfs::new(reach.policy, config), op).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        "imap" => {
            let security = match place.text("security") {
                "starttls" => crate::imap::ImapSecurity::StartTls,
                "none" => crate::imap::ImapSecurity::None,
                _ => crate::imap::ImapSecurity::Tls,
            };
            let mailbox = place.text("folder").trim();
            let config = crate::imap::ImapConfig {
                host: place.text("host").to_owned(),
                port: port(place, security.default_port()),
                security,
                user: place.user.clone(),
                password: Secret::new(place.password.as_str()),
                mailbox: if mailbox.is_empty() { crate::imap::DEFAULT_MAILBOX.to_owned() } else { mailbox.to_owned() },
            };
            apply(&crate::imap::Imap::new(reach.policy, config), op).await
        }
        _ => Err(Fault::NotConfigured),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn port(place: &Place, default: u16) -> u16 {
    place.number("port").and_then(|p| u16::try_from(p).ok()).filter(|p| *p != 0).unwrap_or(default)
}

// ---- signing in ----

/// A sign-in waiting for its next step. Kept here, never sent to the page, which only holds
/// its id: some hold keys derived from the password.
#[allow(missing_debug_implementations)]
enum Pending {
    PCloud(crate::pcloud::PCloudPending),
    Mega(crate::mega::MegaPending),
    Proton(crate::proton::ProtonPending),
    ICloud(crate::icloud::ICloudPending),
}

static PENDING: std::sync::Mutex<Vec<(String, Pending)>> = std::sync::Mutex::new(Vec::new());

fn park(pending: Pending) -> Result<String> {
    let mut raw = [0u8; 16];
    getrandom::fill(&mut raw).map_err(|_| Fault::Io)?;
    let id: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let mut all = PENDING.lock().map_err(|_| Fault::Io)?;
    // A sign-in left half done is dropped once a few newer ones wait.
    if all.len() >= 8 {
        all.remove(0);
    }
    all.push((id.clone(), pending));
    Ok(id)
}

fn take(id: &str) -> Option<Pending> {
    let mut all = PENDING.lock().ok()?;
    let at = all.iter().position(|(key, _)| key == id)?;
    Some(all.remove(at).1)
}

fn needs(what: &str, pending: Option<String>) -> Value {
    let mut out = Map::new();
    out.insert("needs".into(), what.into());
    if let Some(id) = pending {
        out.insert("pending".into(), id.into());
    }
    Value::Object(out)
}

fn signed_in(extra: Value) -> Value {
    serde_json::json!({ "extra": extra })
}

/// One step of signing in to a place that takes a password (pCloud, MEGA, Proton Drive,
/// iCloud Drive, a Synology or QNAP NAS). `step` holds `code`, `mailbox_password` and
/// `pending` when a previous step asked for them. Answers `{ extra }` to keep with the place,
/// or `{ needs, pending }`.
#[allow(clippy::too_many_lines)]
pub async fn sign_in<H: AsyncHttp, F: Fn() -> Client<H>>(reach: &Reach<F>, place: &Place, step: &Value, now_ms: u64) -> Result<Value> {
    let client = (reach.client)();
    let field = |key: &str| step.get(key).and_then(Value::as_str).unwrap_or_default();
    let code = field("code").trim().to_owned();
    let pending = Some(field("pending")).filter(|id| !id.is_empty()).and_then(take);
    let password = Secret::new(place.password.as_str());
    match place.kind.as_str() {
        "pcloud" => {
            use crate::pcloud::{PCloudLogin, PCloudRegion, sign_in, submit_code};
            let mut region = if place.text("region") == "eu" { PCloudRegion::Eu } else { PCloudRegion::Us };
            let mut outcome = match pending {
                Some(Pending::PCloud(waiting)) => submit_code(&client, &waiting, &code).await,
                _ => sign_in(&client, region, &place.user, &password).await,
            };
            if let PCloudLogin::OtherRegion(other) = outcome {
                region = other;
                outcome = sign_in(&client, region, &place.user, &password).await;
            }
            match outcome {
                PCloudLogin::SignedIn(auth) => Ok(signed_in(serde_json::json!({
                    "auth": auth.expose(),
                    "region": if region == PCloudRegion::Eu { "eu" } else { "us" },
                }))),
                PCloudLogin::NeedsCode(waiting) => Ok(needs("code", Some(park(Pending::PCloud(waiting))?))),
                PCloudLogin::OtherRegion(_) => Err(Fault::TargetMissing),
                PCloudLogin::Failed(fault) => Err(fault),
            }
        }
        "mega" => {
            use crate::mega::{MegaSignIn, sign_in, sign_in_with_code};
            let outcome = match pending {
                Some(Pending::Mega(waiting)) => sign_in_with_code(&client, waiting, &code).await,
                _ => sign_in(&client, &place.user, &password).await,
            };
            match outcome {
                MegaSignIn::SignedIn(config) => Ok(signed_in(serde_json::json!({
                    "session_id": config.session_id.expose(),
                    "master_key": config.master_key.expose(),
                }))),
                MegaSignIn::NeedsCode(waiting) => Ok(needs("code", Some(park(Pending::Mega(waiting))?))),
                MegaSignIn::Refused(fault) | MegaSignIn::Unreachable(fault) => Err(fault),
            }
        }
        "proton" => {
            use crate::proton::{ProtonOutcome, sign_in, submit_code};
            let outcome = match pending {
                Some(Pending::Proton(waiting)) => submit_code(&client, waiting, &code).await,
                _ => sign_in(&client, &place.user, &password, &Secret::new(field("mailbox_password"))).await,
            };
            match outcome {
                ProtonOutcome::SignedIn(done) => Ok(signed_in(serde_json::json!({
                    "uid": done.uid,
                    "access_token": done.access_token.expose(),
                    "refresh_token": done.refresh_token.expose(),
                    "salted_key_pass": done.salted_key_pass.expose(),
                }))),
                ProtonOutcome::NeedsCode(waiting) => Ok(needs("code", Some(park(Pending::Proton(waiting))?))),
                ProtonOutcome::NeedsMailboxPassword => Ok(needs("mailbox_password", None)),
                ProtonOutcome::NeedsHumanVerification => Ok(needs("captcha", None)),
                ProtonOutcome::Failed(fault) => Err(fault),
            }
        }
        "icloud" => {
            use crate::icloud::{CLIENT_ID, ICloudOutcome, approve, finish, start};
            let client_id = if place.text("client_id").is_empty() { CLIENT_ID } else { place.text("client_id") };
            let outcome = match pending {
                Some(Pending::ICloud(waiting)) if code.is_empty() => approve(&client, waiting).await,
                Some(Pending::ICloud(waiting)) => finish(&client, waiting, &code).await,
                _ => start(&client, &place.user, &password, client_id, now_ms).await,
            };
            match outcome {
                ICloudOutcome::SignedIn { trust_token, cookies } => Ok(signed_in(serde_json::json!({
                    "trust_token": trust_token.expose(),
                    "cookies": cookies.expose(),
                    "client_id": client_id,
                    "trusted_at_ms": now_ms,
                }))),
                ICloudOutcome::NeedsCode { pending, by_sms } => {
                    let mut answer = needs("code", Some(park(Pending::ICloud(pending))?));
                    answer["by_sms"] = by_sms.into();
                    Ok(answer)
                }
                ICloudOutcome::NeedsApproval(waiting) => Ok(needs("approval", Some(park(Pending::ICloud(waiting))?))),
                ICloudOutcome::Failed(fault) => Err(fault),
            }
        }
        "synology" | "qnap" => {
            use crate::nas::{NasLogin, qnap_login, synology_login};
            let config = nas_config(place);
            let changes: Changes = std::sync::Arc::default();
            let outcome = with_nas_client!(reach, config, changes, |nas| {
                if place.kind == "synology" { synology_login(nas, &config, &code).await } else { qnap_login(nas, &config, &code).await }
            });
            let pin = changes.lock().ok().and_then(|map| map.get("cert_pin").cloned());
            match outcome {
                NasLogin::SignedIn { device_token } => {
                    let mut extra = Map::new();
                    if let Some(token) = device_token {
                        extra.insert("device_token".into(), token.expose().into());
                    }
                    if let Some(pin) = pin {
                        extra.insert("cert_pin".into(), pin);
                    }
                    Ok(signed_in(Value::Object(extra)))
                }
                NasLogin::NeedsCode => Ok(needs("code", None)),
                NasLogin::Failed(fault) => Err(fault),
            }
        }
        _ => Err(Fault::NotConfigured),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Blocking, block_on};
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    fn place(json: Value) -> Place {
        Place::from_json(&json)
    }

    #[test]
    fn a_rotated_refresh_token_comes_back_to_be_saved() {
        let http = FakeHttp::new()
            .answer(crate::oauth::ONEDRIVE.token, 200, r#"{"access_token":"a","refresh_token":"r2"}"#)
            .answer("https://graph.microsoft.com", 200, r#"{"value":[]}"#);
        let reach = Reach { client: || Client::new(Blocking(&http), NetPolicy::Any), policy: NetPolicy::Any };
        let apps = Apps { onedrive: App { id: "id", secret: "" }, ..Apps::default() };
        let one = place(serde_json::json!({ "kind": "onedrive", "extra": { "refresh_token": "r1" } }));
        let ran = block_on(run(&reach, &apps, &one, Op::List, 0));
        assert!(matches!(ran.done, Ok(Done::Files(_))), "{:?}", ran.done);
        assert_eq!(ran.update.get("refresh_token").and_then(Value::as_str), Some("r2"));
        let asked = String::from_utf8(http.asked.lock().unwrap()[0].body.clone().unwrap().1).unwrap();
        assert!(asked.contains("scope=") && asked.contains("refresh_token=r1"), "{asked}");
    }

    #[test]
    fn an_unknown_kind_or_a_missing_sign_in_is_not_configured() {
        let http = FakeHttp::new();
        let reach = Reach { client: || Client::new(Blocking(&http), NetPolicy::Any), policy: NetPolicy::Any };
        for json in [serde_json::json!({ "kind": "fax" }), serde_json::json!({ "kind": "dropbox" })] {
            let ran = block_on(run(&reach, &Apps::default(), &place(json), Op::Readiness, 0));
            assert_eq!(ran.done.err(), Some(Fault::NotConfigured));
        }
        assert!(http.asked.lock().unwrap().is_empty(), "nothing is sent for a place that cannot work");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn numbers_typed_as_text_still_read_and_a_folder_defaults_to_wm_pass() {
        let p = place(serde_json::json!({ "kind": "smb", "extra": { "port": "4450", "folder": "  " } }));
        assert_eq!(port(&p, 445), 4450);
        assert_eq!(p.folder(), "WM Pass");
        assert_eq!(port(&place(serde_json::json!({ "extra": { "port": 0 } })), 22), 22);
    }

    #[test]
    fn a_pending_sign_in_is_taken_once() {
        let sign_in = |http: &FakeHttp| {
            let reach = Reach { client: || Client::new(Blocking(http), NetPolicy::Any), policy: NetPolicy::Any };
            let p = place(serde_json::json!({ "kind": "synology", "user": "u", "password": "p", "extra": { "server": "nas.example" } }));
            block_on(sign_in(&reach, &p, &serde_json::json!({}), 0))
        };
        // A NAS that cannot be reached is a failure, not a request for a code.
        assert!(sign_in(&FakeHttp::new()).is_err());
        assert!(take("no-such-sign-in").is_none());
    }
}
