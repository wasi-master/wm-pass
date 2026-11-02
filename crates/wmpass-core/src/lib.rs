//! The part of WM Pass every platform shares.
//!
//! This crate has no platform code and no network code, so offline mode cannot
//! be bypassed from inside it and its tests need no fakes for the outside world.

pub mod clock;
pub mod crypto;
pub mod error;
pub mod format;
pub mod model;
pub mod otp;
pub mod secret;
pub mod vault;

pub use error::{Error, Result};
pub use secret::Secret;
