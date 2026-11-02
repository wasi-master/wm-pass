//! Folders, each one private to its user.

use axum::extract::{Path, State as Extract};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::Row;

use crate::auth::Auth;
use crate::db::{date, new_guid, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::hub::UpdateKind;
use crate::identity::field;
use crate::State;

/// Routes.
pub fn routes() -> Router<State> {
    Router::new()
        .route("/api/folders", get(list).post(create))
        .route("/api/folders/{id}", get(read).put(update).post(update).delete(remove))
        .route("/api/folders/{id}/delete", axum::routing::post(remove))
}

fn folder_json(row: &sqlx::any::AnyRow) -> Result<Value, sqlx::Error> {
    let id: String = row.try_get("id")?;
    let name: String = row.try_get("name")?;
    let revision: i64 = row.try_get("revision")?;
    Ok(json!({"id": id, "name": name, "revisionDate": date(revision), "object": "folder"}))
}

/// Every folder a user has.
pub async fn list_json(state: &State, user_id: &str) -> ApiResult<Vec<Value>> {
    let rows = sqlx::query("SELECT id, name, revision FROM folders WHERE user_id = $1").bind(user_id).fetch_all(&state.db.pool).await?;
    rows.iter().map(|row| folder_json(row).map_err(ApiError::from)).collect()
}

async fn list(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    Ok(Json(json!({"data": list_json(&state, &auth.user.id).await?, "continuationToken": null, "object": "list"})))
}

async fn create(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let name = field(&body, "name").ok_or(ApiError::Bad("A folder name is required."))?;
    let (id, now) = (new_guid()?, now_ms());
    sqlx::query("INSERT INTO folders (id, user_id, name, revision) VALUES ($1, $2, $3, $4)")
        .bind(&id)
        .bind(&auth.user.id)
        .bind(name)
        .bind(now)
        .execute(&state.db.pool)
        .await?;
    state.db.touch_user(&auth.user.id, now).await?;
    state.hub.push(&auth.user.id, UpdateKind::SyncVault, Some(&auth.device_id));
    Ok(Json(json!({"id": id, "name": name, "revisionDate": date(now), "object": "folder"})))
}

async fn read(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let row = sqlx::query("SELECT id, name, revision FROM folders WHERE id = $1 AND user_id = $2")
        .bind(&id)
        .bind(&auth.user.id)
        .fetch_optional(&state.db.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(folder_json(&row)?))
}

async fn update(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let name = field(&body, "name").ok_or(ApiError::Bad("A folder name is required."))?;
    let now = now_ms();
    let done = sqlx::query("UPDATE folders SET name = $1, revision = $2 WHERE id = $3 AND user_id = $4")
        .bind(name)
        .bind(now)
        .bind(&id)
        .bind(&auth.user.id)
        .execute(&state.db.pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    state.db.touch_user(&auth.user.id, now).await?;
    state.hub.push(&auth.user.id, UpdateKind::SyncVault, Some(&auth.device_id));
    Ok(Json(json!({"id": id, "name": name, "revisionDate": date(now), "object": "folder"})))
}

async fn remove(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let done = sqlx::query("DELETE FROM folders WHERE id = $1 AND user_id = $2").bind(&id).bind(&auth.user.id).execute(&state.db.pool).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query("UPDATE cipher_user SET folder_id = NULL WHERE folder_id = $1 AND user_id = $2").bind(&id).bind(&auth.user.id).execute(&state.db.pool).await?;
    state.db.touch_user(&auth.user.id, now_ms()).await?;
    state.hub.push(&auth.user.id, UpdateKind::SyncVault, Some(&auth.device_id));
    Ok(Json(json!({})))
}
