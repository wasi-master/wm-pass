//! A self-hosted sync server. It speaks Bitwarden's client API so official clients work
//! against it, and adds a small `/wm` namespace for what only WM Pass uses.
//!
//! The server only ever holds what clients encrypted. A database leak gives an attacker
//! ciphertext, wrapped keys and a slow hash of a hash. What it can still see is who has an
//! account, how many items they hold and when they change.
//!
//! The wire format comes from facts: captured traffic and documented behaviour. No Bitwarden
//! or Vaultwarden code is used.

pub mod accounts;
pub mod auth;
pub mod ciphers;
pub mod db;
pub mod error;
pub mod folders;
pub mod hub;
pub mod identity;
pub mod organisations;
pub mod wm;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method, header};
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

/// Who may create an account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Signups {
    /// Nobody. The safe default for something on the internet.
    #[default]
    Closed,
    /// Anybody who can reach it.
    Open,
    /// These addresses only.
    Emails(Vec<String>),
}

impl Signups {
    /// Whether this address may register.
    pub fn allows(&self, email: &str) -> bool {
        match self {
            Self::Closed => false,
            Self::Open => true,
            Self::Emails(list) => list.iter().any(|allowed| allowed.eq_ignore_ascii_case(email)),
        }
    }
}

/// How the server is set up.
#[derive(Clone, Debug)]
pub struct Config {
    /// Holds the signing key, and the database unless `database_url` says otherwise.
    pub data_dir: PathBuf,
    /// `sqlite://…` or `postgres://…`. Defaults to SQLite in the data folder.
    pub database_url: Option<String>,
    /// Who may register.
    pub signups: Signups,
    /// The address clients reach this server at, such as `https://pass.example.com`. When set,
    /// `/api/config` advertises it; when not, it falls back to the request's `Host` header.
    pub public_url: Option<String>,
}

/// Everything handlers share.
#[derive(Debug)]
pub struct AppState {
    /// Database.
    pub db: db::Db,
    /// Token signing key.
    pub jwt_key: [u8; 32],
    /// Config.
    pub config: Config,
    /// Slows down password guessing.
    pub throttle: auth::Throttle,
    /// Limits concurrent hashing, so a flood of logins cannot exhaust memory.
    pub hashing: tokio::sync::Semaphore,
    /// Live notification sockets.
    pub hub: hub::Hub,
}

/// Shared state handle.
pub type State = Arc<AppState>;

/// Opens the database, reads or makes the signing key, and builds the router.
pub async fn build(config: Config) -> Result<Router, String> {
    std::fs::create_dir_all(&config.data_dir).map_err(|e| format!("cannot create the data folder: {e}"))?;
    let jwt_key = auth::load_or_create_key(&config.data_dir)?;
    let url = config.database_url.clone().unwrap_or_else(|| {
        format!("sqlite://{}?mode=rwc", config.data_dir.join("wmpass.sqlite3").display())
    });
    let db = db::Db::connect(&url).await.map_err(|e| format!("cannot open the database: {e}"))?;
    let state = Arc::new(AppState {
        db,
        jwt_key,
        config,
        throttle: auth::Throttle::default(),
        hashing: tokio::sync::Semaphore::new(4),
        hub: hub::Hub::default(),
    });
    Ok(router(state))
}

/// Clients change. Saying which route was missing is what makes a break diagnosable. The
/// query string is left out of the log, since a token could be in it.
async fn unmatched(method: Method, uri: axum::http::Uri) -> error::ApiError {
    eprintln!("no route for {method} {}", uri.path());
    error::ApiError::NotFound
}

fn router(state: State) -> Router {
    Router::new()
        .merge(identity::routes())
        .merge(accounts::routes())
        .merge(ciphers::routes())
        .merge(folders::routes())
        .merge(organisations::routes())
        .merge(hub::routes())
        .merge(wm::routes())
        .fallback(unmatched)
        .with_state(state)
        // One cap for every route, shared vault files included. Ciphers and notes are small,
        // and a shared file is a sealed vault document whose attachments are separate blobs.
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        // Browser extensions need this. A website does not, and gets nothing.
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
                    let origin = origin.as_bytes();
                    origin.starts_with(b"chrome-extension://") || origin.starts_with(b"moz-extension://")
                }))
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers(AllowHeaders::mirror_request()),
        )
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
}
