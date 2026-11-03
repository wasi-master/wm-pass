//! A Synology or QNAP NAS, through the vendor's file manager API: DSM's File Station, or
//! QTS's.
//!
//! **HTTPS only**, as for WebDAV, with the same exception for an address on a Tailscale
//! network: both logins send the password in the request. Most NAS boxes on a home
//! network answer HTTPS with a certificate they made themselves, which no authority
//! vouches for; [`NasConfig::own_certificate`] accepts that one and pins it, through
//! [`nas_http`]. A later connection shown a different certificate is refused with
//! [`Fault::Unsafe`], the same rule as an SFTP host key.
//!
//! Each sink signs in on its first call and keeps the session for the run. A call that
//! comes back "session expired" signs in again once.

use std::sync::Mutex;

use base64::Engine;
use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_core::otp::percent_encode;
use wmpass_net::{Request, Response};

use crate::boxcloud::{FORM_DATA, form_data};
use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

const DEVICE_NAME: &str = "WM Pass";

/// Where a NAS is and how to sign in. The same fields for both vendors.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct NasConfig {
    /// `nas.local`, `192.168.1.20:5001` or a whole `https://` address. A bare host gets
    /// HTTPS on the vendor's default port: 5001 on DSM, 443 on QTS.
    pub server: String,
    /// Account name.
    pub user: String,
    /// Kept, because these sessions do not outlive a day.
    pub password: Secret,
    /// Folder on the NAS, from its top: `/home/Drive/WM Pass` is the user's Synology
    /// Drive, `/Public/WM Pass` a QNAP share.
    pub path: String,
    /// What lets a login skip the 2FA code: DSM's device token (`did`), or QTS's
    /// `qtoken`. Got by signing in once with a code.
    pub device_token: Secret,
    /// Accepts the certificate the NAS itself made, pinned on first use.
    pub own_certificate: bool,
    /// SHA-256 of the accepted certificate's DER encoding, base64. Empty until a
    /// connection sees one.
    pub cert_pin: String,
}

impl NasConfig {
    /// Whether there is enough to sign in.
    pub fn configured(&self) -> bool {
        !self.server.is_empty() && !self.user.is_empty() && !self.password.is_empty()
    }
}

impl std::fmt::Debug for NasConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NasConfig").field("own_certificate", &self.own_certificate).finish_non_exhaustive()
    }
}

/// The client for a NAS: one that pins the NAS's own certificate when
/// [`NasConfig::own_certificate`] is on, the ordinary one otherwise. `on_certificate` is
/// told the pin, base64, the first time a certificate is seen, to store as
/// [`NasConfig::cert_pin`]. Wrap it in [`crate::http::Blocking`] and a [`Client`].
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn nas_http(config: &NasConfig, on_certificate: Box<dyn Fn(String) + Send + Sync>) -> Result<Box<dyn wmpass_net::Http>> {
    if !config.own_certificate {
        return Ok(Box::new(wmpass_net::http::NativeHttp::default()));
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let expected = if config.cert_pin.is_empty() {
        None
    } else {
        // A stored pin that does not decode is not "none yet": trusting the next
        // certificate seen would undo the pin.
        let bytes = engine.decode(config.cert_pin.trim()).map_err(|_| Fault::NotConfigured)?;
        Some(<[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| Fault::NotConfigured)?)
    };
    let http = wmpass_net::http::PinnedHttp::new(expected, Box::new(move |pin| on_certificate(engine.encode(pin))))
        .map_err(|_| Fault::Io)?;
    Ok(Box::new(http))
}

/// The address for what the user typed: `nas.local`, `nas.local:5001`, or a whole URL. A
/// bare host gets HTTPS on `default_port`. Always ends in `/`.
pub fn base_url(server: &str, default_port: u16) -> Option<String> {
    let typed = server.trim().trim_end_matches('/');
    if typed.is_empty() {
        return None;
    }
    let with_scheme = if typed.contains("://") { typed.to_owned() } else { format!("https://{typed}") };
    let mut url = url::Url::parse(&with_scheme).ok()?;
    url.host_str()?;
    // Written in the text, not what the parser reports: it hides a port that is the
    // scheme's default, and an explicit `:443` must not become 5001.
    let authority = with_scheme.split_once("://").map_or("", |(_, rest)| rest.split('/').next().unwrap_or(""));
    let explicit = authority.rsplit_once(':').is_some_and(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
        && !authority.ends_with(']');
    if !explicit {
        url.set_port(Some(default_port)).ok()?;
    }
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

/// A failure the NAS reported in its answer, by its own code, or one on the way there.
enum Failure {
    Nas(i64, Fault),
    Fault(Fault),
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault)
    }
}

fn fault_of(failure: Failure) -> Fault {
    match failure {
        Failure::Nas(_, fault) | Failure::Fault(fault) => fault,
    }
}

/// What a sign-in got.
#[derive(Default)]
struct Answer {
    sid: Option<Secret>,
    device_token: Option<Secret>,
    needs_code: bool,
    error: Option<Fault>,
}

/// Where a NAS sign-in stands, for the settings screen: tried once with the password, and
/// again with the 2FA code if the NAS asks for one.
#[derive(Debug, PartialEq, Eq)]
pub enum NasLogin {
    /// Signed in. A sign-in with a code hands back the device token to store as
    /// [`NasConfig::device_token`], so later ones skip the code.
    SignedIn {
        /// The new device token, when the NAS issued one.
        device_token: Option<Secret>,
    },
    /// The account has 2FA: ask for the code and call again with it.
    NeedsCode,
    /// Refused, unreachable, or a certificate that is not the pinned one.
    Failed(Fault),
}

impl From<Answer> for NasLogin {
    fn from(answer: Answer) -> Self {
        if answer.sid.is_some() {
            Self::SignedIn { device_token: answer.device_token }
        } else if answer.needs_code {
            Self::NeedsCode
        } else {
            Self::Failed(answer.error.unwrap_or(Fault::Io))
        }
    }
}

/// What both vendors share: the address, the folder and one session.
struct Core<H> {
    client: Client<H>,
    config: NasConfig,
    base: Option<String>,
    /// With a leading slash and none at the end; `/` for the top.
    folder: String,
    session: Mutex<Option<Secret>>,
}

impl<H: AsyncHttp> Core<H> {
    fn new(client: Client<H>, config: NasConfig, default_port: u16) -> Self {
        let base = base_url(&config.server, default_port);
        let normalized = config.path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').map(str::trim).filter(|part| !part.is_empty()).collect();
        let folder = format!("/{}", parts.join("/"));
        Self { client, config, base, folder, session: Mutex::new(None) }
    }

    fn base(&self) -> Result<&str> {
        self.base.as_deref().ok_or(Fault::NotConfigured)
    }

    fn path_of(&self, name: &str) -> String {
        if self.folder == "/" { format!("/{name}") } else { format!("{}/{name}", self.folder) }
    }

    /// One request: refusals by status mapped, the answer handed back on a 2xx.
    async fn send(&self, request: Request) -> Result<Response> {
        let response = self.client.send(request).await?;
        match response.status {
            200..=299 => Ok(response),
            401 | 403 => Err(Fault::PermissionLost),
            404 => Err(Fault::TargetMissing),
            _ => Err(Fault::Io),
        }
    }

    /// Runs `block` with a session, signing in again once if the NAS says it expired.
    async fn with_session<T>(
        &self,
        login: impl AsyncFn() -> Result<Secret>,
        expired: fn(i64) -> bool,
        block: impl AsyncFn(&Secret) -> std::result::Result<T, Failure>,
    ) -> Result<T> {
        let held = self.session.lock().map_err(|_| Fault::Io)?.clone();
        let sid = match held {
            Some(sid) => sid,
            None => {
                let sid = login().await?;
                *self.session.lock().map_err(|_| Fault::Io)? = Some(sid.clone());
                sid
            }
        };
        match block(&sid).await {
            Ok(value) => Ok(value),
            Err(Failure::Nas(code, _)) if expired(code) => {
                *self.session.lock().map_err(|_| Fault::Io)? = None;
                let again = login().await?;
                *self.session.lock().map_err(|_| Fault::Io)? = Some(again.clone());
                block(&again).await.map_err(fault_of)
            }
            Err(failure) => Err(fault_of(failure)),
        }
    }
}

fn json_of(response: &Response) -> std::result::Result<Value, Failure> {
    response.json().map_err(|_| Failure::Fault(Fault::Io))
}

fn query(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(key, value)| format!("{key}={}", percent_encode(value))).collect::<Vec<_>>().join("&")
}

fn is_json(response: &Response) -> bool {
    response.header("content-type").is_some_and(|kind| kind.to_ascii_lowercase().contains("json"))
}

// ---- Synology ----

/// A folder on a Synology NAS, through DSM's File Station API (`/webapi/entry.cgi`).
/// Synology Drive's own "My Drive" is the folder `/home/Drive`; any shared folder works
/// the same way.
///
/// Signs in with [`NasConfig::device_token`] when there is one, which is what lets an
/// account with 2FA sign in at night with nobody to type a code.
pub struct Synology<H> {
    core: Core<H>,
}

impl<H> std::fmt::Debug for Synology<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Synology").finish_non_exhaustive()
    }
}

/// DSM's default HTTPS port.
pub const SYNOLOGY_PORT: u16 = 5001;

/// What a failed DSM sign-in means: 400 is a wrong name or password, 401 a disabled
/// account, 402 one without permission, 404 a wrong 2FA code, 407 an address DSM has
/// blocked.
pub fn synology_auth_fault(code: i64) -> Fault {
    match code {
        400 | 401 | 402 | 403 | 404 | 406 | 408 | 409 | 410 => Fault::PermissionLost,
        _ => Fault::Io,
    }
}

/// What a File Station error means: 105 and 407 a missing permission, 408 a missing file
/// or folder, 415 and 416 a full quota or disk, 411 a read-only volume.
pub fn synology_file_fault(code: i64) -> Fault {
    match code {
        105 | 106 | 107 | 119 | 403 | 407 | 411 => Fault::PermissionLost,
        408 | 599 => Fault::TargetMissing,
        415 | 416 => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

fn synology_expired(code: i64) -> bool {
    matches!(code, 106 | 107 | 119)
}

fn synology_error(root: &Value) -> i64 {
    root.pointer("/error/code").and_then(Value::as_i64).unwrap_or(0)
}

async fn synology_authenticate<H: AsyncHttp>(core: &Core<H>, otp: &str) -> Result<Answer> {
    let config = &core.config;
    let user = config.user.trim();
    let code: String = otp.chars().filter(|c| !c.is_whitespace()).collect();
    let mut fields = vec![
        ("api", "SYNO.API.Auth"),
        ("version", "6"),
        ("method", "login"),
        ("account", user),
        ("passwd", config.password.expose()),
        ("session", "FileStation"),
        ("format", "sid"),
    ];
    if !code.is_empty() {
        fields.extend([("otp_code", code.as_str()), ("enable_device_token", "yes"), ("device_name", DEVICE_NAME)]);
    } else if !config.device_token.is_empty() {
        fields.extend([("device_name", DEVICE_NAME), ("device_id", config.device_token.expose())]);
    }
    let mut request = Request::verb("POST", format!("{}webapi/entry.cgi", core.base()?));
    request.body = Some(("application/x-www-form-urlencoded", crate::oauth::form_body(&fields)));
    let root = core.send(request).await?.json().map_err(|_| Fault::Io)?;
    if root.get("success").and_then(Value::as_bool) == Some(true) {
        let text = |key: &str| root.pointer(&format!("/data/{key}")).and_then(Value::as_str).map(Secret::new);
        return Ok(Answer { sid: text("sid"), device_token: text("did"), ..Answer::default() });
    }
    Ok(match synology_error(&root) {
        403 | 406 => Answer { needs_code: true, error: Some(Fault::PermissionLost), ..Answer::default() },
        code => Answer { error: Some(synology_auth_fault(code)), ..Answer::default() },
    })
}

impl<H: AsyncHttp> Synology<H> {
    /// A folder on the NAS `config` names.
    pub fn new(client: Client<H>, config: NasConfig) -> Self {
        Self { core: Core::new(client, config, SYNOLOGY_PORT) }
    }

    async fn login(&self) -> Result<Secret> {
        if !self.core.config.configured() {
            return Err(Fault::NotConfigured);
        }
        let answer = synology_authenticate(&self.core, "").await?;
        answer.sid.ok_or(answer.error.unwrap_or(Fault::Io))
    }

    fn api(&self, api: &str, method: &str, extra: &[(&str, &str)], sid: &Secret) -> Result<String> {
        let mut pairs = vec![("api", api), ("version", "2"), ("method", method)];
        pairs.extend_from_slice(extra);
        pairs.push(("_sid", sid.expose()));
        Ok(format!("{}webapi/entry.cgi?{}", self.core.base()?, query(&pairs)))
    }

    fn checked(root: Value) -> std::result::Result<Value, Failure> {
        if root.get("success").and_then(Value::as_bool) == Some(true) {
            Ok(root)
        } else {
            let code = synology_error(&root);
            Err(Failure::Nas(code, synology_file_fault(code)))
        }
    }

    /// A GET that must answer `success`.
    async fn ok(&self, api: &str, method: &str, extra: &[(&str, &str)]) -> Result<Value> {
        self.core
            .with_session(
                async || self.login().await,
                synology_expired,
                async |sid| {
                    let response = self.core.send(Request::get(self.api(api, method, extra, sid)?)).await?;
                    Self::checked(json_of(&response)?)
                },
            )
            .await
    }

    async fn make_folder(&self) -> Result<()> {
        let folder = &self.core.folder;
        if folder == "/" {
            return Ok(());
        }
        let (parent, leaf) = folder.rsplit_once('/').unwrap_or(("", folder));
        let parent = if parent.is_empty() { "/" } else { parent };
        // force_parent makes every missing level; a folder already there is no error. The
        // top level is a shared folder, which only DSM's own settings can make.
        let parents = json!([parent]).to_string();
        let names = json!([leaf]).to_string();
        self.ok("SYNO.FileStation.CreateFolder", "create", &[("folder_path", &parents), ("name", &names), ("force_parent", "true")])
            .await
            .map(|_| ())
    }
}

impl<H: AsyncHttp> Sink for Synology<H> {
    async fn readiness(&self) -> Result<()> {
        self.make_folder().await
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        self.make_folder().await?;
        let folder = self.core.folder.clone();
        self.core
            .with_session(
                async || self.login().await,
                synology_expired,
                async |sid| {
                    // The file part last: DSM reads the fields in order, and the path has
                    // to be known before the file starts arriving.
                    let mut request = Request::verb("POST", self.api("SYNO.FileStation.Upload", "upload", &[], sid)?);
                    let fields = [("path", folder.as_str()), ("create_parents", "true"), ("overwrite", "true")];
                    request.body = Some((FORM_DATA, form_data(&fields, "file", name, bytes)));
                    request.timeout_ms = 300_000;
                    Self::checked(json_of(&self.core.send(request).await?)?).map(|_| ())
                },
            )
            .await?;
        Ok(Stored { id: self.core.path_of(name), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let mut out = Vec::new();
        let mut offset = 0usize;
        loop {
            let at = offset.to_string();
            let root = self
                .ok(
                    "SYNO.FileStation.List",
                    "list",
                    &[("folder_path", &self.core.folder), ("offset", &at), ("limit", "1000"), ("additional", r#"["size","time"]"#)],
                )
                .await?;
            let files = root.pointer("/data/files").and_then(Value::as_array).cloned().unwrap_or_default();
            for file in &files {
                if file.get("isdir").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                let Some(name) = file.get("name").and_then(Value::as_str) else { continue };
                let size = file.pointer("/additional/size").and_then(Value::as_u64).unwrap_or(0);
                out.push(Stored { id: self.core.path_of(name), name: name.to_owned(), size });
            }
            offset += files.len();
            let total = root.pointer("/data/total").and_then(Value::as_u64).unwrap_or(0);
            if files.is_empty() || offset as u64 >= total {
                return Ok(out);
            }
        }
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let path = self.core.path_of(&file.name);
        self.core
            .with_session(
                async || self.login().await,
                synology_expired,
                async |sid| {
                    let mut request = Request::get(self.api("SYNO.FileStation.Download", "download", &[("path", &path), ("mode", "download")], sid)?);
                    request.timeout_ms = 300_000;
                    let response = self.core.send(request).await?;
                    // A failure is a JSON answer with a 200; a file is bytes.
                    if is_json(&response) {
                        let code = response.json().ok().as_ref().map_or(0, synology_error);
                        return Err(Failure::Nas(code, synology_file_fault(code)));
                    }
                    Ok(response.body)
                },
            )
            .await
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let paths = json!([self.core.path_of(&file.name)]).to_string();
        match self.ok("SYNO.FileStation.Delete", "delete", &[("path", &paths), ("recursive", "false")]).await {
            Ok(_) | Err(Fault::TargetMissing) => Ok(()),
            Err(fault) => Err(fault),
        }
    }
}

/// Signs in to a Synology NAS from the settings screen. `code` empty is the first try;
/// with the 2FA code, the answer carries the device token to keep.
pub async fn synology_login<H: AsyncHttp>(client: Client<H>, config: &NasConfig, code: &str) -> NasLogin {
    let core = Core::new(client, config.clone(), SYNOLOGY_PORT);
    match synology_authenticate(&core, code).await {
        Ok(answer) => answer.into(),
        Err(fault) => NasLogin::Failed(fault),
    }
}

// ---- QNAP ----

/// A folder on a QNAP NAS, through the QTS File Station API
/// (`/cgi-bin/filemanager/utilRequest.cgi`), signed in by `/cgi-bin/authLogin.cgi`.
///
/// QTS answers every file call with HTTP 200 and a `status` number, 1 for success;
/// [`qnap_status_fault`] reads the rest. [`NasConfig::device_token`] holds the `qtoken` a
/// sign-in with a two-step code hands back, which lets later sign-ins skip the code.
pub struct Qnap<H> {
    core: Core<H>,
}

impl<H> std::fmt::Debug for Qnap<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Qnap").finish_non_exhaustive()
    }
}

/// QTS's default HTTPS port.
pub const QNAP_PORT: u16 = 443;
const PAGE: usize = 500;
const STATUS_SUCCESS: i64 = 1;
const STATUS_EXISTS: i64 = 2;
const STATUS_AUTH_FAIL: i64 = 3;
const STATUS_NO_EXIST: i64 = 5;
const STATUS_NAME_DUP: i64 = 33;

/// What a QTS File Station `status` means: 3 a session that is gone, 4, 10, 11 and 69 a
/// missing permission or two-step check, 5 and 25 a missing file or folder, 9 a full quota.
pub fn qnap_status_fault(status: i64) -> Fault {
    match status {
        3 | 4 | 10 | 11 | 69 => Fault::PermissionLost,
        5 | 25 => Fault::TargetMissing,
        9 => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

/// One field of authLogin.cgi's XML, with or without CDATA.
pub fn qnap_tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&close)?;
    let inner = xml[start..end].trim();
    let value = inner.strip_prefix("<![CDATA[").and_then(|rest| rest.strip_suffix("]]>")).unwrap_or(inner);
    Some(value.trim().to_owned())
}

async fn qnap_authenticate<H: AsyncHttp>(core: &Core<H>, code: &str) -> Result<Answer> {
    let config = &core.config;
    // QTS's own page sends the password base64 encoded ("ezEncode"), and so the server
    // expects it that way.
    let encoded = Secret::from(base64::engine::general_purpose::STANDARD.encode(config.password.expose().as_bytes()));
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let mut fields = vec![("user", config.user.trim()), ("pwd", encoded.expose()), ("serviceKey", "1"), ("remme", "1")];
    if !code.is_empty() {
        fields.push(("security_code", code.as_str()));
    }
    if !config.device_token.is_empty() {
        fields.push(("qtoken", config.device_token.expose()));
    }
    let mut request = Request::verb("POST", format!("{}cgi-bin/authLogin.cgi", core.base()?));
    request.body = Some(("application/x-www-form-urlencoded", crate::oauth::form_body(&fields)));
    let response = core.send(request).await?;
    let xml = String::from_utf8_lossy(&response.body);
    let sid = qnap_tag(&xml, "authSid").filter(|sid| !sid.is_empty());
    if qnap_tag(&xml, "authPassed").as_deref() == Some("1") && sid.is_some() {
        let token = qnap_tag(&xml, "qtoken").filter(|token| !token.is_empty()).map(Secret::from);
        return Ok(Answer { sid: sid.map(Secret::from), device_token: token, ..Answer::default() });
    }
    let needs_code = qnap_tag(&xml, "need_2sv").as_deref() == Some("1");
    Ok(Answer { needs_code, error: Some(Fault::PermissionLost), ..Answer::default() })
}

impl<H: AsyncHttp> Qnap<H> {
    /// A folder on the NAS `config` names.
    pub fn new(client: Client<H>, config: NasConfig) -> Self {
        Self { core: Core::new(client, config, QNAP_PORT) }
    }

    async fn login(&self) -> Result<Secret> {
        if !self.core.config.configured() {
            return Err(Fault::NotConfigured);
        }
        let answer = qnap_authenticate(&self.core, "").await?;
        answer.sid.ok_or(answer.error.unwrap_or(Fault::Io))
    }

    fn util(&self, func: &str, extra: &[(&str, &str)], sid: &Secret) -> Result<String> {
        let mut pairs = vec![("func", func), ("sid", sid.expose())];
        pairs.extend_from_slice(extra);
        Ok(format!("{}cgi-bin/filemanager/utilRequest.cgi?{}", self.core.base()?, query(&pairs)))
    }

    fn status(root: &Value, allowed: &[i64]) -> std::result::Result<(), Failure> {
        let status = root.get("status").and_then(Value::as_i64).unwrap_or(STATUS_SUCCESS);
        if status == STATUS_SUCCESS || allowed.contains(&status) {
            Ok(())
        } else {
            Err(Failure::Nas(status, qnap_status_fault(status)))
        }
    }

    async fn get(&self, func: &str, extra: &[(&str, &str)], allowed: &[i64]) -> Result<Value> {
        self.core
            .with_session(
                async || self.login().await,
                |status| status == STATUS_AUTH_FAIL,
                async |sid| {
                    let root = json_of(&self.core.send(Request::get(self.util(func, extra, sid)?)).await?)?;
                    Self::status(&root, allowed)?;
                    Ok(root)
                },
            )
            .await
    }

    /// Makes each level below the share; a level already there answers "name taken".
    async fn make_folder(&self) -> Result<()> {
        let parts: Vec<&str> = self.core.folder.split('/').filter(|part| !part.is_empty()).collect();
        let Some((first, rest)) = parts.split_first() else { return Ok(()) };
        let mut at = format!("/{first}");
        for part in rest {
            self.get("createdir", &[("dest_folder", part), ("dest_path", &at)], &[STATUS_EXISTS, STATUS_NAME_DUP]).await?;
            at.push('/');
            at.push_str(part);
        }
        Ok(())
    }
}

impl<H: AsyncHttp> Sink for Qnap<H> {
    async fn readiness(&self) -> Result<()> {
        self.make_folder().await
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        self.make_folder().await?;
        let progress = self.core.path_of(name).replace('/', "-");
        self.core
            .with_session(
                async || self.login().await,
                |status| status == STATUS_AUTH_FAIL,
                async |sid| {
                    let extra = [("type", "standard"), ("dest_path", self.core.folder.as_str()), ("overwrite", "1"), ("progress", progress.as_str())];
                    let mut request = Request::verb("POST", self.util("upload", &extra, sid)?);
                    request.body = Some((FORM_DATA, form_data(&[], "file", name, bytes)));
                    request.timeout_ms = 300_000;
                    Self::status(&json_of(&self.core.send(request).await?)?, &[])
                },
            )
            .await?;
        Ok(Stored { id: self.core.path_of(name), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let mut out = Vec::new();
        let mut start = 0usize;
        loop {
            let at = start.to_string();
            let limit = PAGE.to_string();
            let root = self
                .get(
                    "get_list",
                    &[
                        ("path", &self.core.folder),
                        ("list_mode", "all"),
                        ("is_iso", "0"),
                        ("start", &at),
                        ("limit", &limit),
                        ("sort", "filename"),
                        ("dir", "ASC"),
                        ("hidden_file", "0"),
                    ],
                    &[],
                )
                .await?;
            let files = root.get("datas").and_then(Value::as_array).cloned().unwrap_or_default();
            for file in &files {
                if file.get("isfolder").and_then(Value::as_i64) == Some(1) {
                    continue;
                }
                let Some(name) = file.get("filename").and_then(Value::as_str) else { continue };
                // QTS sends sizes as strings.
                let size = file.get("filesize").and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64())).unwrap_or(0);
                out.push(Stored { id: self.core.path_of(name), name: name.to_owned(), size });
            }
            start += files.len();
            let total = root.get("total").and_then(Value::as_u64).unwrap_or(0);
            if files.is_empty() || start as u64 >= total {
                return Ok(out);
            }
        }
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        self.core
            .with_session(
                async || self.login().await,
                |status| status == STATUS_AUTH_FAIL,
                async |sid| {
                    let extra = [("isfolder", "0"), ("source_path", self.core.folder.as_str()), ("source_file", file.name.as_str()), ("source_total", "1")];
                    let mut request = Request::get(self.util("download", &extra, sid)?);
                    request.timeout_ms = 300_000;
                    let response = self.core.send(request).await?;
                    if is_json(&response)
                        && let Ok(root) = response.json()
                    {
                        Self::status(&root, &[])?;
                    }
                    Ok(response.body)
                },
            )
            .await
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let extra = [("path", self.core.folder.as_str()), ("file_total", "1"), ("file_name", file.name.as_str()), ("v", "1")];
        self.get("delete", &extra, &[STATUS_NO_EXIST]).await.map(|_| ())
    }
}

/// Signs in to a QNAP NAS from the settings screen. `code` empty is the first try; with
/// the two-step code, the answer carries the `qtoken` to keep.
pub async fn qnap_login<H: AsyncHttp>(client: Client<H>, config: &NasConfig, code: &str) -> NasLogin {
    let core = Core::new(client, config.clone(), QNAP_PORT);
    match qnap_authenticate(&core, code).await {
        Ok(answer) => answer.into(),
        Err(fault) => NasLogin::Failed(fault),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Blocking, block_on};
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    fn config(path: &str) -> NasConfig {
        NasConfig { server: "nas.example".into(), user: "me".into(), password: "pw".into(), path: path.into(), ..NasConfig::default() }
    }

    #[test]
    fn a_nas_address_gets_https_and_the_vendor_port() {
        assert_eq!(base_url("nas.local", 5001).as_deref(), Some("https://nas.local:5001/"));
        assert_eq!(base_url("nas.local:8443", 5001).as_deref(), Some("https://nas.local:8443/"));
        assert_eq!(base_url("https://192.168.1.2/", 443).as_deref(), Some("https://192.168.1.2/"));
        assert_eq!(base_url("https://nas.local:443", 5001).as_deref(), Some("https://nas.local/"));
        assert_eq!(base_url("http://100.64.1.2:5000", 5001).as_deref(), Some("http://100.64.1.2:5000/"));
        assert_eq!(base_url("[fd00::2]", 5001).as_deref(), Some("https://[fd00::2]:5001/"));
        assert_eq!(base_url("  ", 443), None);
    }

    #[test]
    fn qnap_login_fields_come_out_of_its_xml() {
        let xml = "<QDocRoot><authPassed><![CDATA[1]]></authPassed><authSid><![CDATA[abc123]]></authSid><need_2sv>0</need_2sv><qtoken><![CDATA[tok]]></qtoken></QDocRoot>";
        assert_eq!(qnap_tag(xml, "authPassed").as_deref(), Some("1"));
        assert_eq!(qnap_tag(xml, "authSid").as_deref(), Some("abc123"));
        assert_eq!(qnap_tag(xml, "need_2sv").as_deref(), Some("0"));
        assert_eq!(qnap_tag(xml, "qtoken").as_deref(), Some("tok"));
        assert_eq!(qnap_tag(xml, "missing"), None);
    }

    #[test]
    fn each_vendors_failures_mean_the_right_thing() {
        assert_eq!(synology_auth_fault(400), Fault::PermissionLost);
        assert_eq!(synology_file_fault(416), Fault::OutOfSpace);
        assert_eq!(synology_file_fault(408), Fault::TargetMissing);
        assert_eq!(qnap_status_fault(9), Fault::OutOfSpace);
        assert_eq!(qnap_status_fault(5), Fault::TargetMissing);
        assert_eq!(qnap_status_fault(4), Fault::PermissionLost);
    }

    const DSM: &str = "https://nas.example:5001/webapi/entry.cgi";

    #[test]
    fn synology_signs_in_makes_the_folder_and_uploads_with_the_file_last() {
        let http = FakeHttp::new()
            .answer(&format!("{DSM}?api=SYNO.FileStation.Upload"), 200, r#"{"success":true}"#)
            .answer(&format!("{DSM}?api=SYNO.FileStation.CreateFolder"), 200, r#"{"success":true}"#)
            .answer(DSM, 200, r#"{"success":true,"data":{"sid":"S1"}}"#);
        let sink = Synology::new(Client::new(Blocking(&http), NetPolicy::Any), config("/home/Drive/WM Pass/"));
        let stored = block_on(sink.write("v.bin", b"data")).unwrap();
        assert_eq!(stored.id, "/home/Drive/WM Pass/v.bin");
        let asked = http.asked.lock().unwrap();
        assert_eq!(asked.len(), 3, "one sign-in for the whole run");
        let login = String::from_utf8_lossy(&asked[0].body.as_ref().unwrap().1).into_owned();
        assert!(login.contains("method=login") && login.contains("passwd=pw") && !asked[0].url.contains("pw"));
        assert!(asked[1].url.contains("folder_path=%5B%22%2Fhome%2FDrive%22%5D&name=%5B%22WM%20Pass%22%5D&force_parent=true&_sid=S1"));
        let form = String::from_utf8_lossy(&asked[2].body.as_ref().unwrap().1).into_owned();
        assert!(form.find("name=\"overwrite\"").unwrap() < form.find("filename=\"v.bin\"").unwrap());
    }

    #[test]
    fn synology_signs_in_again_once_when_the_session_expired() {
        let http = FakeHttp::new()
            .answer_once(&format!("{DSM}?api=SYNO.FileStation.List"), 200, r#"{"success":false,"error":{"code":119}}"#)
            .answer(&format!("{DSM}?api=SYNO.FileStation.List"), 200, r#"{"success":true,"data":{"total":2,"files":[{"name":"d","isdir":true},{"name":"v","isdir":false,"additional":{"size":3}}]}}"#)
            .answer(DSM, 200, r#"{"success":true,"data":{"sid":"S"}}"#);
        let sink = Synology::new(Client::new(Blocking(&http), NetPolicy::Any), config("/share"));
        let files = block_on(sink.list()).unwrap();
        assert_eq!(files, vec![Stored { id: "/share/v".into(), name: "v".into(), size: 3 }]);
        let logins = http.asked.lock().unwrap().iter().filter(|r| r.method == "POST").count();
        assert_eq!(logins, 2);
    }

    #[test]
    fn a_synology_download_that_answers_json_is_a_failure() {
        let http = FakeHttp::new()
            .answer_with(&format!("{DSM}?api=SYNO.FileStation.Download"), 200, br#"{"success":false,"error":{"code":408}}"#, &[("Content-Type", "application/json; charset=utf-8")])
            .answer(DSM, 200, r#"{"success":true,"data":{"sid":"S"}}"#);
        let sink = Synology::new(Client::new(Blocking(&http), NetPolicy::Any), config("/share"));
        let file = Stored { id: "/share/v".into(), name: "v".into(), size: 0 };
        assert_eq!(block_on(sink.read(&file)), Err(Fault::TargetMissing));
    }

    #[test]
    fn synology_2fa_asks_for_a_code_and_the_code_brings_a_device_token() {
        let asks = FakeHttp::new().answer(DSM, 200, r#"{"success":false,"error":{"code":403}}"#);
        assert_eq!(block_on(synology_login(Client::new(Blocking(&asks), NetPolicy::Any), &config("/s"), "")), NasLogin::NeedsCode);
        let done = FakeHttp::new().answer(DSM, 200, r#"{"success":true,"data":{"sid":"S","did":"D"}}"#);
        let login = block_on(synology_login(Client::new(Blocking(&done), NetPolicy::Any), &config("/s"), "123 456"));
        assert_eq!(login, NasLogin::SignedIn { device_token: Some("D".into()) });
        let body = String::from_utf8(done.last().unwrap().body.unwrap().1).unwrap();
        assert!(body.contains("otp_code=123456&enable_device_token=yes"));
        let wrong = FakeHttp::new().answer(DSM, 200, r#"{"success":false,"error":{"code":400}}"#);
        assert_eq!(block_on(synology_login(Client::new(Blocking(&wrong), NetPolicy::Any), &config("/s"), "")), NasLogin::Failed(Fault::PermissionLost));
    }

    const QTS: &str = "https://nas.example/cgi-bin/";

    #[test]
    fn qnap_sends_the_password_encoded_and_makes_each_level_below_the_share() {
        let http = FakeHttp::new()
            .answer(&format!("{QTS}authLogin.cgi"), 200, "<QDocRoot><authPassed><![CDATA[1]]></authPassed><authSid><![CDATA[Q]]></authSid></QDocRoot>")
            .answer(&format!("{QTS}filemanager/utilRequest.cgi?func=createdir"), 200, r#"{"status":2}"#)
            .answer(QTS, 200, r#"{"status":1}"#);
        let sink = Qnap::new(Client::new(Blocking(&http), NetPolicy::Any), config("Public/WM/Pass"));
        block_on(sink.write("v.bin", b"x")).unwrap();
        let asked = http.asked.lock().unwrap();
        assert!(String::from_utf8_lossy(&asked[0].body.as_ref().unwrap().1).contains("pwd=cHc%3D"));
        assert!(asked[1].url.ends_with("func=createdir&sid=Q&dest_folder=WM&dest_path=%2FPublic"));
        assert!(asked[2].url.ends_with("dest_folder=Pass&dest_path=%2FPublic%2FWM"));
        assert!(asked[3].url.contains("func=upload&sid=Q&type=standard&dest_path=%2FPublic%2FWM%2FPass&overwrite=1&progress=-Public-WM-Pass-v.bin"));
    }

    #[test]
    fn qnap_lists_string_sizes_and_a_missing_file_is_already_deleted() {
        let http = FakeHttp::new()
            .answer(&format!("{QTS}authLogin.cgi"), 200, "<QDocRoot><authPassed>1</authPassed><authSid>Q</authSid></QDocRoot>")
            .answer(&format!("{QTS}filemanager/utilRequest.cgi?func=get_list"), 200, r#"{"total":2,"datas":[{"filename":"d","isfolder":1},{"filename":"v","isfolder":0,"filesize":"12"}]}"#)
            .answer(QTS, 200, r#"{"status":5}"#);
        let sink = Qnap::new(Client::new(Blocking(&http), NetPolicy::Any), config("/Public"));
        let files = block_on(sink.list()).unwrap();
        assert_eq!(files, vec![Stored { id: "/Public/v".into(), name: "v".into(), size: 12 }]);
        assert_eq!(block_on(sink.delete(&files[0])), Ok(()));
    }

    #[test]
    fn qnap_two_step_asks_for_a_code_and_plain_http_never_sends_the_password() {
        let asks = FakeHttp::new().answer(QTS, 200, "<QDocRoot><authPassed>0</authPassed><need_2sv>1</need_2sv></QDocRoot>");
        assert_eq!(block_on(qnap_login(Client::new(Blocking(&asks), NetPolicy::Any), &config("/P"), "")), NasLogin::NeedsCode);
        let done = FakeHttp::new().answer(QTS, 200, "<QDocRoot><authPassed>1</authPassed><authSid>Q</authSid><qtoken>T</qtoken></QDocRoot>");
        let login = block_on(qnap_login(Client::new(Blocking(&done), NetPolicy::Any), &config("/P"), "12 34"));
        assert_eq!(login, NasLogin::SignedIn { device_token: Some("T".into()) });
        assert!(String::from_utf8(done.last().unwrap().body.unwrap().1).unwrap().contains("security_code=1234"));

        let plain = FakeHttp::new();
        let lan = NasConfig { server: "http://192.168.1.2".into(), ..config("/P") };
        let sink = Qnap::new(Client::new(Blocking(&plain), NetPolicy::Any), lan);
        assert_eq!(block_on(sink.readiness()), Ok(()), "nothing to make at the share's top");
        assert_eq!(block_on(sink.list()), Err(Fault::Blocked));
        assert!(plain.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn debug_shows_no_secret_or_address() {
        let config = NasConfig { device_token: "dt".into(), cert_pin: "pin".into(), ..config("/secret/path") };
        let printed = format!("{config:?}");
        assert!(!printed.contains("pw") && !printed.contains("nas.example") && !printed.contains("secret") && !printed.contains("dt"));
    }
}

#[cfg(test)]
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod pin_tests {
    use super::*;

    #[test]
    fn a_stored_pin_must_decode_to_a_sha256() {
        let pinned = NasConfig { own_certificate: true, cert_pin: "not base64!".into(), ..NasConfig::default() };
        assert!(matches!(nas_http(&pinned, Box::new(|_| {})), Err(Fault::NotConfigured)));
        let short = NasConfig { cert_pin: "AAAA".into(), ..pinned.clone() };
        assert!(matches!(nas_http(&short, Box::new(|_| {})), Err(Fault::NotConfigured)));
        let fresh = NasConfig { cert_pin: String::new(), ..pinned };
        assert!(nas_http(&fresh, Box::new(|_| {})).is_ok());
    }

    /// An HTTPS server on this device with its own certificate, answering every request
    /// as a DSM that signs in and does what it is asked.
    fn serve() -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for socket in listener.incoming().flatten() {
                let Ok(connection) = rustls::ServerConnection::new(crate::ftp::test_tls::server()) else { continue };
                let mut tls = rustls::StreamOwned::new(connection, socket);
                let mut seen = Vec::new();
                let mut buffer = [0u8; 4096];
                while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => seen.extend_from_slice(&buffer[..n]),
                    }
                }
                let body = r#"{"success":true,"data":{"sid":"S"}}"#;
                let _ = write!(tls, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });
        port
    }

    #[test]
    fn the_nas_certificate_is_pinned_on_first_use_and_a_changed_one_is_unsafe() {
        use std::sync::{Arc, Mutex};

        use crate::http::{Blocking, block_on};
        use wmpass_net::NetPolicy;

        let port = serve();
        let config = NasConfig {
            server: format!("localhost:{port}"),
            user: "me".into(),
            password: "pw".into(),
            path: "/share/WM".into(),
            own_certificate: true,
            ..NasConfig::default()
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let http = nas_http(&config, Box::new(move |pin| record.lock().unwrap().push(pin))).unwrap();
        block_on(Synology::new(Client::new(Blocking(&*http), NetPolicy::Any), config.clone()).readiness()).unwrap();
        let leaf = include_bytes!("../testdata/leaf.der");
        let expected = base64::engine::general_purpose::STANDARD.encode(wmpass_net::http::certificate_pin(leaf));
        assert_eq!(*seen.lock().unwrap(), vec![expected.clone()]);

        let pinned = NasConfig { cert_pin: expected, ..config.clone() };
        let http = nas_http(&pinned, Box::new(|_| panic!("already pinned"))).unwrap();
        block_on(Synology::new(Client::new(Blocking(&*http), NetPolicy::Any), pinned).readiness()).unwrap();

        let changed = NasConfig { cert_pin: base64::engine::general_purpose::STANDARD.encode([7u8; 32]), ..config.clone() };
        let http = nas_http(&changed, Box::new(|_| panic!("already pinned"))).unwrap();
        assert_eq!(block_on(Synology::new(Client::new(Blocking(&*http), NetPolicy::Any), changed).readiness()), Err(Fault::Unsafe));

        // Without the setting, a certificate no authority vouches for is refused the same way.
        let ordinary = NasConfig { own_certificate: false, ..config };
        let http = nas_http(&ordinary, Box::new(|_| panic!("no pin"))).unwrap();
        assert_eq!(block_on(Qnap::new(Client::new(Blocking(&*http), NetPolicy::Any), ordinary).readiness()), Err(Fault::Unsafe));
    }
}
