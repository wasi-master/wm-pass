//! Proton Drive, with its OpenPGP-encrypted shares, signed in with SRP.
//!
//! A folder in Proton Drive, end-to-end encrypted the way Proton's own apps do it.
//! Proton publishes its Drive SDK (TypeScript and C#) but no API for an app like this to
//! call, and the SDK is not yet open to third-party apps. This follows that SDK's upload
//! model and the API it calls (`drive/v2/volumes`), with the sign-in from go-srp:
//! [`srp`] and [`pgp`] hold the crypto. Proton has said it will retire older clients
//! once the SDK is out and a grace period passes; when that day comes this needs
//! updating.
//!
//! The key chain, from the top: the session's salted key password unlocks the user's
//! keys, which unlock the address key; the address key unlocks the main share's key;
//! that unlocks the root folder's node key; each folder's node key unlocks its
//! children's, and a file's node key unlocks its content session key. Every key, name
//! and block this sink makes is signed with the address key, as Proton's apps sign
//! theirs.

mod api;
pub mod login;
pub mod pgp;
pub mod srp;

use std::sync::{Arc, Mutex};

use base64::Engine;
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use wmpass_core::Secret;

pub use api::{APP_VERSION, status_error};
pub use login::{ProtonOutcome, ProtonPending, ProtonSignedIn, sign_in, submit_code};

use self::api::{Api, Session, lock};
use self::pgp::Key;
use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

const BLOCK_SIZE: usize = 4 * 1024 * 1024;
const BLOCKS_PER_REQUEST: usize = 10;
const BLOCK_PAGE: usize = 100;
const LINKS_PER_CALL: usize = 100;
const STATE_ACTIVE: i64 = 1;
const TYPE_FOLDER: i64 = 1;
const MIME: &str = "application/octet-stream";

/// A folder in Proton Drive. The session and the salted key password, never the account
/// password: the second is what unlocks the account's keys, the same thing Proton's own
/// apps keep.
#[derive(Clone)]
pub struct ProtonConfig {
    /// The account, for the screen.
    pub user: String,
    /// Session id.
    pub uid: String,
    /// Bearer token.
    pub access_token: Secret,
    /// Refresh token. Proton hands out a new one on every refresh.
    pub refresh_token: Secret,
    /// The key password, salted and hashed as Proton's apps do.
    pub salted_key_pass: Secret,
    /// Folder path under My files, `/`-separated; made where missing.
    pub folder: String,
}

impl ProtonConfig {
    /// Where backups go unless the user picks another folder.
    pub const DEFAULT_FOLDER: &str = "WM Pass backups";

    /// Signed in, with a key password.
    pub fn configured(&self) -> bool {
        !self.uid.is_empty() && !self.refresh_token.is_empty() && !self.salted_key_pass.is_empty()
    }
}

impl Default for ProtonConfig {
    fn default() -> Self {
        Self {
            user: String::new(),
            uid: String::new(),
            access_token: Secret::default(),
            refresh_token: Secret::default(),
            salted_key_pass: Secret::default(),
            folder: Self::DEFAULT_FOLDER.to_owned(),
        }
    }
}

impl std::fmt::Debug for ProtonConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtonConfig").field("configured", &self.configured()).finish_non_exhaustive()
    }
}

/// Tokens after a refresh, to store over the old ones.
#[derive(Clone, Debug)]
pub struct ProtonTokens {
    /// Bearer token.
    pub access: Secret,
    /// The new refresh token; the old one no longer works.
    pub refresh: Secret,
}

/// What a run needs once it is signed in and unlocked.
struct Drive {
    volume: String,
    address: Key,
    address_id: String,
    email: String,
    root: Folder,
}

/// A folder's id, its unlocked node key and its hash key.
struct Folder {
    id: String,
    key: Key,
    hash_key: Vec<u8>,
}

/// One child of a folder, its name decrypted.
struct Child {
    id: String,
    name: String,
    folder: bool,
    link: Value,
}

/// A draft revision being uploaded: where it is, and the keys its content goes under.
struct Draft {
    link_id: String,
    revision_id: String,
    node_key: Key,
    session_key: Vec<u8>,
}

/// A Proton Drive folder.
pub struct Proton<H> {
    client: Client<H>,
    config: ProtonConfig,
    now_ms: u64,
    session: Mutex<Session>,
    on_tokens: Box<dyn Fn(&ProtonTokens) + Send + Sync>,
    drive: Mutex<Option<Arc<Drive>>>,
    target: Mutex<Option<Arc<Folder>>>,
}

impl<H> std::fmt::Debug for Proton<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proton").field("config", &self.config).finish_non_exhaustive()
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn int(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn b64(text: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(text).map_err(|_| Fault::Io)
}

fn b64e(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `yyyy-MM-ddTHH:mm:ss+0000`, the shape WM Keyboard's XAttr dates have, in UTC.
fn modification_time(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+0000", rem / 3600, rem % 3600 / 60, rem % 60)
}

impl<H: AsyncHttp> Proton<H> {
    /// A folder for a signed-in `config`. `on_tokens` stores rotated tokens: Proton hands
    /// out a new refresh token on every refresh. `now_ms` dates signatures and files.
    pub fn new(
        client: Client<H>,
        config: ProtonConfig,
        now_ms: u64,
        on_tokens: Box<dyn Fn(&ProtonTokens) + Send + Sync>,
    ) -> Self {
        let session = Mutex::new(Session {
            uid: config.uid.clone(),
            access: config.access_token.clone(),
            refresh: config.refresh_token.clone(),
        });
        Self { client, config, now_ms, session, on_tokens, drive: Mutex::new(None), target: Mutex::new(None) }
    }

    fn api(&self) -> Api<'_, H> {
        Api { client: &self.client, session: &self.session, on_tokens: Some(self.on_tokens.as_ref()) }
    }

    fn now(&self) -> u32 {
        (self.now_ms / 1000) as u32
    }

    async fn drive(&self) -> Result<Arc<Drive>> {
        if let Some(drive) = lock(&self.drive).clone() {
            return Ok(drive);
        }
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        let api = self.api();
        let pass = self.config.salted_key_pass.expose().as_bytes();
        let user = api.get("core/v4/users").await?;
        let user_keys: Vec<Key> = user
            .pointer("/User/Keys")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|k| k.get("PrivateKey").and_then(Value::as_str))
            .filter_map(|armored| Key::unlock(armored, pass))
            .collect();
        if user_keys.is_empty() {
            return Err(Fault::PermissionLost);
        }
        let user_refs: Vec<&Key> = user_keys.iter().collect();

        let my_files = api.get("drive/v2/shares/my-files").await?;
        let volume = my_files.pointer("/Volume/VolumeID").and_then(Value::as_str).ok_or(Fault::TargetMissing)?.to_owned();
        let share = my_files.get("Share").ok_or(Fault::TargetMissing)?;
        let address_id = text(share, "AddressID").ok_or(Fault::Io)?;

        let addresses = api.get("core/v4/addresses").await?;
        let address = addresses
            .get("Addresses")
            .and_then(Value::as_array)
            .and_then(|all| all.iter().find(|a| a.get("ID").and_then(Value::as_str) == Some(address_id.as_str())))
            .ok_or(Fault::Io)?;
        let mut keys: Vec<&Value> = address.get("Keys").and_then(Value::as_array).map(|k| k.iter().collect()).unwrap_or_default();
        keys.sort_by_key(|k| std::cmp::Reverse(int(k, "Primary").unwrap_or(0)));
        let address_key = keys
            .iter()
            .find_map(|key| {
                let armored = key.get("PrivateKey").and_then(Value::as_str)?;
                // A key with a token is unlocked by the token, which the user key
                // decrypts; an older one by the key password itself.
                let token = key
                    .get("Token")
                    .and_then(Value::as_str)
                    .and_then(|token| pgp::decrypt_armored(token, &user_refs).ok());
                Key::unlock(armored, token.as_deref().unwrap_or(pass))
            })
            .ok_or(Fault::PermissionLost)?;

        let share_key = unlock_node(
            share.get("Key").and_then(Value::as_str),
            share.get("Passphrase").and_then(Value::as_str),
            &[&address_key],
        )?;
        let root_link = my_files.get("Link").ok_or(Fault::Io)?;
        let root = folder_of(root_link, &share_key)?;
        let email = text(address, "Email").or_else(|| text(share, "CreatorEmail")).unwrap_or_default();
        let drive = Arc::new(Drive { volume, address: address_key, address_id, email, root });
        *lock(&self.drive) = Some(drive.clone());
        Ok(drive)
    }

    async fn children(&self, d: &Drive, folder: &Folder) -> Result<Vec<Child>> {
        let api = self.api();
        let mut ids: Vec<String> = Vec::new();
        let mut anchor = String::new();
        loop {
            let mut path = format!("drive/v2/volumes/{}/folders/{}/children", d.volume, folder.id);
            if !anchor.is_empty() {
                path.push_str("?AnchorID=");
                path.push_str(&crate::segment(&anchor));
            }
            let page = api.get(&path).await?;
            ids.extend(page.get("LinkIDs").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)));
            let more = page.get("More").is_some_and(|m| m.as_bool() == Some(true) || m.as_i64() == Some(1));
            anchor = text(&page, "AnchorID").unwrap_or_default();
            if !more || anchor.is_empty() {
                break;
            }
        }
        let mut out = Vec::new();
        for batch in ids.chunks(LINKS_PER_CALL) {
            let answer = api.post(&format!("drive/v2/volumes/{}/links", d.volume), &json!({ "LinkIDs": batch })).await?;
            for details in answer.get("Links").and_then(Value::as_array).into_iter().flatten() {
                let Some(link) = details.get("Link") else { continue };
                if int(link, "State") != Some(STATE_ACTIVE) {
                    continue;
                }
                let Some(name) = link
                    .get("Name")
                    .and_then(Value::as_str)
                    .and_then(|name| pgp::decrypt_armored(name, &[&folder.key]).ok())
                    .and_then(|name| String::from_utf8(name).ok())
                else {
                    continue;
                };
                let Some(id) = text(link, "LinkID") else { continue };
                out.push(Child { id, name, folder: int(link, "Type") == Some(TYPE_FOLDER), link: details.clone() });
            }
        }
        Ok(out)
    }

    /// The backup folder, made level by level where missing.
    async fn target(&self, d: &Drive) -> Result<Arc<Folder>> {
        if let Some(target) = lock(&self.target).clone() {
            return Ok(target);
        }
        let mut at: Option<Folder> = None;
        for part in self.config.folder.split('/').map(str::trim).filter(|p| !p.is_empty()) {
            let parent = at.as_ref().unwrap_or(&d.root);
            let existing = self.children(d, parent).await?.into_iter().find(|c| c.folder && c.name == part);
            let next = match existing {
                Some(child) => folder_of(&child.link, &parent.key)?,
                None => self.make_folder(d, parent, part).await?,
            };
            at = Some(next);
        }
        let folder = Arc::new(match at {
            Some(folder) => folder,
            None => Folder { id: d.root.id.clone(), key: d.root.key.clone(), hash_key: d.root.hash_key.clone() },
        });
        *lock(&self.target) = Some(folder.clone());
        Ok(folder)
    }

    async fn make_folder(&self, d: &Drive, parent: &Folder, name: &str) -> Result<Folder> {
        let now = self.now();
        let passphrase = pgp::new_passphrase()?;
        let key = Key::unlock(&pgp::generate_key(&passphrase, now)?, passphrase.as_bytes()).ok_or(Fault::Io)?;
        let hash_key = pgp::new_passphrase()?.into_bytes();
        let body = json!({
            "ParentLinkID": parent.id,
            "NodeKey": key.armored(),
            "NodeHashKey": pgp::encrypt(&key, &hash_key, Some(&key), false, now)?,
            "NodePassphrase": pgp::encrypt(&parent.key, passphrase.as_bytes(), None, false, now)?,
            "NodePassphraseSignature": pgp::sign_armored(&d.address, passphrase.as_bytes(), now)?,
            "SignatureEmail": d.email,
            "Name": pgp::encrypt(&parent.key, name.as_bytes(), Some(&d.address), false, now)?,
            "Hash": pgp::name_hash(&parent.hash_key, name),
        });
        let made = self.api().post(&format!("drive/v2/volumes/{}/folders", d.volume), &body).await?;
        let id = made.pointer("/Folder/ID").and_then(Value::as_str).ok_or(Fault::Io)?.to_owned();
        Ok(Folder { id, key, hash_key })
    }

    async fn new_file(&self, d: &Drive, folder: &Folder, name: &str) -> Result<Draft> {
        let now = self.now();
        let passphrase = pgp::new_passphrase()?;
        let key = Key::unlock(&pgp::generate_key(&passphrase, now)?, passphrase.as_bytes()).ok_or(Fault::Io)?;
        let session_key = pgp::new_session_key()?;
        let body = json!({
            "ParentLinkID": folder.id,
            "Name": pgp::encrypt(&folder.key, name.as_bytes(), Some(&d.address), false, now)?,
            "Hash": pgp::name_hash(&folder.hash_key, name),
            "MIMEType": MIME,
            "NodeKey": key.armored(),
            "NodePassphrase": pgp::encrypt(&folder.key, passphrase.as_bytes(), None, false, now)?,
            "NodePassphraseSignature": pgp::sign_armored(&d.address, passphrase.as_bytes(), now)?,
            "ContentKeyPacket": b64e(&pgp::key_packet(&key, &session_key)?),
            "ContentKeyPacketSignature": pgp::sign_armored(&key, &session_key, now)?,
            "SignatureAddress": d.email,
        });
        let made = self.api().post(&format!("drive/v2/volumes/{}/files", d.volume), &body).await?;
        let file = made.get("File").ok_or(Fault::Io)?;
        Ok(Draft {
            link_id: text(file, "ID").ok_or(Fault::Io)?,
            revision_id: text(file, "RevisionID").ok_or(Fault::Io)?,
            node_key: key,
            session_key,
        })
    }

    /// A new revision of a file already there, under its existing keys: how a sync file
    /// is replaced.
    async fn new_revision(&self, d: &Drive, folder: &Folder, file: &Child) -> Result<Draft> {
        let link = file.link.get("Link").ok_or(Fault::Io)?;
        let properties = file.link.get("File").ok_or(Fault::Io)?;
        let key = unlock_node(
            link.get("NodeKey").and_then(Value::as_str),
            link.get("NodePassphrase").and_then(Value::as_str),
            &[&folder.key],
        )?;
        let packet = b64(properties.get("ContentKeyPacket").and_then(Value::as_str).ok_or(Fault::Io)?)?;
        let session_key = pgp::session_key_of(&packet, &key).ok_or(Fault::Io)?;
        let body = match properties.pointer("/ActiveRevision/RevisionID").and_then(Value::as_str) {
            Some(current) => json!({ "CurrentRevisionID": current }),
            None => json!({}),
        };
        let made = self.api().post(&format!("drive/v2/volumes/{}/files/{}/revisions", d.volume, file.id), &body).await?;
        let revision_id = made.pointer("/Revision/ID").and_then(Value::as_str).ok_or(Fault::Io)?.to_owned();
        Ok(Draft { link_id: file.id.clone(), revision_id, node_key: key, session_key })
    }

    /// Encrypts and uploads the blocks, then commits the revision with its manifest and
    /// extended attributes.
    async fn upload(&self, d: &Drive, draft: &Draft, bytes: &[u8]) -> Result<()> {
        let api = self.api();
        let now = self.now();
        let verification = api
            .get(&format!("drive/v2/volumes/{}/links/{}/revisions/{}/verification", d.volume, draft.link_id, draft.revision_id))
            .await?;
        let code = b64(verification.get("VerificationCode").and_then(Value::as_str).ok_or(Fault::Io)?)?;

        let pieces: Vec<&[u8]> = bytes.chunks(BLOCK_SIZE).collect();
        let mut manifest = Vec::new();
        let mut sha1 = Sha1::new();
        for (batch_index, batch) in pieces.chunks(BLOCKS_PER_REQUEST).enumerate() {
            let mut encrypted = Vec::with_capacity(batch.len());
            let mut list = Vec::with_capacity(batch.len());
            for (offset, plain) in batch.iter().enumerate() {
                let index = batch_index * BLOCKS_PER_REQUEST + offset + 1;
                sha1.update(plain);
                let cipher = pgp::encrypt_with_session_key(&draft.session_key, plain)?;
                let signature = pgp::encrypt(&draft.node_key, &pgp::sign(&d.address, plain, now)?, None, false, now)?;
                // The server's check that the block was made with the key it holds: the
                // code XORed with the ciphertext's first bytes.
                let token: Vec<u8> = code.iter().enumerate().map(|(i, c)| c ^ cipher.get(i).copied().unwrap_or(0)).collect();
                list.push(json!({
                    "Index": index,
                    "EncSignature": signature,
                    "Verifier": { "Token": b64e(&token) },
                }));
                encrypted.push((index, cipher));
            }
            // Hash and Size are left out of each block, as WM Keyboard leaves them out;
            // whether v2 wants them is only known live.
            let request = json!({
                "AddressID": d.address_id,
                "VolumeID": d.volume,
                "LinkID": draft.link_id,
                "RevisionID": draft.revision_id,
                "BlockList": list,
                "ThumbnailList": [],
            });
            let answer = api.post("drive/blocks", &request).await?;
            let links: Vec<&Value> = answer.get("UploadLinks").and_then(Value::as_array).map(|l| l.iter().collect()).unwrap_or_default();
            for (index, cipher) in &encrypted {
                let link = links
                    .iter()
                    .find(|l| int(l, "Index") == Some(*index as i64))
                    .or_else(|| links.get(index - 1 - batch_index * BLOCKS_PER_REQUEST))
                    .ok_or(Fault::Io)?;
                let url = link.get("BareURL").and_then(Value::as_str).ok_or(Fault::Io)?;
                api.upload_block(url, link.get("Token").and_then(Value::as_str).unwrap_or_default(), cipher).await?;
                manifest.extend_from_slice(&Sha256::digest(cipher));
            }
        }
        let xattr = json!({
            "Common": {
                "ModificationTime": modification_time(self.now_ms),
                "Size": bytes.len(),
                "BlockSizes": pieces.iter().map(|p| p.len()).collect::<Vec<_>>(),
                "Digests": { "SHA1": hex(&sha1.finalize()) },
            }
        });
        let commit = json!({
            "ManifestSignature": pgp::sign_armored(&d.address, &manifest, now)?,
            "SignatureAddress": d.email,
            "XAttr": pgp::encrypt(&draft.node_key, xattr.to_string().as_bytes(), Some(&d.address), true, now)?,
            "ChecksumVerified": false,
            "Photo": Value::Null,
        });
        api.put(&format!("drive/v2/volumes/{}/files/{}/revisions/{}", d.volume, draft.link_id, draft.revision_id), &commit)
            .await?;
        Ok(())
    }
}

fn unlock_node(armored_key: Option<&str>, armored_passphrase: Option<&str>, parents: &[&Key]) -> Result<Key> {
    let (Some(armored_key), Some(armored_passphrase)) = (armored_key, armored_passphrase) else { return Err(Fault::Io) };
    let passphrase = pgp::decrypt_armored(armored_passphrase, parents).map_err(|_| Fault::Io)?;
    Key::unlock(armored_key, &passphrase).ok_or(Fault::Io)
}

/// A folder from its link details (`{Link, Folder}`) and its parent's key.
fn folder_of(details: &Value, parent: &Key) -> Result<Folder> {
    let link = details.get("Link").ok_or(Fault::Io)?;
    let key = unlock_node(
        link.get("NodeKey").and_then(Value::as_str),
        link.get("NodePassphrase").and_then(Value::as_str),
        &[parent],
    )?;
    let hash_key = details
        .pointer("/Folder/NodeHashKey")
        .and_then(Value::as_str)
        .and_then(|armored| pgp::decrypt_armored(armored, &[&key]).ok())
        .ok_or(Fault::Io)?;
    Ok(Folder { id: text(link, "LinkID").ok_or(Fault::Io)?, key, hash_key })
}

impl<H: AsyncHttp> Sink for Proton<H> {
    async fn readiness(&self) -> Result<()> {
        let d = self.drive().await?;
        self.target(&d).await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let d = self.drive().await?;
        let folder = self.target(&d).await?;
        let existing = self.children(&d, &folder).await?.into_iter().find(|c| !c.folder && c.name == name);
        let draft = match &existing {
            None => self.new_file(&d, &folder, name).await?,
            Some(file) => self.new_revision(&d, &folder, file).await?,
        };
        if let Err(fault) = self.upload(&d, &draft, bytes).await {
            // Best effort: a draft left behind is invisible, but holds quota.
            let api = self.api();
            let _ = if existing.is_none() {
                api.post(&format!("drive/v2/volumes/{}/delete_multiple", d.volume), &json!({ "LinkIDs": [draft.link_id] })).await
            } else {
                api.delete(&format!("drive/v2/volumes/{}/files/{}/revisions/{}", d.volume, draft.link_id, draft.revision_id))
                    .await
            };
            return Err(fault);
        }
        Ok(Stored { id: draft.link_id, name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let d = self.drive().await?;
        let folder = self.target(&d).await?;
        // Proton keeps only the encrypted size, so the size is left unknown (0).
        Ok(self
            .children(&d, &folder)
            .await?
            .into_iter()
            .filter(|c| !c.folder)
            .map(|c| Stored { id: c.id, name: c.name, size: 0 })
            .collect())
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let d = self.drive().await?;
        let folder = self.target(&d).await?;
        let child = self.children(&d, &folder).await?.into_iter().find(|c| c.id == file.id).ok_or(Fault::TargetMissing)?;
        let link = child.link.get("Link").ok_or(Fault::Io)?;
        let properties = child.link.get("File").ok_or(Fault::Io)?;
        let key = unlock_node(
            link.get("NodeKey").and_then(Value::as_str),
            link.get("NodePassphrase").and_then(Value::as_str),
            &[&folder.key],
        )?;
        let packet = b64(properties.get("ContentKeyPacket").and_then(Value::as_str).ok_or(Fault::Io)?)?;
        let revision = properties.pointer("/ActiveRevision/RevisionID").and_then(Value::as_str).ok_or(Fault::TargetMissing)?;

        let api = self.api();
        let mut out = Vec::new();
        let mut from = 1i64;
        loop {
            let page = api
                .get(&format!(
                    "drive/v2/volumes/{}/files/{}/revisions/{revision}?PageSize={BLOCK_PAGE}&FromBlockIndex={from}",
                    d.volume, child.id
                ))
                .await?;
            let mut blocks: Vec<&Value> = page.pointer("/Revision/Blocks").and_then(Value::as_array).map(|b| b.iter().collect()).unwrap_or_default();
            blocks.sort_by_key(|b| int(b, "Index").unwrap_or(0));
            if blocks.is_empty() {
                break;
            }
            for block in &blocks {
                let url = block.get("BareURL").and_then(Value::as_str).ok_or(Fault::Io)?;
                let cipher = api.download_block(url, block.get("Token").and_then(Value::as_str).unwrap_or_default()).await?;
                if block.get("Hash").and_then(Value::as_str).is_some_and(|hash| hash != b64e(&Sha256::digest(&cipher))) {
                    return Err(Fault::Io);
                }
                let mut message = packet.clone();
                message.extend_from_slice(&cipher);
                out.extend_from_slice(&pgp::decrypt(&message, &[&key])?);
            }
            if blocks.len() < BLOCK_PAGE {
                break;
            }
            from = blocks.last().and_then(|b| int(b, "Index")).unwrap_or(from + blocks.len() as i64 - 1) + 1;
        }
        Ok(out)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let d = self.drive().await?;
        let api = self.api();
        let ids = json!({ "LinkIDs": [file.id] });
        // To the trash, then out of it: Proton counts the trash against the quota, and
        // rotation would fill it.
        api.post(&format!("drive/v2/volumes/{}/trash_multiple", d.volume), &ids).await?;
        api.post(&format!("drive/v2/volumes/{}/trash/delete_multiple", d.volume), &ids).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
