//! What a shell may see. Nothing here holds a secret value.

use serde::{Deserialize, Serialize};
use wmpass_core::format::FactorKind;
use wmpass_core::model::{Entry, FieldKind, Kind, SiteMatcher};
use wmpass_core::vault::hex;
use wmpass_core::{health, otp};

use crate::fields::{self, FieldSpec, Shown};

/// One row in a list.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Summary {
    /// Entry id.
    pub id: String,
    /// Kind name.
    pub kind: &'static str,
    /// Title.
    pub title: String,
    /// One safe line about it.
    pub subtitle: String,
    /// Registrable domain of its first site, for grouping and icons.
    pub site: Option<String>,
    /// Package of the first Android app it is for, for the app's icon.
    pub app: Option<String>,
    /// Icon override.
    pub icon: Option<String>,
    /// Colour label.
    pub color: Option<String>,
    /// Folder id.
    pub folder: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
    /// Pinned to the top.
    pub pinned: bool,
    /// Favourite.
    pub favourite: bool,
    /// On the hidden shelf.
    pub hidden: bool,
    /// In the trash.
    pub trashed: bool,
    /// Whether there is a password to copy.
    pub has_password: bool,
    /// Whether it makes one-time codes.
    pub has_otp: bool,
    /// Whether it holds passkeys.
    pub has_passkey: bool,
    /// Number of attachments.
    pub attachments: usize,
    /// When anything last changed.
    pub modified_ms: u64,
    /// When it destroys itself.
    pub destroy_at_ms: Option<u64>,
}

/// Summarises an entry.
pub fn summary(entry: &Entry) -> Summary {
    let (has_password, has_otp, has_passkey) = match &entry.kind {
        Kind::Login(login) => (
            !login.password.v.is_empty(),
            login.otp.v.is_some(),
            !login.passkeys.v.is_empty(),
        ),
        Kind::Otp(otp) => (false, otp.config.v.is_some(), false),
        Kind::Database(db) => (!db.password.v.is_empty(), false, false),
        Kind::Wifi(wifi) => (!wifi.password.v.is_empty(), false, false),
        _ => (false, false, false),
    };
    Summary {
        id: entry.id.clone(),
        kind: entry.kind.name(),
        title: entry.common.title.v.clone(),
        subtitle: fields::subtitle(entry),
        site: health::entry_domains(entry).into_iter().next(),
        app: entry.login().and_then(|login| {
            login.site_matchers.v.iter().find_map(|matcher| match wmpass_core::matching::normalized_matcher(matcher) {
                SiteMatcher::AndroidApp { package, .. } => Some(package),
                _ => None,
            })
        }),
        icon: entry.common.icon.v.clone(),
        color: entry.common.color.v.clone(),
        folder: entry.common.folder.v.clone(),
        tags: entry.common.tags.v.clone(),
        pinned: entry.common.pinned.v,
        favourite: entry.common.favourite.v,
        hidden: entry.common.hidden.v,
        trashed: entry.is_trashed(),
        has_password,
        has_otp,
        has_passkey,
        attachments: entry.common.attachments.v.len(),
        modified_ms: entry.latest().ms,
        destroy_at_ms: entry.common.destroy_at_ms.v,
    }
}

/// A field as an editor or detail screen draws it.
#[derive(Clone, Debug, Serialize)]
pub struct FieldView {
    /// The table row.
    #[serde(flatten)]
    pub spec: FieldSpec,
    /// Whether it has a value.
    pub has_value: bool,
    /// The value, always absent for a secret.
    pub value: Option<Shown>,
}

/// A custom field.
#[derive(Clone, Debug, Serialize)]
pub struct CustomFieldView {
    /// Position.
    pub index: usize,
    /// Label.
    pub name: String,
    /// `text`, `hidden`, `url`, `email`, `phone`, `date` or `toggle`.
    pub kind: &'static str,
    /// Whether it has a value.
    pub has_value: bool,
    /// The value, absent for a hidden field.
    pub value: Option<String>,
}

/// The name of a custom field's kind.
pub fn field_kind_name(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Text => "text",
        FieldKind::Hidden => "hidden",
        FieldKind::Url => "url",
        FieldKind::Email => "email",
        FieldKind::Phone => "phone",
        FieldKind::Date => "date",
        FieldKind::Toggle => "toggle",
    }
}

/// Parses a custom field's kind.
pub fn field_kind_from(name: &str) -> FieldKind {
    match name {
        "hidden" => FieldKind::Hidden,
        "url" => FieldKind::Url,
        "email" => FieldKind::Email,
        "phone" => FieldKind::Phone,
        "date" => FieldKind::Date,
        "toggle" => FieldKind::Toggle,
        _ => FieldKind::Text,
    }
}

/// An attachment's description.
#[derive(Clone, Debug, Serialize)]
pub struct AttachmentView {
    /// Blob file name.
    pub blob: String,
    /// Original file name.
    pub name: String,
    /// Media type.
    pub mime: String,
    /// Plain size in bytes.
    pub size: u64,
}

/// A passkey's public details.
#[derive(Clone, Debug, Serialize)]
pub struct PasskeyView {
    /// Base64url credential id.
    pub credential_id: String,
    /// Relying party id.
    pub rp_id: String,
    /// Relying party name.
    pub rp_name: String,
    /// User's display name.
    pub user_name: String,
    /// Created, in milliseconds.
    pub created_ms: u64,
}

/// A one-time code's settings, without its seed.
#[derive(Clone, Debug, Serialize)]
pub struct OtpView {
    /// Issuer.
    pub issuer: String,
    /// Account.
    pub account: String,
    /// `totp`, `hotp` or `steam`.
    pub kind: &'static str,
    /// Digits.
    pub digits: u32,
    /// Seconds per code, for time-based ones.
    pub period: Option<u32>,
}

/// Describes OTP settings without the seed.
pub fn otp_view(config: &otp::OtpConfig) -> OtpView {
    let (kind, period) = match config.kind {
        otp::OtpKind::Totp { period } => ("totp", Some(period)),
        otp::OtpKind::Hotp { .. } => ("hotp", None),
        otp::OtpKind::Steam => ("steam", Some(30)),
    };
    OtpView {
        issuer: config.issuer.clone(),
        account: config.account.clone(),
        kind,
        digits: config.digits,
        period,
    }
}

/// A past value's description.
#[derive(Clone, Debug, Serialize)]
pub struct RevisionView {
    /// Position, for revealing it.
    pub index: usize,
    /// Which field.
    pub field: String,
    /// When that value was written.
    pub at_ms: u64,
}

/// An entry's reminders.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RemindersView {
    /// Change every this many days.
    pub change_every_days: Option<u32>,
    /// Fixed expiry.
    pub expires_ms: Option<u64>,
    /// Warn this many days ahead.
    pub warn_days: u32,
}

/// Everything about one entry except secret values.
#[derive(Clone, Debug, Serialize)]
pub struct Detail {
    /// The list row.
    #[serde(flatten)]
    pub summary: Summary,
    /// When it was created.
    pub created_ms: u64,
    /// Kind-specific fields, in editor order.
    pub fields: Vec<FieldView>,
    /// User-defined fields.
    pub custom_fields: Vec<CustomFieldView>,
    /// Whether the free-text notes hold anything.
    pub has_notes: bool,
    /// Where it applies.
    pub sites: Vec<SiteMatcher>,
    /// One-time code settings.
    pub otp: Option<OtpView>,
    /// Passkeys.
    pub passkeys: Vec<PasskeyView>,
    /// Attachments.
    pub attachment_list: Vec<AttachmentView>,
    /// Ids of linked entries.
    pub links: Vec<String>,
    /// Reminders.
    pub reminders: RemindersView,
    /// Past values.
    pub history: Vec<RevisionView>,
    /// Email alias address.
    pub alias: Option<String>,
    /// Security questions, by label. Answers are secret.
    pub questions: Vec<String>,
}

/// Builds the detail view.
pub fn detail(entry: &Entry) -> Detail {
    // The field table's reader takes `&mut` because writes share its lookup. A
    // clone here is dropped and wiped at the end of this function.
    let mut scratch = entry.clone();
    let spec = fields::fields_of(entry.kind.name());
    let field_views = spec
        .iter()
        .filter_map(|row| {
            let (value, has_value) = fields::shown(&mut scratch, row.key)?;
            Some(FieldView {
                spec: *row,
                has_value,
                value: if row.secret { None } else { value },
            })
        })
        .collect();
    let custom_fields = entry
        .common
        .custom_fields
        .v
        .iter()
        .enumerate()
        .map(|(index, field)| CustomFieldView {
            index,
            name: field.name.clone(),
            kind: field_kind_name(field.kind),
            has_value: !field.value.is_empty(),
            value: (field.kind != FieldKind::Hidden).then(|| field.value.expose().to_owned()),
        })
        .collect();
    let (sites, otp, passkeys, alias, questions) = match &entry.kind {
        Kind::Login(login) => (
            // An app saved as a website address, as some exports do, shows as the app.
            login.site_matchers.v.iter().map(wmpass_core::matching::normalized_matcher).collect(),
            login.otp.v.as_ref().map(otp_view),
            login
                .passkeys
                .v
                .iter()
                .map(|p| PasskeyView {
                    credential_id: p.credential_id.clone(),
                    rp_id: p.rp_id.clone(),
                    rp_name: p.rp_name.clone(),
                    user_name: p.user_name.clone(),
                    created_ms: p.created_ms,
                })
                .collect(),
            login.alias.v.as_ref().map(|a| a.address.clone()),
            login.questions.v.iter().map(|q| q.name.clone()).collect(),
        ),
        Kind::Otp(otp) => (
            Vec::new(),
            otp.config.v.as_ref().map(otp_view),
            Vec::new(),
            None,
            Vec::new(),
        ),
        _ => (Vec::new(), None, Vec::new(), None, Vec::new()),
    };
    Detail {
        summary: summary(entry),
        created_ms: entry.created_ms,
        fields: field_views,
        custom_fields,
        has_notes: !entry.common.notes.v.is_empty(),
        sites,
        otp,
        passkeys,
        attachment_list: entry
            .common
            .attachments
            .v
            .iter()
            .map(|a| AttachmentView {
                blob: a.blob.clone(),
                name: a.name.clone(),
                mime: a.mime.clone(),
                size: a.size,
            })
            .collect(),
        links: entry.common.links.v.clone(),
        reminders: RemindersView {
            change_every_days: entry.common.reminders.v.change_every_days,
            expires_ms: entry.common.reminders.v.expires_ms,
            warn_days: entry.common.reminders.v.warn_days,
        },
        history: entry
            .history
            .iter()
            .enumerate()
            .map(|(index, r)| RevisionView {
                index,
                field: r.field.clone(),
                at_ms: r.at.ms,
            })
            .collect(),
        alias,
        questions,
    }
}

/// A second factor a slot asks for.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FactorView {
    /// A key file.
    KeyFile,
    /// A FIDO2 key.
    Fido2 {
        /// Credential id, hex.
        credential_id: String,
        /// hmac-secret salt, hex.
        salt: String,
    },
    /// A YubiKey challenge-response slot.
    Yubikey {
        /// Challenge, hex.
        challenge: String,
        /// OTP slot.
        slot: u8,
    },
}

/// Describes what a slot needs.
pub fn factor_view(kind: &FactorKind) -> FactorView {
    match kind {
        FactorKind::KeyFile => FactorView::KeyFile,
        FactorKind::Fido2 {
            credential_id,
            salt,
        } => FactorView::Fido2 {
            credential_id: hex(credential_id),
            salt: hex(salt),
        },
        FactorKind::YubiKey { challenge, slot } => FactorView::Yubikey {
            challenge: hex(challenge),
            slot: *slot,
        },
    }
}
