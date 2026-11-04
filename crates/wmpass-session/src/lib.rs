//! The one interface every WM Pass shell uses.
//!
//! Vault state stays on this side behind an opaque handle. What crosses to Kotlin,
//! TypeScript or Swift is one of three things:
//!
//! - JSON views, which never contain a secret value. A secret field appears as a
//!   flag saying whether it has a value.
//! - Ciphertext, such as a saved vault file or an encrypted attachment.
//! - A secret in [`Reply::secret`], only when the caller asked for that one value,
//!   as bytes the caller can wipe. Strings in those languages cannot be wiped, which
//!   is why JSON is never used for this.
//!
//! Secrets going the other way arrive in [`Call::secret`] for the same reason.

mod combine;
pub mod fields;
#[cfg(feature = "net")]
pub mod net;
mod remote;
mod service;
mod views;
pub mod wire;

pub use service::{Call, Reply, Service, SessionError, factor_of};
pub use views::*;
