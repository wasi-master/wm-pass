//! Items. The server stores what clients encrypted and never looks inside. Every route goes
//! through [`access`], so an item a user cannot reach answers exactly like one that does
//! not exist.

use axum::extract::{Path, State as Extract};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::{Map, Value, json};
use sqlx::Row;

use crate::auth::Auth;
use crate::db::{date, new_guid, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::hub::UpdateKind;
use crate::identity::field;
use crate::{State, organisations};

/// Fields kept from a client's copy. Anything the server itself owns is set by the server,
/// so a client cannot claim another user's id or another organisation by sending one.
const STORED_FIELDS: &[&str] = &[
    "type", "name", "notes", "login", "card", "identity", "secureNote", "sshKey", "fields", "passwordHistory", "reprompt", "key", "data",
    "archivedDate",
];

/// Routes.
pub fn routes() -> Router<State> {
    Router::new()
        .route("/api/ciphers", get(list).post(create))
        .route("/api/ciphers/create", post(create_with_collections))
        .route("/api/ciphers/import", post(import))
        .route("/api/ciphers/purge", post(purge))
        .route("/api/ciphers/delete", put(bulk_trash).post(bulk_remove))
        .route("/api/ciphers/restore", put(bulk_restore))
        .route("/api/ciphers/move", put(bulk_move).post(bulk_move))
        .route("/api/ciphers/{id}", get(read).put(update).post(update).delete(remove))
        .route("/api/ciphers/{id}/details", get(read))
        .route("/api/ciphers/{id}/partial", put(partial).post(partial))
        .route("/api/ciphers/{id}/delete", put(trash).post(remove))
        .route("/api/ciphers/{id}/restore", put(restore))
        .route("/api/ciphers/{id}/share", put(share).post(share))
        .route("/api/ciphers/{id}/collections", put(set_collections).post(set_collections))
}

/// A stored item as this user is allowed to see it.
#[derive(Debug)]
pub struct Stored {
    id: String,
    user_id: Option<String>,
    org_id: Option<String>,
    data: String,
    created: i64,
    revision: i64,
    deleted: Option<i64>,
}

fn stored_from(row: &sqlx::any::AnyRow) -> Result<Stored, sqlx::Error> {
    Ok(Stored {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        org_id: row.try_get("org_id")?,
        data: row.try_get("data")?,
        created: row.try_get("created")?,
        revision: row.try_get("revision")?,
        deleted: row.try_get("deleted")?,
    })
}

/// Fetches an item only if this user may reach it.
async fn access(state: &State, user_id: &str, id: &str) -> ApiResult<Stored> {
    let row = sqlx::query("SELECT id, user_id, org_id, data, created, revision, deleted FROM ciphers WHERE id = $1").bind(id).fetch_optional(&state.db.pool).await?.ok_or(ApiError::NotFound)?;
    let stored = stored_from(&row)?;
    let allowed = match (&stored.user_id, &stored.org_id) {
        (Some(owner), None) => owner == user_id,
        (_, Some(org)) => organisations::can_reach_cipher(state, user_id, org, &stored.id).await?,
        _ => false,
    };
    if allowed { Ok(stored) } else { Err(ApiError::NotFound) }
}

async fn render(state: &State, user_id: &str, stored: &Stored) -> ApiResult<Value> {
    let mut object: Map<String, Value> = serde_json::from_str(&stored.data).unwrap_or_default();
    let row = sqlx::query("SELECT folder_id, favorite FROM cipher_user WHERE cipher_id = $1 AND user_id = $2")
        .bind(&stored.id)
        .bind(user_id)
        .fetch_optional(&state.db.pool)
        .await?;
    let (folder, favorite) = match row {
        Some(row) => (row.try_get::<Option<String>, _>("folder_id")?, row.try_get::<i64, _>("favorite")? != 0),
        None => (None, false),
    };
    let collections = sqlx::query("SELECT collection_id FROM collection_ciphers WHERE cipher_id = $1").bind(&stored.id).fetch_all(&state.db.pool).await?;
    let collection_ids: Vec<String> = collections.iter().filter_map(|row| row.try_get("collection_id").ok()).collect();
    object.insert("id".into(), json!(stored.id));
    object.insert("organizationId".into(), json!(stored.org_id));
    object.insert("folderId".into(), json!(folder));
    object.insert("favorite".into(), json!(favorite));
    object.insert("edit".into(), json!(true));
    object.insert("viewPassword".into(), json!(true));
    object.insert("organizationUseTotp".into(), json!(true));
    object.insert("attachments".into(), Value::Null);
    object.insert("collectionIds".into(), json!(collection_ids));
    object.insert("creationDate".into(), json!(date(stored.created)));
    object.insert("revisionDate".into(), json!(date(stored.revision)));
    object.insert("deletedDate".into(), json!(stored.deleted.map(date)));
    object.insert("permissions".into(), json!({"delete": true, "restore": true}));
    object.insert("object".into(), json!("cipherDetails"));
    object.entry("reprompt").or_insert(json!(0));
    Ok(Value::Object(object))
}

/// What a user can see: their own items, and those of organisations they may reach.
pub async fn visible_ciphers(state: &State, user_id: &str) -> ApiResult<Vec<Value>> {
    let mut stored: Vec<Stored> = Vec::new();
    for row in sqlx::query("SELECT id, user_id, org_id, data, created, revision, deleted FROM ciphers WHERE user_id = $1").bind(user_id).fetch_all(&state.db.pool).await? {
        stored.push(stored_from(&row)?);
    }
    for org in organisations::confirmed_org_ids(state, user_id).await? {
        for row in sqlx::query("SELECT id, user_id, org_id, data, created, revision, deleted FROM ciphers WHERE org_id = $1").bind(&org).fetch_all(&state.db.pool).await? {
            let item = stored_from(&row)?;
            if organisations::can_reach_cipher(state, user_id, &org, &item.id).await? {
                stored.push(item);
            }
        }
    }
    let mut out = Vec::with_capacity(stored.len());
    for item in &stored {
        out.push(render(state, user_id, item).await?);
    }
    Ok(out)
}

fn clean(body: &Value) -> ApiResult<String> {
    let source = body.as_object().ok_or(ApiError::Bad("An item is required."))?;
    if source.get("type").and_then(Value::as_i64).is_none() {
        return Err(ApiError::Bad("An item needs a type."));
    }
    if source.get("name").and_then(Value::as_str).is_none() {
        return Err(ApiError::Bad("An item needs a name."));
    }
    let mut kept = Map::new();
    for name in STORED_FIELDS {
        if let Some(value) = source.get(*name) {
            kept.insert((*name).to_owned(), value.clone());
        }
    }
    Ok(Value::Object(kept).to_string())
}

/// Clients expect a stale edit to be refused, so they can merge and retry. Revisions are
/// stored as exact milliseconds and clients echo them back, so any difference is a real
/// change, and a tolerance here would let an edit silently overwrite a newer one.
fn stale(body: &Value, stored: &Stored) -> bool {
    let known = field(body, "lastKnownRevisionDate").and_then(wmpass_core::clock::parse_date);
    known.is_some_and(|known| i64::try_from(known).unwrap_or(0) < stored.revision)
}

async fn set_user_state(state: &State, user_id: &str, cipher_id: &str, body: &Value) -> ApiResult<()> {
    let folder = field(body, "folderId");
    // A folder is private to its user, so one that is not theirs is dropped.
    let folder = match folder {
        Some(id) => {
            let owned = sqlx::query("SELECT id FROM folders WHERE id = $1 AND user_id = $2").bind(id).bind(user_id).fetch_optional(&state.db.pool).await?;
            owned.map(|_| id)
        }
        None => None,
    };
    let favorite = i64::from(body.get("favorite").and_then(Value::as_bool).unwrap_or(false));
    sqlx::query("DELETE FROM cipher_user WHERE cipher_id = $1 AND user_id = $2").bind(cipher_id).bind(user_id).execute(&state.db.pool).await?;
    sqlx::query("INSERT INTO cipher_user (cipher_id, user_id, folder_id, favorite) VALUES ($1, $2, $3, $4)")
        .bind(cipher_id)
        .bind(user_id)
        .bind(folder)
        .bind(favorite)
        .execute(&state.db.pool)
        .await?;
    Ok(())
}

async fn insert(state: &State, auth: &Auth, body: &Value, collections: &[String]) -> ApiResult<Value> {
    let data = clean(body)?;
    let (id, now) = (new_guid()?, now_ms());
    let org = field(body, "organizationId").map(str::to_owned);
    if let Some(org) = &org {
        organisations::require_member(state, &auth.user.id, org).await?;
        if collections.is_empty() {
            return Err(ApiError::Bad("An organisation item needs a collection."));
        }
        organisations::require_collections(state, &auth.user.id, org, collections).await?;
    }
    let owner = if org.is_some() { None } else { Some(auth.user.id.as_str()) };
    sqlx::query("INSERT INTO ciphers (id, user_id, org_id, data, created, revision, deleted) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(&id)
        .bind(owner)
        .bind(org.as_deref())
        .bind(&data)
        .bind(now)
        .bind(now)
        .bind(None::<i64>)
        .execute(&state.db.pool)
        .await?;
    for collection in collections {
        sqlx::query("INSERT INTO collection_ciphers (collection_id, cipher_id) VALUES ($1, $2)").bind(collection).bind(&id).execute(&state.db.pool).await?;
    }
    set_user_state(state, &auth.user.id, &id, body).await?;
    changed(state, auth, org.as_deref(), now).await?;
    let stored = access(state, &auth.user.id, &id).await?;
    render(state, &auth.user.id, &stored).await
}

/// Records a change and tells this user's other devices, and an organisation's members.
async fn changed(state: &State, auth: &Auth, org: Option<&str>, now: i64) -> ApiResult<()> {
    state.db.touch_user(&auth.user.id, now).await?;
    state.hub.push(&auth.user.id, UpdateKind::SyncVault, Some(&auth.device_id));
    if let Some(org) = org {
        for member in organisations::confirmed_user_ids(state, org).await? {
            if member != auth.user.id {
                state.db.touch_user(&member, now).await?;
                state.hub.push(&member, UpdateKind::SyncVault, None);
            }
        }
    }
    Ok(())
}

async fn list(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    let data = visible_ciphers(&state, &auth.user.id).await?;
    Ok(Json(json!({"data": data, "continuationToken": null, "object": "list"})))
}

async fn create(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    if field(&body, "organizationId").is_some() {
        return Err(ApiError::Bad("Use the create route with collections for an organisation item."));
    }
    Ok(Json(insert(&state, &auth, &body, &[]).await?))
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect()
}

async fn create_with_collections(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let cipher = body.get("cipher").ok_or(ApiError::Bad("An item is required."))?;
    Ok(Json(insert(&state, &auth, cipher, &strings(body.get("collectionIds"))).await?))
}

async fn read(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let stored = access(&state, &auth.user.id, &id).await?;
    Ok(Json(render(&state, &auth.user.id, &stored).await?))
}

async fn update(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let stored = access(&state, &auth.user.id, &id).await?;
    if stale(&body, &stored) {
        return Err(ApiError::Bad("The client copy of this cipher is out of date. Resync the client and try again."));
    }
    let now = now_ms();
    sqlx::query("UPDATE ciphers SET data = $1, revision = $2 WHERE id = $3").bind(clean(&body)?).bind(now).bind(&id).execute(&state.db.pool).await?;
    set_user_state(&state, &auth.user.id, &id, &body).await?;
    changed(&state, &auth, stored.org_id.as_deref(), now).await?;
    let stored = access(&state, &auth.user.id, &id).await?;
    Ok(Json(render(&state, &auth.user.id, &stored).await?))
}

async fn partial(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let stored = access(&state, &auth.user.id, &id).await?;
    set_user_state(&state, &auth.user.id, &id, &body).await?;
    state.hub.push(&auth.user.id, UpdateKind::SyncVault, Some(&auth.device_id));
    Ok(Json(render(&state, &auth.user.id, &stored).await?))
}

async fn set_deleted(state: &State, auth: &Auth, id: &str, deleted: Option<i64>) -> ApiResult<Stored> {
    let stored = access(state, &auth.user.id, id).await?;
    let now = now_ms();
    sqlx::query("UPDATE ciphers SET deleted = $1, revision = $2 WHERE id = $3").bind(deleted).bind(now).bind(id).execute(&state.db.pool).await?;
    changed(state, auth, stored.org_id.as_deref(), now).await?;
    access(state, &auth.user.id, id).await
}

async fn trash(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    set_deleted(&state, &auth, &id, Some(now_ms())).await?;
    Ok(Json(json!({})))
}

async fn restore(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let stored = set_deleted(&state, &auth, &id, None).await?;
    Ok(Json(render(&state, &auth.user.id, &stored).await?))
}

async fn erase(state: &State, id: &str) -> ApiResult<()> {
    sqlx::query("DELETE FROM collection_ciphers WHERE cipher_id = $1").bind(id).execute(&state.db.pool).await?;
    sqlx::query("DELETE FROM cipher_user WHERE cipher_id = $1").bind(id).execute(&state.db.pool).await?;
    sqlx::query("DELETE FROM ciphers WHERE id = $1").bind(id).execute(&state.db.pool).await?;
    Ok(())
}

async fn remove(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let stored = access(&state, &auth.user.id, &id).await?;
    erase(&state, &id).await?;
    changed(&state, &auth, stored.org_id.as_deref(), now_ms()).await?;
    Ok(Json(json!({})))
}

async fn bulk_trash(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    for id in strings(body.get("ids")) {
        set_deleted(&state, &auth, &id, Some(now_ms())).await?;
    }
    Ok(Json(json!({})))
}

async fn bulk_remove(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    for id in strings(body.get("ids")) {
        let stored = access(&state, &auth.user.id, &id).await?;
        erase(&state, &id).await?;
        changed(&state, &auth, stored.org_id.as_deref(), now_ms()).await?;
    }
    Ok(Json(json!({})))
}

async fn bulk_restore(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let mut data = Vec::new();
    for id in strings(body.get("ids")) {
        let stored = set_deleted(&state, &auth, &id, None).await?;
        data.push(render(&state, &auth.user.id, &stored).await?);
    }
    Ok(Json(json!({"data": data, "continuationToken": null, "object": "list"})))
}

async fn bulk_move(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    for id in strings(body.get("ids")) {
        access(&state, &auth.user.id, &id).await?;
        let existing = sqlx::query("SELECT favorite FROM cipher_user WHERE cipher_id = $1 AND user_id = $2").bind(&id).bind(&auth.user.id).fetch_optional(&state.db.pool).await?;
        let favorite = existing.and_then(|row| row.try_get::<i64, _>("favorite").ok()).unwrap_or(0) != 0;
        let change = json!({"folderId": body.get("folderId"), "favorite": favorite});
        set_user_state(&state, &auth.user.id, &id, &change).await?;
    }
    state.hub.push(&auth.user.id, UpdateKind::SyncVault, Some(&auth.device_id));
    Ok(Json(json!({})))
}

/// Deleting everything needs the password again, in case a session was left open.
async fn purge(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let given = field(&body, "masterPasswordHash").ok_or(ApiError::Bad("The password is required."))?;
    let actual = crate::auth::hash_password(&state, given, &auth.user.password_salt).await?;
    if !crate::auth::same(&actual, &auth.user.password_hash) {
        return Err(ApiError::Bad("The password is not correct."));
    }
    let rows = sqlx::query("SELECT id FROM ciphers WHERE user_id = $1").bind(&auth.user.id).fetch_all(&state.db.pool).await?;
    for row in rows {
        let id: String = row.try_get("id")?;
        erase(&state, &id).await?;
    }
    sqlx::query("DELETE FROM folders WHERE user_id = $1").bind(&auth.user.id).execute(&state.db.pool).await?;
    changed(&state, &auth, None, now_ms()).await?;
    Ok(Json(json!({})))
}

async fn import(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let mut folder_ids = Vec::new();
    for folder in body.get("folders").and_then(Value::as_array).into_iter().flatten() {
        let name = field(folder, "name").ok_or(ApiError::Bad("A folder name is required."))?;
        let (id, now) = (new_guid()?, now_ms());
        sqlx::query("INSERT INTO folders (id, user_id, name, revision) VALUES ($1, $2, $3, $4)").bind(&id).bind(&auth.user.id).bind(name).bind(now).execute(&state.db.pool).await?;
        folder_ids.push(id);
    }
    let mut relations = std::collections::HashMap::new();
    for relation in body.get("folderRelationships").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(key), Some(value)) = (relation.get("key").and_then(Value::as_u64), relation.get("value").and_then(Value::as_u64)) {
            relations.insert(key, value);
        }
    }
    for (index, cipher) in body.get("ciphers").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let mut cipher = cipher.clone();
        if let Some(object) = cipher.as_object_mut() {
            object.remove("organizationId");
            object.remove("folderId");
            let folder = relations.get(&(index as u64)).and_then(|i| folder_ids.get(usize::try_from(*i).ok()?));
            if let Some(folder) = folder {
                object.insert("folderId".into(), json!(folder));
            }
        }
        insert(&state, &auth, &cipher, &[]).await?;
    }
    Ok(Json(json!({})))
}

/// Moves a personal item into an organisation, encrypted by the client under its key.
async fn share(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let stored = access(&state, &auth.user.id, &id).await?;
    let cipher = body.get("cipher").ok_or(ApiError::Bad("An item is required."))?;
    let org = field(cipher, "organizationId").ok_or(ApiError::Bad("An organisation is required."))?;
    let collections = strings(body.get("collectionIds"));
    if collections.is_empty() {
        return Err(ApiError::Bad("At least one collection is required."));
    }
    organisations::require_member(&state, &auth.user.id, org).await?;
    organisations::require_collections(&state, &auth.user.id, org, &collections).await?;
    if stored.org_id.as_deref().is_some_and(|current| current != org) {
        return Err(ApiError::Bad("This item already belongs to an organisation."));
    }
    let now = now_ms();
    sqlx::query("UPDATE ciphers SET data = $1, user_id = NULL, org_id = $2, revision = $3 WHERE id = $4")
        .bind(clean(cipher)?)
        .bind(org)
        .bind(now)
        .bind(&id)
        .execute(&state.db.pool)
        .await?;
    sqlx::query("DELETE FROM collection_ciphers WHERE cipher_id = $1").bind(&id).execute(&state.db.pool).await?;
    for collection in &collections {
        sqlx::query("INSERT INTO collection_ciphers (collection_id, cipher_id) VALUES ($1, $2)").bind(collection).bind(&id).execute(&state.db.pool).await?;
    }
    changed(&state, &auth, Some(org), now).await?;
    let stored = access(&state, &auth.user.id, &id).await?;
    Ok(Json(render(&state, &auth.user.id, &stored).await?))
}

async fn set_collections(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let stored = access(&state, &auth.user.id, &id).await?;
    let org = stored.org_id.clone().ok_or(ApiError::Bad("Only organisation items have collections."))?;
    let collections = strings(body.get("collectionIds"));
    organisations::require_collections(&state, &auth.user.id, &org, &collections).await?;
    sqlx::query("DELETE FROM collection_ciphers WHERE cipher_id = $1").bind(&id).execute(&state.db.pool).await?;
    for collection in &collections {
        sqlx::query("INSERT INTO collection_ciphers (collection_id, cipher_id) VALUES ($1, $2)").bind(collection).bind(&id).execute(&state.db.pool).await?;
    }
    changed(&state, &auth, Some(&org), now_ms()).await?;
    let stored = access(&state, &auth.user.id, &id).await?;
    Ok(Json(render(&state, &auth.user.id, &stored).await?))
}
