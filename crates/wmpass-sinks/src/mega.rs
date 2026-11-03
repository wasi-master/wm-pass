//! MEGA, with its client-side encryption of names and contents.
//!
//! Speaks MEGA's own API the way its apps do: every file is encrypted here with a key of
//! its own, and MEGA only ever holds ciphertext and keys wrapped in the account's master
//! key. Ported from WM Keyboard's sink, which follows the open MEGA clients (go-mega,
//! which rclone uses, and megatools); MEGA publishes the protocol only as its SDK's source.
//!
//! - Sign-in happens once, through [`sign_in`]; the sink only has the session id and the
//!   master key it produced.
//! - Upload: AES-128-CTR over the file under a random key, a CBC-MAC per chunk folded
//!   into the file's MAC, then a `p` call that makes the node with its name encrypted
//!   under the same key. MEGA lets one folder hold two files of one name, so a write
//!   removes the older copies after.
//! - Download decrypts and checks the MAC, so a damaged file fails here rather than as a
//!   vault that does not open.
//! - Delete is permanent, not to the rubbish bin: MEGA counts the bin against the quota,
//!   and rotation would fill it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use base64::Engine;
use num_bigint::BigUint;
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use wmpass_core::Secret;
use wmpass_net::Request;

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

const API: &str = "https://g.api.mega.co.nz";
/// Where backups go unless the user picks a folder.
pub const DEFAULT_FOLDER: &str = "WM Pass backups";

const TYPE_FILE: i64 = 0;
const TYPE_FOLDER: i64 = 1;
const TYPE_ROOT: i64 = 2;
const FILE_KEY_WORDS: usize = 8;
const KEY_BYTES: usize = 16;

const EAGAIN: i64 = -3;
const ERATELIMIT: i64 = -4;
const MFA_REQUIRED: i64 = -26;
const RETRIES: u32 = 6;
const RETRY_START_MS: u64 = 500;
const V2_ROUNDS: u32 = 100_000;

/// A MEGA account's folder, as [`sign_in`] left it.
#[derive(Clone)]
pub struct MegaConfig {
    /// The account's email, lower case. Shown, never sent again.
    pub email: String,
    /// The session MEGA handed out at sign-in.
    pub session_id: Secret,
    /// The account's master key, base64. Decrypts every file and folder key.
    pub master_key: Secret,
    /// Folder path from the account's root, `/` between levels.
    pub folder: String,
}

impl Default for MegaConfig {
    fn default() -> Self {
        Self { email: String::new(), session_id: Secret::default(), master_key: Secret::default(), folder: DEFAULT_FOLDER.to_owned() }
    }
}

impl std::fmt::Debug for MegaConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MegaConfig").field("configured", &self.configured()).finish_non_exhaustive()
    }
}

impl MegaConfig {
    /// Whether there is a session to use.
    pub fn configured(&self) -> bool {
        !self.session_id.is_empty() && !self.master_key.is_empty()
    }
}

/// What a MEGA error number means to the user: -15 an expired session, -16 a blocked
/// account, -11 a refused access, -26 a 2FA code wanted; -9 a missing node; -17 the
/// account over its quota.
fn error_of(code: i64) -> Fault {
    match code {
        -11 | -15 | -16 | -26 => Fault::PermissionLost,
        -9 => Fault::TargetMissing,
        -17 => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

// ---- Crypto ----

/// MEGA's base64: URL-safe, padding optional.
fn b64encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn b64decode(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD_INDIFFERENT.decode(text.trim()).ok()
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| Fault::Io)?;
    Ok(out)
}

fn request_id() -> Result<String> {
    Ok(b64encode(&random::<8>()?)[..10].to_owned())
}

/// Keys are often handled as big-endian 32-bit words ("a32"), which is where the XOR
/// games in node keys come from. A tail short of a word is padded with zeros.
fn to_words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks(4)
        .map(|chunk| {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            u32::from_be_bytes(word)
        })
        .collect()
}

fn to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_be_bytes()).collect()
}

/// A file's AES key from its 8-word node key: the two halves XORed. A folder's is its key.
fn node_key(key: &[u32]) -> [u32; 4] {
    let at = |i: usize| key.get(i).copied().unwrap_or(0);
    if key.len() >= 8 {
        [at(0) ^ at(4), at(1) ^ at(5), at(2) ^ at(6), at(3) ^ at(7)]
    } else {
        [at(0), at(1), at(2), at(3)]
    }
}

fn aes(key: &[u8]) -> Option<Aes128> {
    Aes128::new_from_slice(key).ok()
}

fn encrypt_block(cipher: &Aes128, block: &mut [u8; 16]) {
    let mut b = aes::Block::from(*block);
    cipher.encrypt_block(&mut b);
    block.copy_from_slice(&b);
}

fn ecb_encrypt(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let cipher = aes(key)?;
    if !data.len().is_multiple_of(16) {
        return None;
    }
    let mut out = data.to_vec();
    for chunk in out.as_chunks_mut::<16>().0 {
        encrypt_block(&cipher, chunk);
    }
    Some(out)
}

fn ecb_decrypt(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let cipher = aes(key)?;
    if !data.len().is_multiple_of(16) {
        return None;
    }
    let mut out = data.to_vec();
    for chunk in out.as_chunks_mut::<16>().0 {
        let mut b = aes::Block::from(*chunk);
        cipher.decrypt_block(&mut b);
        chunk.copy_from_slice(&b);
    }
    Some(out)
}

fn cbc_encrypt(cipher: &Aes128, iv: [u8; 16], data: &[u8]) -> Vec<u8> {
    let mut last = iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        for (byte, value) in last.iter_mut().zip(chunk) {
            *byte ^= value;
        }
        encrypt_block(cipher, &mut last);
        out.extend_from_slice(&last);
    }
    out
}

fn cbc_decrypt(cipher: &Aes128, data: &[u8]) -> Option<Vec<u8>> {
    if !data.len().is_multiple_of(16) {
        return None;
    }
    let mut previous = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<16>().0 {
        let mut b = aes::Block::from(*chunk);
        cipher.decrypt_block(&mut b);
        out.extend(b.iter().zip(previous).map(|(byte, mask)| byte ^ mask));
        previous = *chunk;
    }
    Some(out)
}

/// AES-CTR with the whole 128-bit block as a big-endian counter, as Java's CTR runs it.
fn ctr(cipher: &Aes128, iv: [u8; 16], data: &[u8]) -> Vec<u8> {
    let mut counter = u128::from_be_bytes(iv);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut stream = counter.to_be_bytes();
        encrypt_block(cipher, &mut stream);
        out.extend(chunk.iter().zip(stream).map(|(byte, mask)| byte ^ mask));
        counter = counter.wrapping_add(1);
    }
    out
}

/// Zeros to a whole number of blocks, and at least one block.
fn pad16(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    let blocks = data.len().div_ceil(16).max(1);
    out.resize(blocks * 16, 0);
    out
}

/// A node's name, as `MEGA{"n":"…"}` padded with zeros and AES-CBC encrypted under a
/// zero IV.
fn encrypt_name(key: &[u8], name: &str) -> Option<String> {
    let attributes = format!("MEGA{}", json!({ "n": name }));
    Some(b64encode(&cbc_encrypt(&aes(key)?, [0; 16], &pad16(attributes.as_bytes()))))
}

fn decrypt_name(key: &[u8], attributes: &str) -> Option<String> {
    let plain = cbc_decrypt(&aes(key)?, &b64decode(attributes)?)?;
    let text = String::from_utf8_lossy(&plain);
    let body = text.strip_prefix("MEGA")?.trim_end_matches('\0');
    let (start, end) = (body.find('{')?, body.rfind('}')?);
    if end < start {
        return None;
    }
    let parsed: Value = serde_json::from_str(&body[start..=end]).ok()?;
    parsed.get("n")?.as_str().map(str::to_owned)
}

/// The v1 password key: 65,536 rounds of AES over a fixed seed, keyed by the password's
/// blocks.
fn prepare_key(password: &str) -> [u8; 16] {
    let words = to_words(password.as_bytes());
    let ciphers: Vec<Aes128> = words
        .chunks(4)
        .filter_map(|group| {
            let mut key = [0u32; 4];
            key[..group.len()].copy_from_slice(group);
            aes(&to_bytes(&key))
        })
        .collect();
    let mut block = [0u8; 16];
    block.copy_from_slice(&to_bytes(&[0x93C4_67E3, 0x7DB0_C7A4, 0xD1BE_3F81, 0x0152_CB56]));
    for _ in 0..65_536 {
        for cipher in &ciphers {
            encrypt_block(cipher, &mut block);
        }
    }
    block
}

/// The v1 user handle: the email folded into one block, then 16,384 rounds of AES.
fn string_hash(text: &str, key: &[u8; 16]) -> Option<String> {
    let mut h = [0u32; 4];
    for (i, word) in to_words(text.as_bytes()).into_iter().enumerate() {
        h[i & 3] ^= word;
    }
    let cipher = aes(key)?;
    let mut block = [0u8; 16];
    block.copy_from_slice(&to_bytes(&h));
    for _ in 0..16_384 {
        encrypt_block(&cipher, &mut block);
    }
    let out = to_words(&block);
    Some(b64encode(&to_bytes(&[out[0], out[2]])))
}

/// One MPI: a 16-bit bit length, then the bytes. Returns the number and where the next
/// starts.
fn mpi(bytes: &[u8], at: usize) -> Option<(BigUint, usize)> {
    let bits = usize::from(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
    let end = at + 2 + bits.div_ceil(8);
    Some((BigUint::from_bytes_be(bytes.get(at + 2..end)?), end))
}

/// The session id: `csid` decrypted with the account's RSA key, which the master key
/// unwraps. Plain modular exponentiation, as every MEGA client does it; there is no
/// padding to check, and the result is a session token MEGA verifies itself.
fn decrypt_session_id(private_key: &str, csid: &str, master: &[u8]) -> Option<String> {
    const SID_BYTES: usize = 43;
    let key = ecb_decrypt(master, &pad16(&b64decode(private_key)?))?;
    let (p, after_p) = mpi(&key, 0)?;
    let (q, after_q) = mpi(&key, after_p)?;
    let (d, _) = mpi(&key, after_q)?;
    let (m, _) = mpi(&b64decode(csid)?, 0)?;
    let n = p * q;
    if n.bits() == 0 {
        return None;
    }
    let mut plain = m.modpow(&d, &n).to_bytes_be();
    plain.resize(plain.len().max(SID_BYTES), 0);
    Some(b64encode(&plain[..SID_BYTES]))
}

/// MEGA's chunk boundaries: 128 KiB, 256, … up to 1 MiB, then 1 MiB each. The MACs
/// follow them.
fn chunks(size: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let (mut at, mut left, mut i) = (0u64, size, 1u64);
    while left > 0 {
        let chunk = (if i <= 8 { i * 131_072 } else { 1_048_576 }).min(left);
        out.push((at, chunk));
        at += chunk;
        left -= chunk;
        i += 1;
    }
    out
}

/// What [`seal`] produced: the ciphertext and both forms of the key.
struct Sealed {
    ciphertext: Vec<u8>,
    /// Key then nonce: six words.
    file_key: [u32; 6],
    /// The 8-word key MEGA stores (wrapped), which carries the nonce and the MAC.
    node_key: [u32; 8],
}

/// Encrypts a file for upload: AES-CTR over the bytes, and the MAC.
fn seal(plain: &[u8], key: [u32; 6]) -> Option<Sealed> {
    let aes_key = to_bytes(&key[..4]);
    let cipher = aes(&aes_key)?;
    let mut iv = [0u8; 16];
    iv[..8].copy_from_slice(&to_bytes(&key[4..6]));
    let ciphertext = ctr(&cipher, iv, plain);
    let mac = mac(plain, &cipher, key[4], key[5]);
    let node_key = [key[0] ^ key[4], key[1] ^ key[5], key[2] ^ mac[0], key[3] ^ mac[1], key[4], key[5], mac[0], mac[1]];
    Some(Sealed { ciphertext, file_key: key, node_key })
}

/// Decrypts a downloaded file with its 8-word node key, or `None` when its MAC does not
/// match.
fn open(ciphertext: &[u8], key: &[u32]) -> Option<Vec<u8>> {
    if key.len() < FILE_KEY_WORDS {
        return None;
    }
    let cipher = aes(&to_bytes(&node_key(key)))?;
    let mut iv = [0u8; 16];
    iv[..8].copy_from_slice(&to_bytes(&key[4..6]));
    let plain = ctr(&cipher, iv, ciphertext);
    let mac = mac(&plain, &cipher, key[4], key[5]);
    (mac[0] == key[6] && mac[1] == key[7]).then_some(plain)
}

/// The file MAC: a CBC-MAC per chunk under the nonce, the chunk MACs CBC-MACed again,
/// folded to two words.
fn mac(plain: &[u8], cipher: &Aes128, n0: u32, n1: u32) -> [u32; 2] {
    let mut chunk_iv = [0u8; 16];
    chunk_iv.copy_from_slice(&to_bytes(&[n0, n1, n0, n1]));
    let mut last = [0u8; 16];
    let mut list = chunks(plain.len() as u64);
    if list.is_empty() {
        list.push((0, 0));
    }
    for (at, length) in list {
        let piece = pad16(&plain[at as usize..(at + length) as usize]);
        let inner = cbc_encrypt(cipher, chunk_iv, &piece);
        for (byte, value) in last.iter_mut().zip(&inner[inner.len() - 16..]) {
            *byte ^= value;
        }
        encrypt_block(cipher, &mut last);
    }
    let t = to_words(&last);
    [t[0] ^ t[1], t[2] ^ t[3]]
}

/// Answers MEGA's proof-of-work challenge (`X-Hashcash: 1:easiness:time:token`), the 402
/// it sends a client it thinks is a bot: find a 4-byte prefix whose SHA-256 over the
/// token repeated 262,144 times starts at or below a threshold. Gives up after a minute
/// (a fixed number of tries in a browser, which has no clock here); `None` then.
fn hashcash(header: &str) -> Option<String> {
    const REPLICAS: usize = 262_144;
    const SLOT: usize = 48;
    let parts: Vec<&str> = header.split(':').collect();
    if parts.len() != 4 || parts[0] != "1" {
        return None;
    }
    let easiness: u32 = parts[1].parse().ok().filter(|e| *e <= 255)?;
    let token = parts[3];
    let threshold = ((u64::from(easiness & 63) << 1) + 1) << ((easiness >> 6) * 7 + 3);
    let threshold = threshold & 0xffff_ffff;
    let token_bytes = pad16(&b64decode(token)?);
    let mut buffer = vec![0u8; 4 + REPLICAS * SLOT];
    let n = token_bytes.len().min(SLOT);
    for i in 0..REPLICAS {
        buffer[4 + i * SLOT..4 + i * SLOT + n].copy_from_slice(&token_bytes[..n]);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    #[cfg(target_arch = "wasm32")]
    let mut tries = 0u32;
    loop {
        #[cfg(not(target_arch = "wasm32"))]
        if std::time::Instant::now() >= deadline {
            return None;
        }
        #[cfg(target_arch = "wasm32")]
        {
            tries += 1;
            if tries > 2_048 {
                return None;
            }
        }
        let mut j = 0;
        while j < 4 {
            buffer[j] = buffer[j].wrapping_add(1);
            if buffer[j] != 0 {
                break;
            }
            j += 1;
        }
        if j == 4 {
            return None;
        }
        let hash = Sha256::digest(&buffer);
        let value = u64::from(u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]));
        if value <= threshold {
            return Some(format!("1:{token}:{}", b64encode(&buffer[..4])));
        }
    }
}

// ---- The command channel ----

/// Why a command failed: MEGA's own error number, or no usable answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    Code(i64),
    Fault(Fault),
}

impl From<Fault> for Refusal {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault)
    }
}

fn refusal_fault(refusal: Refusal) -> Fault {
    match refusal {
        Refusal::Code(code) => error_of(code),
        Refusal::Fault(fault) => fault,
    }
}

/// MEGA's command channel: a JSON array of one command POSTed to `/cs`, a JSON array of
/// one answer back, where a bare negative number is an error. Busy (-3) and rate limited
/// (-4) are retried; HTTP 402 is a proof-of-work challenge ([`hashcash`]).
async fn command<H: AsyncHttp>(client: &Client<H>, sequence: &AtomicU64, session: Option<&str>, body: &Value) -> std::result::Result<Value, Refusal> {
    let payload = Value::Array(vec![body.clone()]);
    let mut wait = RETRY_START_MS;
    for _ in 0..RETRIES {
        let mut url = format!("{API}/cs?id={}", sequence.fetch_add(1, Ordering::Relaxed));
        if let Some(sid) = session.filter(|sid| !sid.is_empty()) {
            url.push_str("&sid=");
            url.push_str(sid);
        }
        let mut request = Request::json("POST", url, &payload);
        request.timeout_ms = 60_000;
        let mut response = client.send(request.clone()).await?;
        if response.status == 402 {
            let answer = response.header("x-hashcash").and_then(hashcash).ok_or(Fault::Io)?;
            response = client.send(request.header("X-Hashcash", answer)).await?;
        }
        if !(200..300).contains(&response.status) {
            return Err(Fault::Io.into());
        }
        let parsed = response.json().map_err(|_| Fault::Io)?;
        // A whole-request error is a bare number; a per-command one is the number as the
        // array's only element.
        let answer = match parsed {
            Value::Array(mut items) if !items.is_empty() => items.swap_remove(0),
            other => other,
        };
        match answer.as_i64() {
            Some(EAGAIN | ERATELIMIT) => {
                pause(wait);
                wait *= 2;
            }
            Some(code) if code < 0 => return Err(Refusal::Code(code)),
            _ => return Ok(answer),
        }
    }
    Err(Fault::Io.into())
}

/// Waits before a retry. A browser has no blocking sleep, so there it retries at once.
fn pause(ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(std::time::Duration::from_millis(ms));
    #[cfg(target_arch = "wasm32")]
    let _ = ms;
}

fn starting_sequence() -> u64 {
    random::<4>().map_or(1, |bytes| u64::from(u32::from_le_bytes(bytes) >> 2))
}

// ---- The sink ----

/// One node of the account's tree, decrypted.
#[derive(Clone)]
struct Node {
    handle: String,
    parent: String,
    kind: i64,
    name: String,
    size: u64,
    /// The whole decrypted key: 4 words for a folder, 8 for a file.
    key: Vec<u32>,
}

/// A folder in a MEGA account.
pub struct Mega<H> {
    client: Client<H>,
    config: MegaConfig,
    sequence: AtomicU64,
    folder: Mutex<Option<String>>,
}

impl<H> std::fmt::Debug for Mega<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mega").field("config", &self.config).finish_non_exhaustive()
    }
}

impl<H: AsyncHttp> Mega<H> {
    /// A folder in the account `config` signed in to.
    pub fn new(client: Client<H>, config: MegaConfig) -> Self {
        Self { client, config, sequence: AtomicU64::new(starting_sequence()), folder: Mutex::new(None) }
    }

    fn master(&self) -> Result<Vec<u8>> {
        b64decode(self.config.master_key.expose()).filter(|key| key.len() == KEY_BYTES).ok_or(Fault::PermissionLost)
    }

    async fn call(&self, body: Value) -> Result<Value> {
        command(&self.client, &self.sequence, Some(self.config.session_id.expose()), &body).await.map_err(refusal_fault)
    }

    async fn nodes(&self, master: &[u8]) -> Result<Vec<Node>> {
        let answer = self.call(json!({"a": "f", "c": 1})).await?;
        let items = answer.get("f").and_then(Value::as_array).ok_or(Fault::Io)?;
        Ok(items.iter().filter_map(|item| decode(item, master)).collect())
    }

    /// The backup folder's handle, made level by level where missing, and the tree as it
    /// stood.
    async fn folder(&self, master: &[u8]) -> Result<(String, Vec<Node>)> {
        let all = self.nodes(master).await?;
        if let Some(handle) = self.folder.lock().map_err(|_| Fault::Io)?.clone() {
            return Ok((handle, all));
        }
        let mut at = all.iter().find(|node| node.kind == TYPE_ROOT).map(|node| node.handle.clone()).ok_or(Fault::TargetMissing)?;
        for part in self.config.folder.split('/').map(str::trim).filter(|part| !part.is_empty()) {
            at = match all.iter().find(|node| node.kind == TYPE_FOLDER && node.parent == at && node.name == part) {
                Some(node) => node.handle.clone(),
                None => self.make_folder(master, &at, part).await?,
            };
        }
        *self.folder.lock().map_err(|_| Fault::Io)? = Some(at.clone());
        Ok((at, all))
    }

    async fn make_folder(&self, master: &[u8], parent: &str, name: &str) -> Result<String> {
        let key = random::<KEY_BYTES>()?;
        let answer = self
            .call(json!({
                "a": "p",
                "t": parent,
                "n": [{
                    "h": "xxxxxxxx",
                    "t": TYPE_FOLDER,
                    "a": encrypt_name(&key, name).ok_or(Fault::Io)?,
                    "k": b64encode(&ecb_encrypt(master, &key).ok_or(Fault::Io)?),
                }],
                "i": request_id()?,
            }))
            .await?;
        first_handle(&answer)
    }

    async fn remove(&self, handle: &str) -> Result<()> {
        match self.call(json!({"a": "d", "n": handle, "i": request_id()?})).await {
            Ok(_) | Err(Fault::TargetMissing) => Ok(()),
            Err(other) => Err(other),
        }
    }

    /// POSTs one encrypted chunk; the last one's answer is the completion handle.
    async fn upload(&self, url: String, bytes: &[u8]) -> Result<String> {
        let mut request = Request::verb("POST", url);
        request.body = Some(("application/octet-stream", bytes.to_vec()));
        request.timeout_ms = 300_000;
        let response = self.client.send(request).await?;
        if !(200..300).contains(&response.status) {
            return Err(Fault::Io);
        }
        let text = response.text().map_err(|_| Fault::Io)?.trim().to_owned();
        if let Ok(code) = text.parse::<i64>()
            && code < 0
        {
            return Err(error_of(code));
        }
        Ok(text)
    }
}

fn first_handle(answer: &Value) -> Result<String> {
    answer.pointer("/f/0/h").and_then(Value::as_str).map(str::to_owned).ok_or(Fault::Io)
}

fn decode(item: &Value, master: &[u8]) -> Option<Node> {
    let kind = item.get("t")?.as_i64()?;
    let handle = item.get("h")?.as_str()?.to_owned();
    let parent = item.get("p").and_then(Value::as_str).unwrap_or_default().to_owned();
    if kind == TYPE_ROOT {
        return Some(Node { handle, parent, kind, name: String::new(), size: 0, key: Vec::new() });
    }
    if kind != TYPE_FILE && kind != TYPE_FOLDER {
        return None;
    }
    // "owner:key", or several of those split by '/' for shared items. Only the user's own
    // keys are wrapped in the master key.
    let owner = item.get("u")?.as_str()?;
    let wrapped = item.get("k")?.as_str()?.split('/').find_map(|pair| {
        let (who, key) = pair.split_once(':')?;
        (who == owner).then_some(key)
    })?;
    let key = to_words(&ecb_decrypt(master, &b64decode(wrapped)?)?);
    if kind == TYPE_FILE && key.len() < FILE_KEY_WORDS {
        return None;
    }
    let name = decrypt_name(&to_bytes(&node_key(&key)), item.get("a")?.as_str()?)?;
    let size = item.get("s").and_then(Value::as_u64).unwrap_or(0);
    Some(Node { handle, parent, kind, name, size, key })
}

impl<H: AsyncHttp> Sink for Mega<H> {
    async fn readiness(&self) -> Result<()> {
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        self.folder(&self.master()?).await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let master = self.master()?;
        let (parent, before) = self.folder(&master).await?;
        let target = self.call(json!({"a": "u", "s": bytes.len(), "ssl": 2})).await?;
        let target = target.get("p").and_then(Value::as_str).ok_or(Fault::Io)?.to_owned();

        let key = to_words(&random::<24>()?);
        let sealed = seal(bytes, [key[0], key[1], key[2], key[3], key[4], key[5]]).ok_or(Fault::Io)?;
        let mut completion = String::new();
        let mut pieces = chunks(bytes.len() as u64);
        if pieces.is_empty() {
            // An empty file is still one upload, of nothing, at offset 0.
            pieces.push((0, 0));
        }
        for (at, length) in pieces {
            let part = &sealed.ciphertext[at as usize..(at + length) as usize];
            completion = self.upload(format!("{target}/{at}"), part).await?;
        }
        if completion.is_empty() {
            return Err(Fault::Io);
        }

        let made = self
            .call(json!({
                "a": "p",
                "t": parent,
                "n": [{
                    "h": completion,
                    "t": TYPE_FILE,
                    "a": encrypt_name(&to_bytes(&sealed.file_key[..4]), name).ok_or(Fault::Io)?,
                    "k": b64encode(&ecb_encrypt(&master, &to_bytes(&sealed.node_key)).ok_or(Fault::Io)?),
                }],
                "i": request_id()?,
            }))
            .await?;
        let handle = first_handle(&made)?;
        // The older copies of this name, now that the new one is in.
        for old in before.iter().filter(|node| node.parent == parent && node.kind == TYPE_FILE && node.name == name && node.handle != handle) {
            let _ = self.remove(&old.handle).await;
        }
        Ok(Stored { id: handle, name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let (parent, all) = self.folder(&self.master()?).await?;
        Ok(all
            .into_iter()
            .filter(|node| node.parent == parent && node.kind == TYPE_FILE)
            .map(|node| Stored { id: node.handle, name: node.name, size: node.size })
            .collect())
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let master = self.master()?;
        let node = self
            .nodes(&master)
            .await?
            .into_iter()
            .find(|node| node.handle == file.id && node.kind == TYPE_FILE)
            .ok_or(Fault::TargetMissing)?;
        let answer = self.call(json!({"a": "g", "g": 1, "n": node.handle, "ssl": 2})).await?;
        let url = answer.get("g").and_then(Value::as_str).ok_or(Fault::Io)?;
        let mut request = Request::get(url);
        request.timeout_ms = 300_000;
        let response = self.client.send(request).await?;
        if !(200..300).contains(&response.status) {
            return Err(Fault::Io);
        }
        open(&response.body, &node.key).ok_or(Fault::NotVerified)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        self.remove(&file.id).await
    }
}

// ---- Sign-in ----

/// Where a sign-in stands.
#[derive(Debug)]
pub enum MegaSignIn {
    /// Signed in: store this config. The password is not in it.
    SignedIn(MegaConfig),
    /// The account has two-factor sign-in: ask for the code, then call
    /// [`sign_in_with_code`] with this.
    NeedsCode(MegaPending),
    /// MEGA refused: a wrong password or code, a blocked account.
    Refused(Fault),
    /// No usable answer, or network use is off.
    Unreachable(Fault),
}

/// A sign-in waiting for its 2FA code. Holds the key derived from the password, never the
/// password itself.
pub struct MegaPending {
    email: String,
    pass_key: Secret,
    user_handle: Secret,
    version: i64,
}

impl std::fmt::Debug for MegaPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MegaPending").finish_non_exhaustive()
    }
}

fn sign_in_failure(refusal: Refusal) -> MegaSignIn {
    match refusal {
        // An unknown account answers "not found", which is a refused sign-in to the user.
        Refusal::Code(-9) => MegaSignIn::Refused(Fault::PermissionLost),
        Refusal::Code(code) => MegaSignIn::Refused(error_of(code)),
        Refusal::Fault(fault @ (Fault::Io | Fault::Blocked)) => MegaSignIn::Unreachable(fault),
        Refusal::Fault(fault) => MegaSignIn::Refused(fault),
    }
}

/// Signs in with an email and password. v1 accounts stretch the password with MEGA's own
/// AES loop, v2 accounts with PBKDF2-SHA512 over a salt MEGA hands out.
pub async fn sign_in<H: AsyncHttp>(client: &Client<H>, email: &str, password: &Secret) -> MegaSignIn {
    let user = email.trim().to_lowercase();
    let sequence = AtomicU64::new(starting_sequence());
    let pre = match command(client, &sequence, None, &json!({"a": "us0", "user": user})).await {
        Ok(pre) => pre,
        Err(refusal) => return sign_in_failure(refusal),
    };
    let version = pre.get("v").and_then(Value::as_i64).unwrap_or(1);
    let (pass_key, handle) = if version >= 2 {
        let Some(salt) = pre.get("s").and_then(Value::as_str).and_then(b64decode) else {
            return MegaSignIn::Unreachable(Fault::Io);
        };
        let mut derived = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha512>(password.expose().as_bytes(), &salt, V2_ROUNDS, &mut derived);
        (b64encode(&derived[..16]), b64encode(&derived[16..]))
    } else {
        let key = prepare_key(password.expose());
        let Some(handle) = string_hash(&user, &key) else { return MegaSignIn::Unreachable(Fault::Io) };
        (b64encode(&key), handle)
    };
    let pending = MegaPending { email: user, pass_key: Secret::from(pass_key), user_handle: Secret::from(handle), version };
    finish(client, &sequence, pending, "").await
}

/// Finishes a sign-in that asked for a 2FA code.
pub async fn sign_in_with_code<H: AsyncHttp>(client: &Client<H>, pending: MegaPending, code: &str) -> MegaSignIn {
    finish(client, &AtomicU64::new(starting_sequence()), pending, code).await
}

async fn finish<H: AsyncHttp>(client: &Client<H>, sequence: &AtomicU64, pending: MegaPending, code: &str) -> MegaSignIn {
    let mut body = json!({"a": "us", "user": pending.email, "uh": pending.user_handle.expose()});
    if pending.version >= 2 {
        match random::<16>() {
            Ok(sek) => body["sek"] = Value::from(b64encode(&sek)),
            Err(fault) => return MegaSignIn::Unreachable(fault),
        }
    }
    let digits: String = code.chars().filter(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        body["mfa"] = Value::from(digits);
    }
    let answer = match command(client, sequence, None, &body).await {
        Ok(answer) => answer,
        Err(Refusal::Code(MFA_REQUIRED)) => return MegaSignIn::NeedsCode(pending),
        Err(refusal) => return sign_in_failure(refusal),
    };
    let Some(pass_key) = b64decode(pending.pass_key.expose()) else { return MegaSignIn::Unreachable(Fault::Io) };
    let master = answer.get("k").and_then(Value::as_str).and_then(b64decode).and_then(|k| ecb_decrypt(&pass_key, &k));
    let private_key = answer.get("privk").and_then(Value::as_str);
    let csid = answer.get("csid").and_then(Value::as_str);
    let (Some(master), Some(private_key), Some(csid)) = (master, private_key, csid) else {
        return MegaSignIn::Unreachable(Fault::Io);
    };
    if master.len() != KEY_BYTES {
        return MegaSignIn::Unreachable(Fault::Io);
    }
    let Some(sid) = decrypt_session_id(private_key, csid, &master) else { return MegaSignIn::Unreachable(Fault::Io) };
    MegaSignIn::SignedIn(MegaConfig {
        email: pending.email.clone(),
        session_id: Secret::from(sid),
        master_key: Secret::from(b64encode(&master)),
        folder: DEFAULT_FOLDER.to_owned(),
    })
}

#[cfg(test)]
mod tests;
