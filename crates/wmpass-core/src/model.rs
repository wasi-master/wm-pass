//! Entries and everything hanging off them.
//!
//! Each field carries its own clock, so two devices editing different fields of
//! one entry both keep their change. When a secret field loses a merge, its value
//! moves to history instead of disappearing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Secret;
use crate::clock::Hlc;
use crate::otp::OtpConfig;

/// Random hex id.
pub type Id = String;

/// A value plus the moment it was set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lww<T> {
    /// The value.
    pub v: T,
    /// When it was written.
    pub at: Hlc,
}

impl<T> Lww<T> {
    /// A value written at a moment.
    pub fn new(v: T, at: Hlc) -> Self {
        Self { v, at }
    }
}

/// Lets a merge keep a secret that lost.
pub trait KeepsLoser {
    /// The value as text if losing it should leave a revision behind.
    fn loser(&self) -> Option<Secret> {
        None
    }
}

macro_rules! plain_fields {
    ($($ty:ty),* $(,)?) => { $(impl KeepsLoser for $ty {})* };
}

plain_fields!(
    String, bool, u32, u64, Option<String>, Option<u64>, Vec<String>, Vec<Field>, Vec<Attachment>,
    Vec<SiteMatcher>, Vec<Passkey>, Option<OtpConfig>, Reminders, Option<AliasRef>, Option<u16>,
    WifiSecurity, RecoveryForm,
);

impl KeepsLoser for Secret {
    fn loser(&self) -> Option<Secret> {
        (!self.is_empty()).then(|| self.clone())
    }
}

impl<T: Clone + PartialEq + KeepsLoser> Lww<T> {
    fn merge_from(&mut self, other: &Self, field: &'static str, lost: &mut Vec<Revision>) {
        if other.at > self.at {
            if self.v != other.v
                && let Some(value) = self.v.loser()
            {
                lost.push(Revision { field: field.to_owned(), value, at: self.at });
            }
            *self = other.clone();
        } else if other.at < self.at
            && self.v != other.v
            && let Some(value) = other.v.loser()
        {
            lost.push(Revision { field: field.to_owned(), value, at: other.at });
        }
    }
}

macro_rules! record {
    ($(#[$meta:meta])* pub struct $name:ident { $($(#[$field_meta:meta])* pub $field:ident : $ty:ty,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name {
            $($(#[$field_meta])* pub $field: Lww<$ty>,)*
        }

        impl $name {
            fn merge_from(&mut self, other: &Self, lost: &mut Vec<Revision>) {
                $(self.$field.merge_from(&other.$field, stringify!($field), lost);)*
            }

            /// The latest moment any field changed.
            pub fn latest(&self) -> Hlc {
                let mut latest = Hlc::default();
                $(latest = latest.max(self.$field.at);)*
                latest
            }
        }
    };
}

/// One way a login matches a place to fill.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteMatcher {
    /// Same registrable domain (eTLD+1).
    Domain(String),
    /// Exactly this host.
    Host(String),
    /// A URL as saved. Its host is what is matched.
    Url(String),
    /// An Android app, by package and certificate digest. A package name alone is
    /// never trusted, because anyone can reuse one.
    AndroidApp {
        /// Package name.
        package: String,
        /// SHA-256 of the signing certificate, when known.
        cert_sha256: Option<String>,
    },
    /// A regular expression over the whole URL.
    Regex(String),
    /// Never suggested automatically.
    Never,
}

/// How a custom field behaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    /// Shown in the clear.
    #[default]
    Text,
    /// Hidden until revealed.
    Hidden,
    /// A link.
    Url,
    /// An email address.
    Email,
    /// A phone number.
    Phone,
    /// A date.
    Date,
    /// A yes or no.
    Toggle,
}

/// A user-defined field. Values are always [`Secret`], whatever their kind, so a
/// field changed to hidden later has no plain copies around.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Field {
    /// Label.
    pub name: String,
    /// Value.
    pub value: Secret,
    /// Behaviour.
    pub kind: FieldKind,
}

/// An old value of a secret field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    /// Which field.
    pub field: String,
    /// What it held.
    pub value: Secret,
    /// When that value was written.
    pub at: Hlc,
}

/// An encrypted attachment stored beside the vault file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attachment {
    /// Blob file name, random.
    pub blob: String,
    /// The blob's key. Dropping it destroys the attachment.
    pub key: Secret,
    /// Original file name.
    pub name: String,
    /// Media type.
    pub mime: String,
    /// Plaintext bytes.
    pub size: u64,
}

/// A stored passkey.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Passkey {
    /// Base64url credential id.
    pub credential_id: String,
    /// Relying party id, such as `example.com`.
    pub rp_id: String,
    /// Relying party's display name.
    pub rp_name: String,
    /// Base64url user handle.
    pub user_handle: String,
    /// Display name of the user.
    pub user_name: String,
    /// PKCS#8 private key, base64url. ES256 unless `algorithm` says otherwise.
    pub private_key: Secret,
    /// COSE algorithm number.
    pub algorithm: i32,
    /// Signature counter.
    pub counter: u32,
    /// Created, in milliseconds.
    pub created_ms: u64,
}

/// Dates to nudge the owner about.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reminders {
    /// Ask for a password change every this many days.
    pub change_every_days: Option<u32>,
    /// A fixed expiry, in milliseconds.
    pub expires_ms: Option<u64>,
    /// Warn this many days ahead.
    pub warn_days: u32,
}

/// Where an entry's email alias came from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AliasRef {
    /// Provider id, such as `simplelogin`.
    pub provider: String,
    /// Provider's id for the alias.
    pub remote_id: String,
    /// The address.
    pub address: String,
}

/// Wi-Fi security kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiSecurity {
    /// Open network.
    Open,
    /// WEP.
    Wep,
    /// WPA or WPA2 personal.
    #[default]
    Wpa,
    /// WPA3 personal.
    Sae,
    /// Enterprise.
    Eap,
}

/// Shape of a recovery entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryForm {
    /// A word phrase, checked against BIP-39 where possible.
    #[default]
    Phrase,
    /// A list of codes.
    Codes,
}

record! {
    /// A website or app account.
    pub struct Login {
        pub site_matchers: Vec<SiteMatcher>,
        pub username: String,
        pub password: Secret,
        pub otp: Option<OtpConfig>,
        pub passkeys: Vec<Passkey>,
        pub alias: Option<AliasRef>,
        pub questions: Vec<Field>,
    }
}

record! {
    /// A payment card.
    pub struct Card {
        pub holder: String,
        pub number: Secret,
        pub expiry: String,
        pub cvv: Secret,
        pub pin: Secret,
        pub issuer: String,
        pub billing: String,
    }
}

record! {
    /// A person's details.
    pub struct Identity {
        pub full_name: String,
        pub address: String,
        pub phone: String,
        pub email: String,
        pub birth_date: Secret,
        pub document_numbers: Vec<Field>,
    }
}

record! {
    /// A private note.
    pub struct Note {
        pub body: Secret,
    }
}

record! {
    /// A scanned or stored document. The files are the attachments.
    pub struct Document {
        pub label: String,
        pub number: Secret,
    }
}

record! {
    /// An SSH key.
    pub struct SshKey {
        pub private_key: Secret,
        pub public_key: String,
        pub fingerprint: String,
        pub passphrase: Secret,
        pub agent_allowed: bool,
    }
}

record! {
    /// An API credential.
    pub struct ApiKey {
        pub provider: String,
        pub key: Secret,
        pub secret: Secret,
        pub scopes: String,
        pub expires_ms: Option<u64>,
    }
}

record! {
    /// Database credentials.
    pub struct Database {
        pub engine: String,
        pub host: String,
        pub port: Option<u16>,
        pub name: String,
        pub user: String,
        pub password: Secret,
        pub tls: bool,
        pub url: Secret,
    }
}

record! {
    /// A software licence.
    pub struct Licence {
        pub product: String,
        pub key: Secret,
        pub email: String,
        pub order_id: String,
        pub expires_ms: Option<u64>,
    }
}

record! {
    /// A recovery phrase or codes.
    pub struct Recovery {
        pub form: RecoveryForm,
        pub phrase: Secret,
        pub linked_entry: Option<String>,
    }
}

record! {
    /// A Wi-Fi network.
    pub struct Wifi {
        pub ssid: String,
        pub security: WifiSecurity,
        pub password: Secret,
        pub hidden_ssid: bool,
    }
}

record! {
    /// An authenticator-only entry.
    pub struct Otp {
        pub config: Option<OtpConfig>,
    }
}

/// The typed body of an entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    /// See [`Login`].
    Login(Login),
    /// See [`Card`].
    Card(Card),
    /// See [`Identity`].
    Identity(Identity),
    /// See [`Note`].
    Note(Note),
    /// See [`Document`].
    Document(Document),
    /// See [`SshKey`].
    SshKey(SshKey),
    /// See [`ApiKey`].
    ApiKey(ApiKey),
    /// See [`Database`].
    Database(Database),
    /// See [`Licence`].
    Licence(Licence),
    /// See [`Recovery`].
    Recovery(Recovery),
    /// See [`Wifi`].
    Wifi(Wifi),
    /// See [`Otp`].
    Otp(Otp),
}

impl Default for Kind {
    fn default() -> Self {
        Self::Login(Login::default())
    }
}

impl Kind {
    /// The kind's name, as search and filters use it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Login(_) => "login",
            Self::Card(_) => "card",
            Self::Identity(_) => "identity",
            Self::Note(_) => "note",
            Self::Document(_) => "document",
            Self::SshKey(_) => "ssh",
            Self::ApiKey(_) => "api",
            Self::Database(_) => "database",
            Self::Licence(_) => "licence",
            Self::Recovery(_) => "recovery",
            Self::Wifi(_) => "wifi",
            Self::Otp(_) => "otp",
        }
    }

    fn latest(&self) -> Hlc {
        match self {
            Self::Login(v) => v.latest(),
            Self::Card(v) => v.latest(),
            Self::Identity(v) => v.latest(),
            Self::Note(v) => v.latest(),
            Self::Document(v) => v.latest(),
            Self::SshKey(v) => v.latest(),
            Self::ApiKey(v) => v.latest(),
            Self::Database(v) => v.latest(),
            Self::Licence(v) => v.latest(),
            Self::Recovery(v) => v.latest(),
            Self::Wifi(v) => v.latest(),
            Self::Otp(v) => v.latest(),
        }
    }

    /// Merges two bodies of the same kind by field. If the kind itself changed on
    /// one side, the more recently edited body wins whole.
    fn merge_from(&mut self, other: &Self, lost: &mut Vec<Revision>) {
        match (&mut *self, other) {
            (Self::Login(a), Self::Login(b)) => a.merge_from(b, lost),
            (Self::Card(a), Self::Card(b)) => a.merge_from(b, lost),
            (Self::Identity(a), Self::Identity(b)) => a.merge_from(b, lost),
            (Self::Note(a), Self::Note(b)) => a.merge_from(b, lost),
            (Self::Document(a), Self::Document(b)) => a.merge_from(b, lost),
            (Self::SshKey(a), Self::SshKey(b)) => a.merge_from(b, lost),
            (Self::ApiKey(a), Self::ApiKey(b)) => a.merge_from(b, lost),
            (Self::Database(a), Self::Database(b)) => a.merge_from(b, lost),
            (Self::Licence(a), Self::Licence(b)) => a.merge_from(b, lost),
            (Self::Recovery(a), Self::Recovery(b)) => a.merge_from(b, lost),
            (Self::Wifi(a), Self::Wifi(b)) => a.merge_from(b, lost),
            (Self::Otp(a), Self::Otp(b)) => a.merge_from(b, lost),
            _ => {
                if other.latest() > self.latest() {
                    *self = other.clone();
                }
            }
        }
    }
}

record! {
    /// Fields every entry has.
    pub struct Common {
        pub title: String,
        pub icon: Option<String>,
        pub color: Option<String>,
        pub pinned: bool,
        pub favourite: bool,
        pub folder: Option<String>,
        pub tags: Vec<String>,
        pub links: Vec<String>,
        pub custom_fields: Vec<Field>,
        pub attachments: Vec<Attachment>,
        pub reminders: Reminders,
        pub destroy_at_ms: Option<u64>,
        pub hidden: bool,
        pub deleted_at_ms: Option<u64>,
        pub notes: Secret,
    }
}

/// One item in a vault.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    /// Random id.
    pub id: Id,
    /// Created, in milliseconds.
    pub created_ms: u64,
    /// Shared fields.
    pub common: Common,
    /// Typed body.
    pub kind: Kind,
    /// Past values of secret fields, oldest first.
    pub history: Vec<Revision>,
}

impl Entry {
    /// The latest moment anything changed.
    pub fn latest(&self) -> Hlc {
        self.common.latest().max(self.kind.latest())
    }

    /// Merges another copy of this entry into this one. Nothing secret is lost:
    /// values that lose move to history, and both histories are kept.
    pub fn merge_from(&mut self, other: &Self) {
        let mut lost = Vec::new();
        self.common.merge_from(&other.common, &mut lost);
        self.kind.merge_from(&other.kind, &mut lost);
        self.created_ms = self.created_ms.min(other.created_ms);
        for revision in other.history.iter().cloned().chain(lost) {
            if !self.history.contains(&revision) {
                self.history.push(revision);
            }
        }
        self.history.sort_by_key(|r| r.at);
    }

    /// The login body, if this is a login.
    pub fn login(&self) -> Option<&Login> {
        match &self.kind {
            Kind::Login(login) => Some(login),
            _ => None,
        }
    }

    /// The login body for editing, if this is a login.
    pub fn login_mut(&mut self) -> Option<&mut Login> {
        match &mut self.kind {
            Kind::Login(login) => Some(login),
            _ => None,
        }
    }

    /// When the password last changed, or `None` for kinds without one.
    pub fn password_changed(&self) -> Option<Hlc> {
        self.login().map(|login| login.password.at)
    }

    /// Writes a new password, keeping the old one in history.
    pub fn set_password(&mut self, password: Secret, at: Hlc) {
        let mut revision = None;
        if let Kind::Login(login) = &mut self.kind {
            if login.password.v == password {
                return;
            }
            if !login.password.v.is_empty() {
                revision = Some(Revision {
                    field: "password".to_owned(),
                    value: login.password.v.clone(),
                    at: login.password.at,
                });
            }
            login.password = Lww::new(password, at);
        }
        if let Some(revision) = revision {
            self.history.push(revision);
        }
    }

    /// Past values of a field.
    pub fn history_of<'a>(&'a self, field: &'a str) -> impl Iterator<Item = &'a Revision> {
        self.history.iter().filter(move |r| r.field == field)
    }

    /// Whether it is in the trash.
    pub fn is_trashed(&self) -> bool {
        self.common.deleted_at_ms.v.is_some()
    }
}

/// A folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    /// Random id.
    pub id: Id,
    /// Name.
    pub name: Lww<String>,
    /// Parent folder.
    pub parent: Lww<Option<String>>,
}

impl Folder {
    fn merge_from(&mut self, other: &Self) {
        let mut lost = Vec::new();
        self.name.merge_from(&other.name, "name", &mut lost);
        self.parent.merge_from(&other.parent, "parent", &mut lost);
    }
}

/// What a slot on this file is for, kept where only key holders can read it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotInfo {
    /// Whether the slot holds a live record.
    pub used: bool,
    /// The owner's label for it.
    pub label: String,
}

/// A reference from this vault to another file it unlocks. A decoy vault simply
/// has none of these pointing at the real ones.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkedVault {
    /// File name beside this one.
    pub file: String,
    /// Display name.
    pub name: String,
    /// The other file's key, as hex.
    pub key: Secret,
    /// Written at.
    pub at: Hlc,
}

/// A saved generator setting.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultPrefs {
    /// Days before the trash empties. Zero keeps things forever.
    pub trash_days: u32,
    /// Past values kept per field. Zero keeps them all.
    pub history_limit: u32,
}

/// The encrypted contents of a vault file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultDoc {
    /// Minor version of this layout. A newer one than this build knows opens
    /// read-only, because saving would drop fields it cannot see.
    pub minor: u32,
    /// Name.
    pub name: String,
    /// Rises with every save, so a replayed old file can be noticed.
    pub generation: u64,
    /// Items by id.
    pub entries: BTreeMap<Id, Entry>,
    /// Folders by id.
    pub folders: BTreeMap<Id, Folder>,
    /// Permanently deleted items and when.
    pub tombstones: BTreeMap<Id, Hlc>,
    /// Slot labels.
    pub slots: Vec<SlotInfo>,
    /// When slots last changed.
    pub slots_at: Hlc,
    /// Other files this vault unlocks.
    pub linked: Vec<LinkedVault>,
    /// Settings.
    pub prefs: VaultPrefs,
}

impl VaultDoc {
    /// Merges another copy of this vault in. Commutative and idempotent, so devices
    /// converge whatever order they sync in.
    pub fn merge_from(&mut self, other: &Self) {
        for (id, entry) in &other.entries {
            match self.entries.get_mut(id) {
                Some(mine) => mine.merge_from(entry),
                None => {
                    self.entries.insert(id.clone(), entry.clone());
                }
            }
        }
        for (id, folder) in &other.folders {
            match self.folders.get_mut(id) {
                Some(mine) => mine.merge_from(folder),
                None => {
                    self.folders.insert(id.clone(), folder.clone());
                }
            }
        }
        for (id, at) in &other.tombstones {
            let slot = self.tombstones.entry(id.clone()).or_default();
            *slot = (*slot).max(*at);
        }
        // A delete wins unless something on the entry changed after it.
        let tombstones = &self.tombstones;
        self.entries.retain(|id, entry| tombstones.get(id).is_none_or(|at| entry.latest() > *at));
        self.folders.retain(|id, _| !tombstones.contains_key(id));
        for link in &other.linked {
            match self.linked.iter_mut().find(|mine| mine.file == link.file) {
                Some(mine) if link.at > mine.at => *mine = link.clone(),
                Some(_) => {}
                None => self.linked.push(link.clone()),
            }
        }
        self.linked.sort_by(|a, b| a.file.cmp(&b.file));
        if other.slots_at > self.slots_at {
            self.slots = other.slots.clone();
            self.slots_at = other.slots_at;
        }
        self.generation = self.generation.max(other.generation);
        self.minor = self.minor.max(other.minor);
    }
}
