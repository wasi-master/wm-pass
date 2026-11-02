//! Everything in WM Pass that talks to a service, apart from sync.
//!
//! Providers are sans-IO: each builds a [`http::Call`], which is a request plus the
//! function that reads its answer. Blocking clients run calls directly and the browser
//! extension runs them over `fetch`, with the same parsing and the same policy check.

pub mod aliases;
pub mod breach;
pub mod favicon;
pub mod http;
pub mod inbox;
pub mod usernames;

pub use http::{Call, Http, NetError, NetPolicy, Request, Response};
