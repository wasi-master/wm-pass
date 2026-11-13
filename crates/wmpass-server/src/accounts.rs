//! The profile, sync and small read-only endpoints clients check at startup.

use axum::extract::State as Extract;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::Row;

use crate::auth::{Auth, hash_password, random_token, same};
use crate::db::{User, date, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::identity::{account_keys, field, master_password_unlock, revoke_other_devices};
use crate::{State, ciphers, folders, organisations};

/// Routes.
pub fn routes() -> Router<State> {
    Router::new()
        .route("/api/sync", get(sync))
        .route("/api/accounts/profile", get(profile).put(update_profile).post(update_profile))
        .route("/api/accounts/keys", post(set_keys))
        .route("/api/accounts/revision-date", get(revision_date))
        .route("/api/accounts/password", post(change_password))
        .route("/api/accounts/security-stamp", post(rotate_stamp))
        .route("/api/accounts/key-management/user-key-id", post(user_key_id))
        .route("/api/config", get(config))
        .route("/api/alive", get(alive))
        .route("/api/now", get(alive))
        .route("/api/version", get(version))
        .route("/api/devices/knowndevice", get(known_device))
        .route("/api/users/{id}/public-key", get(public_key))
}

/// The profile object.
pub async fn profile_json(state: &State, user: &User) -> ApiResult<Value> {
    Ok(json!({
        "id": user.id,
        "name": user.name,
        "email": user.email,
        "emailVerified": true,
        "premium": true,
        "premiumFromOrganization": false,
        "masterPasswordHint": null,
        "culture": "en-US",
        "twoFactorEnabled": false,
        "key": user.user_key,
        "privateKey": user.private_key,
        "accountKeys": account_keys(user),
        "securityStamp": user.security_stamp,
        "forcePasswordReset": false,
        "usesKeyConnector": false,
        "avatarColor": null,
        "creationDate": date(user.revision),
        "organizations": organisations::profile_organisations(state, user).await?,
        "providers": [],
        "providerOrganizations": [],
        "object": "profile",
    }))
}

async fn sync(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    let device = auth.device_id.clone();
    sqlx::query("UPDATE devices SET last_seen = $1 WHERE id = $2").bind(now_ms()).bind(&device).execute(&state.db.pool).await?;
    Ok(Json(json!({
        "profile": profile_json(&state, &auth.user).await?,
        "folders": folders::list_json(&state, &auth.user.id).await?,
        "collections": organisations::visible_collections(&state, &auth.user.id).await?,
        "policies": [],
        "ciphers": ciphers::visible_ciphers(&state, &auth.user.id).await?,
        "domains": {"equivalentDomains": [], "globalEquivalentDomains": [], "object": "domains"},
        "sends": [],
        "userDecryption": {"masterPasswordUnlock": master_password_unlock(&auth.user)},
        "object": "sync",
    })))
}

async fn profile(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    Ok(Json(profile_json(&state, &auth.user).await?))
}

async fn update_profile(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let name = field(&body, "name").unwrap_or(&auth.user.name).to_owned();
    sqlx::query("UPDATE users SET name = $1 WHERE id = $2").bind(&name).bind(&auth.user.id).execute(&state.db.pool).await?;
    let mut user = auth.user;
    user.name = name;
    Ok(Json(profile_json(&state, &user).await?))
}

/// Only sets a key pair the account lacks. Replacing an existing one would strand every
/// organisation key wrapped to it, or let a stolen session swap in its own.
async fn set_keys(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    if auth.user.public_key.is_some() {
        return Err(ApiError::Bad("This account already has keys."));
    }
    let public_key = field(&body, "publicKey").ok_or(ApiError::Bad("A public key is required."))?;
    let private_key = field(&body, "encryptedPrivateKey").ok_or(ApiError::Bad("A private key is required."))?;
    sqlx::query("UPDATE users SET public_key = $1, private_key = $2 WHERE id = $3")
        .bind(public_key)
        .bind(private_key)
        .bind(&auth.user.id)
        .execute(&state.db.pool)
        .await?;
    Ok(Json(json!({"object": "keys", "publicKey": public_key, "privateKey": private_key})))
}

async fn revision_date(auth: Auth) -> Json<i64> {
    Json(auth.user.revision)
}

async fn change_password(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let current = field(&body, "masterPasswordHash").ok_or(ApiError::Bad("The current password is required."))?;
    let new = field(&body, "newMasterPasswordHash").ok_or(ApiError::Bad("A new password is required."))?;
    let key = field(&body, "key").ok_or(ApiError::Bad("The account key is required."))?;
    if !password_matches(&state, &auth.user, current).await? {
        return Err(ApiError::Bad("The current password is not correct."));
    }
    let salt = random_token()?;
    let hash = hash_password(&state, new, &salt).await?;
    sqlx::query("UPDATE users SET password_hash = $1, password_salt = $2, user_key = $3, security_stamp = $4 WHERE id = $5")
        .bind(&hash)
        .bind(&salt)
        .bind(key)
        .bind(random_token()?)
        .bind(&auth.user.id)
        .execute(&state.db.pool)
        .await?;
    revoke_other_devices(&state, &auth.user.id, &auth.device_id).await?;
    Ok(Json(json!({"object": "password"})))
}

/// Checks the password a signed-in user gives again, before something that cannot be undone.
/// Throttled like logging in, so a session left open cannot be used to guess it.
pub(crate) async fn password_matches(state: &State, user: &User, given: &str) -> ApiResult<bool> {
    let throttle_key = format!("password:{}", user.id);
    if let Some(seconds) = state.throttle.attempt(&throttle_key) {
        return Err(ApiError::TooMany(seconds));
    }
    let actual = hash_password(state, given, &user.password_salt).await?;
    let matches = same(&actual, &user.password_hash);
    if matches {
        state.throttle.succeeded(&throttle_key);
    }
    Ok(matches)
}

async fn rotate_stamp(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    sqlx::query("UPDATE users SET security_stamp = $1 WHERE id = $2").bind(random_token()?).bind(&auth.user.id).execute(&state.db.pool).await?;
    revoke_other_devices(&state, &auth.user.id, &auth.device_id).await?;
    Ok(Json(json!({"object": "securityStamp"})))
}

/// Clients report a label for the account key after login, and block the login on the answer
/// being a success. The label is meaningless to this server, so it is accepted and dropped.
async fn user_key_id(_auth: Auth) -> Json<Value> {
    Json(json!({}))
}

/// The address clients should use. The configured public address when there is one, since
/// `Host` and `X-Forwarded-Proto` come from the client and can say anything.
fn base_url(state: &State, headers: &HeaderMap) -> String {
    if let Some(url) = &state.config.public_url {
        return url.trim_end_matches('/').to_owned();
    }
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
    let scheme = headers.get("x-forwarded-proto").and_then(|h| h.to_str().ok()).unwrap_or("http");
    format!("{scheme}://{host}")
}

async fn config(Extract(state): Extract<State>, headers: HeaderMap) -> Json<Value> {
    let base = base_url(&state, &headers);
    Json(json!({
        "version": "2026.10.0",
        "gitHash": "wmpass",
        "server": {"name": "WM Pass", "url": "https://github.com/wasi-master/wm-pass"},
        "environment": {
            "cloudRegion": null,
            "vault": base,
            "api": format!("{base}/api"),
            "identity": format!("{base}/identity"),
            "notifications": format!("{base}/notifications"),
            "sso": "",
        },
        "featureStates": {},
        "object": "config",
    }))
}

async fn alive() -> Json<String> {
    Json(date(now_ms()))
}

async fn version() -> Json<&'static str> {
    Json("2026.10.0")
}

async fn known_device(Extract(state): Extract<State>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    use base64::Engine;
    let read = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let email = read("x-request-email")
        .and_then(|encoded| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let known = match (email, read("x-device-identifier")) {
        (Some(email), Some(identifier)) => {
            let row = sqlx::query("SELECT d.id FROM devices d JOIN users u ON u.id = d.user_id WHERE u.email = $1 AND d.identifier = $2")
                .bind(email.trim().to_lowercase())
                .bind(identifier)
                .fetch_optional(&state.db.pool)
                .await?;
            row.is_some()
        }
        _ => false,
    };
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))], known.to_string()))
}

async fn public_key(Extract(state): Extract<State>, _auth: Auth, axum::extract::Path(id): axum::extract::Path<String>) -> ApiResult<Json<Value>> {
    let row = sqlx::query("SELECT public_key FROM users WHERE id = $1").bind(&id).fetch_optional(&state.db.pool).await?.ok_or(ApiError::NotFound)?;
    let key: Option<String> = row.try_get("public_key")?;
    Ok(Json(json!({"userId": id, "publicKey": key, "object": "userKey"})))
}

