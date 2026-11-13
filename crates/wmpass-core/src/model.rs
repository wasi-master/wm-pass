//! Entries and everything hanging off them.
//!
//! Each field carries its own clock, so two devices editing different fields of
//! one entry both keep their change. When a secret field loses a merge, its value
//! moves to history instead of disappearing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Secret;
use crate::clock::Hlc;
use crate::format::SLOTS;
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
pub trait KeepsLoser: Clone + PartialEq {
    /// The value as text if losing it should leave a revision behind.
    fn loser(&self) -> Option<Secret> {
        None
    }

    /// Keeps what this value, overtaken by `winner`, would otherwise take with it. By default the
    /// whole value as one revision, when [`KeepsLoser::loser`] gives one. Lists of secrets keep
    /// each item `winner` no longer holds instead, so a list edited on two devices at once does
    /// not drop, key and all, what one of them added.
    fn keep_lost(&self, _winner: &Self, field: &'static str, at: Hlc, lost: &mut Vec<Revision>) {
        if let Some(value) = self.loser() {
            lost.push(Revision { field: field.to_owned(), value, at });
        }
    }

    /// Merges another device's write of the same field into this one. The later write wins and
    /// the earlier one becomes a revision, even when both hold the same value. A revision is
    /// therefore "a write that was overtaken", and the set of them does not depend on the order
    /// devices sync in.
    fn merge_lww(mine: &mut Lww<Self>, theirs: &Lww<Self>, field: &'static str, lost: &mut Vec<Revision>) {
        if theirs.at > mine.at {
            mine.v.keep_lost(&theirs.v, field, mine.at, lost);
            *mine = theirs.clone();
        } else if theirs.at < mine.at {
            theirs.v.keep_lost(&mine.v, field, theirs.at, lost);
        }
    }
}

macro_rules! plain_fields {
    ($($ty:ty),* $(,)?) => { $(impl KeepsLoser for $ty {})* };
}

plain_fields!(
    String,
    bool,
    u32,
    u64,
    Option<String>,
    Option<u64>,
    Vec<String>,
    Vec<SiteMatcher>,
    Option<OtpConfig>,
    Reminders,
    Option<AliasRef>,
    Option<u16>,
    WifiSecurity,
    RecoveryForm,
);

impl KeepsLoser for Secret {
    fn loser(&self) -> Option<Secret> {
        (!self.is_empty()).then(|| self.clone())
    }
}

/// How a revision names a list item that lost a merge: `custom field PIN`, say.
fn item_label(field: &str) -> &str {
    match field {
        "custom_fields" => "custom field",
        "questions" => "security question",
        "document_numbers" => "document number",
        "attachments" => "attachment",
        "passkeys" => "passkey",
        other => other,
    }
}

/// An item of a list that lost, as JSON in a revision, so its key is not lost with it.
fn item_revision<T: Serialize>(field: &str, name: &str, item: &T, at: Hlc) -> Option<Revision> {
    let value = serde_json::to_string(item).ok()?;
    Some(Revision { field: format!("{} {name}", item_label(field)), value: Secret::new(value), at })
}

impl KeepsLoser for Vec<Field> {
    fn keep_lost(&self, winner: &Self, field: &'static str, at: Hlc, lost: &mut Vec<Revision>) {
        for item in self {
            if !item.value.is_empty() && !winner.iter().any(|w| w.name == item.name && w.value == item.value) {
                lost.push(Revision { field: format!("{} {}", item_label(field), item.name), value: item.value.clone(), at });
            }
        }
    }
}

impl KeepsLoser for Vec<Attachment> {
    fn keep_lost(&self, winner: &Self, field: &'static str, at: Hlc, lost: &mut Vec<Revision>) {
        for item in self.iter().filter(|item| !winner.iter().any(|w| w.blob == item.blob)) {
            lost.extend(item_revision(field, &item.name, item, at));
        }
    }
}

impl KeepsLoser for Vec<Passkey> {
    fn keep_lost(&self, winner: &Self, field: &'static str, at: Hlc, lost: &mut Vec<Revision>) {
        for item in self.iter().filter(|item| !winner.iter().any(|w| w.credential_id == item.credential_id)) {
            lost.extend(item_revision(field, &item.rp_id, item, at));
        }
    }

    /// Passkeys are only ever added, and a counter moves on with every sign-in, so the list is
    /// a union by credential: a sign-in on one device must not drop a passkey made on another
    /// meanwhile, which would lock the owner out of that site. The higher counter wins.
    fn merge_lww(mine: &mut Lww<Self>, theirs: &Lww<Self>, _field: &'static str, _lost: &mut Vec<Revision>) {
        if mine.v == theirs.v {
            mine.at = mine.at.max(theirs.at);
            return;
        }
        let (newer, older) = if theirs.at > mine.at { (theirs, &*mine) } else { (&*mine, theirs) };
        let mut keys = newer.v.clone();
        for key in &older.v {
            match keys.iter_mut().find(|k| k.credential_id == key.credential_id) {
                Some(held) if key.counter > held.counter => *held = key.clone(),
                Some(_) => {}
                None => keys.push(key.clone()),
            }
        }
        // One order whichever side merges, so two devices holding the same keys agree.
        keys.sort_by(|a, b| (a.created_ms, &a.credential_id).cmp(&(b.created_ms, &b.credential_id)));
        *mine = Lww::new(keys, newer.at.max(older.at));
    }
}

impl<T: KeepsLoser> Lww<T> {
    /// Merges another write of this field in, by the rule its type keeps.
    fn merge_from(&mut self, other: &Self, field: &'static str, lost: &mut Vec<Revision>) {
        T::merge_lww(self, other, field, lost);
    }
}

impl<T: Clone + Default + PartialEq + KeepsLoser> Lww<T> {
    /// For two different items folded into one: a field both wrote at the same moment, as one
    /// import stamps every item alike, is a tie the merge above leaves alone. Here an empty
    /// value takes the other's, written at `at` so every copy of the vault takes it too, and a
    /// secret that differs is kept as a revision.
    fn fill_tie(&mut self, other: &Self, field: &'static str, lost: &mut Vec<Revision>, at: Hlc) {
        if other.at != self.at || other.v == self.v || other.v == T::default() {
            return;
        }
        if self.v == T::default() {
            *self = Self::new(other.v.clone(), at);
        } else if let Some(value) = other.v.loser() {
            lost.push(Revision {
                field: field.to_owned(),
                value,
                at: other.at,
            });
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

            /// Every secret it holds as a revision, for when the whole body loses a merge.
            #[allow(dead_code)]
            fn keep_secrets(&self, lost: &mut Vec<Revision>) {
                $(self.$field.v.keep_lost(&<$ty>::default(), stringify!($field), self.$field.at, lost);)*
            }

            fn fill_ties(&mut self, other: &Self, lost: &mut Vec<Revision>, at: Hlc) {
                $(self.$field.fill_tie(&other.$field, stringify!($field), lost, at);)*
            }

            fn clear_up_to(&mut self, moment: Hlc) {
                $(if self.$field.at <= moment {
                    self.$field = Lww::default();
                })*
            }

            /// The latest moment any field changed.
            pub fn latest(&self) -> Hlc {
                let mut latest = Hlc::default();
                $(latest = latest.max(self.$field.at);)*
                latest
            }

            /// Every field's write time.
            pub fn stamps(&self, out: &mut Vec<Hlc>) {
                $(out.push(self.$field.at);)*
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

    fn stamps(&self, out: &mut Vec<Hlc>) {
        match self {
            Self::Login(v) => v.stamps(out),
            Self::Card(v) => v.stamps(out),
            Self::Identity(v) => v.stamps(out),
            Self::Note(v) => v.stamps(out),
            Self::Document(v) => v.stamps(out),
            Self::SshKey(v) => v.stamps(out),
            Self::ApiKey(v) => v.stamps(out),
            Self::Database(v) => v.stamps(out),
            Self::Licence(v) => v.stamps(out),
            Self::Recovery(v) => v.stamps(out),
            Self::Wifi(v) => v.stamps(out),
            Self::Otp(v) => v.stamps(out),
        }
    }

    fn clear_up_to(&mut self, moment: Hlc) {
        match self {
            Self::Login(v) => v.clear_up_to(moment),
            Self::Card(v) => v.clear_up_to(moment),
            Self::Identity(v) => v.clear_up_to(moment),
            Self::Note(v) => v.clear_up_to(moment),
            Self::Document(v) => v.clear_up_to(moment),
            Self::SshKey(v) => v.clear_up_to(moment),
            Self::ApiKey(v) => v.clear_up_to(moment),
            Self::Database(v) => v.clear_up_to(moment),
            Self::Licence(v) => v.clear_up_to(moment),
            Self::Recovery(v) => v.clear_up_to(moment),
            Self::Wifi(v) => v.clear_up_to(moment),
            Self::Otp(v) => v.clear_up_to(moment),
        }
    }

    fn fill_ties(&mut self, other: &Self, lost: &mut Vec<Revision>, at: Hlc) {
        match (&mut *self, other) {
            (Self::Login(a), Self::Login(b)) => a.fill_ties(b, lost, at),
            (Self::Card(a), Self::Card(b)) => a.fill_ties(b, lost, at),
            (Self::Identity(a), Self::Identity(b)) => a.fill_ties(b, lost, at),
            (Self::Note(a), Self::Note(b)) => a.fill_ties(b, lost, at),
            (Self::Document(a), Self::Document(b)) => a.fill_ties(b, lost, at),
            (Self::SshKey(a), Self::SshKey(b)) => a.fill_ties(b, lost, at),
            (Self::ApiKey(a), Self::ApiKey(b)) => a.fill_ties(b, lost, at),
            (Self::Database(a), Self::Database(b)) => a.fill_ties(b, lost, at),
            (Self::Licence(a), Self::Licence(b)) => a.fill_ties(b, lost, at),
            (Self::Recovery(a), Self::Recovery(b)) => a.fill_ties(b, lost, at),
            (Self::Wifi(a), Self::Wifi(b)) => a.fill_ties(b, lost, at),
            (Self::Otp(a), Self::Otp(b)) => a.fill_ties(b, lost, at),
            _ => {}
        }
    }

    fn keep_secrets(&self, lost: &mut Vec<Revision>) {
        match self {
            Self::Login(v) => v.keep_secrets(lost),
            Self::Card(v) => v.keep_secrets(lost),
            Self::Identity(v) => v.keep_secrets(lost),
            Self::Note(v) => v.keep_secrets(lost),
            Self::Document(v) => v.keep_secrets(lost),
            Self::SshKey(v) => v.keep_secrets(lost),
            Self::ApiKey(v) => v.keep_secrets(lost),
            Self::Database(v) => v.keep_secrets(lost),
            Self::Licence(v) => v.keep_secrets(lost),
            Self::Recovery(v) => v.keep_secrets(lost),
            Self::Wifi(v) => v.keep_secrets(lost),
            Self::Otp(v) => v.keep_secrets(lost),
        }
    }

    /// Merges two bodies of the same kind by field. If the kind itself changed on
    /// one side, the more recently edited body wins whole, and the secrets of the
    /// other move to history.
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
                    self.keep_secrets(lost);
                    *self = other.clone();
                } else if other.latest() < self.latest() {
                    other.keep_secrets(lost);
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

    /// Every field's write time, for telling whose clock is off.
    pub fn stamps(&self, out: &mut Vec<Hlc>) {
        self.common.stamps(out);
        self.kind.stamps(out);
    }

    /// Merges another copy of this entry into this one. Nothing secret is lost:
    /// values that lose move to history, and both histories are kept.
    pub fn merge_from(&mut self, other: &Self) {
        let mut lost = Vec::new();
        self.common.merge_from(&other.common, &mut lost);
        self.kind.merge_from(&other.kind, &mut lost);
        // Zero is a copy that never learned when it was made, not the earliest moment.
        self.created_ms = match (self.created_ms, other.created_ms) {
            (0, known) | (known, 0) => known,
            (mine, theirs) => mine.min(theirs),
        };
        for revision in other.history.iter().cloned().chain(lost) {
            if !self.history.contains(&revision) {
                self.history.push(revision);
            }
        }
        self.sort_history();
    }

    /// Folds a different item into this one, as clearing a duplicate does: [`Entry::merge_from`],
    /// and then for each field the two wrote at the same moment, an empty one here takes the
    /// other's value and a secret of the other's that differs goes to history. Without this, two
    /// items from one import, every field stamped alike, would lose whatever the other held.
    pub fn absorb_item(&mut self, other: &Self, at: Hlc) {
        self.merge_from(other);
        let mut lost = Vec::new();
        self.common.fill_ties(&other.common, &mut lost, at);
        self.kind.fill_ties(&other.kind, &mut lost, at);
        for revision in lost {
            if !self.history.contains(&revision) {
                self.history.push(revision);
            }
        }
        self.sort_history();
    }

    fn sort_history(&mut self) {
        self.history.sort_by(|a, b| {
            (a.at, &a.field, a.value.expose()).cmp(&(b.at, &b.field, b.value.expose()))
        });
    }

    /// Removes everything a delete at `moment` covers, leaving only what was written
    /// after it. Without this, a stale device's one later edit would bring the whole
    /// deleted entry back, secrets included.
    fn clear_up_to(&mut self, moment: Hlc) {
        self.common.clear_up_to(moment);
        self.kind.clear_up_to(moment);
        self.history.retain(|revision| revision.at > moment);
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
    /// Whether its password opens the vault: false for a duress password, and none for a
    /// slot written before this was kept, until the vault is next opened with it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opens: Option<bool>,
    /// When this slot was last written. Zero for one written before slots kept their own time:
    /// its time is then the whole list's, [`VaultDoc::slots_at`].
    #[serde(skip_serializing_if = "Hlc::is_zero")]
    pub at: Hlc,
    /// Which password this one came from by changes: the time the first of the line was added.
    /// Zero for a slot from before this was kept. Two live slots of one line are one password
    /// changed on two devices at once.
    #[serde(skip_serializing_if = "Hlc::is_zero")]
    pub line: Hlc,
    /// The times of the slots this line's changes replaced, newest last, a few at most: a slot
    /// whose time is here was replaced by this one, not changed alongside it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replaced: Vec<Hlc>,
}

/// How many replaced slots [`SlotInfo::replaced`] remembers.
pub const SLOT_ANCESTRY: usize = 16;

impl SlotInfo {
    /// Whether it holds a password that opens the vault.
    pub fn opens_vault(&self) -> bool {
        self.used && self.opens == Some(true)
    }

    /// Whether this and `other`, written at `at` and `other_at`, are two changes of one password
    /// made without either device seeing the other's.
    fn changed_alongside(&self, at: Hlc, other: &Self, other_at: Hlc) -> bool {
        self.opens_vault()
            && other.opens_vault()
            && !self.line.is_zero()
            && self.line == other.line
            && at != other_at
            && !self.replaced.contains(&other_at)
            && !other.replaced.contains(&at)
    }
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
    /// Set when the other file is a vault shared with other people.
    pub shared: Option<SharedLink>,
}

impl LinkedVault {
    /// A shared vault's newest version seen, and the key that goes with it, never go back in a
    /// merge. Two devices that each saw a newer manifest write the link at their own moments;
    /// without this the later write, with the older version, would win, and the floor that
    /// refuses a replayed older manifest would drop on every device.
    fn keep_highest_version(&mut self, other: &Self) {
        if let (Some(mine), Some(theirs)) = (self.shared.as_mut(), other.shared.as_ref())
            && theirs.min_version > mine.min_version
        {
            mine.min_version = theirs.min_version;
            mine.vault_id.clone_from(&theirs.vault_id);
            self.key = other.key.clone();
        }
    }
}

/// This person's place in a shared vault. Their keys are kept here, in their own vault.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SharedLink {
    /// The owner's signing key from the invitation, as hex. Empty until an invitation arrives.
    pub owner_key: String,
    /// The shared file's id as last seen, as hex.
    pub vault_id: String,
    /// The highest manifest version seen, so an older one can be refused.
    pub min_version: u64,
    /// This person's keys for this vault, as hex: wrapping key, then signing key.
    pub member: Secret,
    /// Whether this person owns it, and so can change who is on it.
    pub owner: bool,
}

/// A saved generator setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultPrefs {
    /// Days before the trash empties. Zero keeps things forever.
    pub trash_days: u32,
    /// When `trash_days` was set, so the latest choice reaches every device. Without it each
    /// device kept its own, and one still on the default emptied the trash of a vault set to
    /// keep it. Zero for a vault from before this was kept.
    pub trash_days_at: Hlc,
    /// Past values kept per field. Zero keeps them all.
    pub history_limit: u32,
    /// When `history_limit` was set.
    pub history_limit_at: Hlc,
    /// The glyph each tag is drawn with, by tag name. Empty means the automatic one. Kept in the
    /// vault rather than the app's settings because tag names say what is in it.
    pub tag_icons: BTreeMap<String, Lww<String>>,
    /// The apps' own settings that follow the vault to every device, by key, each value as JSON
    /// text. Inside the vault so any sync carries them and the file says nothing about them. An
    /// empty value is a setting some device took back out.
    pub app: BTreeMap<String, Lww<String>>,
    /// Disposable mailboxes kept between visits, by id. `None` is one that was closed.
    pub inboxes: BTreeMap<String, Lww<Option<Inbox>>>,
    /// What each device that opened the vault calls itself, by its number in decimal, so a
    /// warning about a device's clock can name it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub devices: BTreeMap<String, Lww<String>>,
}

/// Merges two maps of moments: each key keeps the later.
fn merge_max(mine: &mut BTreeMap<Id, Hlc>, theirs: &BTreeMap<Id, Hlc>) {
    for (id, at) in theirs {
        let held = mine.entry(id.clone()).or_default();
        *held = (*held).max(*at);
    }
}

/// Merges two maps of values set one key at a time: for each key the later write wins.
fn merge_keyed<T: Clone>(mine: &mut BTreeMap<String, Lww<T>>, theirs: &BTreeMap<String, Lww<T>>) {
    for (key, value) in theirs {
        match mine.get_mut(key) {
            Some(held) if value.at > held.at => *held = value.clone(),
            Some(_) => {}
            None => {
                mine.insert(key.clone(), value.clone());
            }
        }
    }
}

/// A disposable mailbox: where it lives and how to get back in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Inbox {
    /// `mailtm`, `mailgw`, `custom` (a server with the Mail.tm API), `guerrilla` or `inbucket`.
    pub provider: String,
    /// The server, for the kinds that need one.
    pub base: String,
    /// The address.
    pub address: String,
    /// What the owner calls it.
    pub label: String,
    /// The service's session token.
    pub token: Secret,
    /// The mailbox's own password, where tokens run out and it has to sign in again.
    pub password: Secret,
    /// The service's id for the mailbox, for deleting it.
    pub account: String,
    /// When it was made.
    pub created_ms: u64,
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
    /// For each item emptied from the trash, when its place in or out of the trash was last
    /// written as the device that emptied it saw it. An item put back from the trash (or into
    /// it again) after that, on a device that had not heard of the delete, is kept whole: the
    /// delete only removed the item as it was then. Empty for deletes from before this was kept,
    /// which keep the older rule, that only what was written after the delete survives.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub purge_seen: BTreeMap<Id, Hlc>,
    /// Slot labels.
    pub slots: Vec<SlotInfo>,
    /// When slots last changed.
    pub slots_at: Hlc,
    /// Set when two devices changed one password at once and both changes landed in the same
    /// slot, so only the later one still opens the vault: the later one's time. Shown to the
    /// owner until [`VaultDoc::password_dropped_seen`] reaches it.
    #[serde(skip_serializing_if = "Hlc::is_zero")]
    pub password_dropped: Hlc,
    /// The latest [`VaultDoc::password_dropped`] the owner has been told about.
    #[serde(skip_serializing_if = "Hlc::is_zero")]
    pub password_dropped_seen: Hlc,
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
        merge_max(&mut self.tombstones, &other.tombstones);
        merge_max(&mut self.purge_seen, &other.purge_seen);
        self.apply_tombstones();
        let tombstones = &self.tombstones;
        self.folders.retain(|id, _| !tombstones.contains_key(id));
        for link in &other.linked {
            match self.linked.iter_mut().find(|mine| mine.file == link.file) {
                Some(mine) if link.at > mine.at => {
                    let before = std::mem::replace(mine, link.clone());
                    mine.keep_highest_version(&before);
                }
                Some(mine) => mine.keep_highest_version(link),
                None => self.linked.push(link.clone()),
            }
        }
        self.linked.sort_by(|a, b| a.file.cmp(&b.file));
        self.merge_slots(other);
        // A folder that is gone lets go of what was put in it, on another device meanwhile,
        // rather than leave it hanging from nothing and out of every list. Every copy does the
        // same, the write time unchanged, so they still agree.
        for folder in self.folders.values_mut() {
            if folder.parent.v.as_ref().is_some_and(|parent| self.tombstones.contains_key(parent)) {
                folder.parent.v = None;
            }
        }
        for entry in self.entries.values_mut() {
            if entry.common.folder.v.as_ref().is_some_and(|folder| self.tombstones.contains_key(folder)) {
                entry.common.folder.v = None;
            }
        }
        if other.prefs.trash_days_at > self.prefs.trash_days_at {
            self.prefs.trash_days = other.prefs.trash_days;
            self.prefs.trash_days_at = other.prefs.trash_days_at;
        }
        if other.prefs.history_limit_at > self.prefs.history_limit_at {
            self.prefs.history_limit = other.prefs.history_limit;
            self.prefs.history_limit_at = other.prefs.history_limit_at;
        }
        merge_keyed(&mut self.prefs.tag_icons, &other.prefs.tag_icons);
        merge_keyed(&mut self.prefs.app, &other.prefs.app);
        merge_keyed(&mut self.prefs.inboxes, &other.prefs.inboxes);
        merge_keyed(&mut self.prefs.devices, &other.prefs.devices);
        self.generation = self.generation.max(other.generation);
        self.minor = self.minor.max(other.minor);
    }

    /// Removes what deletes cover. An item emptied from the trash goes, unless it was put back
    /// (or in the trash again) after the delete last saw it: then a device that had not heard
    /// of the delete restored it, and it stays whole, so restoring always wins over emptying the
    /// trash elsewhere at the same time. Otherwise a delete covers every write before it and
    /// only later writes survive. Each copy decides from what it holds, and a later write only
    /// ever raises what an item holds, so every order of syncing ends the same.
    pub fn apply_tombstones(&mut self) {
        let (tombstones, seen) = (&self.tombstones, &self.purge_seen);
        self.entries.retain(|id, entry| {
            let Some(deleted) = tombstones.get(id) else {
                return true;
            };
            if seen.get(id).is_some_and(|seen| entry.common.deleted_at_ms.at > *seen) {
                return true;
            }
            if entry.latest() <= *deleted {
                return false;
            }
            entry.clear_up_to(*deleted);
            true
        });
    }

    /// When slot `index` was last written: its own time, or for a slot from before slots kept
    /// their own, the list's.
    pub fn slot_time(&self, index: usize) -> Hlc {
        match self.slots.get(index) {
            Some(slot) if !slot.at.is_zero() => slot.at,
            _ => self.slots_at,
        }
    }

    /// The slots `other` wrote later than this copy did, which a merge takes from it. Slots
    /// merge one by one, the later write of each winning, so a way in added on one device and
    /// a password changed on another both stay. Lists from before slots kept their own times
    /// all carry the list's time, and merge whole as they used to.
    pub fn newer_slots(&self, other: &Self) -> Vec<usize> {
        (0..SLOTS).filter(|&index| other.slot_time(index) > self.slot_time(index)).collect()
    }

    fn merge_slots(&mut self, other: &Self) {
        // Two changes of one password that both went into one slot: the later stays and the
        // other no longer opens anything. Every copy that sees both notes it, and the note
        // travels with the vault.
        for index in 0..SLOTS {
            if let (Some(mine), Some(theirs)) = (self.slots.get(index), other.slots.get(index))
                && mine.changed_alongside(self.slot_time(index), theirs, other.slot_time(index))
            {
                let later = self.slot_time(index).max(other.slot_time(index));
                self.password_dropped = self.password_dropped.max(later);
            }
        }
        let newer = self.newer_slots(other);
        if !newer.is_empty() {
            if self.slots.len() < SLOTS {
                self.slots.resize(SLOTS, SlotInfo::default());
            }
            // Before the list's time moves, each slot here takes the time it had from it.
            for slot in &mut self.slots {
                if slot.at.is_zero() {
                    slot.at = self.slots_at;
                }
            }
            for index in newer {
                let mut slot = other.slots.get(index).cloned().unwrap_or_default();
                slot.at = other.slot_time(index);
                self.slots[index] = slot;
            }
        }
        self.slots_at = self.slots_at.max(other.slots_at);
        self.password_dropped = self.password_dropped.max(other.password_dropped);
        self.password_dropped_seen = self.password_dropped_seen.max(other.password_dropped_seen);
    }

    /// The slots holding passwords changed from one on two devices at once, each of which
    /// still opens the vault. Empty once all but one of them is removed. Duress passwords and
    /// slots from before this was kept are never among them.
    pub fn password_conflict(&self) -> Vec<usize> {
        let live: Vec<(usize, &SlotInfo)> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.opens_vault() && !slot.line.is_zero())
            .collect();
        live.iter()
            .filter(|(index, slot)| live.iter().any(|(other, them)| other != index && them.line == slot.line))
            .map(|(index, _)| *index)
            .collect()
    }

    /// Whether a change of password was lost to another made at the same time, and the owner
    /// has not been told yet.
    pub fn password_dropped(&self) -> bool {
        self.password_dropped > self.password_dropped_seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_creation_time_does_not_win_a_merge() {
        let at = Hlc { ms: 1000, ..Hlc::default() };
        let mut dated = crate::vault::login_entry("Mail", "me", "pw", at);
        dated.created_ms = 5000;
        let mut undated = dated.clone();
        undated.created_ms = 0;
        let (mut a, mut b) = (dated.clone(), undated.clone());
        a.merge_from(&undated);
        b.merge_from(&dated);
        assert_eq!((a.created_ms, b.created_ms), (5000, 5000));
    }

    #[test]
    fn a_body_of_another_kind_that_loses_a_merge_keeps_its_secrets() {
        let (old, new) = (Hlc { ms: 1000, ..Hlc::default() }, Hlc { ms: 2000, ..Hlc::default() });
        let login = crate::vault::login_entry("Mail", "me", "hunter2", old);
        let mut note = login.clone();
        note.kind = Kind::Note(Note { body: Lww::new(Secret::new("now a note"), new) });
        let kept = |merged: &Entry| merged.history_of("password").map(|r| r.value.expose().to_owned()).collect::<Vec<_>>();
        let (mut a, mut b) = (login.clone(), note.clone());
        a.merge_from(&note);
        b.merge_from(&login);
        assert!(matches!(a.kind, Kind::Note(_)) && matches!(b.kind, Kind::Note(_)));
        assert_eq!(kept(&a), vec!["hunter2"]);
        assert_eq!(a.history, b.history, "either order keeps the same");
    }

    #[test]
    fn a_shared_vaults_newest_version_never_goes_back_in_a_merge() {
        let link = |version: u64, at_ms: u64| LinkedVault {
            file: "shared".into(),
            name: "Family".into(),
            key: Secret::new(format!("key-{version}")),
            at: Hlc { ms: at_ms, ..Hlc::default() },
            shared: Some(SharedLink { min_version: version, vault_id: format!("id-{version}"), ..SharedLink::default() }),
        };
        // One device saw version 5, another version 4 a moment later.
        let (five, four) = (VaultDoc { linked: vec![link(5, 1000)], ..VaultDoc::default() }, VaultDoc { linked: vec![link(4, 2000)], ..VaultDoc::default() });
        let (mut a, mut b) = (five.clone(), four.clone());
        a.merge_from(&four);
        b.merge_from(&five);
        for doc in [&a, &b] {
            let shared = doc.linked[0].shared.as_ref().unwrap();
            assert_eq!((shared.min_version, shared.vault_id.as_str(), doc.linked[0].key.expose()), (5, "id-5", "key-5"));
        }
        assert_eq!(a, b);
        // And the device that wrote version 4 comes round too.
        let mut c = four.clone();
        c.merge_from(&a);
        assert_eq!(c, a);
    }
}
