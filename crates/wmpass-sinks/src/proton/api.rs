//! Proton's API: JSON in and out, a `Code` of 1000 or 1001 for success, the session in
//! `x-pm-uid` and a bearer token. A 401 refreshes the token once.

use std::sync::{Mutex, MutexGuard};

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_net::{Request, Response};

use super::ProtonTokens;
use crate::Fault;
use crate::http::{AsyncHttp, Client};

pub(crate) const HOST: &str = "https://drive-api.proton.me";

/// What Proton asks third-party Drive clients to send: `external-drive-<project>@<version>`.
/// The value WM Keyboard sends, kept as it is: whether Proton accepts it is only known
/// live.
pub const APP_VERSION: &str = "external-drive-wmkeyboard@1.0.0";

const CODE_OK: i64 = 1000;
const CODE_MULTI: i64 = 1001;
const STATE_CHARS: usize = 16;
const BOUNDARY: &str = "wmpass-proton-block-5c1e";

/// A failure Proton reported, with its own code: the sign-in reads it, the sink only the
/// fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApiError {
    pub code: i64,
    pub fault: Fault,
}

impl From<Fault> for ApiError {
    fn from(fault: Fault) -> Self {
        Self { code: 0, fault }
    }
}

impl From<ApiError> for Fault {
    fn from(error: ApiError) -> Self {
        error.fault
    }
}

pub(crate) type ApiResult<T> = std::result::Result<T, ApiError>;

/// What a Proton failure means to the user: 401 and 10013 a session that is gone, 8002 a
/// wrong password, 9001 a CAPTCHA wanted; 2501 a missing link; 2003 or "storage" in the
/// message a full account.
pub fn status_error(status: u16, code: i64, message: &str) -> Fault {
    let lower = message.to_lowercase();
    if code == 200_001 || code == 2003 || (lower.contains("storage") && lower.contains("quota")) || lower.contains("insufficient") {
        Fault::OutOfSpace
    } else if status == 401 || status == 403 || code == 10013 || code == 8002 || code == 9001 {
        Fault::PermissionLost
    } else if status == 404 || code == 2501 {
        Fault::TargetMissing
    } else {
        Fault::Io
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The session's current tokens.
#[derive(Default)]
pub(crate) struct Session {
    pub uid: String,
    pub access: Secret,
    pub refresh: Secret,
}

pub(crate) struct Api<'a, H> {
    pub client: &'a Client<H>,
    pub session: &'a Mutex<Session>,
    pub on_tokens: Option<&'a (dyn Fn(&ProtonTokens) + Send + Sync)>,
}

fn answer(response: &Response) -> ApiResult<Value> {
    let root: Value = serde_json::from_slice(&response.body).unwrap_or_else(|_| json!({}));
    let code = root.get("Code").and_then(Value::as_i64).unwrap_or(0);
    if (200..300).contains(&response.status) && (code == 0 || code == CODE_OK || code == CODE_MULTI) {
        return Ok(root);
    }
    let message = root.get("Error").and_then(Value::as_str).unwrap_or_default();
    Err(ApiError { code, fault: status_error(response.status, code, message) })
}

impl<H: AsyncHttp> Api<'_, H> {
    fn request(&self, method: &'static str, path: &str, body: Option<&Value>) -> Request {
        let mut request = match body {
            Some(body) => Request::json(method, format!("{HOST}/{path}"), body),
            None => Request::verb(method, format!("{HOST}/{path}")),
        }
        .header("x-pm-appversion", APP_VERSION)
        .header("Accept", "application/vnd.protonmail.v1+json");
        let session = lock(self.session);
        if !session.uid.is_empty() {
            request = request.header("x-pm-uid", session.uid.clone());
        }
        if !session.access.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", session.access.expose()));
        }
        request.timeout_ms = 60_000;
        request
    }

    pub async fn get(&self, path: &str) -> ApiResult<Value> {
        self.call("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> ApiResult<Value> {
        self.call("POST", path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: &Value) -> ApiResult<Value> {
        self.call("PUT", path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> ApiResult<Value> {
        self.call("DELETE", path, None).await
    }

    async fn call(&self, method: &'static str, path: &str, body: Option<&Value>) -> ApiResult<Value> {
        let first = self.client.send(self.request(method, path, body)).await?;
        if first.status != 401 || lock(self.session).refresh.is_empty() {
            return answer(&first);
        }
        self.refresh_tokens().await?;
        answer(&self.client.send(self.request(method, path, body)).await?)
    }

    /// Trades the refresh token for new tokens; Proton rotates both.
    async fn refresh_tokens(&self) -> ApiResult<()> {
        let state: String = super::pgp::new_passphrase()?.chars().take(STATE_CHARS).collect();
        let body = {
            let session = lock(self.session);
            json!({
                "UID": session.uid,
                "RefreshToken": session.refresh.expose(),
                "ResponseType": "token",
                "GrantType": "refresh_token",
                "RedirectURI": "https://protonmail.ch",
                "State": state,
            })
        };
        let saved = std::mem::take(&mut lock(self.session).access);
        let root = match self.client.send(self.request("POST", "auth/v4/refresh", Some(&body))).await {
            Ok(response) => answer(&response),
            Err(fault) => Err(fault.into()),
        };
        let root = match root {
            Ok(root) => root,
            Err(error) => {
                lock(self.session).access = saved;
                // A refused refresh token is the user signing this app out, or the
                // session having been revoked: only a new sign-in helps.
                return Err(if error.fault == Fault::Io { error } else { Fault::PermissionLost.into() });
            }
        };
        let access = root.get("AccessToken").and_then(Value::as_str).ok_or(Fault::PermissionLost)?;
        let tokens = {
            let mut session = lock(self.session);
            session.access = Secret::new(access);
            if let Some(refresh) = root.get("RefreshToken").and_then(Value::as_str) {
                session.refresh = Secret::new(refresh);
            }
            ProtonTokens { access: session.access.clone(), refresh: session.refresh.clone() }
        };
        if let Some(on_tokens) = self.on_tokens {
            on_tokens(&tokens);
        }
        Ok(())
    }

    pub async fn upload_block(&self, url: &str, token: &str, block: &[u8]) -> ApiResult<()> {
        let mut body = Vec::with_capacity(block.len() + 256);
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"Block\"; filename=\"blob\"\r\nContent-Type: application/octet-stream\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(block);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
        let mut request = Request::verb("POST", url)
            .header("pm-storage-token", token)
            .header("x-pm-appversion", APP_VERSION);
        request.body = Some(("multipart/form-data; boundary=wmpass-proton-block-5c1e", body));
        request.timeout_ms = 300_000;
        answer(&self.client.send(request).await?).map(|_| ())
    }

    pub async fn download_block(&self, url: &str, token: &str) -> ApiResult<Vec<u8>> {
        let mut request = Request::get(url)
            .header("pm-storage-token", token)
            .header("x-pm-appversion", APP_VERSION);
        request.timeout_ms = 300_000;
        let response = self.client.send(request).await?;
        if !(200..300).contains(&response.status) {
            return Err(status_error(response.status, 0, "").into());
        }
        Ok(response.body)
    }
}
