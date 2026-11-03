//! A client for Bitwarden's API: bitwarden.com, Vaultwarden, or the WM Pass server.
//!
//! Bitwarden's envelope and wire format, written from public descriptions and captured
//! traffic. Its crates are GPL and are not used. The local model stays canonical, and
//! ciphers are mapped in and out, keeping whatever a mapping does not understand.

pub mod account;
pub mod api;
pub mod crypto;
pub mod mapping;

/// Why a Bitwarden operation failed. Nothing here carries a secret or an address.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BwError {
    /// A network or server failure.
    #[error(transparent)]
    Net(#[from] wmpass_net::NetError),
    /// A value could not be decrypted: a wrong key or a damaged item.
    #[error("an item could not be decrypted")]
    Crypto,
    /// The server asks for key settings too weak to trust. A hostile server could use weak
    /// settings to make a captured password hash cheap to crack.
    #[error("the server asked for weaker password protection than WM Pass accepts")]
    WeakKdf,
    /// The answer was not what the API documents.
    #[error("the server's answer could not be read")]
    BadAnswer,
    /// A second factor is needed. Holds the provider numbers on offer.
    #[error("this account needs a second factor")]
    TwoFactor(Vec<u32>),
    /// The email or password was refused.
    #[error("the email or password was not accepted")]
    BadLogin,
    /// The server took the password, but the account key it sent back would not open with it.
    #[error("Bitwarden accepted the password, but this account's key could not be opened")]
    KeyUnreadable,
    /// Bitwarden has emailed a code to confirm a device it has not seen before.
    #[error("Bitwarden has emailed you a code to confirm this device")]
    NewDevice,
    /// A two-step or new-device code was refused.
    #[error("that code was not accepted")]
    BadCode,
    /// The login service refused for a reason of its own. Holds its message.
    #[error("{0}")]
    Refused(String),
    /// Something this client cannot do yet.
    #[error("{0}")]
    Unsupported(&'static str),
}

/// Shorthand.
pub type Result<T> = std::result::Result<T, BwError>;
