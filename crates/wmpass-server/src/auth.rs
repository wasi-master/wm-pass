//! Tokens, password checks and throttling.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use wmpass_core::crypto::{self, KdfParams};
use wmpass_core::vault::hex;

use crate::db::{User, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::{AppState, State};

/// Access tokens last an hour. Refresh tokens last until revoked.
pub const ACCESS_SECONDS: i64 = 3600;
const JWT_HEADER: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// Cost of the server's own hash. The client already stretched the password, and this
/// stops a database leak from being usable to log in directly.
const SERVER_HASH: KdfParams = KdfParams { m_kib: 64 * 1024, t: 3, p: 1 };

/// Reads the signing key, or makes one readable only by this user.
pub fn load_or_create_key(dir: &Path) -> Result<[u8; 32], String> {
    let path = dir.join("jwt.key");
    match std::fs::read(&path) {
        Ok(bytes) => bytes.as_slice().try_into().map_err(|_| "the signing key file is damaged".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let key: [u8; 32] = crypto::random_array().map_err(|_| "no system randomness".to_owned())?;
            write_private(&path, &key).map_err(|e| format!("cannot write the signing key: {e}"))?;
            Ok(key)
        }
        Err(error) => Err(format!("cannot read the signing key: {error}")),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

fn sign(key: &[u8; 32], message: &[u8]) -> ApiResult<Vec<u8>> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| ApiError::Internal)?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Issues an access token. Clients read the claims, so they follow what they expect.
pub fn issue_access_token(key: &[u8; 32], user: &User, device_id: &str) -> ApiResult<String> {
    let now = now_ms() / 1000;
    let claims = json!({
        "nbf": now,
        "exp": now + ACCESS_SECONDS,
        "iss": "wmpass-server",
        "sub": user.id,
        "premium": true,
        "name": user.name,
        "email": user.email,
        "email_verified": true,
        "sstamp": user.security_stamp,
        "device": device_id,
        "scope": ["api", "offline_access"],
        "amr": ["Application"],
    });
    let head = format!("{}.{}", URL_SAFE_NO_PAD.encode(JWT_HEADER), URL_SAFE_NO_PAD.encode(claims.to_string()));
    let signature = sign(key, head.as_bytes())?;
    Ok(format!("{head}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

/// Checks a token and returns its claims. Only this server's exact header is accepted, so
/// the classic tricks of changing the algorithm or stripping the signature do nothing.
pub fn verify_access_token(key: &[u8; 32], token: &str) -> Option<Value> {
    let (head, signature) = token.rsplit_once('.')?;
    let (header, claims) = head.split_once('.')?;
    if URL_SAFE_NO_PAD.decode(header).ok()? != JWT_HEADER.as_bytes() {
        return None;
    }
    let given = URL_SAFE_NO_PAD.decode(signature).ok()?;
    let expected = sign(key, head.as_bytes()).ok()?;
    if !bool::from(expected.as_slice().ct_eq(&given)) {
        return None;
    }
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).ok()?).ok()?;
    let now = now_ms() / 1000;
    (claims.get("exp")?.as_i64()? > now && claims.get("nbf")?.as_i64()? <= now + 60).then_some(claims)
}

/// The hash stored for a login hash.
pub async fn hash_password(state: &AppState, login_hash: &str, salt: &str) -> ApiResult<String> {
    let _slot = state.hashing.acquire().await.map_err(|_| ApiError::Internal)?;
    let input = login_hash.as_bytes().to_vec();
    let salt = salt.as_bytes().to_vec();
    let output = tokio::task::spawn_blocking(move || crypto::argon2id(&input, &salt, SERVER_HASH))
        .await
        .map_err(|_| ApiError::Internal)?
        .map_err(|_| ApiError::Internal)?;
    Ok(STANDARD.encode(&output[..]))
}

/// A new random salt or token.
pub fn random_token() -> ApiResult<String> {
    let bytes: [u8; 32] = crypto::random_array().map_err(|_| ApiError::Internal)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Refresh tokens are stored hashed, so a leaked database does not hold live sessions.
pub fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Constant-time comparison of two stored values.
pub fn same(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Failed-login tracking: five free tries, then a wait that doubles from 30 seconds to 15
/// minutes. Kept per address, so an attacker's many IP addresses do not help.
#[derive(Debug, Default)]
pub struct Throttle {
    state: Mutex<HashMap<String, (u32, Instant)>>,
}

const FREE_TRIES: u32 = 5;

fn wait_after(failures: u32, last: Instant) -> Option<u64> {
    if failures < FREE_TRIES {
        return None;
    }
    let doubling = (failures - FREE_TRIES).min(5);
    let wait = Duration::from_secs(30 << doubling).min(Duration::from_secs(900));
    wait.checked_sub(last.elapsed()).map(|left| left.as_secs().max(1))
}

fn note_failure(state: &mut HashMap<String, (u32, Instant)>, key: &str) {
    // Bounded, so a flood of invented addresses cannot grow this without limit.
    if state.len() > 100_000 {
        state.retain(|_, (_, last)| last.elapsed() < Duration::from_secs(900));
    }
    let slot = state.entry(key.to_owned()).or_insert((0, Instant::now()));
    slot.0 = slot.0.saturating_add(1);
    slot.1 = Instant::now();
}

impl Throttle {
    /// Seconds to wait before another try, if any.
    pub fn wait(&self, key: &str) -> Option<u64> {
        let state = self.state.lock().ok()?;
        let (failures, last) = state.get(key)?;
        wait_after(*failures, *last)
    }

    /// Starts a password check: the seconds to wait if it must not run, or else counts it as a
    /// failure until [`Self::succeeded`] says otherwise. Checking and counting under one lock
    /// means many guesses sent at once cannot all pass while the first are still hashing.
    pub fn attempt(&self, key: &str) -> Option<u64> {
        let mut state = self.state.lock().ok()?;
        if let Some(wait) = state.get(key).and_then(|(failures, last)| wait_after(*failures, *last)) {
            return Some(wait);
        }
        note_failure(&mut state, key);
        None
    }

    /// Notes a failure.
    pub fn failed(&self, key: &str) {
        if let Ok(mut state) = self.state.lock() {
            note_failure(&mut state, key);
        }
    }

    /// Clears after a success.
    pub fn succeeded(&self, key: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.remove(key);
        }
    }
}

/// A request from a logged-in user.
#[derive(Clone, Debug)]
pub struct Auth {
    /// Who.
    pub user: User,
    /// From which device.
    pub device_id: String,
}

/// Checks a bearer token against the current user and device.
pub async fn authenticate(state: &State, token: &str) -> ApiResult<Auth> {
    let claims = verify_access_token(&state.jwt_key, token).ok_or(ApiError::Unauthorised)?;
    let user_id = claims.get("sub").and_then(Value::as_str).ok_or(ApiError::Unauthorised)?;
    let device_id = claims.get("device").and_then(Value::as_str).ok_or(ApiError::Unauthorised)?.to_owned();
    let stamp = claims.get("sstamp").and_then(Value::as_str).ok_or(ApiError::Unauthorised)?;
    let user = state.db.user_by_id(user_id).await?.ok_or(ApiError::Unauthorised)?;
    // A password change rotates the stamp, which ends every session.
    if !same(&user.security_stamp, stamp) {
        return Err(ApiError::Unauthorised);
    }
    // A revoked device loses access at once, not when its token expires.
    let device = sqlx::query("SELECT id FROM devices WHERE id = $1 AND user_id = $2")
        .bind(&device_id)
        .bind(&user.id)
        .fetch_optional(&state.db.pool)
        .await?;
    if device.is_none() {
        return Err(ApiError::Unauthorised);
    }
    Ok(Auth { user, device_id })
}

impl FromRequestParts<State> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, Self::Rejection> {
        let header = parts.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).ok_or(ApiError::Unauthorised)?;
        let token = header.strip_prefix("Bearer ").ok_or(ApiError::Unauthorised)?;
        authenticate(state, token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> User {
        User {
            id: "u".into(),
            email: "a@b.c".into(),
            name: "A".into(),
            password_hash: String::new(),
            password_salt: String::new(),
            kdf: 0,
            kdf_iterations: 0,
            kdf_memory: None,
            kdf_parallelism: None,
            user_key: String::new(),
            public_key: None,
            private_key: None,
            security_stamp: "s".into(),
            revision: 0,
        }
    }

    #[test]
    fn tokens_verify_and_any_forgery_fails() {
        let key = [7u8; 32];
        let token = issue_access_token(&key, &user(), "d").unwrap();
        assert_eq!(verify_access_token(&key, &token).unwrap()["sub"], "u");
        assert!(verify_access_token(&[8u8; 32], &token).is_none(), "wrong key");

        let (head, _) = token.rsplit_once('.').unwrap();
        assert!(verify_access_token(&key, &format!("{head}.")).is_none(), "no signature");
        let none = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
        let claims = token.split('.').nth(1).unwrap();
        assert!(verify_access_token(&key, &format!("{none}.{claims}.")).is_none(), "alg none");
        let other = URL_SAFE_NO_PAD.encode(r#"{"sub":"admin","exp":99999999999,"nbf":0}"#);
        let sig = token.rsplit('.').next().unwrap();
        let header = token.split('.').next().unwrap();
        assert!(verify_access_token(&key, &format!("{header}.{other}.{sig}")).is_none(), "swapped claims");
    }

    #[test]
    fn expired_tokens_are_refused() {
        let key = [7u8; 32];
        let claims = json!({"sub":"u","exp": now_ms()/1000 - 1,"nbf":0});
        let head = format!("{}.{}", URL_SAFE_NO_PAD.encode(JWT_HEADER), URL_SAFE_NO_PAD.encode(claims.to_string()));
        let token = format!("{head}.{}", URL_SAFE_NO_PAD.encode(sign(&key, head.as_bytes()).unwrap()));
        assert!(verify_access_token(&key, &token).is_none());
    }

    #[test]
    fn throttle_waits_after_five_failures_and_clears_on_success() {
        let throttle = Throttle::default();
        for _ in 0..4 {
            throttle.failed("a");
        }
        assert_eq!(throttle.wait("a"), None);
        throttle.failed("a");
        assert!(throttle.wait("a").is_some_and(|s| s <= 30));
        for _ in 0..20 {
            throttle.failed("a");
        }
        assert!(throttle.wait("a").is_some_and(|s| s <= 900 && s > 30));
        assert_eq!(throttle.wait("b"), None);
        throttle.succeeded("a");
        assert_eq!(throttle.wait("a"), None);
    }

    #[test]
    fn an_attempt_counts_before_it_is_judged() {
        let throttle = Throttle::default();
        // Five guesses in flight at once, none finished: the sixth must already wait.
        for _ in 0..FREE_TRIES {
            assert_eq!(throttle.attempt("a"), None);
        }
        assert!(throttle.attempt("a").is_some());
        assert!(throttle.wait("a").is_some());
        throttle.succeeded("a");
        assert_eq!(throttle.attempt("a"), None);
        assert_eq!(throttle.attempt("b"), None);
    }
}
