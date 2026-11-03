//! An account: thin client state plus the queue of local changes.
//!
//! What is cached is what the server sent, still encrypted under keys that open only with
//! the master password, so a Bitwarden account works offline with nothing new at rest.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wmpass_core::Secret;
use wmpass_core::backend::{Backend, BackendKind};
use wmpass_core::model::{Entry, Id};
use wmpass_core::vault::new_id;
use wmpass_net::{Http, NetError, NetPolicy};
use zeroize::Zeroizing;

use crate::api::{self, Device, Endpoints, Registration, Saved, SecondFactor, Tokens, prop, text};
use crate::crypto::{Kdf, MasterKey, SymKey, generate_key_pair, unwrap_with_private};
use crate::mapping::{cipher_from_entry, entry_from_cipher};
use crate::{BwError, Result};

/// A local change that has not reached the server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pending {
    /// Created or changed.
    Upsert(Id),
    /// Trashed.
    Trash(Id),
}

/// What is kept on disk. Everything sensitive here is encrypted by the server's format, or
/// sealed under the account key.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cache {
    email: String,
    wrapped_key: String,
    kdf_kind: u64,
    kdf_iterations: u64,
    kdf_memory: Option<u64>,
    kdf_parallelism: Option<u64>,
    /// Sealed under the account key.
    refresh: String,
    /// The last sync, as received.
    sync: String,
    /// Local entries not yet sent, sealed under the account key.
    local: String,
    pending: Vec<Pending>,
}

/// An unlocked account.
pub struct Account {
    endpoints: Endpoints,
    email: String,
    kdf: Kdf,
    wrapped_key: String,
    key: SymKey,
    access: Option<Secret>,
    refresh: Option<Secret>,
    organisations: BTreeMap<String, SymKey>,
    sync: Value,
    raw: BTreeMap<Id, Value>,
    entries: BTreeMap<Id, Entry>,
    /// Items that did not decrypt. Kept visible, so nothing vanishes silently.
    unreadable: usize,
    pending: Vec<Pending>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("entries", &self.entries.len()).finish_non_exhaustive()
    }
}

fn kdf_numbers(kdf: Kdf) -> (u64, u64, Option<u64>, Option<u64>) {
    match kdf {
        Kdf::Pbkdf2 { iterations } => (0, u64::from(iterations), None, None),
        Kdf::Argon2id { iterations, memory_mib, parallelism } => {
            (1, u64::from(iterations), Some(u64::from(memory_mib)), Some(u64::from(parallelism)))
        }
    }
}

/// Creates an account on a self-hosted server.
pub fn register(http: &dyn Http, policy: NetPolicy, endpoints: &Endpoints, email: &str, name: &str, password: &[u8], kdf: Kdf) -> Result<()> {
    let master = MasterKey::derive(password, email, kdf)?;
    let account = SymKey::generate()?;
    let (public_key, private_key) = generate_key_pair(&account)?;
    let form = Registration {
        email: email.to_owned(),
        name: name.to_owned(),
        kdf,
        login_hash: master.login_hash(password),
        key: master.stretch()?.encrypt(&account.to_bytes())?,
        public_key,
        private_key,
    };
    api::run(api::register(endpoints, &form), http, policy)
}

impl Account {
    /// Logs in and fetches everything.
    pub fn login(
        http: &dyn Http,
        policy: NetPolicy,
        endpoints: Endpoints,
        email: &str,
        password: &[u8],
        device: &Device,
        factor: Option<&SecondFactor>,
    ) -> Result<Self> {
        let kdf = api::run(api::prelogin(&endpoints, email), http, policy)?;
        let master = MasterKey::derive(password, email, kdf)?;
        let hash = master.login_hash(password);
        let tokens: Tokens = api::run(api::login(&endpoints, email, &hash, device, factor), http, policy)??;
        let wrapped_key = tokens.key.clone().ok_or(BwError::BadAnswer)?;
        // The server has taken the password by now, so a key that will not open is not a wrong guess.
        let key = master.unwrap_user_key(&wrapped_key).map_err(|_| BwError::KeyUnreadable)?;
        let mut account = Self {
            endpoints,
            email: email.trim().to_lowercase(),
            kdf,
            wrapped_key,
            key,
            access: Some(tokens.access),
            refresh: Some(tokens.refresh),
            organisations: BTreeMap::new(),
            sync: Value::Null,
            raw: BTreeMap::new(),
            entries: BTreeMap::new(),
            unreadable: 0,
            pending: Vec::new(),
        };
        account.pull(http, policy)?;
        Ok(account)
    }

    /// Opens a saved account with the master password, without the network.
    pub fn unlock_offline(endpoints: Endpoints, cached: &[u8], password: &[u8]) -> Result<Self> {
        let cache: Cache = rmp_serde::from_slice(cached).map_err(|_| BwError::BadAnswer)?;
        let kdf = Kdf::from_api(cache.kdf_kind, cache.kdf_iterations, cache.kdf_memory, cache.kdf_parallelism)?;
        let master = MasterKey::derive(password, &cache.email, kdf)?;
        let key = master.unwrap_user_key(&cache.wrapped_key).map_err(|_| BwError::BadLogin)?;
        let refresh = if cache.refresh.is_empty() { None } else { Some(key.decrypt_text(&cache.refresh)?) };
        let sync = serde_json::from_str(&cache.sync).unwrap_or(Value::Null);
        let mut account = Self {
            endpoints,
            email: cache.email,
            kdf,
            wrapped_key: cache.wrapped_key,
            key,
            access: None,
            refresh,
            organisations: BTreeMap::new(),
            sync: Value::Null,
            raw: BTreeMap::new(),
            entries: BTreeMap::new(),
            unreadable: 0,
            pending: cache.pending,
        };
        account.absorb(sync)?;
        if !cache.local.is_empty() {
            let plain = account.key.decrypt(&cache.local)?;
            let local: Vec<Entry> = rmp_serde::from_slice(&plain).map_err(|_| BwError::BadAnswer)?;
            for entry in local {
                account.entries.insert(entry.id.clone(), entry);
            }
        }
        Ok(account)
    }

    /// The account's email.
    pub fn email(&self) -> &str {
        &self.email
    }

    /// Items the last sync could not decrypt.
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }

    /// Local changes waiting for the server.
    pub fn pending(&self) -> &[Pending] {
        &self.pending
    }

    /// Folders, id and decrypted name.
    pub fn folders(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for folder in prop(&self.sync, "folders").and_then(Value::as_array).into_iter().flatten() {
            if let (Some(id), Some(name)) = (text(folder, "id"), text(folder, "name"))
                && let Ok(name) = self.key.decrypt_text(name)
            {
                out.push((id.to_owned(), name.expose().to_owned()));
            }
        }
        out
    }

    /// Organisations this account is in, id and name.
    pub fn organisations(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for organisation in prop(&self.sync, "profile").and_then(|p| prop(p, "organizations")).and_then(Value::as_array).into_iter().flatten() {
            if let (Some(id), Some(name)) = (text(organisation, "id"), text(organisation, "name")) {
                out.push((id.to_owned(), name.to_owned()));
            }
        }
        out
    }

    fn absorb(&mut self, sync: Value) -> Result<()> {
        let profile = prop(&sync, "profile").cloned().unwrap_or(Value::Null);
        let private = match text(&profile, "privateKey") {
            Some(wrapped) => Some(self.key.decrypt(wrapped)?),
            None => None,
        };
        self.organisations.clear();
        for organisation in prop(&profile, "organizations").and_then(Value::as_array).into_iter().flatten() {
            if let (Some(id), Some(wrapped), Some(private)) = (text(organisation, "id"), text(organisation, "key"), private.as_ref())
                && let Ok(key) = unwrap_with_private(private, wrapped)
            {
                self.organisations.insert(id.to_owned(), key);
            }
        }
        self.raw.clear();
        self.entries.clear();
        self.unreadable = 0;
        for cipher in prop(&sync, "ciphers").and_then(Value::as_array).into_iter().flatten() {
            let Some(id) = text(cipher, "id").map(str::to_owned) else { continue };
            let organisation = text(cipher, "organizationId").and_then(|org| self.organisations.get(org));
            match entry_from_cipher(cipher, &self.key, organisation) {
                Ok(entry) => {
                    self.entries.insert(id.clone(), entry);
                    self.raw.insert(id, cipher.clone());
                }
                Err(_) => self.unreadable += 1,
            }
        }
        self.sync = sync;
        Ok(())
    }

    fn access(&mut self, http: &dyn Http, policy: NetPolicy) -> Result<Secret> {
        if let Some(access) = &self.access {
            return Ok(access.clone());
        }
        let refresh = self.refresh.clone().ok_or(BwError::Net(NetError::Unauthorised))?;
        let tokens = api::run(api::refresh(&self.endpoints, &refresh), http, policy)?;
        self.access = Some(tokens.access.clone());
        self.refresh = Some(tokens.refresh);
        Ok(tokens.access)
    }

    /// Runs a call, refreshing once if the access token has expired.
    fn with_token<T>(&mut self, http: &dyn Http, policy: NetPolicy, build: impl Fn(&Endpoints, &Secret) -> wmpass_net::Call<T>) -> Result<T> {
        let access = self.access(http, policy)?;
        match build(&self.endpoints, &access).run(http, policy) {
            Err(NetError::Unauthorised) => {
                self.access = None;
                let access = self.access(http, policy)?;
                Ok(build(&self.endpoints, &access).run(http, policy)?)
            }
            other => Ok(other?),
        }
    }

    /// Fetches the server's state. Local changes not yet sent are kept over it.
    pub fn pull(&mut self, http: &dyn Http, policy: NetPolicy) -> Result<()> {
        let sync = self.with_token(http, policy, api::sync)?;
        let held: Vec<Entry> = self
            .pending
            .iter()
            .filter_map(|pending| match pending {
                Pending::Upsert(id) => self.entries.get(id).cloned(),
                Pending::Trash(_) => None,
            })
            .collect();
        self.absorb(sync)?;
        for mine in held {
            match self.entries.get_mut(&mine.id) {
                // Field by field. What this device edited carries a later moment, so it
                // wins, and any password it replaces moves to history.
                Some(server) => server.merge_from(&mine),
                None => {
                    self.entries.insert(mine.id.clone(), mine);
                }
            }
        }
        Ok(())
    }

    fn key_for(&self, id: &str) -> SymKey {
        self.raw
            .get(id)
            .and_then(|raw| text(raw, "organizationId"))
            .and_then(|org| self.organisations.get(org))
            .unwrap_or(&self.key)
            .clone()
    }

    /// Sends local changes. On a conflict, fetches and merges, then tries once more; anything
    /// still unsent stays queued for next time.
    pub fn push(&mut self, http: &dyn Http, policy: NetPolicy) -> Result<()> {
        for pending in std::mem::take(&mut self.pending) {
            let id = match &pending {
                Pending::Upsert(id) | Pending::Trash(id) => id.clone(),
            };
            let outcome = match &pending {
                Pending::Upsert(_) => self.push_one(http, policy, &id),
                Pending::Trash(_) => self.with_token(http, policy, |e, a| api::trash_cipher(e, a, &id)),
            };
            if let Err(error) = outcome {
                self.pending.push(pending);
                return Err(error);
            }
        }
        self.pull(http, policy)
    }

    fn push_one(&mut self, http: &dyn Http, policy: NetPolicy, id: &str) -> Result<()> {
        for _ in 0..2 {
            let Some(entry) = self.entries.get(id).cloned() else { return Ok(()) };
            let server = self.raw.get(id).cloned();
            let key = self.key_for(id);
            let cipher = cipher_from_entry(&entry, server.as_ref(), &key)?;
            let saved = if server.is_some() {
                self.with_token(http, policy, |e, a| api::update_cipher(e, a, id, &cipher))?
            } else {
                self.with_token(http, policy, |e, a| api::create_cipher(e, a, &cipher))?
            };
            match saved {
                Saved::Stored(stored) => {
                    // A new item gets its id from the server. Replace the local placeholder.
                    if server.is_none() {
                        self.entries.remove(id);
                    }
                    let real = text(&stored, "id").unwrap_or(id).to_owned();
                    let organisation = text(&stored, "organizationId").and_then(|org| self.organisations.get(org));
                    let fresh = entry_from_cipher(&stored, &self.key, organisation)?;
                    self.entries.insert(real.clone(), fresh);
                    self.raw.insert(real, stored);
                    return Ok(());
                }
                Saved::Conflict => {
                    self.pending.push(Pending::Upsert(id.to_owned()));
                    self.pull(http, policy)?;
                    self.pending.retain(|pending| *pending != Pending::Upsert(id.to_owned()));
                }
            }
        }
        self.pending.push(Pending::Upsert(id.to_owned()));
        Ok(())
    }
}

impl Backend for Account {
    fn kind(&self) -> BackendKind {
        BackendKind::Bitwarden
    }

    fn entries(&self, _now_ms: u64) -> Vec<Entry> {
        self.entries.values().filter(|entry| !entry.is_trashed()).cloned().collect()
    }

    fn upsert(&mut self, mut entry: Entry, now_ms: u64) -> wmpass_core::Result<Id> {
        if entry.id.is_empty() {
            entry.id = new_id()?;
        }
        if entry.created_ms == 0 {
            entry.created_ms = now_ms;
        }
        let id = entry.id.clone();
        self.entries.insert(id.clone(), entry);
        let marker = Pending::Upsert(id.clone());
        if !self.pending.contains(&marker) {
            self.pending.push(marker);
        }
        Ok(id)
    }

    fn remove(&mut self, id: &str, now_ms: u64) -> wmpass_core::Result<()> {
        // Not on the server yet: nothing to tell it.
        let known = self.raw.contains_key(id);
        self.pending.retain(|pending| *pending != Pending::Upsert(id.to_owned()));
        if known {
            self.pending.push(Pending::Trash(id.to_owned()));
            if let Some(entry) = self.entries.get_mut(id) {
                let at = wmpass_core::clock::Hlc { ms: now_ms, counter: 0, device: 0 };
                entry.common.deleted_at_ms = wmpass_core::model::Lww::new(Some(now_ms), at);
            }
        } else {
            self.entries.remove(id);
        }
        Ok(())
    }

    fn save(&mut self) -> wmpass_core::Result<Vec<u8>> {
        let (kdf_kind, kdf_iterations, kdf_memory, kdf_parallelism) = kdf_numbers(self.kdf);
        let seal = |plain: &[u8]| self.key.encrypt(plain).map_err(|_| wmpass_core::Error::Encoding);
        let local: Vec<&Entry> = self
            .pending
            .iter()
            .filter_map(|pending| match pending {
                Pending::Upsert(id) => self.entries.get(id),
                Pending::Trash(_) => None,
            })
            .collect();
        let local = if local.is_empty() {
            String::new()
        } else {
            let plain = Zeroizing::new(rmp_serde::to_vec_named(&local).map_err(|_| wmpass_core::Error::Encoding)?);
            seal(&plain)?
        };
        let refresh = match &self.refresh {
            Some(token) => seal(token.expose().as_bytes())?,
            None => String::new(),
        };
        let cache = Cache {
            email: self.email.clone(),
            wrapped_key: self.wrapped_key.clone(),
            kdf_kind,
            kdf_iterations,
            kdf_memory,
            kdf_parallelism,
            refresh,
            sync: self.sync.to_string(),
            local,
            pending: self.pending.clone(),
        };
        rmp_serde::to_vec_named(&cache).map_err(|_| wmpass_core::Error::Encoding)
    }
}
