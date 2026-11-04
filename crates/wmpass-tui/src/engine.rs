//! The open vault, through the session every shell uses. Views come back without secrets;
//! a secret comes back only when one is asked for, as bytes wiped once used.
//!
//! The file on disk is written after every change, through a temporary file and a rename.
//! When another program, such as a sync client, writes the file meanwhile, its copy is
//! merged in before the next write rather than overwritten.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wmpass_session::{Call, Reply, Service, SessionError};
use zeroize::Zeroizing;

use crate::app_folder;
use crate::store;

/// The time now, in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// One row in the list, as the session describes it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub site: Option<String>,
    pub color: Option<String>,
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub favourite: bool,
    pub hidden: bool,
    pub trashed: bool,
    pub has_password: bool,
    pub has_otp: bool,
    pub has_passkey: bool,
    pub attachments: usize,
    pub modified_ms: u64,
}

/// A field of an entry's kind.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FieldRow {
    pub key: String,
    pub label: String,
    pub input: String,
    pub secret: bool,
    pub choices: Vec<String>,
    pub has_value: bool,
    pub value: Option<Value>,
}

/// A field the user added.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CustomRow {
    pub index: usize,
    pub name: String,
    pub kind: String,
    pub has_value: bool,
    pub value: Option<String>,
}

/// One-time code settings, without the seed.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct OtpInfo {
    pub issuer: String,
    pub account: String,
    pub kind: String,
    pub digits: u32,
    pub period: Option<u32>,
}

/// An attachment.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Attachment {
    pub blob: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// A passkey's public side.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Passkey {
    pub rp_id: String,
    pub user_name: String,
    pub created_ms: u64,
}

/// A past value.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Revision {
    pub index: usize,
    pub field: String,
    pub at_ms: u64,
}

/// Everything about one entry except its secrets.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Detail {
    #[serde(flatten)]
    pub row: Row,
    pub created_ms: u64,
    pub fields: Vec<FieldRow>,
    pub custom_fields: Vec<CustomRow>,
    pub has_notes: bool,
    pub sites: Vec<Value>,
    pub otp: Option<OtpInfo>,
    pub passkeys: Vec<Passkey>,
    pub attachment_list: Vec<Attachment>,
    pub history: Vec<Revision>,
    pub alias: Option<String>,
    pub questions: Vec<String>,
    /// The items this one lists under Linked.
    pub links: Vec<String>,
}

/// A live one-time code.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Code {
    pub id: String,
    pub title: String,
    pub site: Option<String>,
    pub otp: OtpInfo,
    pub code: Option<Wiped>,
    pub next_code: Option<Wiped>,
    pub seconds_left: Option<u32>,
}

/// Text that is wiped when dropped and never printed by `Debug`.
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Wiped(String);

impl Wiped {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Wiped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Wiped(..)")
    }
}

impl Drop for Wiped {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

/// A folder.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
}

/// The health report, as entry ids.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Health {
    pub weak: Vec<String>,
    pub reused: Vec<Vec<String>>,
    pub old: Vec<String>,
    pub missing_two_factor: Vec<String>,
    pub insecure_url: Vec<String>,
    pub duplicates: Vec<Vec<String>>,
    pub expiring: Vec<String>,
    pub score: u8,
}

/// How strong a password looks.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Strength {
    pub score: u8,
    pub crack_slow_hash: String,
    pub crack_fast_hash: String,
    pub warning: Option<String>,
    pub suggestions: Vec<String>,
}

/// What an import found, before it is applied.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportPreview {
    pub format: String,
    pub count: usize,
    pub titles: Vec<String>,
    pub skipped: Vec<String>,
}

/// How an unlock went.
#[derive(Debug)]
pub enum Unlock {
    /// Open.
    Opened(Box<Engine>, Option<(u64, u64)>),
    /// The password is wrong, or it is a duress password, which looks the same from here.
    Wrong,
    /// A key file is needed as well.
    NeedsKeyFile,
    /// A hardware key is needed, which this app cannot talk to.
    NeedsHardware,
    /// Too many wrong passwords: the next try is allowed at this time, in ms since 1970.
    Waiting(u64),
}

/// One unlocked vault and its file.
#[derive(Debug)]
pub struct Engine {
    service: Service,
    handle: u64,
    path: PathBuf,
    /// The vault's name.
    pub name: String,
    /// Whether it cannot be changed.
    pub read_only: bool,
    /// The file as last read or written, to notice someone else writing it.
    known: Option<(SystemTime, u64)>,
    /// The desktop app's folder, when the vault is the one it keeps. Its files all carry one
    /// time, so a change shows only in what they hold: `digest` is the file's hash.
    root: Option<PathBuf>,
    digest: Option<[u8; 32]>,
}

fn reply_json<T: DeserializeOwned>(reply: &Reply) -> Result<T, String> {
    serde_json::from_str(&reply.json).map_err(|e| format!("unexpected reply: {e}"))
}

fn err(error: SessionError) -> String {
    error.message
}

fn secret_text(reply: Reply) -> Result<Zeroizing<String>, String> {
    let bytes = reply.secret.ok_or("no value came back")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "that value is not text".to_owned())?;
    Ok(Zeroizing::new(text.to_owned()))
}

/// Creates a vault file and opens it. A folder makes the vault in the desktop app's folder
/// there, beside a decoy, as the app makes its first one.
pub fn create(path: &Path, name: &str, password: &[u8], device: u32) -> Result<Engine, String> {
    if path.is_dir() {
        return create_in_folder(path, name, password, device);
    }
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let mut service = Service::new(device);
    let args = json!({"name": name}).to_string();
    let reply = service
        .call("vault.create", Call { args: &args, secret: password, now_ms: now_ms(), ..Call::default() })
        .map_err(err)?;
    let opened: Value = reply_json(&reply)?;
    let mut engine = Engine {
        service,
        handle: opened["handle"].as_u64().ok_or("no handle came back")?,
        path: path.to_owned(),
        name: name.to_owned(),
        read_only: false,
        known: None,
        root: None,
        digest: None,
    };
    engine.save()?;
    Ok(engine)
}

fn create_in_folder(root: &Path, name: &str, password: &[u8], device: u32) -> Result<Engine, String> {
    let mut service = Service::new(device);
    let args = json!({"name": name}).to_string();
    let reply = service
        .call("vault.create", Call { args: &args, secret: password, now_ms: now_ms(), ..Call::default() })
        .map_err(err)?;
    let opened: Value = reply_json(&reply)?;
    let mut engine = Engine {
        service,
        handle: opened["handle"].as_u64().ok_or("no handle came back")?,
        path: app_folder::vault_path(root, &app_folder::new_name()?),
        name: name.to_owned(),
        read_only: false,
        known: None,
        root: Some(root.to_owned()),
        digest: None,
    };
    engine.save()?;
    // A decoy that never changes, so a hidden vault's unchanging file is nothing unusual. Its
    // partner, which keeps pace with this vault, came with the save.
    let length = usize::try_from(std::fs::metadata(&engine.path).map_err(|e| e.to_string())?.len()).unwrap_or(0);
    let decoy = engine.chaff(length)?;
    app_folder::write(&app_folder::vault_path(root, &app_folder::new_name()?), &decoy)?;
    app_folder::cover_times(root)?;
    Ok(engine)
}

/// Opens the vault the desktop app keeps in `root` that this password opens, trying each file
/// as the app does, and counting a wrong password against the app's own limit.
fn unlock_folder(root: &Path, password: &[u8], key_file: Option<&[u8]>, device: u32) -> Result<Unlock, String> {
    let mut door = app_folder::DoorState::load(root);
    if let Some(at) = door.waiting(now_ms()) {
        return Ok(Unlock::Waiting(at));
    }
    let files = app_folder::files(root)?;
    if files.is_empty() {
        return Err("The WM Pass app has no vault on this computer yet".to_owned());
    }
    let mut service = Service::new(device);
    let mut blobs: Vec<&[u8]> = Vec::new();
    let args = match key_file {
        Some(key) => {
            blobs.push(key);
            json!({"factors": ["key_file"]}).to_string()
        }
        None => "{}".to_owned(),
    };
    blobs.extend(files.iter().map(|(_, bytes)| bytes.as_slice()));
    let reply = service
        .call("vault.open", Call { args: &args, secret: password, blobs: &blobs, now_ms: now_ms(), ..Call::default() })
        .map_err(err)?;
    let opened: Value = reply_json(&reply)?;
    match opened["outcome"].as_str() {
        Some("opened") => {
            door.opened();
            door.save(root);
            let index = usize::try_from(opened["index"].as_u64().unwrap_or(0)).unwrap_or(0);
            let (name, file) = files.get(index).ok_or("the vault file went missing")?;
            let generation = opened["generation"].as_u64().unwrap_or(0);
            let rollback = store::rolled_back(file, generation);
            store::remember_generation(file, generation);
            Ok(Unlock::Opened(
                Box::new(Engine {
                    service,
                    handle: opened["handle"].as_u64().ok_or("no handle came back")?,
                    path: app_folder::vault_path(root, name),
                    name: opened["name"].as_str().unwrap_or("Vault").to_owned(),
                    read_only: opened["read_only"].as_bool().unwrap_or(false),
                    known: None,
                    root: Some(root.to_owned()),
                    digest: Some(*blake3::hash(file).as_bytes()),
                }),
                rollback,
            ))
        }
        Some("needs_factor") => {
            let wants_file = opened["factors"].as_array().is_some_and(|f| f.iter().any(|v| v["kind"] == "key_file"));
            Ok(if wants_file && key_file.is_none() { Unlock::NeedsKeyFile } else { Unlock::NeedsHardware })
        }
        // A duress password does what it is set to do in the app, and then looks like a wrong
        // one to anyone watching.
        Some("duress") => {
            if opened["action"] == "wipe" {
                app_folder::wipe(root);
            } else {
                door.hard_lock(now_ms());
                door.save(root);
            }
            Ok(Unlock::Wrong)
        }
        _ => {
            door.failed(now_ms());
            door.save(root);
            Ok(Unlock::Wrong)
        }
    }
}

/// Opens a vault file with a password, and a key file if it has one.
pub fn unlock(path: &Path, password: &[u8], key_file: Option<&[u8]>, device: u32) -> Result<Unlock, String> {
    if app_folder::is_app_folder(path) {
        return unlock_folder(path, password, key_file, device);
    }
    if let Some(root) = app_folder::root_of(path) {
        return unlock_folder(&root, password, key_file, device);
    }
    let file = store::read(path)?;
    let mut service = Service::new(device);
    let (args, blobs): (String, Vec<&[u8]>) = match key_file {
        Some(key) => (json!({"factors": ["key_file"]}).to_string(), vec![key, &file]),
        None => ("{}".to_owned(), vec![&file]),
    };
    let reply = service
        .call("vault.open", Call { args: &args, secret: password, blobs: &blobs, now_ms: now_ms(), ..Call::default() })
        .map_err(err)?;
    let opened: Value = reply_json(&reply)?;
    match opened["outcome"].as_str() {
        Some("opened") => {
            let generation = opened["generation"].as_u64().unwrap_or(0);
            let rollback = store::rolled_back(&file, generation);
            store::remember_generation(&file, generation);
            Ok(Unlock::Opened(
                Box::new(Engine {
                    service,
                    handle: opened["handle"].as_u64().ok_or("no handle came back")?,
                    path: path.to_owned(),
                    name: opened["name"].as_str().unwrap_or("Vault").to_owned(),
                    read_only: opened["read_only"].as_bool().unwrap_or(false),
                    known: store::stamp(path),
                    root: None,
                    digest: None,
                }),
                rollback,
            ))
        }
        Some("needs_factor") => {
            let wants_file = opened["factors"]
                .as_array()
                .is_some_and(|f| f.iter().any(|v| v["kind"] == "key_file"));
            Ok(if wants_file && key_file.is_none() { Unlock::NeedsKeyFile } else { Unlock::NeedsHardware })
        }
        // A duress password must not be told apart from a wrong one by anyone watching.
        _ => Ok(Unlock::Wrong),
    }
}

impl Engine {
    /// The vault file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn call(&mut self, method: &str, args: &Value, secret: &[u8], blobs: &[&[u8]]) -> Result<Reply, String> {
        let args = args.to_string();
        self.service
            .call(method, Call { handle: self.handle, args: &args, secret, blobs, now_ms: now_ms() })
            .map_err(err)
    }

    fn view<T: DeserializeOwned>(&mut self, method: &str, args: &Value) -> Result<T, String> {
        let reply = self.call(method, args, &[], &[])?;
        reply_json(&reply)
    }

    /// Runs a change and writes the file.
    fn change(&mut self, method: &str, args: &Value, secret: &[u8]) -> Result<Reply, String> {
        if self.read_only {
            return Err("this vault is read-only".to_owned());
        }
        let reply = self.call(method, args, secret, &[])?;
        self.save()?;
        Ok(reply)
    }

    /// The desktop app's folder, when this is its vault.
    pub fn app_folder(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Moves the vault into the desktop app's folder at `root`, attachments too, so the app and
    /// this terminal use one vault. The old file is left where it was.
    pub fn move_into(&mut self, root: &Path) -> Result<(), String> {
        let first = app_folder::names(root).map(|n| n.is_empty()).unwrap_or(true);
        let old_blobs = store::blob_dir(&self.path);
        let path = app_folder::vault_path(root, &app_folder::new_name()?);
        if let Ok(dir) = std::fs::read_dir(&old_blobs) {
            let blobs = store::blob_dir(&path);
            std::fs::create_dir_all(&blobs).map_err(|e| e.to_string())?;
            for entry in dir.flatten() {
                std::fs::copy(entry.path(), blobs.join(entry.file_name())).map_err(|e| e.to_string())?;
            }
        }
        let (old_path, old_known) = (std::mem::replace(&mut self.path, path), self.known.take());
        self.root = Some(root.to_owned());
        if let Err(e) = self.save() {
            self.path = old_path;
            self.known = old_known;
            self.root = None;
            return Err(e);
        }
        if first {
            let length = usize::try_from(std::fs::metadata(&self.path).map_err(|e| e.to_string())?.len()).unwrap_or(0);
            let decoy = self.chaff(length)?;
            app_folder::write(&app_folder::vault_path(root, &app_folder::new_name()?), &decoy)?;
            app_folder::cover_times(root)?;
        }
        Ok(())
    }

    /// A decoy the size of a vault file, under this vault's key settings.
    fn chaff(&mut self, length: usize) -> Result<Vec<u8>, String> {
        let handle = self.handle;
        let reply = self.call("vault.chaff", &json!({"file_len": length, "same_kdf_as": handle}), &[], &[])?;
        reply.blobs.into_iter().next().ok_or_else(|| "no decoy came back".to_owned())
    }

    /// Merges in the file if someone else wrote it since. Returns whether anything came in.
    pub fn pull(&mut self) -> Result<bool, String> {
        if self.root.is_some() {
            let Ok(file) = std::fs::read(&self.path) else { return Ok(false) };
            let digest = *blake3::hash(&file).as_bytes();
            if Some(digest) == self.digest {
                return Ok(false);
            }
            let seen = store::seen_generation(&file);
            self.call("vault.merge", &json!({"seen_generation": seen}), &[], &[&file])?;
            self.digest = Some(digest);
            return Ok(true);
        }
        let now = store::stamp(&self.path);
        if now.is_none() || now == self.known {
            return Ok(false);
        }
        let file = store::read(&self.path)?;
        let seen = store::seen_generation(&file);
        let reply = self.call("vault.merge", &json!({"seen_generation": seen}), &[], &[&file])?;
        let merged: Value = reply_json(&reply)?;
        if merged["rollback"].as_bool() == Some(true) {
            // An older copy merges harmlessly, since the newer changes stay, but it is news.
            self.known = now;
            return Err("the file on disk is older than one seen here before; your copy was kept".to_owned());
        }
        self.known = now;
        Ok(true)
    }

    /// Writes the vault to its file, taking in anyone else's changes first.
    pub fn save(&mut self) -> Result<(), String> {
        if self.known.is_some() || self.digest.is_some() {
            // A rollback is reported by `pull` on its own tick; here the write goes ahead.
            let _ = self.pull();
        }
        let reply = self.call("vault.save", &json!({}), &[], &[])?;
        let file = reply.blobs.first().ok_or("nothing came back to write")?;
        if let Some(root) = self.root.clone() {
            // The app's folder: no backup file beside it, which would be one more file in
            // `vaults/`, and the decoy and the times kept as the app keeps them.
            app_folder::write(&self.path, file)?;
            self.digest = Some(*blake3::hash(file).as_bytes());
            let length = file.len();
            let info: Value = reply_json(&reply)?;
            store::remember_generation(file, info["generation"].as_u64().unwrap_or(0));
            let partner = self.view::<Value>("vault.chaff_name", &json!({}))?["name"].as_str().unwrap_or_default().to_owned();
            return app_folder::cover_tracks(&root, &partner, length, |n| self.chaff(n));
        }
        store::write_atomic(&self.path, file)?;
        let info: Value = reply_json(&reply)?;
        store::remember_generation(file, info["generation"].as_u64().unwrap_or(0));
        self.known = store::stamp(&self.path);
        Ok(())
    }

    /// The settings other devices left in the vault, by key.
    pub fn synced_settings(&mut self) -> Result<serde_json::Map<String, Value>, String> {
        self.view("prefs.synced", &json!({}))
    }

    /// Leaves settings in the vault for the other devices, writing the file if any changed.
    pub fn sync_settings(&mut self, values: serde_json::Map<String, Value>) -> Result<(), String> {
        if self.read_only || values.is_empty() {
            return Ok(());
        }
        let reply = self.call("prefs.sync", &json!({ "values": values }), &[], &[])?;
        let done: Value = reply_json(&reply)?;
        if done["changed"].as_u64().unwrap_or(0) > 0 {
            self.save()?;
        }
        Ok(())
    }

    /// Locks the vault, wiping it from memory.
    pub fn lock(mut self) {
        let _ = self.call("vault.lock", &json!({}), &[], &[]);
        self.service.lock_all();
    }

    /// Entries matching a query, in the session's search language.
    pub fn list(&mut self, query: &str, sort: &str) -> Result<Vec<Row>, String> {
        self.view("entry.list", &json!({"query": query, "sort": sort}))
    }

    /// One entry.
    pub fn detail(&mut self, id: &str) -> Result<Detail, String> {
        self.view("entry.get", &json!({"id": id}))
    }

    /// Folders.
    pub fn folders(&mut self) -> Result<Vec<Folder>, String> {
        self.view("folder.list", &json!({}))
    }

    /// Live codes, for every entry or the ones named.
    pub fn codes(&mut self, ids: Option<&[String]>) -> Result<Vec<Code>, String> {
        self.view("otp.codes", &json!({"ids": ids}))
    }

    /// The health report.
    pub fn health(&mut self) -> Result<Health, String> {
        self.view("health.report", &json!({}))
    }

    /// A secret field's value.
    pub fn reveal(&mut self, what: &Secret, id: &str) -> Result<Zeroizing<String>, String> {
        let reply = match what {
            Secret::Field(field) => self.call("entry.reveal", &json!({"id": id, "field": field}), &[], &[])?,
            Secret::Notes => self.call("entry.reveal", &json!({"id": id, "field": "notes"}), &[], &[])?,
            Secret::Custom(index) => self.call("entry.reveal_custom", &json!({"id": id, "index": index}), &[], &[])?,
            Secret::Question(index) => self.call("entry.reveal_question", &json!({"id": id, "index": index}), &[], &[])?,
            Secret::History(index) => self.call("entry.reveal_history", &json!({"id": id, "index": index}), &[], &[])?,
            Secret::Code => self.call("otp.code", &json!({"id": id}), &[], &[])?,
        };
        secret_text(reply)
    }

    /// The next code of a counter-based entry, which uses it up.
    pub fn hotp_next(&mut self, id: &str) -> Result<Zeroizing<String>, String> {
        let reply = self.change("otp.hotp_next", &json!({"id": id}), &[])?;
        secret_text(reply)
    }

    /// Makes an entry. Plain fields go as JSON; secrets follow one call each.
    pub fn create(&mut self, kind: &str, title: &str, plain: &serde_json::Map<String, Value>, sites: &[Value]) -> Result<String, String> {
        let reply = self.change(
            "entry.create",
            &json!({"kind": kind, "title": title, "fields": plain, "sites": sites}),
            &[],
        )?;
        let made: Value = reply_json(&reply)?;
        made["id"].as_str().map(str::to_owned).ok_or_else(|| "no id came back".to_owned())
    }

    /// Changes plain fields.
    pub fn set_plain(&mut self, id: &str, plain: &serde_json::Map<String, Value>) -> Result<(), String> {
        if plain.is_empty() {
            return Ok(());
        }
        self.change("entry.set", &json!({"id": id, "fields": plain}), &[]).map(drop)
    }

    /// Changes a secret field, or the notes.
    pub fn set_secret(&mut self, id: &str, field: &str, value: &[u8]) -> Result<(), String> {
        self.change("entry.set_secret", &json!({"id": id, "field": field}), value).map(drop)
    }

    /// Sets or clears the one-time code, from an `otpauth://` link or a bare seed.
    pub fn set_otp(&mut self, id: &str, value: Option<&[u8]>) -> Result<(), String> {
        match value {
            Some(value) => self.change("entry.otp_set", &json!({"id": id}), value).map(drop),
            None => self.change("entry.otp_set", &json!({"id": id, "clear": true}), &[]).map(drop),
        }
    }

    /// Changes what every kind shares: title, tags, folder, flags.
    pub fn set_common(&mut self, id: &str, changes: Value) -> Result<(), String> {
        let mut args = changes;
        args["id"] = json!(id);
        self.change("entry.common", &args, &[]).map(drop)
    }

    /// Does one thing to several entries: `op` is trash, restore, purge, folder, color, favourite,
    /// pin, hide, tag_add or tag_remove, with its value in `extra`.
    pub fn bulk(&mut self, ids: &[String], op: &str, extra: Value) -> Result<(), String> {
        let mut args = if extra.is_object() { extra } else { json!({}) };
        args["ids"] = json!(ids);
        args["op"] = json!(op);
        let reply = self.change("entries.bulk", &args, &[])?;
        self.delete_blobs(&reply);
        Ok(())
    }

    fn delete_blobs(&self, reply: &Reply) {
        let gone: Value = serde_json::from_str(&reply.json).unwrap_or_default();
        let dir = store::blob_dir(&self.path);
        for name in gone["delete_blobs"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            // Names come from the vault; one with a path in it is not ours to delete.
            if !name.contains(['/', '\\']) && !name.contains("..") {
                let _ = std::fs::remove_file(dir.join(name));
            }
        }
    }

    /// A copy of an entry.
    pub fn duplicate(&mut self, id: &str) -> Result<String, String> {
        let reply = self.change("entry.duplicate", &json!({"id": id}), &[])?;
        let made: Value = reply_json(&reply)?;
        made["id"].as_str().map(str::to_owned).ok_or_else(|| "no id came back".to_owned())
    }

    /// Makes a folder.
    pub fn add_folder(&mut self, name: &str) -> Result<String, String> {
        let reply = self.change("folder.add", &json!({"name": name}), &[])?;
        let made: Value = reply_json(&reply)?;
        made["id"].as_str().map(str::to_owned).ok_or_else(|| "no id came back".to_owned())
    }

    /// Opens the hidden shelf, which asks for the password again.
    pub fn open_shelf(&mut self, password: &[u8]) -> Result<(), String> {
        self.call("shelf.open", &json!({}), password, &[]).map(drop)
    }

    /// Closes the hidden shelf.
    pub fn close_shelf(&mut self) -> Result<(), String> {
        self.call("shelf.close", &json!({}), &[], &[]).map(drop)
    }

    /// Generates a value from a recipe. Needs no vault, but the session is here.
    pub fn generate(&mut self, recipe: &Value) -> Result<(Zeroizing<String>, f64), String> {
        let reply = self.call("gen.generate", recipe, &[], &[])?;
        let bits: Value = reply_json(&reply)?;
        let bits = bits["bits"].as_f64().unwrap_or(0.0);
        Ok((secret_text(reply)?, bits))
    }

    /// Rates a password.
    pub fn strength(&mut self, password: &str) -> Result<Strength, String> {
        let reply = self.call("strength.assess", &json!({}), password.as_bytes(), &[])?;
        reply_json(&reply)
    }

    /// Reads a file from another app, without changing anything yet.
    pub fn import_read(&mut self, data: &[u8], password: &[u8]) -> Result<ImportPreview, String> {
        let reply = self.call("import.read", &json!({}), password, &[data])?;
        reply_json(&reply)
    }

    /// Applies the import just read. Returns what was added and what was already there.
    pub fn import_apply(&mut self) -> Result<(u64, u64), String> {
        let reply = self.call("import.apply", &json!({}), &[], &[])?;
        let report: Value = reply_json(&reply)?;
        let names: Vec<String> = report["blob_names"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if !names.is_empty() {
            let dir = store::blob_dir(&self.path);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for (name, sealed) in names.iter().zip(&reply.blobs) {
                std::fs::write(dir.join(name), sealed).map_err(|e| e.to_string())?;
            }
        }
        self.save()?;
        Ok((report["added"].as_u64().unwrap_or(0), report["duplicates"].as_u64().unwrap_or(0)))
    }

    /// Drops an import that was read but not applied.
    pub fn import_cancel(&mut self) {
        let _ = self.call("import.cancel", &json!({}), &[], &[]);
    }

    /// An export file's bytes. Every format but KDBX holds the secrets in the clear.
    pub fn export(&mut self, format: &str, password: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        let reply = self.call("export.file", &json!({"format": format}), password, &[])?;
        if let Some(secret) = reply.secret {
            return Ok(secret);
        }
        reply
            .blobs
            .into_iter()
            .next()
            .map(Zeroizing::new)
            .ok_or_else(|| "nothing came back".to_owned())
    }

    /// An attachment's plain bytes.
    pub fn open_attachment(&mut self, id: &str, blob: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        let sealed = store::read(&store::blob_dir(&self.path).join(blob))?;
        let reply = self.call("attachment.open", &json!({"id": id, "blob": blob}), &[], &[&sealed])?;
        reply.secret.ok_or_else(|| "nothing came back".to_owned())
    }

    /// Adds a file to an entry.
    pub fn add_attachment(&mut self, id: &str, file: &Path) -> Result<(), String> {
        let data = Zeroizing::new(store::read(file)?);
        let name = file.file_name().map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
        let reply = self.call("attachment.add", &json!({"id": id, "name": name, "mime": mime_of(&name)}), &[], &[&data])?;
        let made: Value = reply_json(&reply)?;
        let blob = made["blob"].as_str().ok_or("no name came back")?;
        let dir = store::blob_dir(&self.path);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(blob), reply.blobs.first().ok_or("nothing came back")?).map_err(|e| e.to_string())?;
        self.save()
    }
}

/// What to reveal.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Secret {
    /// A field of the kind, by key.
    Field(String),
    /// The notes.
    Notes,
    /// A custom field, by position.
    Custom(usize),
    /// A security question's answer.
    Question(usize),
    /// A past value.
    History(usize),
    /// The current one-time code.
    Code,
}

fn mime_of(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vault_round_trips_through_its_file() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("t.wmvault");
        let mut engine = create(&path, "Test", b"pw", 9).expect("create");
        let mut plain = serde_json::Map::new();
        plain.insert("username".into(), json!("octo"));
        let id = engine
            .create("login", "GitHub", &plain, &[json!({"Domain": "github.com"})])
            .expect("entry");
        engine.set_secret(&id, "password", b"hunter2-long-enough").expect("secret");
        engine.lock();

        assert!(matches!(unlock(&path, b"nope", None, 9).expect("tries"), Unlock::Wrong));
        let Unlock::Opened(mut engine, rollback) = unlock(&path, b"pw", None, 9).expect("opens") else {
            panic!("did not open");
        };
        assert!(rollback.is_none());
        let rows = engine.list("git", "").expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subtitle, "octo");
        let detail = engine.detail(&rows[0].id).expect("detail");
        assert!(
            detail.fields.iter().all(|f| !f.secret || f.value.is_none()),
            "a view never carries a secret"
        );
        let password = engine.reveal(&Secret::Field("password".into()), &rows[0].id).expect("reveal");
        assert_eq!(password.as_str(), "hunter2-long-enough");
    }

    #[test]
    fn someone_elses_write_is_merged_not_lost() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("t.wmvault");
        let mut first = create(&path, "Test", b"pw", 1).expect("create");
        let Unlock::Opened(mut second, _) = unlock(&path, b"pw", None, 2).expect("opens") else {
            panic!("did not open");
        };
        first.create("note", "From one", &serde_json::Map::new(), &[]).expect("one");
        // Make sure the second sees a different stamp even on coarse file clocks.
        std::thread::sleep(std::time::Duration::from_millis(20));
        second.known = Some((SystemTime::UNIX_EPOCH, 0));
        second.create("note", "From two", &serde_json::Map::new(), &[]).expect("two");
        let titles: Vec<String> = second.list("", "title").expect("list").into_iter().map(|r| r.title).collect();
        assert_eq!(titles, ["From one", "From two"]);
    }

    #[test]
    fn the_desktop_apps_folder_is_shared_not_copied() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path();
        let mut first = create(root, "Personal", b"pw", 1).expect("create");
        let names = app_folder::names(root).expect("names");
        assert_eq!(names.len(), 3, "the vault, its partner decoy, and a decoy that never changes");
        let times: std::collections::HashSet<_> = names
            .iter()
            .map(|n| std::fs::metadata(app_folder::vault_path(root, n)).and_then(|m| m.modified()).expect("time"))
            .collect();
        assert_eq!(times.len(), 1, "every file carries the same time");
        assert!(!names.iter().any(|n| n.ends_with(".bak") || n.ends_with(".tmp")));

        // Another program, as the desktop app would, opens the same folder and adds an item.
        let Unlock::Opened(mut second, _) = unlock(root, b"pw", None, 2).expect("opens") else { panic!("did not open") };
        assert_eq!(second.path(), first.path());
        second.create("note", "From the app", &serde_json::Map::new(), &[]).expect("note");
        // The file's time did not move, so only its contents say it changed.
        assert!(first.pull().expect("pull"));
        assert_eq!(first.list("From the app", "").expect("list").len(), 1);
        first.create("note", "From the terminal", &serde_json::Map::new(), &[]).expect("note");
        assert!(second.pull().expect("pull"));
        assert_eq!(second.list("", "").expect("list").len(), 2);
        assert_eq!(app_folder::names(root).expect("names").len(), 3, "writes add no files");

        // Wrong passwords count against the app's own limit.
        for _ in 0..6 {
            assert!(matches!(unlock(root, b"nope", None, 3).expect("tries"), Unlock::Wrong));
        }
        assert!(matches!(unlock(root, b"pw", None, 3).expect("tries"), Unlock::Waiting(_)));
    }
}
