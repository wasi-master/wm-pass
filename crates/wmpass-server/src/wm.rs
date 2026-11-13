//! Routes only WM Pass uses: the device list, and a registry of shared vault files. Shared
//! files are opaque here. Members' keys are wrapped inside them, so all the server can do
//! is hold the bytes and decide who may fetch them.

use axum::extract::{Path, State as Extract};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::Row;

use crate::auth::Auth;
use crate::db::{date, new_guid, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::identity::field;
use crate::{Signups, State};

/// Routes.
pub fn routes() -> Router<State> {
    Router::new()
        .route("/wm/status", get(status))
        .route("/wm/devices", get(devices))
        .route("/wm/devices/{id}", delete(revoke_device))
        .route("/wm/shared", get(shared_list).post(shared_create))
        .route("/wm/shared/{id}", get(shared_read).put(shared_write).delete(shared_delete))
        .route("/wm/shared/{id}/members", post(shared_add_member))
        .route("/wm/shared/{id}/members/{user}", delete(shared_remove_member))
}

async fn status(Extract(state): Extract<State>) -> Json<Value> {
    Json(json!({
        "name": "WM Pass server",
        "version": env!("CARGO_PKG_VERSION"),
        "signups": state.config.signups != Signups::Closed,
    }))
}

async fn devices(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT id, name, kind, created, last_seen FROM devices WHERE user_id = $1").bind(&auth.user.id).fetch_all(&state.db.pool).await?;
    let mut data = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let name: String = row.try_get("name")?;
        let kind: i64 = row.try_get("kind")?;
        let created: i64 = row.try_get("created")?;
        let last_seen: i64 = row.try_get("last_seen")?;
        data.push(json!({
            "id": id,
            "name": name,
            "type": kind,
            "current": id == auth.device_id,
            "creationDate": date(created),
            "lastSeen": date(last_seen),
        }));
    }
    Ok(Json(json!({"data": data})))
}

async fn revoke_device(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let done = sqlx::query("DELETE FROM devices WHERE id = $1 AND user_id = $2").bind(&id).bind(&auth.user.id).execute(&state.db.pool).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    state.hub.close_device(&auth.user.id, &id);
    Ok(Json(json!({})))
}

async fn can_reach(state: &State, user: &str, file: &str) -> ApiResult<bool> {
    let row = sqlx::query("SELECT file_id FROM shared_members WHERE file_id = $1 AND user_id = $2").bind(file).bind(user).fetch_optional(&state.db.pool).await?;
    Ok(row.is_some())
}

async fn owner_of(state: &State, file: &str) -> ApiResult<String> {
    let row = sqlx::query("SELECT owner_id FROM shared_files WHERE id = $1").bind(file).fetch_optional(&state.db.pool).await?.ok_or(ApiError::NotFound)?;
    Ok(row.try_get("owner_id")?)
}

async fn shared_list(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    let rows = sqlx::query("SELECT f.id, f.owner_id, f.revision FROM shared_files f JOIN shared_members m ON m.file_id = f.id WHERE m.user_id = $1")
        .bind(&auth.user.id)
        .fetch_all(&state.db.pool)
        .await?;
    let mut data = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let owner: String = row.try_get("owner_id")?;
        let revision: i64 = row.try_get("revision")?;
        data.push(json!({"id": id, "owner": owner == auth.user.id, "revision": revision}));
    }
    Ok(Json(json!({"data": data})))
}

async fn shared_create(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let content = field(&body, "body").ok_or(ApiError::Bad("A file is required."))?;
    let (id, now) = (new_guid()?, now_ms());
    sqlx::query("INSERT INTO shared_files (id, owner_id, body, revision) VALUES ($1, $2, $3, $4)").bind(&id).bind(&auth.user.id).bind(content).bind(now).execute(&state.db.pool).await?;
    sqlx::query("INSERT INTO shared_members (file_id, user_id) VALUES ($1, $2)").bind(&id).bind(&auth.user.id).execute(&state.db.pool).await?;
    Ok(Json(json!({"id": id, "revision": now})))
}

async fn shared_read(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if !can_reach(&state, &auth.user.id, &id).await? {
        return Err(ApiError::NotFound);
    }
    let row = sqlx::query("SELECT body, revision FROM shared_files WHERE id = $1").bind(&id).fetch_optional(&state.db.pool).await?.ok_or(ApiError::NotFound)?;
    let content: String = row.try_get("body")?;
    let revision: i64 = row.try_get("revision")?;
    Ok(Json(json!({"id": id, "body": content, "revision": revision})))
}

/// Refuses a write based on an old copy. The client fetches, merges and tries again, which
/// is safe to repeat because vault merges are commutative.
async fn shared_write(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    if !can_reach(&state, &auth.user.id, &id).await? {
        return Err(ApiError::NotFound);
    }
    let content = field(&body, "body").ok_or(ApiError::Bad("A file is required."))?;
    let expected = body.get("revision").and_then(Value::as_i64).ok_or(ApiError::Bad("The revision it was based on is required."))?;
    // The revision comes from the client, so the step past it must not overflow.
    let now = now_ms().max(expected.saturating_add(1));
    let done = sqlx::query("UPDATE shared_files SET body = $1, revision = $2 WHERE id = $3 AND revision = $4").bind(content).bind(now).bind(&id).bind(expected).execute(&state.db.pool).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::Bad("This file changed. Fetch it and try again."));
    }
    Ok(Json(json!({"id": id, "revision": now})))
}

async fn shared_delete(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if owner_of(&state, &id).await? != auth.user.id {
        return Err(ApiError::NotFound);
    }
    sqlx::query("DELETE FROM shared_members WHERE file_id = $1").bind(&id).execute(&state.db.pool).await?;
    sqlx::query("DELETE FROM shared_files WHERE id = $1").bind(&id).execute(&state.db.pool).await?;
    Ok(Json(json!({})))
}

async fn shared_add_member(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    if owner_of(&state, &id).await? != auth.user.id {
        return Err(ApiError::NotFound);
    }
    let email = field(&body, "email").ok_or(ApiError::Bad("An address is required."))?.trim().to_lowercase();
    let user = state.db.user_by_email(&email).await?.ok_or(ApiError::Bad("That address has no account here."))?;
    // One statement: two adds of the same person at once would otherwise both find them
    // missing, and the second insert fail on the key.
    sqlx::query("INSERT INTO shared_members (file_id, user_id) VALUES ($1, $2) ON CONFLICT (file_id, user_id) DO NOTHING")
        .bind(&id)
        .bind(&user.id)
        .execute(&state.db.pool)
        .await?;
    Ok(Json(json!({"userId": user.id, "publicKey": user.public_key})))
}

async fn shared_remove_member(Extract(state): Extract<State>, auth: Auth, Path((id, user)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let owner = owner_of(&state, &id).await?;
    if owner != auth.user.id && user != auth.user.id {
        return Err(ApiError::NotFound);
    }
    if user == owner {
        return Err(ApiError::Bad("The owner cannot leave. Delete the file instead."));
    }
    sqlx::query("DELETE FROM shared_members WHERE file_id = $1 AND user_id = $2").bind(&id).bind(&user).execute(&state.db.pool).await?;
    Ok(Json(json!({})))
}
