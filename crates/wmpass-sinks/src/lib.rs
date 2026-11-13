//! Places a vault file can be kept, and the routines that sync and back up over them.
//!
//! Everything that reaches a destination is already encrypted, so a destination learns
//! file sizes and timing, and nothing else. The code is async so the browser extension
//! can run it over `fetch`. Blocking clients run it with [`http::block_on`].

pub mod backups;
pub mod boxcloud;
pub mod drive;
pub mod dropbox;
#[cfg(not(target_arch = "wasm32"))]
pub mod folder;
#[cfg(not(target_arch = "wasm32"))]
pub mod ftp;
pub mod git;
pub mod http;
pub mod icloud;
#[cfg(not(target_arch = "wasm32"))]
pub mod imap;
pub mod mega;
pub mod nas;
#[cfg(not(target_arch = "wasm32"))]
pub mod nfs;
pub mod oauth;
pub mod onedrive;
pub mod pcloud;
pub mod places;
pub mod proton;
pub mod routines;
pub mod s3;
#[cfg(not(target_arch = "wasm32"))]
pub mod sftp;
#[cfg(not(target_arch = "wasm32"))]
pub mod smb;
#[cfg(not(target_arch = "wasm32"))]
pub mod ssh;
pub mod webdav;

use std::future::Future;

/// One stored file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    /// What addresses it at the destination.
    pub id: String,
    /// File name.
    pub name: String,
    /// Bytes.
    pub size: u64,
}

/// Why a destination refused. No variant carries a path, key or address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// An address, user name or key is missing.
    NotConfigured,
    /// The credentials were refused or have expired.
    PermissionLost,
    /// The folder or bucket is not there.
    TargetMissing,
    /// Out of space or over quota.
    OutOfSpace,
    /// Network use is off, or this connection is not allowed.
    Blocked,
    /// No answer, or not the answer the service documents.
    Io,
    /// A file failed verification after being written.
    NotVerified,
    /// Reachable but not safe to use: a server key that changed since the first
    /// connection, a repository anyone can read, a share that will not encrypt.
    Unsafe,
    /// The server takes the password only over an encrypted connection, and this one is plain:
    /// an IMAP server that advertises `LOGINDISABLED` or answers `[PRIVACYREQUIRED]`.
    NeedsEncryption,
    /// The service refused the request's time stamp: this device's clock is too far off, as
    /// S3's `RequestTimeTooSkewed` says. Not a wrong key.
    ClockWrong,
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotConfigured => "this location is not fully set up",
            Self::PermissionLost => "the location refused the sign-in",
            Self::TargetMissing => "the folder or bucket is not there",
            Self::OutOfSpace => "the location is out of space",
            Self::Blocked => "network use is off",
            Self::Io => "the location could not be reached",
            Self::NotVerified => "the file could not be read back correctly",
            Self::Unsafe => "the location is not safe to use: its identity changed, or it would expose the file",
            Self::NeedsEncryption => "the server needs an encrypted connection: choose TLS or STARTTLS",
            Self::ClockWrong => "this device's clock is wrong, so the location refused the request: set the date and time",
        })
    }
}

impl std::error::Error for Fault {}

/// Shorthand for results carrying [`Fault`].
pub type Result<T> = std::result::Result<T, Fault>;

/// What every destination offers.
pub trait Sink {
    /// Whether a write could succeed now: one cheap query, no transfer.
    fn readiness(&self) -> impl Future<Output = Result<()>>;
    /// Writes a file whole, replacing any of that name, staged and moved where the protocol allows.
    fn write(&self, name: &str, bytes: &[u8]) -> impl Future<Output = Result<Stored>>;
    /// Files here.
    fn list(&self) -> impl Future<Output = Result<Vec<Stored>>>;
    /// Reads a file.
    fn read(&self, file: &Stored) -> impl Future<Output = Result<Vec<u8>>>;
    /// Deletes a file. Already gone counts as done.
    fn delete(&self, file: &Stored) -> impl Future<Output = Result<()>>;
}

pub(crate) fn status_fault(status: u16) -> Fault {
    match status {
        401 | 403 => Fault::PermissionLost,
        404 | 409 => Fault::TargetMissing,
        413 | 507 => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

/// The characters an XML text or reference event stands for. quick-xml reports `&amp;`,
/// `&#39;` and the like as their own events between runs of text, so a value split by one
/// is only whole once every piece is appended. Anything unreadable is dropped.
pub(crate) fn xml_text(event: &quick_xml::events::Event<'_>) -> String {
    use quick_xml::events::Event;
    match event {
        Event::Text(text) => quick_xml::escape::unescape(text).map(|v| v.into_owned()).unwrap_or_default(),
        Event::GeneralRef(reference) => match reference.resolve_char_ref() {
            Ok(Some(c)) => c.to_string(),
            Ok(None) => quick_xml::escape::resolve_xml_entity(reference).unwrap_or_default().to_owned(),
            Err(_) => String::new(),
        },
        _ => String::new(),
    }
}

/// Encodes one path segment.
pub(crate) fn segment(name: &str) -> String {
    wmpass_core::otp::percent_encode(name)
}
