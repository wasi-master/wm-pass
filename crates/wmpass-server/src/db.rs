//! Storage. One schema for SQLite and Postgres through sqlx's `Any` driver: the SQL here
//! sticks to what both accept, including `$1` placeholders.

use sqlx::any::{AnyPoolOptions, install_default_drivers};
use sqlx::{AnyPool, Row};

const SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS users (
        id TEXT PRIMARY KEY,
        email TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        password_hash TEXT NOT NULL,
        password_salt TEXT NOT NULL,
        kdf BIGINT NOT NULL,
        kdf_iterations BIGINT NOT NULL,
        kdf_memory BIGINT,
        kdf_parallelism BIGINT,
        user_key TEXT NOT NULL,
        public_key TEXT,
        private_key TEXT,
        security_stamp TEXT NOT NULL,
        created BIGINT NOT NULL,
        revision BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS devices (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        identifier TEXT NOT NULL,
        name TEXT NOT NULL,
        kind BIGINT NOT NULL,
        refresh_hash TEXT NOT NULL,
        created BIGINT NOT NULL,
        last_seen BIGINT NOT NULL,
        UNIQUE (user_id, identifier)
    )",
    "CREATE TABLE IF NOT EXISTS folders (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        name TEXT NOT NULL,
        revision BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS ciphers (
        id TEXT PRIMARY KEY,
        user_id TEXT,
        org_id TEXT,
        data TEXT NOT NULL,
        created BIGINT NOT NULL,
        revision BIGINT NOT NULL,
        deleted BIGINT
    )",
    "CREATE TABLE IF NOT EXISTS cipher_user (
        cipher_id TEXT NOT NULL,
        user_id TEXT NOT NULL,
        folder_id TEXT,
        favorite BIGINT NOT NULL,
        PRIMARY KEY (cipher_id, user_id)
    )",
    "CREATE TABLE IF NOT EXISTS organisations (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        billing_email TEXT NOT NULL,
        public_key TEXT,
        private_key TEXT,
        created BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS org_users (
        id TEXT PRIMARY KEY,
        org_id TEXT NOT NULL,
        user_id TEXT,
        email TEXT NOT NULL,
        status BIGINT NOT NULL,
        kind BIGINT NOT NULL,
        org_key TEXT,
        access_all BIGINT NOT NULL,
        UNIQUE (org_id, email)
    )",
    "CREATE TABLE IF NOT EXISTS collections (
        id TEXT PRIMARY KEY,
        org_id TEXT NOT NULL,
        name TEXT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS collection_users (
        collection_id TEXT NOT NULL,
        org_user_id TEXT NOT NULL,
        PRIMARY KEY (collection_id, org_user_id)
    )",
    "CREATE TABLE IF NOT EXISTS collection_ciphers (
        collection_id TEXT NOT NULL,
        cipher_id TEXT NOT NULL,
        PRIMARY KEY (collection_id, cipher_id)
    )",
    "CREATE TABLE IF NOT EXISTS shared_files (
        id TEXT PRIMARY KEY,
        owner_id TEXT NOT NULL,
        body TEXT NOT NULL,
        revision BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS shared_members (
        file_id TEXT NOT NULL,
        user_id TEXT NOT NULL,
        PRIMARY KEY (file_id, user_id)
    )",
    "CREATE INDEX IF NOT EXISTS ciphers_user ON ciphers (user_id)",
    "CREATE INDEX IF NOT EXISTS ciphers_org ON ciphers (org_id)",
    "CREATE INDEX IF NOT EXISTS org_users_user ON org_users (user_id)",
];

/// The pool.
#[derive(Clone, Debug)]
pub struct Db {
    /// Connections.
    pub pool: AnyPool,
}

impl Db {
    /// Connects and brings the schema up to date.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        install_default_drivers();
        let sqlite = url.starts_with("sqlite:");
        let pool = AnyPoolOptions::new().max_connections(if sqlite { 4 } else { 16 }).connect(url).await?;
        if sqlite {
            sqlx::query("PRAGMA journal_mode = WAL").execute(&pool).await?;
            sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;
        }
        for statement in SCHEMA {
            sqlx::query(*statement).execute(&pool).await?;
        }
        Ok(Self { pool })
    }
}

/// A user row.
#[derive(Clone, Debug)]
pub struct User {
    /// Id.
    pub id: String,
    /// Lower-case email.
    pub email: String,
    /// Display name.
    pub name: String,
    /// Slow hash of the client's hash.
    pub password_hash: String,
    /// Salt for that hash.
    pub password_salt: String,
    /// KDF kind.
    pub kdf: i64,
    /// KDF iterations.
    pub kdf_iterations: i64,
    /// KDF memory.
    pub kdf_memory: Option<i64>,
    /// KDF parallelism.
    pub kdf_parallelism: Option<i64>,
    /// Account key, wrapped by the client.
    pub user_key: String,
    /// Public key.
    pub public_key: Option<String>,
    /// Private key, wrapped by the client.
    pub private_key: Option<String>,
    /// Changing this invalidates every token.
    pub security_stamp: String,
    /// When the account's contents last changed.
    pub revision: i64,
}

fn user_from(row: &sqlx::any::AnyRow) -> Result<User, sqlx::Error> {
    Ok(User {
        id: row.try_get("id")?,
        email: row.try_get("email")?,
        name: row.try_get("name")?,
        password_hash: row.try_get("password_hash")?,
        password_salt: row.try_get("password_salt")?,
        kdf: row.try_get("kdf")?,
        kdf_iterations: row.try_get("kdf_iterations")?,
        kdf_memory: row.try_get("kdf_memory")?,
        kdf_parallelism: row.try_get("kdf_parallelism")?,
        user_key: row.try_get("user_key")?,
        public_key: row.try_get("public_key")?,
        private_key: row.try_get("private_key")?,
        security_stamp: row.try_get("security_stamp")?,
        revision: row.try_get("revision")?,
    })
}

impl Db {
    /// Finds a user by address.
    pub async fn user_by_email(&self, email: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query("SELECT id, email, name, password_hash, password_salt, kdf, kdf_iterations, kdf_memory, kdf_parallelism, user_key, public_key, private_key, security_stamp, revision FROM users WHERE email = $1")
            .bind(email).fetch_optional(&self.pool).await?.as_ref().map(user_from).transpose()
    }

    /// Finds a user by id.
    pub async fn user_by_id(&self, id: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query("SELECT id, email, name, password_hash, password_salt, kdf, kdf_iterations, kdf_memory, kdf_parallelism, user_key, public_key, private_key, security_stamp, revision FROM users WHERE id = $1")
            .bind(id).fetch_optional(&self.pool).await?.as_ref().map(user_from).transpose()
    }

    /// Marks that an account's contents changed.
    pub async fn touch_user(&self, id: &str, now: i64) -> Result<(), sqlx::Error> {
        // Only ever forward. Two changes whose requests finish out of order, or a clock set
        // back, would otherwise move it back, and a client that synced at the later time would
        // read "nothing new" and miss the other change until the next one.
        sqlx::query("UPDATE users SET revision = CASE WHEN revision >= $1 THEN revision + 1 ELSE $1 END WHERE id = $2")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Now, in milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

/// A date as the API writes them.
pub fn date(ms: i64) -> String {
    wmpass_core::clock::format_date(u64::try_from(ms).unwrap_or(0))
}

/// A new id in the GUID form clients expect.
pub fn new_guid() -> Result<String, crate::error::ApiError> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|_| crate::error::ApiError::Internal)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = wmpass_core::vault::hex(&b);
    Ok(format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..]))
}
