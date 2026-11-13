//! Organisations and collections.
//!
//! Member kinds follow Bitwarden's numbers: 0 owner, 1 admin, 2 user, 3 manager. Status: 0
//! invited with no account yet, 1 accepted, 2 confirmed. Only a confirmed member has the
//! organisation key, which an owner or admin wraps to that member's public key on their own
//! device. The server never sees it unwrapped.
//!
//! There is no email here, so an invitation to an existing account is accepted at once, and
//! one to an unknown address waits until that address registers. Confirmation by an owner
//! is the real gate, since it is the step that hands over the key.

use axum::extract::{Path, State as Extract};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::Row;

use crate::auth::Auth;
use crate::db::{User, new_guid, now_ms};
use crate::error::{ApiError, ApiResult};
use crate::hub::UpdateKind;
use crate::identity::field;
use crate::State;

const OWNER: i64 = 0;
const ADMIN: i64 = 1;
const USER: i64 = 2;
const INVITED: i64 = 0;
const ACCEPTED: i64 = 1;
const CONFIRMED: i64 = 2;

/// Routes.
pub fn routes() -> Router<State> {
    Router::new()
        .route("/api/organizations", post(create))
        .route("/api/organizations/{id}", get(read))
        .route("/api/organizations/{id}/keys", get(keys))
        .route("/api/organizations/{id}/collections", get(collections).post(create_collection))
        .route("/api/organizations/{id}/collections/{collection}", get(read_collection).delete(delete_collection))
        .route("/api/organizations/{id}/users", get(members))
        .route("/api/organizations/{id}/users/invite", post(invite))
        .route("/api/organizations/{id}/users/{member}/confirm", post(confirm))
        .route("/api/organizations/{id}/users/{member}", axum::routing::delete(remove_member))
        .route("/api/collections", get(my_collections))
}

/// A member's standing in an organisation.
struct Standing {
    id: String,
    status: i64,
    kind: i64,
    access_all: bool,
}

async fn standing(state: &State, user_id: &str, org: &str) -> ApiResult<Option<Standing>> {
    let row = sqlx::query("SELECT id, status, kind, access_all FROM org_users WHERE org_id = $1 AND user_id = $2")
        .bind(org)
        .bind(user_id)
        .fetch_optional(&state.db.pool)
        .await?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(Standing {
        id: row.try_get("id")?,
        status: row.try_get("status")?,
        kind: row.try_get("kind")?,
        access_all: row.try_get::<i64, _>("access_all")? != 0,
    }))
}

/// A confirmed member. Anyone else gets the same answer as for an organisation that does
/// not exist.
pub async fn require_member(state: &State, user_id: &str, org: &str) -> ApiResult<()> {
    match standing(state, user_id, org).await? {
        Some(member) if member.status == CONFIRMED => Ok(()),
        _ => Err(ApiError::NotFound),
    }
}

async fn require_admin(state: &State, user_id: &str, org: &str) -> ApiResult<Standing> {
    match standing(state, user_id, org).await? {
        Some(member) if member.status == CONFIRMED && member.kind <= ADMIN => Ok(member),
        _ => Err(ApiError::NotFound),
    }
}

/// Whether a user may reach an organisation's item.
pub async fn can_reach_cipher(state: &State, user_id: &str, org: &str, cipher: &str) -> ApiResult<bool> {
    let Some(member) = standing(state, user_id, org).await? else { return Ok(false) };
    if member.status != CONFIRMED {
        return Ok(false);
    }
    if member.access_all || member.kind <= ADMIN {
        return Ok(true);
    }
    let row = sqlx::query(
        "SELECT cc.cipher_id FROM collection_ciphers cc
         JOIN collection_users cu ON cu.collection_id = cc.collection_id
         WHERE cc.cipher_id = $1 AND cu.org_user_id = $2",
    )
    .bind(cipher)
    .bind(&member.id)
    .fetch_optional(&state.db.pool)
    .await?;
    Ok(row.is_some())
}

/// Checks that collections belong to the organisation and that this user may use them.
pub async fn require_collections(state: &State, user_id: &str, org: &str, ids: &[String]) -> ApiResult<()> {
    let member = standing(state, user_id, org).await?.ok_or(ApiError::NotFound)?;
    for id in ids {
        let row = sqlx::query("SELECT id FROM collections WHERE id = $1 AND org_id = $2").bind(id).bind(org).fetch_optional(&state.db.pool).await?;
        if row.is_none() {
            return Err(ApiError::Bad("That collection is not available."));
        }
        if !member.access_all && member.kind > ADMIN {
            let assigned = sqlx::query("SELECT collection_id FROM collection_users WHERE collection_id = $1 AND org_user_id = $2")
                .bind(id)
                .bind(&member.id)
                .fetch_optional(&state.db.pool)
                .await?;
            if assigned.is_none() {
                return Err(ApiError::Bad("That collection is not available."));
            }
        }
    }
    Ok(())
}

/// Organisations a user is a confirmed member of.
pub async fn confirmed_org_ids(state: &State, user_id: &str) -> ApiResult<Vec<String>> {
    let rows = sqlx::query("SELECT org_id FROM org_users WHERE user_id = $1 AND status = $2").bind(user_id).bind(CONFIRMED).fetch_all(&state.db.pool).await?;
    Ok(rows.iter().filter_map(|row| row.try_get("org_id").ok()).collect())
}

/// Users who are confirmed members.
pub async fn confirmed_user_ids(state: &State, org: &str) -> ApiResult<Vec<String>> {
    let rows = sqlx::query("SELECT user_id FROM org_users WHERE org_id = $1 AND status = $2 AND user_id IS NOT NULL").bind(org).bind(CONFIRMED).fetch_all(&state.db.pool).await?;
    Ok(rows.iter().filter_map(|row| row.try_get("user_id").ok()).collect())
}

/// Connects invitations waiting for this address.
pub async fn claim_invitations(state: &State, user_id: &str, email: &str) -> ApiResult<()> {
    sqlx::query("UPDATE org_users SET user_id = $1, status = $2 WHERE email = $3 AND status = $4")
        .bind(user_id)
        .bind(ACCEPTED)
        .bind(email)
        .bind(INVITED)
        .execute(&state.db.pool)
        .await?;
    Ok(())
}

/// The organisations list in a profile.
pub async fn profile_organisations(state: &State, user: &User) -> ApiResult<Vec<Value>> {
    let rows = sqlx::query(
        "SELECT o.id, o.name, m.status, m.kind, m.org_key, m.access_all, o.public_key, o.private_key
         FROM org_users m JOIN organisations o ON o.id = m.org_id
         WHERE m.user_id = $1 AND m.status >= $2",
    )
    .bind(&user.id)
    .bind(ACCEPTED)
    .fetch_all(&state.db.pool)
    .await?;
    let mut out = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let name: String = row.try_get("name")?;
        let status: i64 = row.try_get("status")?;
        let kind: i64 = row.try_get("kind")?;
        let key: Option<String> = row.try_get("org_key")?;
        let public_key: Option<String> = row.try_get("public_key")?;
        out.push(json!({
            "id": id,
            "name": name,
            "key": key,
            "status": status,
            "type": kind,
            "enabled": true,
            "userId": user.id,
            "usePolicies": false,
            "useGroups": false,
            "useDirectory": false,
            "useEvents": false,
            "useTotp": true,
            "use2fa": false,
            "useApi": false,
            "useSso": false,
            "useKeyConnector": false,
            "useResetPassword": false,
            "usersGetPremium": true,
            "selfHost": true,
            "seats": null,
            "maxCollections": null,
            "maxStorageGb": null,
            "ssoBound": false,
            "identifier": null,
            "hasPublicAndPrivateKeys": public_key.is_some(),
            "resetPasswordEnrolled": false,
            "keyConnectorEnabled": false,
            "providerId": null,
            "providerName": null,
            "permissions": {},
            "object": "profileOrganization",
        }));
    }
    Ok(out)
}

fn organisation_json(row: &sqlx::any::AnyRow) -> Result<Value, sqlx::Error> {
    let id: String = row.try_get("id")?;
    let name: String = row.try_get("name")?;
    let billing: String = row.try_get("billing_email")?;
    Ok(json!({
        "id": id,
        "name": name,
        "billingEmail": billing,
        "useTotp": true,
        "selfHost": true,
        "usersGetPremium": true,
        "planType": 0,
        "seats": null,
        "object": "organization",
    }))
}

async fn create(Extract(state): Extract<State>, auth: Auth, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let name = field(&body, "name").ok_or(ApiError::Bad("A name is required."))?;
    let key = field(&body, "key").ok_or(ApiError::Bad("The organisation key is required."))?;
    let billing = field(&body, "billingEmail").unwrap_or(&auth.user.email);
    let (id, now) = (new_guid()?, now_ms());
    sqlx::query("INSERT INTO organisations (id, name, billing_email, public_key, private_key, created) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(&id)
        .bind(name)
        .bind(billing)
        .bind(body.pointer("/keys/publicKey").and_then(Value::as_str))
        .bind(body.pointer("/keys/encryptedPrivateKey").and_then(Value::as_str))
        .bind(now)
        .execute(&state.db.pool)
        .await?;
    let member = new_guid()?;
    sqlx::query("INSERT INTO org_users (id, org_id, user_id, email, status, kind, org_key, access_all) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(&member)
        .bind(&id)
        .bind(&auth.user.id)
        .bind(&auth.user.email)
        .bind(CONFIRMED)
        .bind(OWNER)
        .bind(key)
        .bind(1_i64)
        .execute(&state.db.pool)
        .await?;
    if let Some(collection) = field(&body, "collectionName") {
        let collection_id = new_guid()?;
        sqlx::query("INSERT INTO collections (id, org_id, name) VALUES ($1, $2, $3)").bind(&collection_id).bind(&id).bind(collection).execute(&state.db.pool).await?;
    }
    state.db.touch_user(&auth.user.id, now).await?;
    let row = sqlx::query("SELECT id, name, billing_email FROM organisations WHERE id = $1").bind(&id).fetch_one(&state.db.pool).await?;
    Ok(Json(organisation_json(&row)?))
}

async fn read(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    require_member(&state, &auth.user.id, &id).await?;
    let row = sqlx::query("SELECT id, name, billing_email FROM organisations WHERE id = $1").bind(&id).fetch_one(&state.db.pool).await?;
    Ok(Json(organisation_json(&row)?))
}

async fn keys(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    require_member(&state, &auth.user.id, &id).await?;
    let row = sqlx::query("SELECT public_key, private_key FROM organisations WHERE id = $1").bind(&id).fetch_one(&state.db.pool).await?;
    let public_key: Option<String> = row.try_get("public_key")?;
    let private_key: Option<String> = row.try_get("private_key")?;
    Ok(Json(json!({"publicKey": public_key, "privateKey": private_key, "object": "organizationKeys"})))
}

fn collection_json(id: &str, org: &str, name: &str) -> Value {
    json!({"id": id, "organizationId": org, "name": name, "externalId": null, "object": "collectionDetails", "readOnly": false, "hidePasswords": false, "manage": true})
}

async fn collections(Extract(state): Extract<State>, auth: Auth, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    require_member(&state, &auth.user.id, &id).await?;
    let visible = visible_collections(&state, &auth.user.id).await?;
    let data: Vec<&Value> = visible.iter().filter(|c| c["organizationId"] == id.as_str()).collect();
    Ok(Json(json!({"data": data, "continuationToken": null, "object": "list"})))
}

async fn my_collections(Extract(state): Extract<State>, auth: Auth) -> ApiResult<Json<Value>> {
    Ok(Json(json!({"data": visible_collections(&state, &auth.user.id).await?, "continuationToken": null, "object": "list"})))
}

/// Collections this user can see.
pub async fn visible_collections(state: &State, user_id: &str) -> ApiResult<Vec<Value>> {
    let mut out = Vec::new();
    for org in confirmed_org_ids(state, user_id).await? {
        let Some(member) = standing(state, user_id, &org).await? else { continue };
        let rows = if member.access_all || member.kind <= ADMIN {
            sqlx::query("SELECT id, name FROM collections WHERE org_id = $1").bind(&org).fetch_all(&state.db.pool).await?
        } else {
            sqlx::query("SELECT c.id, c.name FROM collections c JOIN collection_users cu ON cu.collection_id = c.id WHERE c.org_id = $1 AND cu.org_user_id = $2")
                .bind(&org)
                .bind(&member.id)
                .fetch_all(&state.db.pool)
                .await?
        };
        for row in rows {
            let id: String = row.try_get("id")?;
            let name: String = row.try_get("name")?;
            out.push(collection_json(&id, &org, &name));
        }
    }
    Ok(out)
}

async fn create_collection(Extract(state): Extract<State>, auth: Auth, Path(org): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    require_admin(&state, &auth.user.id, &org).await?;
    let name = field(&body, "name").ok_or(ApiError::Bad("A name is required."))?;
    let id = new_guid()?;
    sqlx::query("INSERT INTO collections (id, org_id, name) VALUES ($1, $2, $3)").bind(&id).bind(&org).bind(name).execute(&state.db.pool).await?;
    Ok(Json(collection_json(&id, &org, name)))
}

async fn read_collection(Extract(state): Extract<State>, auth: Auth, Path((org, id)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let visible = visible_collections(&state, &auth.user.id).await?;
    visible.into_iter().find(|c| c["id"] == id.as_str() && c["organizationId"] == org.as_str()).map(Json).ok_or(ApiError::NotFound)
}

async fn delete_collection(Extract(state): Extract<State>, auth: Auth, Path((org, id)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    require_admin(&state, &auth.user.id, &org).await?;
    let done = sqlx::query("DELETE FROM collections WHERE id = $1 AND org_id = $2").bind(&id).bind(&org).execute(&state.db.pool).await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query("DELETE FROM collection_users WHERE collection_id = $1").bind(&id).execute(&state.db.pool).await?;
    sqlx::query("DELETE FROM collection_ciphers WHERE collection_id = $1").bind(&id).execute(&state.db.pool).await?;
    Ok(Json(json!({})))
}

async fn members(Extract(state): Extract<State>, auth: Auth, Path(org): Path<String>) -> ApiResult<Json<Value>> {
    require_admin(&state, &auth.user.id, &org).await?;
    let rows = sqlx::query(
        "SELECT m.id, m.user_id, m.email, m.status, m.kind, m.access_all, u.name, u.public_key FROM org_users m LEFT JOIN users u ON u.id = m.user_id WHERE m.org_id = $1",
    )
    .bind(&org)
    .fetch_all(&state.db.pool)
    .await?;
    let mut data = Vec::new();
    for row in rows {
        let id: String = row.try_get("id")?;
        let user_id: Option<String> = row.try_get("user_id")?;
        let email: String = row.try_get("email")?;
        let name: Option<String> = row.try_get("name")?;
        let status: i64 = row.try_get("status")?;
        let kind: i64 = row.try_get("kind")?;
        let access_all: i64 = row.try_get("access_all")?;
        data.push(json!({
            "id": id,
            "userId": user_id,
            "email": email,
            "name": name,
            "status": status,
            "type": kind,
            "accessAll": access_all != 0,
            "externalId": null,
            "twoFactorEnabled": false,
            "object": "organizationUserUserDetails",
        }));
    }
    Ok(Json(json!({"data": data, "continuationToken": null, "object": "list"})))
}

async fn invite(Extract(state): Extract<State>, auth: Auth, Path(org): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let inviter = require_admin(&state, &auth.user.id, &org).await?;
    let kind = body.get("type").and_then(Value::as_i64).unwrap_or(USER);
    // An admin cannot mint someone above themselves.
    if kind < inviter.kind || !(0..=3).contains(&kind) {
        return Err(ApiError::Bad("That role cannot be given."));
    }
    let access_all = i64::from(body.get("accessAll").and_then(Value::as_bool).unwrap_or(false));
    for email in body.get("emails").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let email = email.trim().to_lowercase();
        let user = state.db.user_by_email(&email).await?;
        let (user_id, status) = match &user {
            Some(user) => (Some(user.id.as_str()), ACCEPTED),
            None => (None, INVITED),
        };
        let member = new_guid()?;
        let inserted = sqlx::query("INSERT INTO org_users (id, org_id, user_id, email, status, kind, org_key, access_all) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
            .bind(&member)
            .bind(&org)
            .bind(user_id)
            .bind(&email)
            .bind(status)
            .bind(kind)
            .bind(None::<String>)
            .bind(access_all)
            .execute(&state.db.pool)
            .await;
        if inserted.is_err() {
            return Err(ApiError::Bad("That address has already been invited."));
        }
        for collection in body.get("collections").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = collection.get("id").and_then(Value::as_str) else { continue };
            let found = sqlx::query("SELECT id FROM collections WHERE id = $1 AND org_id = $2").bind(id).bind(&org).fetch_optional(&state.db.pool).await?;
            if found.is_none() {
                return Err(ApiError::Bad("That collection is not available."));
            }
            sqlx::query("INSERT INTO collection_users (collection_id, org_user_id) VALUES ($1, $2)").bind(id).bind(&member).execute(&state.db.pool).await?;
        }
    }
    Ok(Json(json!({})))
}

async fn confirm(Extract(state): Extract<State>, auth: Auth, Path((org, member)): Path<(String, String)>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    require_admin(&state, &auth.user.id, &org).await?;
    let key = field(&body, "key").ok_or(ApiError::Bad("The wrapped key is required."))?;
    let done = sqlx::query("UPDATE org_users SET org_key = $1, status = $2 WHERE id = $3 AND org_id = $4 AND status = $5")
        .bind(key)
        .bind(CONFIRMED)
        .bind(&member)
        .bind(&org)
        .bind(ACCEPTED)
        .execute(&state.db.pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    let row = sqlx::query("SELECT user_id FROM org_users WHERE id = $1").bind(&member).fetch_one(&state.db.pool).await?;
    if let Ok(Some(user_id)) = row.try_get::<Option<String>, _>("user_id") {
        state.db.touch_user(&user_id, now_ms()).await?;
        state.hub.push(&user_id, UpdateKind::SyncVault, None);
    }
    Ok(Json(json!({})))
}

/// Removes a member. Their copy of the organisation key still works on anything they already
/// downloaded, so the owner should rotate shared passwords. The app says so.
///
/// Two owners removing each other at once must not leave the organisation without one, so
/// the checks and the delete run in one transaction that first locks the organisation's row.
/// That serialises membership changes on Postgres and takes SQLite's write lock up front.
async fn remove_member(Extract(state): Extract<State>, auth: Auth, Path((org, member)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    require_admin(&state, &auth.user.id, &org).await?;
    let mut tx = state.db.pool.begin().await?;
    let locked = sqlx::query("UPDATE organisations SET name = name WHERE id = $1").bind(&org).execute(&mut *tx).await?;
    if locked.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    // Looked at again under the lock: the actor may have been demoted or removed meanwhile.
    let actor = sqlx::query("SELECT kind FROM org_users WHERE org_id = $1 AND user_id = $2 AND status = $3")
        .bind(&org)
        .bind(&auth.user.id)
        .bind(CONFIRMED)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    let actor_kind: i64 = actor.try_get("kind")?;
    if actor_kind > ADMIN {
        return Err(ApiError::NotFound);
    }
    let row = sqlx::query("SELECT kind, user_id FROM org_users WHERE id = $1 AND org_id = $2").bind(&member).bind(&org).fetch_optional(&mut *tx).await?.ok_or(ApiError::NotFound)?;
    let kind: i64 = row.try_get("kind")?;
    if kind < actor_kind {
        return Err(ApiError::Bad("That member cannot be removed."));
    }
    // One statement, so even without the lock it never deletes the last confirmed owner.
    let done = sqlx::query(
        "DELETE FROM org_users WHERE id = $1 AND org_id = $2 AND (kind <> $3 OR EXISTS (
            SELECT 1 FROM org_users other WHERE other.org_id = $2 AND other.kind = $3 AND other.status = $4 AND other.id <> $1
        ))",
    )
    .bind(&member)
    .bind(&org)
    .bind(OWNER)
    .bind(CONFIRMED)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiError::Bad("An organisation needs an owner."));
    }
    sqlx::query("DELETE FROM collection_users WHERE org_user_id = $1").bind(&member).execute(&mut *tx).await?;
    tx.commit().await?;
    if let Ok(Some(user_id)) = row.try_get::<Option<String>, _>("user_id") {
        state.db.touch_user(&user_id, now_ms()).await?;
        state.hub.push(&user_id, UpdateKind::SyncVault, None);
    }
    Ok(Json(json!({})))
}
