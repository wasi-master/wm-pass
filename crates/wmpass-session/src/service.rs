//! Handles and the call table.

use std::collections::{BTreeMap, HashMap};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wmpass_core::crypto::KdfParams;
use wmpass_core::format::{self, FactorKind, FactorOutput, KdfConfig, SlotAction, VaultKey};
use wmpass_core::generator::{self, Recipe};
use wmpass_core::matching::{self, MatchLevel, Target};
use wmpass_core::model::{Entry, Field, Kind, LinkedVault, Lww, Reminders, SharedLink, SiteMatcher};
use wmpass_core::otp::OtpConfig;
use wmpass_core::sharing::{self, Manifest, MemberKeys, MemberPublic};
use wmpass_core::search::{self, Context, Flags, FolderNames, Query};
use wmpass_core::vault::{Freshness, OpenOutcome, Vault, check_freshness, hex, new_id, open_set, unhex};
use wmpass_core::{Error, Secret, categorise, emergency, health, io, passkey, recovery, related, ssh, strength, wifi};
use zeroize::Zeroizing;

use crate::fields;
use crate::views::{self, FactorView};

/// One request.
#[derive(Debug, Default, Clone, Copy)]
pub struct Call<'a> {
    /// The vault it is for. Zero when it needs none.
    pub handle: u64,
    /// The arguments, as JSON. Never holds a secret.
    pub args: &'a str,
    /// One secret value going in: a password, a field's new value.
    pub secret: &'a [u8],
    /// Binary values: vault files, attachments, second factor outputs.
    pub blobs: &'a [&'a [u8]],
    /// The time now, in milliseconds. Passed in because a browser extension's wasm has
    /// no clock of its own.
    pub now_ms: u64,
}

/// One answer.
#[derive(Debug, Default)]
pub struct Reply {
    /// The view, as JSON. Never holds a secret.
    pub json: String,
    /// The value asked for. The caller wipes it once used.
    pub secret: Option<Zeroizing<Vec<u8>>>,
    /// Binary values: only ever ciphertext, or a file the caller asked to export.
    pub blobs: Vec<Vec<u8>>,
}

impl Reply {
    pub(crate) fn view(value: Value) -> Self {
        Self {
            json: value.to_string(),
            ..Self::default()
        }
    }

    fn of<T: serde::Serialize>(value: &T) -> Result<Self, SessionError> {
        Ok(Self {
            json: serde_json::to_string(value).map_err(|_| Error::Encoding)?,
            ..Self::default()
        })
    }

    fn ok() -> Self {
        Self::view(json!({}))
    }

    fn secret(value: &str) -> Self {
        Self {
            json: "{}".to_owned(),
            secret: Some(Zeroizing::new(value.as_bytes().to_vec())),
            blobs: Vec::new(),
        }
    }
}

/// Why a call failed. The message is safe to show and never holds a secret.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{message}")]
pub struct SessionError {
    /// Stable code a shell can branch on.
    pub code: &'static str,
    /// Words to show.
    pub message: String,
}

impl From<Error> for SessionError {
    fn from(error: Error) -> Self {
        let code = match &error {
            Error::Random => "random",
            Error::Malformed => "malformed",
            Error::UnsupportedVersion(_) => "unsupported_version",
            Error::KdfOutOfRange => "kdf_out_of_range",
            Error::Integrity => "integrity",
            Error::NoFreeSlot => "no_free_slot",
            Error::TooLong => "too_long",
            Error::Encoding => "encoding",
            Error::Crypto => "crypto",
            Error::Import(_) => "import",
            Error::Invalid(_) => "invalid",
        };
        Self {
            code,
            message: error.to_string(),
        }
    }
}

/// A QR code as rows of dark and light cells, `1` for dark, for any shell to draw.
fn qr_view(bytes: &[u8]) -> Result<Reply, SessionError> {
    let code = qrcode::QrCode::new(bytes).map_err(|_| invalid("too long for a QR code"))?;
    let size = code.width();
    let rows: String = code.to_colors().iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect();
    Ok(Reply::view(json!({"size": size, "rows": rows})))
}

fn invalid(message: impl Into<String>) -> SessionError {
    SessionError {
        code: "invalid",
        message: message.into(),
    }
}

/// For a field that can be cleared: absent is `None`, null is `Some(None)`. Serde alone reads
/// both as `None`, and then asking to clear a colour, icon or folder changed nothing.
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn parse<T: DeserializeOwned>(args: &str) -> Result<T, SessionError> {
    serde_json::from_str(if args.is_empty() { "{}" } else { args })
        .map_err(|e| invalid(format!("bad arguments: {e}")))
}

/// An unlocked vault and what goes with it.
#[derive(Debug)]
struct Session {
    vault: Vault,
    dirty: bool,
    shelf_open: bool,
    health: Option<health::HealthReport>,
    pending_import: Option<io::Imported>,
    remote: Option<crate::remote::Remote>,
    unreadable: usize,
}

impl Session {
    fn new(vault: Vault) -> Self {
        Self {
            vault,
            dirty: false,
            shelf_open: false,
            health: None,
            pending_import: None,
            remote: None,
            unreadable: 0,
        }
    }

    fn touched(&mut self) {
        self.dirty = true;
        self.health = None;
    }

    fn entry(&self, id: &str) -> Result<&Entry, SessionError> {
        let entry = self
            .vault
            .get(id)
            .ok_or_else(|| invalid("that entry is gone"))?;
        self.reachable(entry)?;
        Ok(entry)
    }

    /// The shelf is closed to everything until a fresh unlock opens it, including a
    /// shell that holds an id from earlier.
    fn reachable(&self, entry: &Entry) -> Result<(), SessionError> {
        if entry.common.hidden.v && !self.shelf_open {
            return Err(SessionError {
                code: "shelf_closed",
                message: "unlock the hidden shelf first".to_owned(),
            });
        }
        Ok(())
    }

    fn report(&mut self, now_ms: u64, old_after_days: u32) -> &health::HealthReport {
        let vault = &self.vault;
        self.health.get_or_insert_with(|| {
            health::report(
                vault,
                &health::HealthInput {
                    now_ms,
                    old_after_days,
                    ..health::HealthInput::default()
                },
            )
        })
    }
}

#[derive(Debug)]
struct Names<'a>(&'a Vault);

impl FolderNames for Names<'_> {
    fn folder_name(&self, id: &str) -> Option<String> {
        self.0.doc.folders.get(id).map(|f| f.name.v.clone())
    }
}

/// Everything unlocked in this process. Dropping it wipes every vault.
#[derive(Debug, Default)]
pub struct Service {
    next: u64,
    device: u32,
    sessions: HashMap<u64, Session>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct KdfArg {
    m_kib: Option<u32>,
    t: Option<u32>,
    p: Option<u32>,
}

impl KdfArg {
    fn config(&self) -> Result<KdfConfig, SessionError> {
        let base = KdfParams::default();
        let params = KdfParams {
            m_kib: self.m_kib.unwrap_or(base.m_kib),
            t: self.t.unwrap_or(base.t),
            p: self.p.unwrap_or(base.p),
        };
        Ok(KdfConfig::new(params)?)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Id {
    id: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SharedArgs {
    file: String,
    name: String,
    my_name: String,
    code: String,
    key: String,
}

/// Limits on the settings a vault carries for the apps, so a damaged shell cannot bloat the file.
const SYNCED_KEY_MAX: usize = 96;
const SYNCED_VALUE_MAX: usize = 256 * 1024;
const SYNCED_KEYS_MAX: usize = 1024;
const MEMBER_PREFIX: &str = "wmpass-member:";
const INVITE_PREFIX: &str = "wmpass-invite:";

fn key32(text: &str) -> Result<[u8; 32], SessionError> {
    let raw = Zeroizing::new(unhex(text).ok_or_else(|| invalid("bad key"))?);
    raw.as_slice().try_into().map_err(|_| invalid("bad key"))
}

fn member_secret(keys: &MemberKeys) -> Secret {
    let (wrapping, signing) = keys.to_bytes();
    let mut raw = Zeroizing::new(Vec::with_capacity(64));
    raw.extend_from_slice(&wrapping);
    raw.extend_from_slice(&signing);
    Secret::from(hex(&raw))
}

fn member_keys(link: &SharedLink) -> Result<MemberKeys, SessionError> {
    let raw = Zeroizing::new(unhex(link.member.expose()).ok_or_else(|| invalid("bad key"))?);
    if raw.len() != 64 {
        return Err(invalid("bad key"));
    }
    let wrapping: [u8; 32] = raw[..32].try_into().map_err(|_| invalid("bad key"))?;
    let signing: [u8; 32] = raw[32..].try_into().map_err(|_| invalid("bad key"))?;
    Ok(MemberKeys::from_bytes(wrapping, signing))
}

fn member_from_code(code: &str) -> Result<MemberPublic, SessionError> {
    let text = code.trim().strip_prefix(MEMBER_PREFIX).ok_or_else(|| invalid("that is not a member code"))?;
    let raw = B64.decode(text).map_err(|_| invalid("that is not a member code"))?;
    serde_json::from_slice(&raw).map_err(|_| invalid("that is not a member code"))
}

fn action_of(name: &str) -> Result<SlotAction, SessionError> {
    match name {
        "open" | "" => Ok(SlotAction::Open),
        "wipe" => Ok(SlotAction::WipeLocal),
        "lock" => Ok(SlotAction::HardLock),
        _ => Err(invalid("unknown slot action")),
    }
}

/// A factor as the core wants it, from the view a shell holds.
pub fn factor_of(view: &FactorView) -> Result<FactorKind, SessionError> {
    let raw = |text: &str| unhex(text).ok_or_else(|| invalid("bad hex"));
    Ok(match view {
        FactorView::KeyFile => FactorKind::KeyFile,
        FactorView::Fido2 {
            credential_id,
            salt,
        } => FactorKind::Fido2 {
            credential_id: raw(credential_id)?,
            salt: raw(salt)?.try_into().map_err(|_| invalid("bad salt"))?,
        },
        FactorView::Yubikey { challenge, slot } => FactorKind::YubiKey {
            challenge: raw(challenge)?
                .try_into()
                .map_err(|_| invalid("bad challenge"))?,
            slot: *slot,
        },
        FactorView::Webauthn {
            credential_id,
            input,
        } => FactorKind::WebAuthn {
            credential_id: raw(credential_id)?,
            input: raw(input)?.try_into().map_err(|_| invalid("bad input"))?,
        },
    })
}

/// Splits `blobs` into factor outputs and the rest. `factors` names each leading blob:
/// `key_file` is a file to hash, anything else is already a device's output.
fn split_factors<'a>(
    factors: &[String],
    blobs: &'a [&'a [u8]],
) -> Result<(Vec<FactorOutput>, &'a [&'a [u8]]), SessionError> {
    if factors.len() > blobs.len() {
        return Err(invalid("a factor is missing its data"));
    }
    let (first, rest) = blobs.split_at(factors.len());
    let outputs = factors
        .iter()
        .zip(first)
        .map(|(kind, blob)| match kind.as_str() {
            "key_file" => format::key_file_factor(blob),
            _ => Zeroizing::new(blob.to_vec()),
        })
        .collect();
    Ok((outputs, rest))
}

/// Every QR code in a picture, as text.
fn read_qr(pixels: &[u8], width: usize, height: usize, rgba: bool) -> Result<Zeroizing<Vec<String>>, SessionError> {
    let per = if rgba { 4 } else { 1 };
    if width == 0 || height == 0 || width.checked_mul(height).and_then(|n| n.checked_mul(per)) != Some(pixels.len()) {
        return Err(invalid("the picture's size does not match its pixels"));
    }
    let grey = |x: usize, y: usize| -> u8 {
        let at = (y * width + x) * per;
        if rgba {
            // Rec. 601 weights, in integers.
            let (r, g, b) = (u32::from(pixels[at]), u32::from(pixels[at + 1]), u32::from(pixels[at + 2]));
            u8::try_from((r * 299 + g * 587 + b * 114) / 1000).unwrap_or(u8::MAX)
        } else {
            pixels[at]
        }
    };
    let mut image = rqrr::PreparedImage::prepare_from_greyscale(width, height, grey);
    let mut out = Zeroizing::new(Vec::new());
    for grid in image.detect_grids() {
        if let Ok((_, text)) = grid.decode() {
            out.push(text);
        }
    }
    Ok(out)
}

fn blob<'a>(call: &Call<'a>, at: usize) -> Result<&'a [u8], SessionError> {
    call.blobs
        .get(at)
        .copied()
        .ok_or_else(|| invalid("a file is missing"))
}

/// A KeePass key file and YubiKey answer, either of which may be absent.
type KdbxKeys<'a> = (Option<&'a [u8]>, Option<&'a [u8]>);

/// A KeePass database's key file and YubiKey answer, after the database itself: the key
/// file next when there is one, the answer last when `response` says there is one.
fn kdbx_keys<'a>(call: &Call<'a>, response: bool) -> Result<KdbxKeys<'a>, SessionError> {
    let rest = call.blobs.get(1..).unwrap_or_default();
    match (rest, response) {
        ([], false) => Ok((None, None)),
        ([key_file], false) => Ok((Some(*key_file), None)),
        ([answer], true) => Ok((None, Some(*answer))),
        ([key_file, answer], true) => Ok((Some(*key_file), Some(*answer))),
        _ => Err(invalid("unexpected files with the database")),
    }
}

fn text(bytes: &[u8]) -> Result<&str, SessionError> {
    std::str::from_utf8(bytes).map_err(|_| invalid("that value is not valid text"))
}

fn level_name(level: MatchLevel) -> &'static str {
    match level {
        MatchLevel::Unverified => "unverified",
        MatchLevel::Domain => "domain",
        MatchLevel::Exact => "exact",
    }
}

fn health_flags(report: &health::HealthReport) -> Flags {
    Flags {
        weak: report.weak.iter().cloned().collect(),
        reused: report.reused.iter().flatten().cloned().collect(),
        breached: report.breached.iter().map(|(id, _)| id.clone()).collect(),
        old: report.old.iter().cloned().collect(),
        duplicate: report.duplicates.iter().flatten().cloned().collect(),
    }
}

/// Which words in a query need the password checks, which take a moment to run.
fn wants_health(query: &str) -> bool {
    query
        .split_whitespace()
        .any(|word| ["weak", "reused", "breached", "old", "duplicate"].contains(&word.to_lowercase().as_str()))
}

fn sort_rows(rows: &mut [views::Summary], sort: &str) {
    match sort {
        "title" => rows.sort_by_key(|r| r.title.to_lowercase()),
        "modified" => rows.sort_by_key(|r| std::cmp::Reverse(r.modified_ms)),
        _ => {}
    }
    rows.sort_by_key(|r| !r.pinned);
}

impl Service {
    /// A service for one device. `device` is that device's random number, stored
    /// by the shell, and it breaks ties in a merge.
    pub fn new(device: u32) -> Self {
        Self {
            next: 1,
            device,
            sessions: HashMap::new(),
        }
    }

    fn adopt(&mut self, vault: Vault) -> u64 {
        let handle = self.next;
        self.next += 1;
        self.sessions.insert(handle, Session::new(vault));
        handle
    }

    fn session(&mut self, handle: u64) -> Result<&mut Session, SessionError> {
        self.sessions.get_mut(&handle).ok_or(SessionError {
            code: "locked",
            message: "this vault is locked".to_owned(),
        })
    }

    /// Lends a call on the network the open vault for one step.
    pub fn with_vault<T>(&mut self, handle: u64, run: impl FnOnce(&mut Vault) -> T) -> Result<T, SessionError> {
        Ok(run(&mut self.session(handle)?.vault))
    }

    /// Marks a vault changed after [`Self::with_vault`].
    pub fn touch(&mut self, handle: u64) -> Result<(), SessionError> {
        self.session(handle)?.touched();
        Ok(())
    }

    /// An account's entries get a vault of their own in memory, which is never saved.
    fn mirror(&mut self, name: &str, mut remote: crate::remote::Remote, now: u64) -> Result<u64, SessionError> {
        let throwaway: [u8; 32] = wmpass_core::crypto::random_array()?;
        let config = KdfArg::default().config()?;
        let mut vault = Vault::create(name, &throwaway, &config, self.device, now)?;
        remote.refresh(&mut vault, now);
        let handle = self.adopt(vault);
        self.session(handle)?.remote = Some(remote);
        Ok(handle)
    }

    /// The address a Proxynova check may ask about, and the passwords to compare. Refuses an
    /// address that is not in this vault, so the check cannot look up someone else.
    #[cfg(feature = "online")]
    pub fn comb_inputs(
        &mut self,
        handle: u64,
        email: &str,
    ) -> Result<(wmpass_net::breach::VaultEmail, Vec<Secret>, Vec<String>), SessionError> {
        let session = self.session(handle)?;
        let wanted = email.trim().to_lowercase();
        let known = wmpass_net::breach::vault_emails(session.vault.visible());
        let address = known
            .into_iter()
            .find(|e| e.as_str() == wanted)
            .ok_or_else(|| invalid("that address is not in this vault"))?;
        let mut secrets = Vec::new();
        let mut ids = Vec::new();
        for entry in session.vault.visible() {
            if let Some(login) = entry.login()
                && login.username.v.trim().eq_ignore_ascii_case(&wanted)
                && !login.password.v.expose().is_empty()
            {
                secrets.push(login.password.v.clone());
                ids.push(entry.id.clone());
            }
        }
        Ok((address, secrets, ids))
    }

    /// Shows a Bitwarden account through a mirror.
    #[cfg(feature = "online")]
    pub fn adopt_bitwarden(
        &mut self,
        name: &str,
        account: Box<wmpass_bw::account::Account>,
        unreadable: usize,
        now: u64,
    ) -> Result<Reply, SessionError> {
        let remote = crate::remote::Remote::new(crate::remote::Account::Bitwarden(account));
        let handle = self.mirror(name, remote, now)?;
        let session = self.session(handle)?;
        session.unreadable = unreadable;
        Ok(Reply::view(Self::opened(handle, 0, &session.vault)))
    }

    /// Passes changes on to a Bitwarden account and takes it out for a trip to the server.
    #[cfg(feature = "online")]
    pub fn take_bitwarden(&mut self, handle: u64, now: u64) -> Result<Box<wmpass_bw::account::Account>, SessionError> {
        let session = self.session(handle)?;
        let vault = &mut session.vault;
        let Some(remote) = session.remote.as_mut() else {
            return Err(invalid("this vault has no account behind it"));
        };
        remote.collect(vault, now)?;
        let Some(crate::remote::Remote { account: crate::remote::Account::Bitwarden(account), .. }) = session.remote.take() else {
            return Err(invalid("this is not a Bitwarden account"));
        };
        Ok(account)
    }

    /// Puts an account back after a trip to the server and shows what it brought.
    #[cfg(feature = "online")]
    pub fn return_bitwarden(&mut self, handle: u64, account: Box<wmpass_bw::account::Account>, now: u64) -> Result<(), SessionError> {
        let session = self.session(handle)?;
        let mut remote = crate::remote::Remote::new(crate::remote::Account::Bitwarden(account));
        remote.refresh(&mut session.vault, now);
        session.unreadable = match &remote.account {
            crate::remote::Account::Bitwarden(account) => account.unreadable(),
            crate::remote::Account::Kdbx(_) => 0,
        };
        session.remote = Some(remote);
        session.touched();
        Ok(())
    }

    /// Locks every vault, as a panic trigger or a screen timeout does.
    pub fn lock_all(&mut self) {
        self.sessions.clear();
    }

    /// Handles still open.
    pub fn open_handles(&self) -> Vec<u64> {
        self.sessions.keys().copied().collect()
    }

    fn config_for(&mut self, kdf: &KdfArg, same_as: Option<u64>) -> Result<KdfConfig, SessionError> {
        match same_as {
            Some(handle) => Ok(self.session(handle)?.vault.kdf_config()?),
            None => kdf.config(),
        }
    }

    fn opened(handle: u64, index: usize, vault: &Vault) -> Value {
        json!({
            "outcome": "opened",
            "handle": handle,
            "index": index,
            "name": vault.doc.name,
            "generation": vault.doc.generation,
            "read_only": vault.is_read_only(),
        })
    }

    /// Runs one call.
    #[allow(clippy::too_many_lines)]
    pub fn call(&mut self, method: &str, call: Call<'_>) -> Result<Reply, SessionError> {
        let now = call.now_ms;
        match method {
            // ---- files, keys, slots ----
            "qr.encode" => qr_view(call.secret),
            "kinds" => {
                let kinds: Vec<Value> = fields::KINDS
                    .iter()
                    .map(|k| json!({"kind": k, "fields": fields::fields_of(k)}))
                    .collect();
                Ok(Reply::view(json!(kinds)))
            }
            "vault.create" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    name: String,
                    kdf: KdfArg,
                    same_kdf_as: Option<u64>,
                }
                let a: A = parse(call.args)?;
                // Files that share a salt share one derivation, so a hidden vault beside
                // the real one adds no time an observer could measure.
                let config = self.config_for(&a.kdf, a.same_kdf_as)?;
                let vault = Vault::create(&a.name, call.secret, &config, self.device, now)?;
                let handle = self.adopt(vault);
                let session = self.session(handle)?;
                session.dirty = true;
                Ok(Reply::view(Self::opened(handle, 0, &session.vault)))
            }
            "vault.open" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    factors: Vec<String>,
                }
                let a: A = parse(call.args)?;
                let (factors, files) = split_factors(&a.factors, call.blobs)?;
                match open_set(files, call.secret, &factors, self.device)? {
                    OpenOutcome::Opened { index, vault } => {
                        let handle = self.adopt(*vault);
                        let session = self.session(handle)?;
                        Ok(Reply::view(Self::opened(handle, index, &session.vault)))
                    }
                    OpenOutcome::Duress { action } => Ok(Reply::view(json!({
                        "outcome": "duress",
                        "action": match action {
                            SlotAction::WipeLocal => "wipe",
                            SlotAction::HardLock => "lock",
                            SlotAction::Open => "open",
                        },
                    }))),
                    OpenOutcome::NeedsFactor(kinds) => {
                        let needed: Vec<FactorView> =
                            kinds.iter().map(views::factor_view).collect();
                        Ok(Reply::view(json!({"outcome": "needs_factor", "factors": needed})))
                    }
                    OpenOutcome::NoMatch => Ok(Reply::view(json!({"outcome": "no_match"}))),
                }
            }
            "vault.open_key" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    /// A number pages already know this vault by, when a suspended worker comes back.
                    handle: Option<u64>,
                }
                let a: A = parse(call.args)?;
                let raw: [u8; 32] = call.secret.try_into().map_err(|_| invalid("bad key"))?;
                let vault = Vault::from_file(blob(&call, 0)?, VaultKey::from_bytes(raw), self.device)?;
                let handle = match a.handle {
                    Some(wanted) if !self.sessions.contains_key(&wanted) => {
                        self.next = self.next.max(wanted + 1);
                        self.sessions.insert(wanted, Session::new(vault));
                        wanted
                    }
                    _ => self.adopt(vault),
                };
                let session = self.session(handle)?;
                Ok(Reply::view(Self::opened(handle, 0, &session.vault)))
            }
            // Quick unlock in a browser: the vault key, sealed by `vault.quick_seal` under what a
            // passkey's PRF answered, opened again with a fresh answer. The key never leaves here.
            "vault.quick_open" => {
                let wrapping = quick_wrapping(call.secret)?;
                let key = wmpass_core::crypto::open(&wrapping, blob(&call, 0)?, QUICK_AAD).ok_or_else(|| SessionError {
                    code: "wrong_password",
                    message: "the passkey's answer does not open this vault".to_owned(),
                })?;
                let raw: [u8; 32] = key.as_slice().try_into().map_err(|_| invalid("bad key"))?;
                let vault = Vault::from_file(blob(&call, 1)?, VaultKey::from_bytes(raw), self.device)?;
                let handle = self.adopt(vault);
                let session = self.session(handle)?;
                Ok(Reply::view(Self::opened(handle, 0, &session.vault)))
            }
            "vault.chaff" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    size: usize,
                    file_len: Option<usize>,
                    kdf: KdfArg,
                    same_kdf_as: Option<u64>,
                }
                let a: A = parse(call.args)?;
                let config = self.config_for(&a.kdf, a.same_kdf_as)?;
                let file = match a.file_len {
                    Some(len) => format::chaff_like(&config, len)?,
                    None => format::chaff_file(&config, a.size)?,
                };
                Ok(Reply {
                    json: "{}".to_owned(),
                    blobs: vec![file],
                    ..Reply::default()
                })
            }
            // What a YubiKey is asked for a KeePass database, for the shell's hardware
            // layer to put to the key. Its answer then goes with `kdbx.open` or `import.read`.
            "kdbx.challenge" => {
                let challenge = wmpass_core::io::kdbx::challenge(blob(&call, 0)?)?;
                Ok(Reply::view(json!({"challenge": hex(&challenge)})))
            }
            "kdbx.open" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    name: String,
                    /// The last blob is a YubiKey's answer to `kdbx.challenge`.
                    response: bool,
                }
                let a: A = parse(call.args)?;
                let password = Secret::new(text(call.secret)?);
                let (key_file, response) = kdbx_keys(&call, a.response)?;
                let backend = wmpass_core::backend::KdbxBackend::open_with_response(
                    blob(&call, 0)?,
                    password,
                    key_file.map(<[u8]>::to_vec),
                    response.map(|r| Zeroizing::new(r.to_vec())),
                )?;
                // KDBX 3 is read but written as KDBX 4, which apps from before 2017 cannot open.
                let upgrade = backend.will_upgrade();
                let remote = crate::remote::Remote::new(crate::remote::Account::Kdbx(Box::new(backend)));
                let handle = self.mirror(&a.name, remote, now)?;
                let session = self.session(handle)?;
                let mut view = Self::opened(handle, 0, &session.vault);
                view["kdbx_upgrade"] = json!(upgrade);
                Ok(Reply::view(view))
            }
            "emergency.recover" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    shares: Vec<String>,
                }
                let a: A = parse(call.args)?;
                let shares: Vec<&str> = a.shares.iter().map(String::as_str).collect();
                let vault = emergency::recover(blob(&call, 0)?, &shares, self.device)?;
                let handle = self.adopt(vault);
                let session = self.session(handle)?;
                Ok(Reply::view(Self::opened(handle, 0, &session.vault)))
            }
            // A new key for the editor to fill in, before the item exists.
            "ssh.new" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    comment: String,
                }
                let a: A = parse(call.args)?;
                let key = ssh::generate_ed25519(&a.comment)?;
                Ok(Reply {
                    json: json!({"public_key": key.public_key, "fingerprint": key.fingerprint}).to_string(),
                    secret: Some(Zeroizing::new(key.private_key.expose().as_bytes().to_vec())),
                    blobs: Vec::new(),
                })
            }
            "gen.presets" => Ok(Reply::of(&generator::default_presets())?),
            "gen.generate" => {
                let recipe: Recipe = parse(call.args)?;
                let value = generator::generate(&recipe)?;
                Ok(Reply {
                    json: json!({"bits": generator::entropy_bits(&recipe)}).to_string(),
                    secret: Some(Zeroizing::new(value.expose().as_bytes().to_vec())),
                    blobs: Vec::new(),
                })
            }
            "gen.for_site" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    domain: String,
                    length: u32,
                }
                let a: A = parse(call.args)?;
                let rules = generator::rules_for_site(&a.domain);
                let value = match &rules {
                    Some(rules) => generator::password_for_rules(rules, a.length.max(8))?,
                    None => generator::password(&generator::PasswordSpec {
                        length: a.length.max(8),
                        ..generator::PasswordSpec::default()
                    })?,
                };
                Ok(Reply {
                    json: json!({"site_rules": rules.is_some()}).to_string(),
                    secret: Some(Zeroizing::new(value.expose().as_bytes().to_vec())),
                    blobs: Vec::new(),
                })
            }
            "qr.read" => {
                // A picture or a camera frame, as RGBA or one byte of grey per pixel. What the
                // codes say often holds a secret, such as a one-time code's seed, so it comes
                // back as the secret, a JSON list of texts.
                #[derive(Deserialize)]
                struct A {
                    width: usize,
                    height: usize,
                    #[serde(default)]
                    rgba: bool,
                }
                let a: A = parse(call.args)?;
                let pixels = blob(&call, 0)?;
                let codes = read_qr(pixels, a.width, a.height, a.rgba)?;
                let found = codes.len();
                let secret = (found > 0).then(|| Zeroizing::new(serde_json::to_vec(&*codes).unwrap_or_default()));
                Ok(Reply { json: json!({"found": found}).to_string(), secret, blobs: Vec::new() })
            }
            "site.change_password" => {
                // Where a saved address lets you change the password, for any app's own button:
                // the listed page, or for a site off the list the domain to ask, which the app
                // passes to site.probe_change_password when it may use the network.
                #[derive(Deserialize)]
                struct A {
                    site: String,
                }
                let a: A = parse(call.args)?;
                Ok(Reply::view(json!({
                    "url": wmpass_core::change_password::url(&a.site),
                    "ask": wmpass_core::change_password::ask(&a.site),
                })))
            }
            "strength.assess" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    context: Vec<String>,
                }
                let a: A = parse(call.args)?;
                let context: Vec<&str> = a.context.iter().map(String::as_str).collect();
                let s = strength::assess(text(call.secret)?, &context);
                Ok(Reply::view(json!({
                    "score": s.score,
                    "crack_slow_hash": s.crack_slow_hash,
                    "crack_fast_hash": s.crack_fast_hash,
                    "warning": s.warning,
                    "suggestions": s.suggestions,
                })))
            }
            "strength.compare" => {
                let c = strength::compare(text(blob(&call, 0)?)?, text(blob(&call, 1)?)?, &[]);
                let side = |s: &strength::Strength| {
                    json!({
                        "score": s.score,
                        "crack_slow_hash": s.crack_slow_hash,
                        "crack_fast_hash": s.crack_fast_hash,
                        "warning": s.warning,
                    })
                };
                Ok(Reply::view(json!({
                    "first": side(&c.first),
                    "second": side(&c.second),
                    "winner": match c.winner {
                        strength::Winner::First => "first",
                        strength::Winner::Second => "second",
                        strength::Winner::Even => "even",
                    },
                    "factor": c.factor,
                })))
            }
            "recovery.check" => {
                let check = recovery::check_phrase(text(call.secret)?);
                Ok(Reply::view(json!({"result": format!("{check:?}").split_whitespace().next()})))
            }
            "import.detect" => {
                let format = io::detect(blob(&call, 0)?);
                Ok(Reply::view(json!({"format": format.map(io::Format::name)})))
            }
            "import.csv_preview" => {
                let rows = io::preview_csv(text(blob(&call, 0)?)?, 6)?;
                Ok(Reply::view(json!(rows)))
            }
            "categories" => Ok(Reply::view(json!({
                "all": categorise::CATEGORIES,
                "kinds": categorise::KIND_CATEGORIES,
            }))),
            "otp.parse" => {
                let config = OtpConfig::from_uri(text(call.secret)?)?;
                Ok(Reply::of(&views::otp_view(&config))?)
            }
            // The name, websites and apps of a login suggest each other.
            "related.find" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    title: String,
                    sites: Vec<String>,
                    apps: Vec<String>,
                    brand: Option<related::Brand>,
                    site_brands: Vec<String>,
                    installed: Option<Vec<related::App>>,
                }
                let a: A = parse(call.args)?;
                let found = related::find(&related::Have {
                    title: &a.title,
                    sites: &a.sites,
                    apps: &a.apps,
                    brand: a.brand.as_ref(),
                    site_brands: &a.site_brands,
                    installed: a.installed.as_deref(),
                });
                Ok(Reply::of(&found)?)
            }
            "related.apps" => Ok(Reply::of(&related::known_apps())?),
            // A `WIFI:` text read from a QR code. The password comes back as the secret.
            "wifi.parse" => {
                let network = wifi::parse(text(call.secret)?).ok_or_else(|| invalid("that is not a Wi-Fi network"))?;
                Ok(Reply {
                    json: json!({"ssid": network.ssid, "security": network.security, "hidden": network.hidden}).to_string(),
                    secret: Some(Zeroizing::new(network.password.as_bytes().to_vec())),
                    blobs: Vec::new(),
                })
            }
            _ => self.vault_call(method, call),
        }
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn vault_call(&mut self, method: &str, call: Call<'_>) -> Result<Reply, SessionError> {
        let now = call.now_ms;
        let device = self.device;
        if method == "vault.lock" {
            self.sessions.remove(&call.handle);
            return Ok(Reply::ok());
        }
        if method == "shared.open" {
            let a: SharedArgs = parse(call.args)?;
            let session = self.session(call.handle)?;
            let at = session.vault.tick(now);
            let link = session
                .vault
                .doc
                .linked
                .iter_mut()
                .find(|l| l.file == a.file)
                .ok_or_else(|| invalid("no such shared vault"))?;
            let shared = link.shared.as_mut().ok_or_else(|| invalid("that vault is not shared"))?;
            if shared.owner_key.is_empty() {
                return Err(invalid("the invitation has not arrived yet"));
            }
            let keys = member_keys(shared)?;
            let owner = key32(&shared.owner_key)?;
            let manifest = Manifest::from_bytes(blob(&call, 0)?)?;
            let (key, vault_id, version) = manifest.open_latest(&keys, &owner, shared.min_version)?;
            let raw = hex(key.expose_for_hardware_wrap());
            if version != shared.min_version {
                shared.min_version = version;
                shared.vault_id = hex(&vault_id);
                link.key = Secret::from(raw);
                link.at = at;
                session.dirty = true;
            }
            let vault = Vault::from_file(blob(&call, 1)?, key, device)?;
            let handle = self.adopt(vault);
            let session = self.session(handle)?;
            return Ok(Reply::view(Self::opened(handle, 0, &session.vault)));
        }
        if method == "linked.open" {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                file: String,
            }
            let a: A = parse(call.args)?;
            let session = self.session(call.handle)?;
            let link = session
                .vault
                .doc
                .linked
                .iter()
                .find(|l| l.file == a.file)
                .ok_or_else(|| invalid("no such linked vault"))?;
            let raw = Zeroizing::new(unhex(link.key.expose()).ok_or_else(|| invalid("bad key"))?);
            let key: [u8; 32] = raw.as_slice().try_into().map_err(|_| invalid("bad key"))?;
            let vault = Vault::from_file(blob(&call, 0)?, VaultKey::from_bytes(key), device)?;
            let handle = self.adopt(vault);
            let session = self.session(handle)?;
            return Ok(Reply::view(Self::opened(handle, 0, &session.vault)));
        }
        let session = self.session(call.handle)?;
        match method {
            "vault.info" => {
                let vault = &session.vault;
                Ok(Reply::view(json!({
                    "name": vault.doc.name,
                    "generation": vault.doc.generation,
                    "read_only": vault.is_read_only(),
                    "dirty": session.dirty,
                    "shelf_open": session.shelf_open,
                    "entries": vault.doc.entries.len(),
                    "trash_days": vault.doc.prefs.trash_days,
                    "history_limit": vault.doc.prefs.history_limit,
                    "kdf": {
                        "m_kib": vault.kdf_config()?.params.m_kib,
                        "t": vault.kdf_config()?.params.t,
                        "p": vault.kdf_config()?.params.p,
                    },
                })))
            }
            "vault.rename" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    name: String,
                }
                let a: A = parse(call.args)?;
                session.vault.doc.name = a.name;
                session.dirty = true;
                Ok(Reply::ok())
            }
            "vault.prefs" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    trash_days: Option<u32>,
                    history_limit: Option<u32>,
                }
                let a: A = parse(call.args)?;
                if let Some(days) = a.trash_days {
                    session.vault.doc.prefs.trash_days = days;
                }
                if let Some(limit) = a.history_limit {
                    session.vault.doc.prefs.history_limit = limit;
                }
                session.dirty = true;
                Ok(Reply::ok())
            }
            "vault.save" => {
                let file = session.vault.save()?;
                session.dirty = false;
                Ok(Reply {
                    json: json!({
                        "generation": session.vault.doc.generation,
                        "blobs": session.vault.referenced_blobs(),
                    })
                    .to_string(),
                    blobs: vec![file],
                    ..Reply::default()
                })
            }
            "vault.sweep" => {
                let gone = session.vault.sweep(now);
                if !gone.is_empty() {
                    session.touched();
                }
                Ok(Reply::view(json!({"delete_blobs": gone})))
            }
            "vault.merge" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    seen_generation: u64,
                }
                let a: A = parse(call.args)?;
                let generation = session.vault.merge_file(blob(&call, 0)?)?;
                session.touched();
                let rollback = matches!(
                    check_freshness(a.seen_generation, generation),
                    Freshness::Rollback { .. }
                );
                Ok(Reply::view(json!({"generation": generation, "rollback": rollback})))
            }
            "remote.save" => {
                let vault = &mut session.vault;
                let Some(remote) = session.remote.as_mut() else {
                    return Err(invalid("this vault has no account behind it"));
                };
                remote.collect(vault, now)?;
                let bytes = remote.save()?;
                let unreadable = session.unreadable;
                Ok(Reply {
                    json: json!({"unreadable": unreadable}).to_string(),
                    blobs: vec![bytes],
                    ..Reply::default()
                })
            }
            "remote.kind" => Ok(Reply::view(json!({"kind": match session.remote.as_ref().map(|r| &r.account) {
                None => "vault",
                Some(crate::remote::Account::Kdbx(_)) => "kdbx",
                #[cfg(feature = "online")]
                Some(crate::remote::Account::Bitwarden(_)) => "bitwarden",
            }}))),
            "vault.chaff_name" => Ok(Reply::view(json!({"name": session.vault.chaff_file_name()?}))),
            "vault.sync_name" => Ok(Reply::view(json!({"name": session.vault.sync_file_name()?}))),
            "vault.state_hash" => Ok(Reply::view(json!({"hash": hex(&session.vault.state_hash()?)}))),
            "vault.export_key" => {
                if !session.vault.verify_password(call.secret, &[])? {
                    return Err(wrong_password());
                }
                Ok(wrapped_key(&session.vault))
            }
            // Handing the key to the platform's hardware right after an unlock, so the
            // password is not asked for twice in a row.
            "vault.key_for_wrapping" => Ok(wrapped_key(&session.vault)),
            "vault.quick_seal" => {
                let wrapping = quick_wrapping(call.secret)?;
                let sealed = wmpass_core::crypto::seal(&wrapping, session.vault.key().expose_for_hardware_wrap(), QUICK_AAD)?;
                Ok(Reply { json: "{}".to_owned(), secret: None, blobs: vec![sealed] })
            }
            "vault.verify" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    factors: Vec<String>,
                }
                let a: A = parse(call.args)?;
                let (factors, _) = split_factors(&a.factors, call.blobs)?;
                let ok = session.vault.verify_password(call.secret, &factors)?;
                Ok(Reply::view(json!({"ok": ok})))
            }
            "vault.change_password" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    index: usize,
                }
                let a: A = parse(call.args)?;
                session.vault.change_password(a.index, call.secret, now)?;
                session.dirty = true;
                Ok(Reply::ok())
            }
            "vault.rekey" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    kdf: KdfArg,
                }
                let a: A = parse(call.args)?;
                session.vault.rekey(call.secret, &a.kdf.config()?, now)?;
                session.dirty = true;
                Ok(Reply::ok())
            }
            "slot.list" => {
                let slots: Vec<Value> = session
                    .vault
                    .doc
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.used)
                    .map(|(index, s)| json!({"index": index, "label": s.label}))
                    .collect();
                Ok(Reply::view(json!(slots)))
            }
            "slot.add" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    label: String,
                    action: String,
                    factor: Option<FactorView>,
                }
                let a: A = parse(call.args)?;
                let action = action_of(&a.action)?;
                let factor = a.factor.as_ref().map(factor_of).transpose()?;
                let output = match &factor {
                    Some(FactorKind::KeyFile) => Some(format::key_file_factor(blob(&call, 0)?)),
                    Some(_) => Some(Zeroizing::new(blob(&call, 0)?.to_vec())),
                    None => None,
                };
                let pair = factor.as_ref().zip(output.as_ref());
                let index = session.vault.add_slot(call.secret, &a.label, action, pair, now)?;
                session.dirty = true;
                Ok(Reply::view(json!({"index": index})))
            }
            "slot.remove" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    index: usize,
                }
                let a: A = parse(call.args)?;
                session.vault.remove_slot(a.index, now)?;
                session.dirty = true;
                Ok(Reply::ok())
            }
            "emergency.kit" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    threshold: u8,
                    total: u8,
                }
                let a: A = parse(call.args)?;
                let shares = emergency::create_shares(&mut session.vault, a.threshold, a.total, now)?;
                session.dirty = true;
                // Whole share texts in one secret value, one per line: the owner prints
                // them and they should not sit in a string on the other side.
                let joined = Zeroizing::new(
                    shares
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                Ok(Reply::secret(&joined))
            }
            "linked.list" => {
                let links: Vec<Value> = session
                    .vault
                    .doc
                    .linked
                    .iter()
                    .map(|l| {
                        json!({
                            "file": l.file,
                            "name": l.name,
                            "shared": l.shared.is_some(),
                            "owner": l.shared.as_ref().is_some_and(|s| s.owner),
                            "pending": l.shared.as_ref().is_some_and(|s| s.owner_key.is_empty()),
                        })
                    })
                    .collect();
                Ok(Reply::view(json!(links)))
            }
            "linked.create" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    name: String,
                    kdf: KdfArg,
                }
                let a: A = parse(call.args)?;
                let config = a.kdf.config()?;
                let mut child = Vault::create(&a.name, call.secret, &config, device, now)?;
                let file = new_id()?;
                let sealed = child.save()?;
                let at = session.vault.tick(now);
                let key = Secret::from(hex(&child_key_bytes(&child)));
                session.vault.doc.linked.push(LinkedVault {
                    file: file.clone(),
                    name: a.name,
                    key,
                    at,
                    shared: None,
                });
                session.dirty = true;
                Ok(Reply {
                    json: json!({"file": file}).to_string(),
                    blobs: vec![sealed],
                    ..Reply::default()
                })
            }
            "linked.forget" => {
                let a: SharedArgs = parse(call.args)?;
                session.vault.doc.linked.retain(|l| l.file != a.file);
                session.dirty = true;
                Ok(Reply::ok())
            }
            // ---- vaults shared with other people ----
            "shared.create" => {
                let a: SharedArgs = parse(call.args)?;
                let config = KdfConfig::new(KdfParams::default())?;
                let mut child = Vault::create_shared(&a.name, &config, device, now)?;
                let keys = MemberKeys::generate()?;
                let vault_id = child.vault_id().to_vec();
                let manifest = Manifest::create(&keys, &a.my_name, &vault_id, child.key())?;
                let file = new_id()?;
                let sealed = child.save()?;
                let at = session.vault.tick(now);
                let owner_key = hex(&keys.public("").ed25519);
                session.vault.doc.linked.push(LinkedVault {
                    file: file.clone(),
                    name: a.name.clone(),
                    key: Secret::from(hex(&child_key_bytes(&child))),
                    at,
                    shared: Some(SharedLink {
                        owner_key: owner_key.clone(),
                        vault_id: hex(&vault_id),
                        min_version: 1,
                        member: member_secret(&keys),
                        owner: true,
                    }),
                });
                session.dirty = true;
                Ok(Reply {
                    json: json!({"file": file}).to_string(),
                    blobs: vec![sealed, manifest.to_bytes()?],
                    ..Reply::default()
                })
            }
            // A joining person makes their keys first and passes the public half to the owner.
            "shared.join_code" => {
                let a: SharedArgs = parse(call.args)?;
                let keys = MemberKeys::generate()?;
                let public = keys.public(&a.my_name);
                let file = new_id()?;
                let at = session.vault.tick(now);
                session.vault.doc.linked.push(LinkedVault {
                    file: file.clone(),
                    name: "Waiting for an invitation".to_owned(),
                    key: Secret::default(),
                    at,
                    shared: Some(SharedLink { member: member_secret(&keys), ..SharedLink::default() }),
                });
                session.dirty = true;
                let code = format!("{MEMBER_PREFIX}{}", B64.encode(serde_json::to_vec(&public).map_err(|_| invalid("bad keys"))?));
                Ok(Reply::view(json!({"file": file, "code": code, "fingerprint": sharing::fingerprint(&public)})))
            }
            // Shows the owner whose keys a code holds, to compare before adding them.
            "shared.read_code" => {
                let a: SharedArgs = parse(call.args)?;
                let public = member_from_code(&a.code)?;
                Ok(Reply::view(json!({"name": public.name, "fingerprint": sharing::fingerprint(&public)})))
            }
            "shared.add_member" => {
                let a: SharedArgs = parse(call.args)?;
                let link = session.vault.doc.linked.iter().find(|l| l.file == a.file).ok_or_else(|| invalid("no such shared vault"))?;
                let shared = link.shared.as_ref().filter(|s| s.owner).ok_or_else(|| invalid("only the owner can invite people"))?;
                let keys = member_keys(shared)?;
                let vault_id = unhex(&shared.vault_id).ok_or_else(|| invalid("bad id"))?;
                let key = VaultKey::from_bytes(key32(link.key.expose())?);
                let public = member_from_code(&a.code)?;
                let manifest = Manifest::from_bytes(blob(&call, 0)?)?.add_member(&keys, &vault_id, &key, public)?;
                let invite = json!({"file": link.file, "name": link.name, "owner_key": shared.owner_key});
                let invite = format!("{INVITE_PREFIX}{}", B64.encode(invite.to_string()));
                Ok(Reply { json: json!({"invite": invite}).to_string(), blobs: vec![manifest.to_bytes()?], ..Reply::default() })
            }
            "shared.accept" => {
                let a: SharedArgs = parse(call.args)?;
                let text = a.code.trim().strip_prefix(INVITE_PREFIX).ok_or_else(|| invalid("that is not an invitation"))?;
                let raw = B64.decode(text).map_err(|_| invalid("that is not an invitation"))?;
                let invite: Value = serde_json::from_slice(&raw).map_err(|_| invalid("that is not an invitation"))?;
                let at = session.vault.tick(now);
                let links = &mut session.vault.doc.linked;
                let link = links.iter_mut().find(|l| l.file == a.file).ok_or_else(|| invalid("no invitation was asked for"))?;
                let shared = link.shared.as_mut().ok_or_else(|| invalid("no invitation was asked for"))?;
                let text_of = |key: &str| invite[key].as_str().map(str::to_owned).ok_or_else(|| invalid("that is not an invitation"));
                shared.owner_key = text_of("owner_key")?;
                key32(&shared.owner_key)?;
                link.file = text_of("file")?;
                link.name = text_of("name")?;
                link.at = at;
                let file = link.file.clone();
                session.dirty = true;
                Ok(Reply::view(json!({"file": file})))
            }
            "shared.members" => {
                let a: SharedArgs = parse(call.args)?;
                let link = session.vault.doc.linked.iter().find(|l| l.file == a.file).ok_or_else(|| invalid("no such shared vault"))?;
                let shared = link.shared.as_ref().ok_or_else(|| invalid("that vault is not shared"))?;
                let vault_id = unhex(&shared.vault_id).ok_or_else(|| invalid("bad id"))?;
                let members = Manifest::from_bytes(blob(&call, 0)?)?.members(&key32(&shared.owner_key)?, &vault_id)?;
                let rows: Vec<Value> = members
                    .iter()
                    .map(|m| json!({"name": m.name, "key": hex(&m.ed25519), "fingerprint": sharing::fingerprint(m), "owner": hex(&m.ed25519) == shared.owner_key}))
                    .collect();
                Ok(Reply::view(json!(rows)))
            }
            // Removing someone moves the vault to a new key, so what they hold stops opening
            // anything written after this. Copies they already have stay readable.
            "shared.remove_member" => {
                let a: SharedArgs = parse(call.args)?;
                let at = session.vault.tick(now);
                let link = session.vault.doc.linked.iter_mut().find(|l| l.file == a.file).ok_or_else(|| invalid("no such shared vault"))?;
                let shared = link.shared.as_mut().filter(|s| s.owner).ok_or_else(|| invalid("only the owner can remove people"))?;
                let keys = member_keys(shared)?;
                let old_id = unhex(&shared.vault_id).ok_or_else(|| invalid("bad id"))?;
                let manifest = Manifest::from_bytes(blob(&call, 0)?)?;
                let mut child = Vault::from_file(blob(&call, 1)?, VaultKey::from_bytes(key32(link.key.expose())?), device)?;
                child.rekey_shared(&KdfConfig::new(KdfParams::default())?, now)?;
                let new_id = child.vault_id().to_vec();
                let next = manifest.remove_member(&keys, &old_id, &new_id, child.key(), &key32(&a.key)?)?;
                shared.vault_id = hex(&new_id);
                shared.min_version += 1;
                link.key = Secret::from(hex(&child_key_bytes(&child)));
                link.at = at;
                let sealed = child.save()?;
                session.dirty = true;
                Ok(Reply { json: "{}".to_owned(), blobs: vec![next.to_bytes()?, sealed], ..Reply::default() })
            }
            // ---- reading ----
            "entry.list" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    query: String,
                    sort: String,
                }
                let a: A = parse(call.args)?;
                let mut flags = None;
                if wants_health(&a.query) {
                    flags = Some(health_flags(session.report(now, 365)));
                }
                let query = Query::parse(&a.query);
                let names = Names(&session.vault);
                let context = Context {
                    now_ms: now,
                    flags: flags.as_ref(),
                    folder_name: Some(&names),
                };
                let shelf_open = session.shelf_open;
                let mut rows: Vec<views::Summary> = search::search(
                    session.vault.doc.entries.values(),
                    &query,
                    &context,
                )
                .into_iter()
                .filter(|e| shelf_open || !e.common.hidden.v)
                .map(views::summary)
                .collect();
                sort_rows(&mut rows, &a.sort);
                Ok(Reply::of(&rows)?)
            }
            "entry.get" => {
                let a: Id = parse(call.args)?;
                Ok(Reply::of(&views::detail(session.entry(&a.id)?))?)
            }
            "entry.reveal" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    field: String,
                }
                let a: A = parse(call.args)?;
                let entry = session.entry(&a.id)?;
                let mut scratch = entry.clone();
                let value = if a.field == "notes" {
                    Some(entry.common.notes.v.clone())
                } else {
                    fields::secret_of(&mut scratch, &a.field)
                }
                .ok_or_else(|| invalid("no such secret field"))?;
                Ok(Reply::secret(value.expose()))
            }
            "entry.reveal_custom" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    index: usize,
                }
                let a: A = parse(call.args)?;
                let field = session
                    .entry(&a.id)?
                    .common
                    .custom_fields
                    .v
                    .get(a.index)
                    .ok_or_else(|| invalid("no such field"))?;
                Ok(Reply::secret(field.value.expose()))
            }
            "entry.reveal_question" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    index: usize,
                }
                let a: A = parse(call.args)?;
                let answer = session
                    .entry(&a.id)?
                    .login()
                    .and_then(|l| l.questions.v.get(a.index))
                    .ok_or_else(|| invalid("no such question"))?;
                Ok(Reply::secret(answer.value.expose()))
            }
            "entry.reveal_history" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    index: usize,
                }
                let a: A = parse(call.args)?;
                let revision = session
                    .entry(&a.id)?
                    .history
                    .get(a.index)
                    .ok_or_else(|| invalid("no such revision"))?;
                Ok(Reply::secret(revision.value.expose()))
            }
            "otp.codes" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    ids: Option<Vec<String>>,
                }
                let a: A = parse(call.args)?;
                let seconds = now / 1000;
                let mut out = Vec::new();
                for entry in session.vault.visible() {
                    if a.ids.as_ref().is_some_and(|ids| !ids.contains(&entry.id)) {
                        continue;
                    }
                    let Some(config) = otp_of(entry) else {
                        continue;
                    };
                    let view = views::otp_view(config);
                    // A code on screen is meant to be read, and it lasts seconds. HOTP
                    // has to be asked for, since showing it uses it up.
                    let code = if view.kind == "hotp" {
                        None
                    } else {
                        Some(config.code(seconds)?.expose().to_owned())
                    };
                    let left = config.seconds_left(seconds);
                    // Near the end the next code is shown too, so a late copy still works.
                    let next_code = match left {
                        Some(left) if left <= 10 => {
                            Some(config.code(seconds + u64::from(left))?.expose().to_owned())
                        }
                        _ => None,
                    };
                    out.push(json!({
                        "id": entry.id,
                        "title": entry.common.title.v,
                        "site": health::entry_domains(entry).into_iter().next(),
                        "icon": entry.common.icon.v,
                        "otp": view,
                        "code": code,
                        "next_code": next_code,
                        "seconds_left": left,
                    }));
                }
                Ok(Reply::view(json!(out)))
            }
            "otp.code" => {
                let a: Id = parse(call.args)?;
                let seconds = now / 1000;
                let entry = session.entry(&a.id)?;
                let config = otp_of(entry).ok_or_else(|| invalid("no one-time codes here"))?;
                Ok(Reply {
                    json: json!({"seconds_left": config.seconds_left(seconds)}).to_string(),
                    secret: Some(Zeroizing::new(config.code(seconds)?.expose().as_bytes().to_vec())),
                    blobs: Vec::new(),
                })
            }
            "otp.hotp_next" => {
                let a: Id = parse(call.args)?;
                let seconds = now / 1000;
                let code = {
                    let entry = session.entry(&a.id)?;
                    otp_of(entry)
                        .ok_or_else(|| invalid("no one-time codes here"))?
                        .code(seconds)?
                };
                session.vault.edit(&a.id, now, |entry, at| match &mut entry.kind {
                    Kind::Login(login) => {
                        if let Some(mut config) = login.otp.v.clone() {
                            config.advance();
                            login.otp = Lww::new(Some(config), at);
                        }
                    }
                    Kind::Otp(otp) => {
                        if let Some(mut config) = otp.config.v.clone() {
                            config.advance();
                            otp.config = Lww::new(Some(config), at);
                        }
                    }
                    _ => {}
                })?;
                session.touched();
                Ok(Reply::secret(code.expose()))
            }
            "folder.list" => {
                let folders: Vec<Value> = session
                    .vault
                    .doc
                    .folders
                    .values()
                    .map(|f| json!({"id": f.id, "name": f.name.v, "parent": f.parent.v}))
                    .collect();
                Ok(Reply::view(json!(folders)))
            }
            "health.report" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    old_after_days: Option<u32>,
                    breached: BTreeMap<String, u64>,
                    ignored: BTreeMap<String, std::collections::BTreeSet<String>>,
                    skip_local: bool,
                }
                let a: A = parse(call.args)?;
                session.health = Some(health::report(
                    &session.vault,
                    &health::HealthInput {
                        now_ms: now,
                        old_after_days: a.old_after_days.unwrap_or(365),
                        breached: a.breached,
                        ignored: a.ignored,
                        skip_local: a.skip_local,
                    },
                ));
                let report = session.report(now, 365);
                Ok(Reply::view(json!({
                    "weak": report.weak,
                    "reused": report.reused,
                    "old": report.old,
                    "breached": report.breached,
                    "missing_two_factor": report.missing_two_factor,
                    "insecure_url": report.insecure_url,
                    "duplicates": report.duplicates,
                    "similar": report.similar,
                    "expiring": report.expiring,
                    "score": report.score,
                })))
            }
            "categorise.suggest" => {
                let a: Id = parse(call.args)?;
                let entry = session.entry(&a.id)?;
                let suggestion = categorise::suggest(entry);
                Ok(Reply::view(json!({
                    "category": suggestion.map(|s| s.category),
                    "categories": categorise::suggest_all(entry),
                    // The site's alone, without the kind's, for tagging a new item.
                    "site": categorise::site_category(entry).map(|(category, _)| category),
                    // How sure that is: the site list, the item's name, or the classifier's guess.
                    "site_source": categorise::site_category(entry).map(|(_, source)| source.word()),
                })))
            }
            // What each live item would be tagged, and how sure each tag is, for adding them in bulk.
            "categorise.all" => {
                let rows: Vec<Value> = session
                    .vault
                    .doc
                    .entries
                    .values()
                    .filter(|entry| !entry.is_trashed())
                    .map(|entry| {
                        let site = categorise::site_category(entry);
                        json!({
                            "id": entry.id,
                            "title": entry.common.title.v,
                            "kind": entry.kind.name(),
                            "tags": entry.common.tags.v,
                            "site": site.map(|(category, _)| category),
                            "source": site.map(|(_, source)| source.word()),
                            "by_kind": categorise::kind_suggestion(entry),
                        })
                    })
                    .collect();
                Ok(Reply::view(json!(rows)))
            }
            // The QR code another phone's camera reads to join a Wi-Fi item's network. The
            // password goes into the code without leaving the session as text.
            "wifi.qr" => {
                let a: Id = parse(call.args)?;
                let Kind::Wifi(network) = &session.entry(&a.id)?.kind else {
                    return Err(invalid("only a Wi-Fi network has a QR code"));
                };
                let security = fields::wifi_name(network.security.v);
                let code = wifi::code(&network.ssid.v, security, network.password.v.expose(), network.hidden_ssid.v)
                    .ok_or_else(|| invalid("an enterprise network or one without a name has no QR code"))?;
                qr_view(code.as_bytes())
            }
            "tags.icons" => {
                let icons: BTreeMap<&str, &str> = session
                    .vault
                    .doc
                    .prefs
                    .tag_icons
                    .iter()
                    .filter(|(_, icon)| !icon.v.is_empty())
                    .map(|(tag, icon)| (tag.as_str(), icon.v.as_str()))
                    .collect();
                Ok(Reply::view(json!(icons)))
            }
            "tags.set_icon" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    tag: String,
                    icon: Option<String>,
                }
                let a: A = parse(call.args)?;
                if a.tag.trim().is_empty() {
                    return Err(invalid("a tag has a name"));
                }
                let at = session.vault.tick(now);
                session.vault.doc.prefs.tag_icons.insert(a.tag, Lww::new(a.icon.unwrap_or_default(), at));
                session.touched();
                Ok(Reply::ok())
            }
            "prefs.synced" => {
                let values: serde_json::Map<String, Value> = session
                    .vault
                    .doc
                    .prefs
                    .app
                    .iter()
                    .filter(|(_, value)| !value.v.is_empty())
                    .filter_map(|(key, value)| Some((key.clone(), serde_json::from_str(&value.v).ok()?)))
                    .collect();
                Ok(Reply::view(Value::Object(values)))
            }
            "prefs.sync" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    values: serde_json::Map<String, Value>,
                }
                let a: A = parse(call.args)?;
                let held = &session.vault.doc.prefs.app;
                let mut changes = Vec::new();
                for (key, value) in a.values {
                    let fits = (1..=SYNCED_KEY_MAX).contains(&key.len())
                        && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-:".contains(&b));
                    if !fits {
                        return Err(invalid("a setting's name is short and plain"));
                    }
                    let text = if value.is_null() { String::new() } else { value.to_string() };
                    if text.len() > SYNCED_VALUE_MAX {
                        return Err(invalid("that setting is too long to sync"));
                    }
                    if held.get(&key).is_none_or(|current| current.v != text) {
                        changes.push((key, text));
                    }
                }
                if held.len() + changes.len() > SYNCED_KEYS_MAX {
                    return Err(invalid("too many synced settings"));
                }
                let changed = changes.len();
                if changed > 0 {
                    let at = session.vault.tick(now);
                    for (key, text) in changes {
                        session.vault.doc.prefs.app.insert(key, Lww::new(text, at));
                    }
                    session.dirty = true;
                }
                Ok(Reply::view(json!({ "changed": changed })))
            }
            "match.web" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    url: String,
                }
                let a: A = parse(call.args)?;
                Ok(match_rows(&session.vault, Target::Web { url: &a.url }))
            }
            // Only for an entry that really matches the page, so a request cannot name any login
            // and carry its password off to some other site.
            "match.fill" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    url: String,
                }
                let a: A = parse(call.args)?;
                let seconds = now / 1000;
                let entry = session.entry(&a.id)?;
                if matching::match_entry(entry, Target::Web { url: &a.url }).is_none() {
                    return Err(invalid("that entry does not belong to this page"));
                }
                let Kind::Login(login) = &entry.kind else {
                    return Err(invalid("that entry is not a login"));
                };
                let code = match otp_of(entry) {
                    Some(config) => Some(config.code(seconds)?),
                    None => None,
                };
                let filled = Zeroizing::new(serde_json::to_vec(&json!({
                    "username": login.username.v,
                    "password": login.password.v.expose(),
                    "code": code.as_ref().map(|c| c.expose().to_owned()),
                })).map_err(|_| invalid("that entry cannot be filled"))?);
                Ok(Reply { json: "{}".to_owned(), secret: Some(filled), blobs: Vec::new() })
            }
            "match.app" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    package: String,
                    cert_sha256: String,
                }
                let a: A = parse(call.args)?;
                Ok(match_rows(
                    &session.vault,
                    Target::App {
                        package: &a.package,
                        cert_sha256: &a.cert_sha256,
                    },
                ))
            }
            // ---- the hidden shelf ----
            "shelf.open" => {
                if !session.vault.verify_password(call.secret, &[])? {
                    return Err(wrong_password());
                }
                session.shelf_open = true;
                Ok(Reply::ok())
            }
            "shelf.open_verified" => {
                session.shelf_open = true;
                Ok(Reply::ok())
            }
            "shelf.close" => {
                session.shelf_open = false;
                Ok(Reply::ok())
            }
            // ---- changing entries ----
            "entry.create" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    kind: String,
                    title: String,
                    folder: Option<String>,
                    fields: BTreeMap<String, Value>,
                    sites: Vec<SiteMatcher>,
                    color: Option<String>,
                }
                let a: A = parse(call.args)?;
                let at = session.vault.tick(now);
                let mut entry = Entry {
                    kind: fields::new_kind(&a.kind)?,
                    ..Entry::default()
                };
                entry.common.title = Lww::new(a.title, at);
                entry.common.folder = Lww::new(a.folder, at);
                entry.common.color = Lww::new(a.color, at);
                for (key, value) in &a.fields {
                    fields::set_plain(&mut entry, key, value, at)?;
                }
                if let Some(login) = entry.login_mut() {
                    login.site_matchers = Lww::new(a.sites, at);
                }
                let id = session.vault.add(entry, now)?;
                session.touched();
                Ok(Reply::view(json!({"id": id})))
            }
            "entry.set" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    fields: BTreeMap<String, Value>,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                session.vault.edit(&a.id, now, |entry, at| {
                    a.fields
                        .iter()
                        .try_for_each(|(key, value)| fields::set_plain(entry, key, value, at))
                })??;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.set_secret" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    field: String,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                if a.field == "notes" {
                    let value = Secret::new(text(call.secret)?);
                    session.vault.edit(&a.id, now, |entry, at| {
                        if entry.common.notes.v != value {
                            entry.common.notes = Lww::new(value, at);
                        }
                    })?;
                } else {
                    session.vault.edit(&a.id, now, |entry, at| {
                        fields::set_secret(entry, &a.field, call.secret, at)
                    })??;
                }
                session.touched();
                Ok(Reply::ok())
            }
            "entry.common" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    title: Option<String>,
                    #[serde(deserialize_with = "nullable")]
                    icon: Option<Option<String>>,
                    #[serde(deserialize_with = "nullable")]
                    color: Option<Option<String>>,
                    pinned: Option<bool>,
                    favourite: Option<bool>,
                    #[serde(deserialize_with = "nullable")]
                    folder: Option<Option<String>>,
                    tags: Option<Vec<String>>,
                    links: Option<Vec<String>>,
                    hidden: Option<bool>,
                    #[serde(deserialize_with = "nullable")]
                    destroy_at_ms: Option<Option<u64>>,
                    reminders: Option<views::RemindersView>,
                    sites: Option<Vec<SiteMatcher>>,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                session.vault.edit(&a.id, now, |entry, at| {
                    let common = &mut entry.common;
                    if let Some(v) = a.title {
                        common.title = Lww::new(v, at);
                    }
                    if let Some(v) = a.icon {
                        common.icon = Lww::new(v, at);
                    }
                    if let Some(v) = a.color {
                        common.color = Lww::new(v, at);
                    }
                    if let Some(v) = a.pinned {
                        common.pinned = Lww::new(v, at);
                    }
                    if let Some(v) = a.favourite {
                        common.favourite = Lww::new(v, at);
                    }
                    if let Some(v) = a.folder {
                        common.folder = Lww::new(v, at);
                    }
                    if let Some(v) = a.tags {
                        common.tags = Lww::new(v, at);
                    }
                    if let Some(v) = a.links {
                        common.links = Lww::new(v, at);
                    }
                    if let Some(v) = a.hidden {
                        common.hidden = Lww::new(v, at);
                    }
                    if let Some(v) = a.destroy_at_ms {
                        common.destroy_at_ms = Lww::new(v, at);
                    }
                    if let Some(v) = a.reminders {
                        common.reminders = Lww::new(
                            Reminders {
                                change_every_days: v.change_every_days,
                                expires_ms: v.expires_ms,
                                warn_days: v.warn_days,
                            },
                            at,
                        );
                    }
                    if let (Some(v), Some(login)) = (a.sites, entry.login_mut()) {
                        login.site_matchers = Lww::new(v, at);
                    }
                })?;
                session.touched();
                Ok(Reply::ok())
            }
            // A code that gains a username and password becomes a login that keeps its code.
            "entry.convert" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    kind: String,
                }
                let a: A = parse(call.args)?;
                if a.kind != "login" {
                    return Err(invalid("only a code can become a login"));
                }
                if !matches!(session.entry(&a.id)?.kind, Kind::Otp(_)) {
                    return Err(invalid("only a code can become a login"));
                }
                session.vault.edit(&a.id, now, |entry, at| {
                    let config = match &entry.kind {
                        Kind::Otp(otp) => otp.config.v.clone(),
                        _ => None,
                    };
                    entry.kind = Kind::Login(wmpass_core::model::Login {
                        otp: Lww::new(config, at),
                        ..wmpass_core::model::Login::default()
                    });
                })?;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.custom_set" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    index: Option<usize>,
                    name: String,
                    kind: String,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                let value = Secret::new(text(call.secret)?);
                session.vault.edit(&a.id, now, |entry, at| {
                    let mut list = entry.common.custom_fields.v.clone();
                    let field = Field {
                        name: a.name,
                        value,
                        kind: views::field_kind_from(&a.kind),
                    };
                    match a.index.and_then(|i| list.get_mut(i)) {
                        Some(slot) => *slot = field,
                        None => list.push(field),
                    }
                    entry.common.custom_fields = Lww::new(list, at);
                })?;
                session.touched();
                Ok(Reply::ok())
            }
            // A custom field's name or kind, keeping its value: a field made plain can turn secret
            // without being typed again, and the other way.
            "entry.custom_meta" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    index: usize,
                    name: Option<String>,
                    kind: Option<String>,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                if a.name.as_deref().is_some_and(|name| name.trim().is_empty()) {
                    return Err(invalid("a field has a name"));
                }
                session.vault.edit(&a.id, now, |entry, at| {
                    let mut list = entry.common.custom_fields.v.clone();
                    let Some(field) = list.get_mut(a.index) else { return Err(invalid("that field is gone")) };
                    let mut changed = false;
                    if let Some(name) = a.name.as_deref().map(str::trim)
                        && field.name != name
                    {
                        field.name = name.to_owned();
                        changed = true;
                    }
                    if let Some(kind) = a.kind.as_deref().map(views::field_kind_from)
                        && field.kind != kind
                    {
                        field.kind = kind;
                        changed = true;
                    }
                    if changed {
                        entry.common.custom_fields = Lww::new(list, at);
                    }
                    Ok(())
                })??;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.custom_remove" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    index: usize,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                session.vault.edit(&a.id, now, |entry, at| {
                    let mut list = entry.common.custom_fields.v.clone();
                    if a.index < list.len() {
                        list.remove(a.index);
                        entry.common.custom_fields = Lww::new(list, at);
                    }
                })?;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.question_set" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    index: Option<usize>,
                    question: String,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                let value = Secret::new(text(call.secret)?);
                session.vault.edit(&a.id, now, |entry, at| {
                    if let Some(login) = entry.login_mut() {
                        let mut list = login.questions.v.clone();
                        let field = Field {
                            name: a.question,
                            value,
                            kind: wmpass_core::model::FieldKind::Hidden,
                        };
                        match a.index.and_then(|i| list.get_mut(i)) {
                            Some(slot) => *slot = field,
                            None => list.push(field),
                        }
                        login.questions = Lww::new(list, at);
                    }
                })?;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.otp_set" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    clear: bool,
                    issuer: Option<String>,
                    account: Option<String>,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                let config = if a.clear {
                    None
                } else {
                    let raw = text(call.secret)?.trim();
                    let mut config = if raw.contains("://") {
                        OtpConfig::from_uri(raw)?
                    } else {
                        let seed: String = raw.split_whitespace().collect();
                        // Checked now so a mistyped seed fails here, not at login time.
                        wmpass_core::otp::base32_decode(&seed)?;
                        OtpConfig::totp(seed)
                    };
                    if let Some(issuer) = a.issuer {
                        config.issuer = issuer;
                    }
                    if let Some(account) = a.account {
                        config.account = account;
                    }
                    Some(config)
                };
                session.vault.edit(&a.id, now, |entry, at| match &mut entry.kind {
                    Kind::Login(login) => {
                        login.otp = Lww::new(config, at);
                        Ok(())
                    }
                    Kind::Otp(otp) => {
                        otp.config = Lww::new(config, at);
                        Ok(())
                    }
                    _ => Err(invalid("this kind has no one-time codes")),
                })??;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.trash" => {
                let a: Id = parse(call.args)?;
                session.entry(&a.id)?;
                session.vault.trash(&a.id, now)?;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.restore" => {
                let a: Id = parse(call.args)?;
                session.entry(&a.id)?;
                session.vault.restore(&a.id, now)?;
                session.touched();
                Ok(Reply::ok())
            }
            "entry.purge" => {
                let a: Id = parse(call.args)?;
                session.entry(&a.id)?;
                let blobs = session.vault.purge(&a.id, now);
                session.touched();
                Ok(Reply::view(json!({"delete_blobs": blobs})))
            }
            "entry.duplicate" => {
                let a: Id = parse(call.args)?;
                let mut copy = session.entry(&a.id)?.clone();
                copy.id = String::new();
                copy.created_ms = 0;
                copy.history.clear();
                // A copy's attachments would otherwise share blobs with the original,
                // and purging either would take the other's file.
                let at = session.vault.tick(now);
                copy.common.attachments = Lww::new(Vec::new(), at);
                let id = session.vault.add(copy, now)?;
                session.touched();
                Ok(Reply::view(json!({"id": id})))
            }
            "entries.bulk" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    ids: Vec<String>,
                    op: String,
                    folder: Option<String>,
                    color: Option<String>,
                    tag: String,
                    /// The new name, for a rename.
                    to: String,
                    flag: bool,
                }
                let a: A = parse(call.args)?;
                if a.op == "tag_rename" && a.to.trim().is_empty() {
                    return Err(invalid("a tag has a name"));
                }
                let mut delete_blobs = Vec::new();
                for id in &a.ids {
                    session.entry(id)?;
                    match a.op.as_str() {
                        "trash" => session.vault.trash(id, now)?,
                        "restore" => session.vault.restore(id, now)?,
                        "purge" => delete_blobs.extend(session.vault.purge(id, now)),
                        "folder" => session.vault.edit(id, now, |e, at| {
                            e.common.folder = Lww::new(a.folder.clone(), at);
                        })?,
                        "color" => session.vault.edit(id, now, |e, at| {
                            e.common.color = Lww::new(a.color.clone(), at);
                        })?,
                        "favourite" => session.vault.edit(id, now, |e, at| {
                            e.common.favourite = Lww::new(a.flag, at);
                        })?,
                        "pin" => session.vault.edit(id, now, |e, at| {
                            e.common.pinned = Lww::new(a.flag, at);
                        })?,
                        "hide" => session.vault.edit(id, now, |e, at| {
                            e.common.hidden = Lww::new(a.flag, at);
                        })?,
                        "tag_add" => session.vault.edit(id, now, |e, at| {
                            let mut tags = e.common.tags.v.clone();
                            if !tags.contains(&a.tag) {
                                tags.push(a.tag.clone());
                                e.common.tags = Lww::new(tags, at);
                            }
                        })?,
                        "tag_remove" => session.vault.edit(id, now, |e, at| {
                            let mut tags = e.common.tags.v.clone();
                            tags.retain(|t| *t != a.tag);
                            e.common.tags = Lww::new(tags, at);
                        })?,
                        // In place, so the tag keeps its spot; merged away if the item already had the new name.
                        "tag_rename" => session.vault.edit(id, now, |e, at| {
                            let mut tags: Vec<String> = Vec::new();
                            for tag in &e.common.tags.v {
                                let name = if *tag == a.tag { &a.to } else { tag };
                                if !tags.contains(name) {
                                    tags.push(name.clone());
                                }
                            }
                            e.common.tags = Lww::new(tags, at);
                        })?,
                        _ => return Err(invalid("unknown bulk action")),
                    }
                }
                session.touched();
                Ok(Reply::view(json!({"delete_blobs": delete_blobs})))
            }
            // Several items folded into one: first what differs between them, then the merge with
            // the owner's answers.
            "entries.merge_plan" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    ids: Vec<String>,
                }
                let a: A = parse(call.args)?;
                Ok(Reply::view(crate::combine::plan(&session.vault, &a.ids)?))
            }
            "entries.merge" => {
                let a: crate::combine::Choices = parse(call.args)?;
                crate::combine::merge(&mut session.vault, &a, now)?;
                session.touched();
                Ok(Reply::ok())
            }
            "health.merge_duplicates" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    keep: String,
                    remove: String,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.keep)?;
                session.entry(&a.remove)?;
                health::merge_duplicates(&mut session.vault, &a.keep, &a.remove, now)?;
                session.touched();
                Ok(Reply::ok())
            }
            "folder.add" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    name: String,
                    parent: Option<String>,
                }
                let a: A = parse(call.args)?;
                let id = session.vault.add_folder(&a.name, a.parent, now)?;
                session.touched();
                Ok(Reply::view(json!({"id": id})))
            }
            "folder.rename" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    name: String,
                }
                let a: A = parse(call.args)?;
                let at = session.vault.tick(now);
                let folder = session
                    .vault
                    .doc
                    .folders
                    .get_mut(&a.id)
                    .ok_or_else(|| invalid("no such folder"))?;
                folder.name = Lww::new(a.name, at);
                session.touched();
                Ok(Reply::ok())
            }
            "folder.remove" => {
                let a: Id = parse(call.args)?;
                let at = session.vault.tick(now);
                session.vault.doc.folders.remove(&a.id);
                session.vault.doc.tombstones.insert(a.id.clone(), at);
                let inside: Vec<String> = session
                    .vault
                    .doc
                    .entries
                    .values()
                    .filter(|e| e.common.folder.v.as_deref() == Some(a.id.as_str()))
                    .map(|e| e.id.clone())
                    .collect();
                for id in inside {
                    session.vault.edit(&id, now, |e, at| {
                        e.common.folder = Lww::new(None, at);
                    })?;
                }
                session.touched();
                Ok(Reply::ok())
            }
            // ---- attachments ----
            "attachment.add" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    name: String,
                    mime: String,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                let (record, sealed) = Vault::seal_attachment(&a.name, &a.mime, blob(&call, 0)?)?;
                let name = record.blob.clone();
                session.vault.edit(&a.id, now, |entry, at| {
                    let mut list = entry.common.attachments.v.clone();
                    list.push(record);
                    entry.common.attachments = Lww::new(list, at);
                })?;
                session.touched();
                Ok(Reply {
                    json: json!({"blob": name}).to_string(),
                    blobs: vec![sealed],
                    ..Reply::default()
                })
            }
            "attachment.open" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    blob: String,
                }
                let a: A = parse(call.args)?;
                let record = session
                    .entry(&a.id)?
                    .common
                    .attachments
                    .v
                    .iter()
                    .find(|r| r.blob == a.blob)
                    .ok_or_else(|| invalid("no such attachment"))?;
                let plain = Vault::open_attachment(record, blob(&call, 0)?)?;
                Ok(Reply {
                    json: "{}".to_owned(),
                    secret: Some(plain),
                    blobs: Vec::new(),
                })
            }
            "attachment.remove" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    blob: String,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                session.vault.edit(&a.id, now, |entry, at| {
                    let mut list = entry.common.attachments.v.clone();
                    list.retain(|r| r.blob != a.blob);
                    entry.common.attachments = Lww::new(list, at);
                })?;
                session.touched();
                Ok(Reply::view(json!({"delete_blobs": [a.blob]})))
            }
            // ---- import and export ----
            "import.read" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    format: Option<String>,
                    /// The last blob is a YubiKey's answer to `kdbx.challenge`.
                    response: bool,
                }
                let a: A = parse(call.args)?;
                let data = blob(&call, 0)?;
                let format = match a.format.as_deref() {
                    Some(name) => format_by_name(name).ok_or_else(|| invalid("unknown format"))?,
                    None => io::detect(data)
                        .ok_or_else(|| invalid("this file's format was not recognized"))?,
                };
                let password = (!call.secret.is_empty())
                    .then(|| text(call.secret))
                    .transpose()?;
                // A KeePass key file and a YubiKey's answer, when the database has them.
                let (key_file, response) = kdbx_keys(&call, a.response)?;
                let at = session.vault.tick(now);
                let imported = io::import_with_keys(format, data, password, key_file, response, at)?;
                let titles: Vec<&str> = imported
                    .entries
                    .iter()
                    .take(50)
                    .map(|e| e.entry.common.title.v.as_str())
                    .collect();
                let reply = json!({
                    "format": format.name(),
                    "count": imported.entries.len(),
                    "titles": titles,
                    "skipped": imported.skipped,
                });
                session.pending_import = Some(imported);
                Ok(Reply::view(reply))
            }
            "import.apply" => {
                let imported = session
                    .pending_import
                    .take()
                    .ok_or_else(|| invalid("nothing to import"))?;
                let report = io::apply(&mut session.vault, imported, now)?;
                session.touched();
                let (names, sealed): (Vec<String>, Vec<Vec<u8>>) = report.blobs.into_iter().unzip();
                Ok(Reply {
                    json: json!({
                        "added": report.added,
                        "duplicates": report.duplicates,
                        "blob_names": names,
                    })
                    .to_string(),
                    blobs: sealed,
                    ..Reply::default()
                })
            }
            "import.cancel" => {
                session.pending_import = None;
                Ok(Reply::ok())
            }
            "export.file" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    format: String,
                }
                let a: A = parse(call.args)?;
                let value = match a.format.as_str() {
                    "csv" => io::export::to_csv(&session.vault)?,
                    "bitwarden" => io::export::to_bitwarden_json(&session.vault)?,
                    "html" => io::export::to_print_html(&session.vault),
                    "kdbx" => {
                        let entries: Vec<(&Entry, String)> = session
                            .vault
                            .visible()
                            .map(|entry| (entry, io::export::folder_path(&session.vault, entry)))
                            .collect();
                        let file = io::kdbx::export_in_folders(&entries, text(call.secret)?)?;
                        return Ok(Reply {
                            json: "{}".to_owned(),
                            blobs: vec![file],
                            ..Reply::default()
                        });
                    }
                    _ => return Err(invalid("unknown export format")),
                };
                Ok(Reply::secret(value.expose()))
            }
            // ---- passkeys and SSH ----
            "passkey.create" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: Option<String>,
                    rp_id: String,
                    rp_name: String,
                    user_handle: String,
                    user_name: String,
                    user_verified: bool,
                    origin_host: String,
                }
                let a: A = parse(call.args)?;
                if !passkey::rp_id_allowed(&a.origin_host, &a.rp_id) {
                    return Err(invalid("this site may not use that passkey domain"));
                }
                let handle = B64
                    .decode(&a.user_handle)
                    .map_err(|_| invalid("bad user handle"))?;
                let created = passkey::create(
                    &a.rp_id,
                    &a.rp_name,
                    &handle,
                    &a.user_name,
                    a.user_verified,
                    now,
                )?;
                let id = match a.id {
                    Some(id) => {
                        session.entry(&id)?;
                        id
                    }
                    None => {
                        let at = session.vault.tick(now);
                        let mut entry = Entry {
                            kind: fields::new_kind("login")?,
                            ..Entry::default()
                        };
                        entry.common.title = Lww::new(a.rp_name.clone(), at);
                        if let Some(login) = entry.login_mut() {
                            login.username = Lww::new(a.user_name.clone(), at);
                            login.site_matchers =
                                Lww::new(vec![SiteMatcher::Domain(a.rp_id.clone())], at);
                        }
                        session.vault.add(entry, now)?
                    }
                };
                session.vault.edit(&id, now, |entry, at| {
                    if let Some(login) = entry.login_mut() {
                        let mut list = login.passkeys.v.clone();
                        list.push(created.passkey.clone());
                        login.passkeys = Lww::new(list, at);
                    }
                })?;
                session.touched();
                Ok(Reply::view(json!({
                    "id": id,
                    "credential_id": B64.encode(&created.credential_id),
                    "attestation_object": B64.encode(&created.attestation_object),
                    "authenticator_data": B64.encode(&created.authenticator_data),
                    "public_key_der": B64.encode(&created.public_key_der),
                })))
            }
            "passkey.list" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    rp_id: String,
                    origin_host: String,
                }
                let a: A = parse(call.args)?;
                if !passkey::rp_id_allowed(&a.origin_host, &a.rp_id) {
                    return Err(invalid("this site may not use that passkey domain"));
                }
                let mut out = Vec::new();
                for entry in session.vault.visible() {
                    let Some(login) = entry.login() else { continue };
                    for key in login.passkeys.v.iter().filter(|k| k.rp_id == a.rp_id) {
                        out.push(json!({
                            "id": entry.id,
                            "title": entry.common.title.v,
                            "credential_id": key.credential_id,
                            "user_name": key.user_name,
                            "rp_id": key.rp_id,
                        }));
                    }
                }
                Ok(Reply::view(json!(out)))
            }
            "passkey.sign" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    credential_id: String,
                    client_data_hash: String,
                    user_verified: bool,
                    origin_host: String,
                }
                let a: A = parse(call.args)?;
                let hash: [u8; 32] = B64
                    .decode(&a.client_data_hash)
                    .map_err(|_| invalid("bad hash"))?
                    .try_into()
                    .map_err(|_| invalid("bad hash"))?;
                let key = session
                    .entry(&a.id)?
                    .login()
                    .and_then(|l| l.passkeys.v.iter().find(|k| k.credential_id == a.credential_id))
                    .ok_or_else(|| invalid("no such passkey"))?
                    .clone();
                if !passkey::rp_id_allowed(&a.origin_host, &key.rp_id) {
                    return Err(invalid("this site may not use that passkey"));
                }
                let assertion = passkey::sign(&key, &hash, a.user_verified)?;
                Ok(Reply::view(json!({
                    "credential_id": B64.encode(&assertion.credential_id),
                    "authenticator_data": B64.encode(&assertion.authenticator_data),
                    "signature": B64.encode(&assertion.signature),
                    "user_handle": B64.encode(&assertion.user_handle),
                })))
            }
            "ssh.generate" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    id: String,
                    comment: String,
                }
                let a: A = parse(call.args)?;
                session.entry(&a.id)?;
                let key = ssh::generate_ed25519(&a.comment)?;
                session.vault.edit(&a.id, now, |entry, at| {
                    fields::set_secret(entry, "private_key", key.private_key.expose().as_bytes(), at)?;
                    fields::set_plain(entry, "public_key", &json!(key.public_key), at)?;
                    fields::set_plain(entry, "fingerprint", &json!(key.fingerprint), at)
                })??;
                session.touched();
                Ok(Reply::view(json!({
                    "public_key": key.public_key,
                    "fingerprint": key.fingerprint,
                })))
            }
            "ssh.agent_keys" => {
                let keys: Vec<Value> = session
                    .vault
                    .visible()
                    .filter_map(|entry| {
                        let Kind::SshKey(key) = &entry.kind else { return None };
                        if !key.agent_allowed.v {
                            return None;
                        }
                        let public = ssh::public_blob(&key.public_key.v).ok()?;
                        Some(json!({
                            "title": entry.common.title.v,
                            "comment": public.1,
                            "blob": base64_encode(&public.0),
                        }))
                    })
                    .collect();
                Ok(Reply::view(json!(keys)))
            }
            "ssh.agent_sign" => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct A {
                    blob: String,
                    data: String,
                }
                let a: A = parse(call.args)?;
                let data = base64_decode(&a.data)?;
                let entry = session
                    .vault
                    .visible()
                    .find(|entry| {
                        let Kind::SshKey(key) = &entry.kind else { return false };
                        key.agent_allowed.v && ssh::public_blob(&key.public_key.v).is_ok_and(|public| base64_encode(&public.0) == a.blob)
                    })
                    .ok_or_else(|| invalid("that key is not offered"))?;
                let Kind::SshKey(key) = &entry.kind else {
                    return Err(invalid("that key is not offered"));
                };
                let signature = ssh::agent_sign(key.private_key.v.expose(), key.passphrase.v.expose(), &data)?;
                Ok(Reply { json: "{}".to_owned(), secret: Some(signature), blobs: Vec::new() })
            }
            _ => Err(invalid(format!("unknown call {method}"))),
        }
    }
}

/// Decodes what a shell passes as text.
fn base64_encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

/// Shells encode with what their platform has to hand: `btoa` and Android's `Base64` give the
/// standard alphabet with padding, Rust shells the URL-safe one without. Either is taken.
pub(crate) fn base64_decode(text: &str) -> Result<Vec<u8>, SessionError> {
    let text: String = text
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect();
    B64.decode(text).map_err(|_| invalid("bad data"))
}

fn wrong_password() -> SessionError {
    SessionError {
        code: "wrong_password",
        message: "that password is not right".to_owned(),
    }
}

/// What a sealed quick-unlock key is bound to, so it cannot pass for anything else.
const QUICK_AAD: &[u8] = b"wmpass quick unlock v1";

/// The key that seals the vault key for quick unlock, from a passkey's PRF answer.
fn quick_wrapping(prf: &[u8]) -> Result<Zeroizing<[u8; 32]>, SessionError> {
    if prf.len() != 32 {
        return Err(invalid("a passkey's answer is 32 bytes"));
    }
    Ok(wmpass_core::crypto::hkdf(prf, b"", "wmpass quick unlock")?)
}

fn wrapped_key(vault: &Vault) -> Reply {
    Reply {
        json: "{}".to_owned(),
        secret: Some(Zeroizing::new(vault.key().expose_for_hardware_wrap().to_vec())),
        blobs: Vec::new(),
    }
}

fn child_key_bytes(child: &Vault) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(child.key().expose_for_hardware_wrap().to_vec())
}

fn otp_of(entry: &Entry) -> Option<&OtpConfig> {
    match &entry.kind {
        Kind::Login(login) => login.otp.v.as_ref(),
        Kind::Otp(otp) => otp.config.v.as_ref(),
        _ => None,
    }
}

fn match_rows(vault: &Vault, target: Target<'_>) -> Reply {
    let rows: Vec<Value> = matching::matches_for(vault.visible(), target)
        .into_iter()
        .map(|(entry, level)| {
            json!({
                "entry": views::summary(entry),
                "level": level_name(level),
            })
        })
        .collect();
    Reply::view(json!(rows))
}

fn format_by_name(name: &str) -> Option<io::Format> {
    io::Format::ALL.into_iter().find(|f| f.name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop shell's `btoa` and Android's `Base64.NO_WRAP` pad and use `+` and `/`.
    #[test]
    fn base64_from_any_shell_decodes() {
        let bytes: Vec<u8> = (0..=255u8).chain([0xff]).collect();
        let standard = base64::engine::general_purpose::STANDARD.encode(&bytes);
        assert!(standard.ends_with('=') && standard.contains('+') && standard.contains('/'));
        assert_eq!(base64_decode(&standard).unwrap(), bytes);
        assert_eq!(base64_decode(&B64.encode(&bytes)).unwrap(), bytes);
        assert!(base64_decode("not base64!").is_err());
    }
}
