//! The fields of each entry kind, as data.
//!
//! Every editor on every platform is drawn from this one table, so a new field
//! needs no UI change anywhere, and whether a field is secret is decided here
//! rather than remembered by each shell.

use serde::Serialize;
use wmpass_core::clock::Hlc;
use wmpass_core::model::{
    ApiKey, Card, Database, Document, Entry, Identity, Kind, Licence, Login, Lww, Note, Otp,
    Recovery, RecoveryForm, Revision, SshKey, Wifi, WifiSecurity,
};
use wmpass_core::{Error, Result, Secret};

/// How a field is edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// One line.
    Text,
    /// A person's address on the internet.
    Email,
    /// A link.
    Url,
    /// A phone number.
    Phone,
    /// A whole number.
    Number,
    /// A date, stored as milliseconds.
    Date,
    /// Several lines.
    Multiline,
    /// On or off.
    Toggle,
    /// One of a few values.
    Choice,
    /// A password, with the generator beside it.
    Password,
    /// A secret that is not a password: a PIN, a key, a phrase.
    Hidden,
}

/// One row in an editor.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct FieldSpec {
    /// Stable key, used for reading and writing.
    pub key: &'static str,
    /// English label. Shells translate by key and fall back to this.
    pub label: &'static str,
    /// How it is edited.
    pub input: Input,
    /// Whether the value stays behind [`crate::Service`] until explicitly revealed.
    pub secret: bool,
    /// The values of a [`Input::Choice`].
    pub choices: &'static [&'static str],
    /// Values people type often, offered as the field is filled.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub suggestions: &'static [&'static str],
}

const fn spec(key: &'static str, label: &'static str, input: Input) -> FieldSpec {
    let secret = matches!(input, Input::Password | Input::Hidden);
    FieldSpec {
        key,
        label,
        input,
        secret,
        choices: &[],
        suggestions: &[],
    }
}

/// A line of text with common values to pick from.
const fn suggested(key: &'static str, label: &'static str, suggestions: &'static [&'static str]) -> FieldSpec {
    FieldSpec {
        suggestions,
        ..spec(key, label, Input::Text)
    }
}

const fn hidden_multiline(key: &'static str, label: &'static str) -> FieldSpec {
    FieldSpec {
        key,
        label,
        input: Input::Multiline,
        secret: true,
        choices: &[],
        suggestions: &[],
    }
}

const fn choice(
    key: &'static str,
    label: &'static str,
    choices: &'static [&'static str],
) -> FieldSpec {
    FieldSpec {
        key,
        label,
        input: Input::Choice,
        secret: false,
        choices,
        suggestions: &[],
    }
}

const WIFI: &[&str] = &["open", "wep", "wpa", "sae", "eap"];
const ENGINES: &[&str] = &[
    "PostgreSQL", "MySQL", "MariaDB", "SQLite", "SQL Server", "Oracle", "MongoDB", "Redis", "CockroachDB", "ClickHouse",
    "Cassandra", "DynamoDB", "Elasticsearch", "Supabase", "Firebird",
];
const PROVIDERS: &[&str] = &[
    "OpenAI", "Anthropic", "Google Cloud", "AWS", "Azure", "GitHub", "GitLab", "Stripe", "Cloudflare", "Twilio",
    "SendGrid", "Mailgun", "DigitalOcean", "Hugging Face", "Mistral", "Firebase", "Vercel", "Netlify",
];
const DOCUMENTS: &[&str] = &[
    "Passport", "Driver's license", "ID card", "Residence permit", "Social security card", "Health insurance card",
    "Birth certificate",
];
const ISSUERS: &[&str] = &["Visa", "Mastercard", "American Express", "Discover", "JCB", "UnionPay", "Diners Club"];
const FORMS: &[&str] = &["phrase", "codes"];

/// Every kind's name, in the order a chooser lists them.
pub const KINDS: [&str; 12] = [
    "login", "note", "card", "identity", "otp", "ssh", "api", "database", "licence", "recovery",
    "wifi", "document",
];

/// The rows of a kind.
pub fn fields_of(kind: &str) -> &'static [FieldSpec] {
    const LOGIN: &[FieldSpec] = &[
        spec("username", "Username", Input::Text),
        spec("password", "Password", Input::Password),
    ];
    const CARD: &[FieldSpec] = &[
        spec("holder", "Name on card", Input::Text),
        spec("number", "Card number", Input::Hidden),
        spec("expiry", "Expires", Input::Text),
        spec("cvv", "Security code", Input::Hidden),
        spec("pin", "PIN", Input::Hidden),
        suggested("issuer", "Issuer", ISSUERS),
        spec("billing", "Billing address", Input::Multiline),
    ];
    const IDENTITY: &[FieldSpec] = &[
        spec("full_name", "Full name", Input::Text),
        spec("email", "Email", Input::Email),
        spec("phone", "Phone", Input::Phone),
        spec("address", "Address", Input::Multiline),
        spec("birth_date", "Date of birth", Input::Hidden),
    ];
    const NOTE: &[FieldSpec] = &[hidden_multiline("body", "Note")];
    const DOCUMENT: &[FieldSpec] = &[
        suggested("label", "Document type", DOCUMENTS),
        spec("number", "Number", Input::Hidden),
    ];
    const SSH: &[FieldSpec] = &[
        hidden_multiline("private_key", "Private key"),
        spec("public_key", "Public key", Input::Multiline),
        spec("fingerprint", "Fingerprint", Input::Text),
        spec("passphrase", "Passphrase", Input::Password),
        spec("agent_allowed", "Offer to the SSH agent", Input::Toggle),
    ];
    const API: &[FieldSpec] = &[
        suggested("provider", "Provider", PROVIDERS),
        spec("key", "Key", Input::Hidden),
        spec("secret", "Secret", Input::Hidden),
        spec("scopes", "Scopes", Input::Text),
        spec("expires_ms", "Expires", Input::Date),
    ];
    const DATABASE: &[FieldSpec] = &[
        suggested("engine", "Engine", ENGINES),
        spec("host", "Host", Input::Text),
        spec("port", "Port", Input::Number),
        spec("name", "Database", Input::Text),
        spec("user", "User", Input::Text),
        spec("password", "Password", Input::Password),
        spec("tls", "Use TLS", Input::Toggle),
        spec("url", "Connection string", Input::Hidden),
    ];
    const LICENCE: &[FieldSpec] = &[
        spec("product", "Product", Input::Text),
        spec("key", "License key", Input::Hidden),
        spec("email", "Licensed to", Input::Email),
        spec("order_id", "Order number", Input::Text),
        spec("expires_ms", "Expires", Input::Date),
    ];
    const RECOVERY: &[FieldSpec] = &[
        choice("form", "Form", FORMS),
        hidden_multiline("phrase", "Phrase or codes"),
        spec("linked_entry", "For the account", Input::Text),
    ];
    const WIFI_FIELDS: &[FieldSpec] = &[
        spec("ssid", "Network name", Input::Text),
        choice("security", "Security", WIFI),
        spec("password", "Password", Input::Password),
        spec("hidden_ssid", "Hidden network", Input::Toggle),
    ];
    match kind {
        "login" => LOGIN,
        "card" => CARD,
        "identity" => IDENTITY,
        "note" => NOTE,
        "document" => DOCUMENT,
        "ssh" => SSH,
        "api" => API,
        "database" => DATABASE,
        "licence" => LICENCE,
        "recovery" => RECOVERY,
        "wifi" => WIFI_FIELDS,
        _ => &[],
    }
}

/// A new, empty body of a kind.
pub fn new_kind(kind: &str) -> Result<Kind> {
    Ok(match kind {
        "login" => Kind::Login(Login::default()),
        "card" => Kind::Card(Card::default()),
        "identity" => Kind::Identity(Identity::default()),
        "note" => Kind::Note(Note::default()),
        "document" => Kind::Document(Document::default()),
        "ssh" => Kind::SshKey(SshKey::default()),
        "api" => Kind::ApiKey(ApiKey::default()),
        "database" => Kind::Database(Database::default()),
        "licence" => Kind::Licence(Licence::default()),
        "recovery" => Kind::Recovery(Recovery::default()),
        "wifi" => Kind::Wifi(Wifi::default()),
        "otp" => Kind::Otp(Otp::default()),
        other => return Err(Error::Invalid(format!("unknown kind {other}"))),
    })
}

enum Slot<'a> {
    Text(&'a mut Lww<String>),
    Secret(&'a mut Lww<Secret>),
    Flag(&'a mut Lww<bool>),
    Millis(&'a mut Lww<Option<u64>>),
    Port(&'a mut Lww<Option<u16>>),
    Link(&'a mut Lww<Option<String>>),
    Wifi(&'a mut Lww<WifiSecurity>),
    Form(&'a mut Lww<RecoveryForm>),
}

fn slot<'a>(kind: &'a mut Kind, key: &str) -> Option<Slot<'a>> {
    Some(match kind {
        Kind::Login(v) => match key {
            "username" => Slot::Text(&mut v.username),
            "password" => Slot::Secret(&mut v.password),
            _ => return None,
        },
        Kind::Card(v) => match key {
            "holder" => Slot::Text(&mut v.holder),
            "number" => Slot::Secret(&mut v.number),
            "expiry" => Slot::Text(&mut v.expiry),
            "cvv" => Slot::Secret(&mut v.cvv),
            "pin" => Slot::Secret(&mut v.pin),
            "issuer" => Slot::Text(&mut v.issuer),
            "billing" => Slot::Text(&mut v.billing),
            _ => return None,
        },
        Kind::Identity(v) => match key {
            "full_name" => Slot::Text(&mut v.full_name),
            "address" => Slot::Text(&mut v.address),
            "phone" => Slot::Text(&mut v.phone),
            "email" => Slot::Text(&mut v.email),
            "birth_date" => Slot::Secret(&mut v.birth_date),
            _ => return None,
        },
        Kind::Note(v) => match key {
            "body" => Slot::Secret(&mut v.body),
            _ => return None,
        },
        Kind::Document(v) => match key {
            "label" => Slot::Text(&mut v.label),
            "number" => Slot::Secret(&mut v.number),
            _ => return None,
        },
        Kind::SshKey(v) => match key {
            "private_key" => Slot::Secret(&mut v.private_key),
            "public_key" => Slot::Text(&mut v.public_key),
            "fingerprint" => Slot::Text(&mut v.fingerprint),
            "passphrase" => Slot::Secret(&mut v.passphrase),
            "agent_allowed" => Slot::Flag(&mut v.agent_allowed),
            _ => return None,
        },
        Kind::ApiKey(v) => match key {
            "provider" => Slot::Text(&mut v.provider),
            "key" => Slot::Secret(&mut v.key),
            "secret" => Slot::Secret(&mut v.secret),
            "scopes" => Slot::Text(&mut v.scopes),
            "expires_ms" => Slot::Millis(&mut v.expires_ms),
            _ => return None,
        },
        Kind::Database(v) => match key {
            "engine" => Slot::Text(&mut v.engine),
            "host" => Slot::Text(&mut v.host),
            "port" => Slot::Port(&mut v.port),
            "name" => Slot::Text(&mut v.name),
            "user" => Slot::Text(&mut v.user),
            "password" => Slot::Secret(&mut v.password),
            "tls" => Slot::Flag(&mut v.tls),
            "url" => Slot::Secret(&mut v.url),
            _ => return None,
        },
        Kind::Licence(v) => match key {
            "product" => Slot::Text(&mut v.product),
            "key" => Slot::Secret(&mut v.key),
            "email" => Slot::Text(&mut v.email),
            "order_id" => Slot::Text(&mut v.order_id),
            "expires_ms" => Slot::Millis(&mut v.expires_ms),
            _ => return None,
        },
        Kind::Recovery(v) => match key {
            "form" => Slot::Form(&mut v.form),
            "phrase" => Slot::Secret(&mut v.phrase),
            "linked_entry" => Slot::Link(&mut v.linked_entry),
            _ => return None,
        },
        Kind::Wifi(v) => match key {
            "ssid" => Slot::Text(&mut v.ssid),
            "security" => Slot::Wifi(&mut v.security),
            "password" => Slot::Secret(&mut v.password),
            "hidden_ssid" => Slot::Flag(&mut v.hidden_ssid),
            _ => return None,
        },
        Kind::Otp(_) => return None,
    })
}

/// A field's value as a view may show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Shown {
    /// Text.
    Text(String),
    /// On or off.
    Flag(bool),
    /// A number.
    Number(u64),
}

pub(crate) fn wifi_name(value: WifiSecurity) -> &'static str {
    match value {
        WifiSecurity::Open => "open",
        WifiSecurity::Wep => "wep",
        WifiSecurity::Wpa => "wpa",
        WifiSecurity::Sae => "sae",
        WifiSecurity::Eap => "eap",
    }
}

/// What a view gets for a field: nothing for a secret, however it is typed, plus
/// whether a value exists at all.
pub fn shown(entry: &mut Entry, key: &str) -> Option<(Option<Shown>, bool)> {
    Some(match slot(&mut entry.kind, key)? {
        Slot::Text(v) => {
            let has = !v.v.is_empty();
            (Some(Shown::Text(v.v.clone())), has)
        }
        Slot::Secret(v) => (None, !v.v.is_empty()),
        Slot::Flag(v) => (Some(Shown::Flag(v.v)), v.v),
        Slot::Millis(v) => (v.v.map(Shown::Number), v.v.is_some()),
        Slot::Port(v) => (v.v.map(|p| Shown::Number(u64::from(p))), v.v.is_some()),
        Slot::Link(v) => (v.v.clone().map(Shown::Text), v.v.is_some()),
        Slot::Wifi(v) => (Some(Shown::Text(wifi_name(v.v).to_owned())), true),
        Slot::Form(v) => (
            Some(Shown::Text(
                match v.v {
                    RecoveryForm::Phrase => "phrase",
                    RecoveryForm::Codes => "codes",
                }
                .to_owned(),
            )),
            true,
        ),
    })
}

/// A secret field's value. `None` for an unknown key or a field that is not secret.
pub fn secret_of(entry: &mut Entry, key: &str) -> Option<Secret> {
    match slot(&mut entry.kind, key)? {
        Slot::Secret(v) => Some(v.v.clone()),
        _ => None,
    }
}

/// Writes a plain field. Refuses a secret field, which goes through [`set_secret`]
/// so its value never sits in a JSON string on the other side.
pub fn set_plain(entry: &mut Entry, key: &str, value: &serde_json::Value, at: Hlc) -> Result<()> {
    let bad = || Error::Invalid(format!("{key} cannot take that value"));
    let unknown = || Error::Invalid(format!("no field {key} on this kind"));
    match slot(&mut entry.kind, key).ok_or_else(unknown)? {
        Slot::Secret(_) => return Err(Error::Invalid(format!("{key} is secret"))),
        Slot::Text(v) => {
            let text = value.as_str().ok_or_else(bad)?.to_owned();
            if v.v != text {
                *v = Lww::new(text, at);
            }
        }
        Slot::Flag(v) => {
            let flag = value.as_bool().ok_or_else(bad)?;
            if v.v != flag {
                *v = Lww::new(flag, at);
            }
        }
        Slot::Millis(v) => {
            let ms = if value.is_null() {
                None
            } else {
                Some(value.as_u64().ok_or_else(bad)?)
            };
            if v.v != ms {
                *v = Lww::new(ms, at);
            }
        }
        Slot::Port(v) => {
            let port = if value.is_null() {
                None
            } else {
                let raw = value.as_u64().ok_or_else(bad)?;
                Some(u16::try_from(raw).map_err(|_| bad())?)
            };
            if v.v != port {
                *v = Lww::new(port, at);
            }
        }
        Slot::Link(v) => {
            let link = value.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
            if v.v != link {
                *v = Lww::new(link, at);
            }
        }
        Slot::Wifi(v) => {
            let security = match value.as_str().ok_or_else(bad)? {
                "open" => WifiSecurity::Open,
                "wep" => WifiSecurity::Wep,
                "wpa" => WifiSecurity::Wpa,
                "sae" => WifiSecurity::Sae,
                "eap" => WifiSecurity::Eap,
                _ => return Err(bad()),
            };
            if v.v != security {
                *v = Lww::new(security, at);
            }
        }
        Slot::Form(v) => {
            let form = match value.as_str().ok_or_else(bad)? {
                "phrase" => RecoveryForm::Phrase,
                "codes" => RecoveryForm::Codes,
                _ => return Err(bad()),
            };
            if v.v != form {
                *v = Lww::new(form, at);
            }
        }
    }
    Ok(())
}

/// Writes a secret field. The old value joins the entry's history, and writing the
/// same value again changes nothing, so a save with no edit adds no revision.
pub fn set_secret(entry: &mut Entry, key: &str, value: &[u8], at: Hlc) -> Result<()> {
    let text = std::str::from_utf8(value)
        .map_err(|_| Error::Invalid("that value is not valid text".into()))?;
    let new = Secret::new(text);
    let mut old: Option<Revision> = None;
    match slot(&mut entry.kind, key) {
        Some(Slot::Secret(v)) => {
            if v.v == new {
                return Ok(());
            }
            if !v.v.is_empty() {
                old = Some(Revision {
                    field: key.to_owned(),
                    value: v.v.clone(),
                    at: v.at,
                });
            }
            *v = Lww::new(new, at);
        }
        _ => return Err(Error::Invalid(format!("{key} is not a secret field"))),
    }
    entry.history.extend(old);
    Ok(())
}

/// One line under an entry's title in a list. Never a secret: a card shows its last
/// four digits and nothing else, as printed receipts do.
pub fn subtitle(entry: &Entry) -> String {
    match &entry.kind {
        Kind::Login(v) => v.username.v.clone(),
        Kind::Card(v) => {
            let number = v.number.v.expose();
            let digits: Vec<char> = number.chars().filter(char::is_ascii_digit).collect();
            let last: String = digits[digits.len().saturating_sub(4)..].iter().collect();
            match (v.issuer.v.is_empty(), last.is_empty()) {
                (false, false) => format!("{} •••• {last}", v.issuer.v),
                (true, false) => format!("•••• {last}"),
                _ => v.issuer.v.clone(),
            }
        }
        Kind::Identity(v) => {
            if v.full_name.v.is_empty() {
                v.email.v.clone()
            } else {
                v.full_name.v.clone()
            }
        }
        Kind::Note(_) => String::new(),
        Kind::Document(v) => v.label.v.clone(),
        Kind::SshKey(v) => v.fingerprint.v.clone(),
        Kind::ApiKey(v) => v.provider.v.clone(),
        Kind::Database(v) => {
            if v.name.v.is_empty() {
                v.host.v.clone()
            } else {
                format!("{} on {}", v.name.v, v.host.v)
            }
        }
        Kind::Licence(v) => v.product.v.clone(),
        Kind::Recovery(v) => match v.form.v {
            RecoveryForm::Phrase => "Recovery phrase".to_owned(),
            RecoveryForm::Codes => "Recovery codes".to_owned(),
        },
        Kind::Wifi(v) => v.ssid.v.clone(),
        Kind::Otp(v) => v
            .config
            .v
            .as_ref()
            .map(|c| c.account.clone())
            .unwrap_or_default(),
    }
}
