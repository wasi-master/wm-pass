//! Hardware keys as a second unlock factor, without any platform code in the protocol.
//!
//! Each protocol runs over a small transport trait, so the same code serves USB on desktop,
//! and USB or NFC on Android, where Kotlin passes frames through JNI. The output of either
//! protocol goes into the vault format as a [`wmpass_core::format::FactorOutput`].

pub mod ctap;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod otp;
pub mod transport;

/// Why a hardware key operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HwError {
    /// The key went away or did not answer.
    #[error("the security key could not be reached")]
    Transport,
    /// The key answered with something that does not follow the protocol.
    #[error("the security key sent something unexpected")]
    Protocol,
    /// It does not support what is needed.
    #[error("this security key does not support hmac-secret")]
    Unsupported,
    /// Nobody touched it in time, or the request was refused on the key.
    #[error("the security key was not touched in time")]
    NotConfirmed,
    /// It does not hold this credential. Probably a different key.
    #[error("this is not the security key that was set up")]
    WrongKey,
    /// It asks for its PIN, which WM Pass does not handle here.
    #[error("this security key requires its PIN for this operation")]
    PinRequired,
    /// The OTP slot is empty or not set up for challenge-response.
    #[error("that slot on the key is not set up for challenge-response")]
    SlotNotConfigured,
    /// Crypto failed.
    #[error("the security key's answer did not verify")]
    Crypto,
}

/// Shorthand.
pub type Result<T> = std::result::Result<T, HwError>;
