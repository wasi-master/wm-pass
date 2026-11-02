//! Prelogin, registration and tokens.

use std::collections::HashMap;

use axum::extract::State as Extract;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Form, Json, Router};
use serde_json::{Value, json};
use sqlx::Row;

use crate::auth::{self, hash_password, hash_token, random_token, same};
use crate::db::{User, new_guid, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::{State, hub, organisations};

/// Settings told to an address with no account, so that prelogin does not reveal which
/// addresses have one.
const DEFAULT_ITERATIONS: i64 = 600_000;

/// Routes.
pub fn routes() -> Router<State> {
    Router::new()
        .route("/identity/accounts/prelogin", post(prelogin))
        .route("/identity/accounts/prelogin/password", post(prelogin))
        .route("/api/accounts/prelogin", post(prelogin))
        .route("/identity/accounts/register", post(register))
        .route("/api/accounts/register", post(register))
        .route("/identity/connect/token", post(token))
}

pub(crate) fn field<'a>(body: &'a Value, name: &str) -> Option<&'a str> {
    body.get(name).and_then(Value::as_str)
}

fn normal_email(email: &str) -> Option<String> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254 && email.contains('@') && !email.contains(char::is_whitespace);
    valid.then_some(email)
}

async fn prelogin(Extract(state): Extract<State>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let email = field(&body, "email").and_then(normal_email).ok_or(ApiError::Bad("An email address is required."))?;
    let user = state.db.user_by_email(&email).await?;
    let (kind, iterations, memory, parallelism) = match &user {
        Some(user) => (user.kdf, user.kdf_iterations, user.kdf_memory, user.kdf_parallelism),
        None => (0, DEFAULT_ITERATIONS, None, None),
    };
    // Older clients read the flat fields, and newer ones the nested settings and the salt.
    Ok(Json(json!({
        "kdf": kind,
        "kdfIterations": iterations,
        "kdfMemory": memory,
        "kdfParallelism": parallelism,
        "kdfSettings": {"kdfType": kind, "iterations": iterations, "memory": memory, "parallelism": parallelism},
        "salt": email,
    })))
}

fn kdf_acceptable(kind: i64, iterations: i64, memory: Option<i64>, parallelism: Option<i64>) -> bool {
    match kind {
        0 => (100_000..=2_000_000).contains(&iterations),
        1 => {
            (2..=64).contains(&iterations)
                && memory.is_some_and(|m| (16..=1024).contains(&m))
                && parallelism.is_some_and(|p| (1..=16).contains(&p))
        }
        _ => false,
    }
}

async fn register(Extract(state): Extract<State>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let email = field(&body, "email").and_then(normal_email).ok_or(ApiError::Bad("An email address is required."))?;
    if !state.config.signups.allows(&email) {
        return Err(ApiError::Bad("Registration is not open on this server."));
    }
    let login_hash = field(&body, "masterPasswordHash").ok_or(ApiError::Bad("A password hash is required."))?;
    let key = field(&body, "key").ok_or(ApiError::Bad("An account key is required."))?;
    let kdf = body.get("kdf").and_then(Value::as_i64).unwrap_or(0);
    let iterations = body.get("kdfIterations").and_then(Value::as_i64).unwrap_or(0);
    let memory = body.get("kdfMemory").and_then(Value::as_i64);
    let parallelism = body.get("kdfParallelism").and_then(Value::as_i64);
    if !kdf_acceptable(kdf, iterations, memory, parallelism) {
        return Err(ApiError::Bad("The password protection settings are outside the accepted range."));
    }
    let public_key = body.pointer("/keys/publicKey").and_then(Value::as_str);
    let private_key = body.pointer("/keys/encryptedPrivateKey").and_then(Value::as_str);
    let salt = random_token()?;
    let password_hash = hash_password(&state, login_hash, &salt).await?;
    let id = new_guid()?;
    let now = now_ms();
    let inserted = sqlx::query(
        "INSERT INTO users (id, email, name, password_hash, password_salt, kdf, kdf_iterations, kdf_memory, kdf_parallelism, user_key, public_key, private_key, security_stamp, created, revision)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(&id)
    .bind(&email)
    .bind(field(&body, "name").unwrap_or(""))
    .bind(&password_hash)
    .bind(&salt)
    .bind(kdf)
    .bind(iterations)
    .bind(memory)
    .bind(parallelism)
    .bind(key)
    .bind(public_key)
    .bind(private_key)
    .bind(random_token()?)
    .bind(now)
    .bind(now)
    .execute(&state.db.pool)
    .await;
    if inserted.is_err() {
        return Err(ApiError::Bad("Registration could not be completed."));
    }
    organisations::claim_invitations(&state, &id, &email).await?;
    Ok(Json(json!({"object": "register"})))
}

fn invalid_grant() -> Response {
    let body = json!({
        "error": "invalid_grant",
        "error_description": "invalid_username_or_password",
        "ErrorModel": {"Message": "Username or password is incorrect. Try again.", "Object": "error"},
    });
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

/// The account's key pair, in the form newer clients read.
pub fn account_keys(user: &User) -> Value {
    match (&user.public_key, &user.private_key) {
        (Some(public_key), Some(private_key)) => json!({
            "publicKeyEncryptionKeyPair": {"publicKey": public_key, "wrappedPrivateKey": private_key},
            "object": "privateKeys",
        }),
        _ => Value::Null,
    }
}

/// How to unlock with the password, in the form newer clients read.
pub fn master_password_unlock(user: &User) -> Value {
    json!({
        "kdf": {
            "kdfType": user.kdf,
            "iterations": user.kdf_iterations,
            "memory": user.kdf_memory,
            "parallelism": user.kdf_parallelism,
        },
        "masterKeyEncryptedUserKey": user.user_key,
        "masterKeyWrappedUserKey": user.user_key,
        "salt": user.email,
    })
}

fn token_answer(state: &State, user: &User, device_id: &str, refresh: &str) -> ApiResult<Json<Value>> {
    let access = auth::issue_access_token(&state.jwt_key, user, device_id)?;
    Ok(Json(json!({
        "access_token": access,
        "expires_in": auth::ACCESS_SECONDS,
        "token_type": "Bearer",
        "refresh_token": refresh,
        "scope": "api offline_access",
        "Key": user.user_key,
        "PrivateKey": user.private_key,
        "AccountKeys": account_keys(user),
        "Kdf": user.kdf,
        "KdfIterations": user.kdf_iterations,
        "KdfMemory": user.kdf_memory,
        "KdfParallelism": user.kdf_parallelism,
        "ForcePasswordReset": false,
        "ResetMasterPassword": false,
        "MasterPasswordPolicy": {"Object": "masterPasswordPolicy"},
        "UserDecryptionOptions": {
            "HasMasterPassword": true,
            "MasterPasswordUnlock": master_password_unlock(user),
            "Object": "userDecryptionOptions",
        },
    })))
}

async fn token(Extract(state): Extract<State>, Form(form): Form<HashMap<String, String>>) -> Result<Response, ApiError> {
    match form.get("grant_type").map(String::as_str) {
        Some("password") => password_grant(&state, &form).await,
        Some("refresh_token") => refresh_grant(&state, &form).await,
        _ => Err(ApiError::Bad("That grant type is not supported.")),
    }
}

async fn password_grant(state: &State, form: &HashMap<String, String>) -> Result<Response, ApiError> {
    let email = form.get("username").and_then(|e| normal_email(e)).ok_or(ApiError::Bad("An email address is required."))?;
    let login_hash = form.get("password").ok_or(ApiError::Bad("A password is required."))?;
    let identifier = form.get("deviceIdentifier").map_or("", String::as_str);
    if identifier.is_empty() {
        return Err(ApiError::Bad("A device identifier is required."));
    }
    if let Some(seconds) = state.throttle.wait(&email) {
        return Err(ApiError::TooMany(seconds));
    }
    let user = state.db.user_by_email(&email).await?;
    // An unknown address costs the same work as a known one, so timing does not reveal it.
    let (salt, expected) = match &user {
        Some(user) => (user.password_salt.clone(), user.password_hash.clone()),
        None => ("no-such-account-salt-padding".to_owned(), String::new()),
    };
    let actual = hash_password(state, login_hash, &salt).await?;
    let user = match user {
        Some(user) if same(&actual, &expected) => user,
        _ => {
            state.throttle.failed(&email);
            return Ok(invalid_grant());
        }
    };
    state.throttle.succeeded(&email);

    let refresh = random_token()?;
    let refresh_hash = hash_token(&refresh);
    let now = now_ms();
    let name = form.get("deviceName").map_or("Unknown", String::as_str);
    let kind = form.get("deviceType").and_then(|k| k.parse::<i64>().ok()).unwrap_or(14);
    let existing = sqlx::query("SELECT id FROM devices WHERE user_id = $1 AND identifier = $2")
        .bind(&user.id)
        .bind(identifier)
        .fetch_optional(&state.db.pool)
        .await?;
    let device_id = match existing {
        Some(row) => {
            let id: String = row.try_get("id")?;
            sqlx::query("UPDATE devices SET name = $1, kind = $2, refresh_hash = $3, last_seen = $4 WHERE id = $5")
                .bind(name)
                .bind(kind)
                .bind(&refresh_hash)
                .bind(now)
                .bind(&id)
                .execute(&state.db.pool)
                .await?;
            id
        }
        None => {
            let id = new_guid()?;
            sqlx::query("INSERT INTO devices (id, user_id, identifier, name, kind, refresh_hash, created, last_seen) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
                .bind(&id)
                .bind(&user.id)
                .bind(identifier)
                .bind(name)
                .bind(kind)
                .bind(&refresh_hash)
                .bind(now)
                .bind(now)
                .execute(&state.db.pool)
                .await?;
            id
        }
    };
    Ok(token_answer(state, &user, &device_id, &refresh)?.into_response())
}

async fn refresh_grant(state: &State, form: &HashMap<String, String>) -> Result<Response, ApiError> {
    let token = form.get("refresh_token").ok_or(ApiError::Bad("A refresh token is required."))?;
    let row = sqlx::query("SELECT id, user_id FROM devices WHERE refresh_hash = $1")
        .bind(hash_token(token))
        .fetch_optional(&state.db.pool)
        .await?;
    let Some(row) = row else { return Ok(invalid_grant()) };
    let device_id: String = row.try_get("id")?;
    let user_id: String = row.try_get("user_id")?;
    let user = state.db.user_by_id(&user_id).await?.ok_or(ApiError::Unauthorised)?;
    sqlx::query("UPDATE devices SET last_seen = $1 WHERE id = $2").bind(now_ms()).bind(&device_id).execute(&state.db.pool).await?;
    Ok(token_answer(state, &user, &device_id, token)?.into_response())
}

/// Ends every session of a user except one device's, and tells live clients.
pub async fn revoke_other_devices(state: &State, user_id: &str, keep_device: &str) -> ApiResult<()> {
    sqlx::query("DELETE FROM devices WHERE user_id = $1 AND id <> $2").bind(user_id).bind(keep_device).execute(&state.db.pool).await?;
    state.hub.push(user_id, hub::UpdateKind::LogOut, Some(keep_device));
    Ok(())
}
