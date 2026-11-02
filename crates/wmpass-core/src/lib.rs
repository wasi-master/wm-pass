//! The part of WM Pass every platform shares.
//!
//! This crate has no platform code and no network code, so offline mode cannot
//! be bypassed from inside it and its tests need no fakes for the outside world.

pub mod backend;
pub mod categorise;
pub mod clock;
pub mod crypto;
pub mod emergency;
pub mod error;
pub mod format;
pub mod generator;
pub mod health;
pub mod io;
pub mod matching;
pub mod model;
pub mod otp;
pub mod recovery;
pub mod search;
pub mod secret;
pub mod ssh;
pub mod strength;
pub mod vault;

pub use error::{Error, Result};
pub use secret::Secret;
