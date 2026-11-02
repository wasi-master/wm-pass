//! The one error type. Messages never contain secret material.

/// Everything the core can refuse.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// The operating system's random source failed. Never fall back from this.
    #[error("the system random source failed")]
    Random,
    /// The file is not a WM Pass vault, or it was damaged.
    #[error("this is not a readable WM Pass vault")]
    Malformed,
    /// A newer format than this build understands.
    #[error("this vault needs a newer version of WM Pass (format {0})")]
    UnsupportedVersion(u16),
    /// Key derivation settings outside the accepted range. A tampered header
    /// could otherwise weaken the derivation or exhaust memory.
    #[error("the vault's key derivation settings are outside the accepted range")]
    KdfOutOfRange,
    /// Authentication failed after the key was accepted: tampering or corruption.
    #[error("the vault failed its integrity check")]
    Integrity,
    /// All slots are taken.
    #[error("this vault has no free unlock slots")]
    NoFreeSlot,
    /// A value that must fit a fixed-size record does not.
    #[error("that value is too long")]
    TooLong,
    /// The body could not be encoded or decoded.
    #[error("the vault contents could not be read")]
    Encoding,
    /// A crypto primitive refused its input.
    #[error("a cryptographic operation failed")]
    Crypto,
    /// An imported file could not be understood.
    #[error("the file could not be imported: {0}")]
    Import(String),
    /// The input to a feature was not valid.
    #[error("{0}")]
    Invalid(String),
}

/// Shorthand for results carrying [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
