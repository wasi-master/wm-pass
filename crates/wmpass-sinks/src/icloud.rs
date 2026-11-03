//! iCloud Drive, through the web API iCloud.com uses, signed in with SRP.
//!
//! Apple has no public iCloud Drive API outside its own platforms, so this is that
//! site's protocol, as rclone's `iclouddrive` backend and pyicloud have worked it out;
//! it can change without notice, and when it does a backup fails with an IO error until
//! the app is updated.
//!
//! Signing in is Apple's SRP-6a ([`ICloudSrp`]) with the password, then a code from a
//! trusted device the first time ([`start`], [`finish`]). The trust token that hands
//! back lets a later sign-in skip the code for about a month. A run first tries the
//! cookies the last one left, which iCloud honours for a day or so, and signs in again
//! with password and trust token when they are stale. Accounts with Advanced Data
//! Protection need "Access iCloud Data on the Web" turned on, and the first sign-in then
//! waits for the approval on a trusted device ([`approve`]).

use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use num_bigint::BigUint;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use wmpass_core::Secret;
use wmpass_net::{Request, Response};

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored, segment};

/// The iCloud.com web sign-in's own widget key, which every client of this API sends.
pub const CLIENT_ID: &str = "d39ba9916b7251055b22c7f910e2ea796ee65e98b2ddecea8f5dde8d9d1a815d";

/// How long a trust token lasts, give or take, for the screen to say when a sign-in is
/// due.
pub const TRUST_LIFETIME_MS: u64 = 30 * 24 * 60 * 60 * 1000;

const BASE: &str = "https://www.icloud.com";
const SETUP: &str = "https://setup.icloud.com/setup/ws/1";
const AUTH: &str = "https://idmsa.apple.com/appleauth/auth";
const AUTH_ORIGIN: &str = "https://idmsa.apple.com";
const PCS_COOKIE: &str = "X-APPLE-WEBAUTH-PCS-Documents";
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3.1 Safari/605.1.15";
const DEFAULT_ZONE: &str = "com.apple.CloudDocs";
const ROOT_ID: &str = "FOLDER::com.apple.CloudDocs::root";
const MIME: &str = "application/octet-stream";
const MAX_REDIRECTS: usize = 5;

/// A folder in iCloud Drive.
#[derive(Clone)]
pub struct ICloudConfig {
    /// The Apple Account.
    pub apple_id: String,
    /// Its password: a trusted sign-in still needs it, with the trust token.
    pub password: Secret,
    /// From the 2FA sign-in. Empty means not signed in yet.
    pub trust_token: Secret,
    /// The cookies the last session left, as one `name=value; ...` string.
    pub cookies: Secret,
    /// Apple's per-client id; empty for the iCloud.com one.
    pub client_id: String,
    /// When `trust_token` was got, so the screen can say a sign-in is due.
    pub trusted_at_ms: u64,
    /// Folder path under iCloud Drive, `/`-separated; made where missing.
    pub folder: String,
}

impl ICloudConfig {
    /// Where backups go unless the user picks another folder.
    pub const DEFAULT_FOLDER: &str = "WM Pass backups";

    /// Signed in.
    pub fn configured(&self) -> bool {
        !self.apple_id.is_empty() && !self.password.is_empty() && !self.trust_token.is_empty()
    }
}

impl Default for ICloudConfig {
    fn default() -> Self {
        Self {
            apple_id: String::new(),
            password: Secret::default(),
            trust_token: Secret::default(),
            cookies: Secret::default(),
            client_id: String::new(),
            trusted_at_ms: 0,
            folder: Self::DEFAULT_FOLDER.to_owned(),
        }
    }
}

impl std::fmt::Debug for ICloudConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ICloudConfig")
            .field("configured", &self.configured())
            .field("trusted_at_ms", &self.trusted_at_ms)
            .finish_non_exhaustive()
    }
}

/// What a sign-in leaves to keep: the cookies, so the next run can skip it, and the trust
/// token, which Apple may hand back renewed.
#[derive(Clone, Debug)]
pub struct ICloudSessionUpdate {
    /// `name=value; ...`.
    pub cookies: Secret,
    /// The current trust token.
    pub trust_token: Secret,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| Fault::Io)?;
    Ok(bytes)
}

fn uuid() -> Result<String> {
    let mut b: [u8; 16] = random()?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A cookie `Expires` date, in either of the shapes servers send, as epoch ms.
fn cookie_date(text: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let tokens: Vec<&str> = text.split([' ', ',', '-', ':']).filter(|t| !t.is_empty()).collect();
    let month = tokens.iter().find_map(|t| {
        let lower = t.to_ascii_lowercase();
        MONTHS.iter().position(|m| lower.starts_with(m))
    })? as i64
        + 1;
    let numbers: Vec<&str> = tokens.iter().copied().filter(|t| t.bytes().all(|b| b.is_ascii_digit())).collect();
    let year_at = numbers.iter().position(|n| n.len() == 4)?;
    let year: i64 = numbers[year_at].parse().ok()?;
    let day: i64 = numbers.iter().find(|n| n.len() <= 2)?.parse().ok()?;
    let time: Vec<i64> = numbers[year_at + 1..].iter().take(3).filter_map(|n| n.parse().ok()).collect();
    let (h, m, s) = match time.as_slice() {
        [h, m, s] => (*h, *m, *s),
        _ => (0, 0, 0),
    };
    Some(((days_from_civil(year, month, day) * 86_400) + h * 3600 + m * 60 + s) * 1000)
}

// ---------------------------------------------------------------------------------------
// SRP.

/// Apple's SRP-6a: the 2048-bit group of RFC 5054 with SHA-256, and the password
/// stretched by PBKDF2 over its own SHA-256 (`s2k`), or over that hash's hex (`s2k_fo`)
/// for older accounts. x leaves the user name out, as Apple's does.
pub struct ICloudSrp {
    a: BigUint,
    big_a: BigUint,
}

impl std::fmt::Debug for ICloudSrp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ICloudSrp(***)")
    }
}

const N_HEX: &[u8] = b"AC6BDB41324A9A9BF166DE5E1389582FAF72B6651987EE07FC3192943DB56050\
A37329CBB4A099ED8193E0757767A13DD52312AB4B03310DCD7F48A9DA04FD50\
E8083969EDB767B0CF6095179A163AB3661A05FBD5FAAAE82918A9962F0B93B8\
55F97993EC975EEAA80D740ADBF4FF747359D041D5C33EA71D281E446B14773B\
CA97B43A23FB801676BD207A436C6481F1D2B9078717461A5B9D32E688F87748\
544523B524B0D57D5EA77A2775D2ECFA032CFBDBF52FB37861602790\
04E57AE6AF874E7303CE53299CCC041C7BC308D82A5698F3A8D0C38271AE35F8\
E9DBFBB694B5C803D89F7AE435DE236D525F54759B65E372FCD68EF20FA7111F\
9E4AFF73";
const N_BYTES: usize = 256;

fn group() -> (BigUint, BigUint) {
    (BigUint::parse_bytes(N_HEX, 16).unwrap_or_default(), BigUint::from(2u32))
}

fn sha(parts: &[&[u8]]) -> Vec<u8> {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    digest.finalize().to_vec()
}

fn pad(n: &BigUint) -> Vec<u8> {
    let raw = n.to_bytes_be();
    if raw.len() >= N_BYTES {
        return raw;
    }
    let mut out = vec![0u8; N_BYTES - raw.len()];
    out.extend_from_slice(&raw);
    out
}

fn pbkdf2_sha256(key: &[u8], salt: &[u8], iterations: u32, length: usize) -> Option<Vec<u8>> {
    let keyed = <Hmac<Sha256> as KeyInit>::new_from_slice(key).ok()?;
    let mut out = Vec::with_capacity(length + 32);
    let mut block = 1u32;
    while out.len() < length {
        let mut mac = keyed.clone();
        mac.update(salt);
        mac.update(&block.to_be_bytes());
        let mut u = mac.finalize().into_bytes();
        let mut acc = u;
        for _ in 1..iterations {
            let mut mac = keyed.clone();
            mac.update(&u);
            u = mac.finalize().into_bytes();
            for (a, b) in acc.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        out.extend_from_slice(&acc);
        block += 1;
    }
    out.truncate(length);
    Some(out)
}

impl ICloudSrp {
    /// A client with a fresh random secret.
    pub fn new() -> Result<Self> {
        Ok(Self::with_secret(&random::<32>()?))
    }

    /// A client with a given secret, for the tests.
    pub fn with_secret(secret: &[u8]) -> Self {
        let (n, g) = group();
        let a = BigUint::from_bytes_be(secret);
        let big_a = g.modpow(&a, &n);
        Self { a, big_a }
    }

    /// A, padded to 256 bytes.
    pub fn public_a(&self) -> Vec<u8> {
        pad(&self.big_a)
    }

    /// M1 and M2 for the server's `server_b`, or `None` when B is not a valid value.
    pub fn proofs(&self, user: &[u8], derived_key: &[u8], salt: &[u8], server_b: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
        let (n, g) = group();
        let b = BigUint::from_bytes_be(server_b);
        if b == BigUint::ZERO || b >= n {
            return None;
        }
        let x = BigUint::from_bytes_be(&sha(&[salt, &sha(&[b":", derived_key])]));
        let u = BigUint::from_bytes_be(&sha(&[&pad(&self.big_a), &pad(&b)]));
        if u == BigUint::ZERO {
            return None;
        }
        let k = BigUint::from_bytes_be(&sha(&[&n.to_bytes_be(), &pad(&g)]));
        let kgx = (&k * g.modpow(&x, &n)) % &n;
        let base = (&b + &n - kgx) % &n;
        let s = base.modpow(&(&self.a + &u * &x), &n);
        let key = sha(&[&pad(&s)]);
        let hg = sha(&[&pad(&g)]);
        let hn = sha(&[&n.to_bytes_be()]);
        let xor: Vec<u8> = hg.iter().zip(&hn).map(|(a, b)| a ^ b).collect();
        let m1 = sha(&[&xor, &sha(&[user]), salt, &pad(&self.big_a), &pad(&b), &key]);
        let m2 = sha(&[&pad(&self.big_a), &m1, &key]);
        Some((m1, m2))
    }

    /// The stretched password, or `None` for a protocol this does not know.
    pub fn derive_password(password: &str, salt: &[u8], iterations: u32, protocol: &str) -> Option<Vec<u8>> {
        let hash = Sha256::digest(password.as_bytes());
        // PBKDF2 keyed by raw bytes: the s2k hash is not text.
        let input: Vec<u8> = match protocol {
            "s2k" => hash.to_vec(),
            "s2k_fo" => hash.iter().map(|b| format!("{b:02x}")).collect::<String>().into_bytes(),
            _ => return None,
        };
        pbkdf2_sha256(&input, salt, iterations, 32)
    }
}

// ---------------------------------------------------------------------------------------
// The session.

/// Where the Drive and Docs services are for this account, from `accountLogin` or
/// `validate`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Services {
    drive: String,
    docs: String,
    needs_web_access_approval: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SignIn {
    Done,
    NeedsCode,
    Refused,
}

/// Why a service call failed: iCloud saying the session is gone (401, 421 or 423), or
/// anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CallError {
    Expired,
    Fault(Fault),
}

impl From<Fault> for CallError {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault)
    }
}

#[derive(Default)]
struct State {
    cookies: Vec<(String, String)>,
    trust_token: Secret,
    session_token: Secret,
    session_id: String,
    scnt: String,
    account_country: String,
    auth_attributes: String,
    sms_phone: Option<i64>,
}

/// The iCloud web sign-in and session: Apple's auth endpoints (`idmsa`), the setup
/// endpoint that hands out the service addresses, and the cookies that carry the session
/// between them.
pub struct ICloudSession {
    apple_id: String,
    client_id: String,
    frame: String,
    now_ms: u64,
    state: Mutex<State>,
}

impl std::fmt::Debug for ICloudSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ICloudSession(***)")
    }
}

fn json_body(method: &'static str, url: impl Into<String>, body: &Value) -> Request {
    Request::json(method, url, body)
}

fn empty_body(method: &'static str, url: impl Into<String>) -> Request {
    let mut request = Request::verb(method, url);
    request.body = Some(("application/json", Vec::new()));
    request
}

fn parse(response: &Response) -> Result<Value> {
    serde_json::from_slice(&response.body).map_err(|_| Fault::Io)
}

impl ICloudSession {
    /// A session for `apple_id`, carrying stored `cookies` and `trust_token`. `now_ms`
    /// judges cookie expiry.
    pub fn new(apple_id: &str, client_id: &str, cookies: &Secret, trust_token: &Secret, now_ms: u64) -> Result<Self> {
        let mut state = State { trust_token: trust_token.clone(), ..State::default() };
        for pair in cookies.expose().split(';') {
            if let Some((name, value)) = pair.split_once('=') {
                let name = name.trim();
                if !name.is_empty() {
                    state.cookies.push((name.to_owned(), value.trim().to_owned()));
                }
            }
        }
        Ok(Self {
            apple_id: apple_id.to_owned(),
            client_id: if client_id.is_empty() { CLIENT_ID.to_owned() } else { client_id.to_owned() },
            frame: format!("auth-{}", uuid()?),
            now_ms,
            state: Mutex::new(state),
        })
    }

    /// The cookies as one `name=value; ...` string.
    pub fn cookie_string(&self) -> Secret {
        let state = lock(&self.state);
        Secret::new(state.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "))
    }

    /// The current trust token.
    pub fn trust_token(&self) -> Secret {
        lock(&self.state).trust_token.clone()
    }

    fn drop_session(&self) {
        lock(&self.state).cookies.clear();
    }

    fn has_cookie(&self, name: &str) -> bool {
        lock(&self.state).cookies.iter().any(|(k, _)| k == name)
    }

    fn absorb(&self, response: &Response) {
        let mut state = lock(&self.state);
        for header in response.headers_named("set-cookie") {
            let mut parts = header.split(';');
            let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else { continue };
            let (name, value) = (name.trim(), value.trim().trim_matches('"'));
            if name.is_empty() {
                continue;
            }
            let mut expired = false;
            let mut max_age = None;
            for attribute in parts {
                let (key, val) = attribute.split_once('=').unwrap_or((attribute, ""));
                match key.trim().to_ascii_lowercase().as_str() {
                    "max-age" => max_age = val.trim().parse::<i64>().ok(),
                    "expires" => {
                        if let Some(at) = cookie_date(val) {
                            expired = at < self.now_ms as i64;
                        }
                    }
                    _ => {}
                }
            }
            // Max-Age wins over Expires.
            if let Some(age) = max_age {
                expired = age <= 0;
            }
            state.cookies.retain(|(k, _)| k != name);
            if !value.is_empty() && !expired {
                state.cookies.push((name.to_owned(), value.to_owned()));
            }
        }
        let header = |name: &str| response.header(name).map(str::to_owned);
        if let Some(v) = header("X-Apple-ID-Account-Country") {
            state.account_country = v;
        }
        if let Some(v) = header("X-Apple-ID-Session-Id") {
            state.session_id = v;
        }
        if let Some(v) = header("X-Apple-Session-Token") {
            state.session_token = Secret::new(v);
        }
        if let Some(v) = header("X-Apple-TwoSV-Trust-Token") {
            state.trust_token = Secret::new(v);
        }
        if let Some(v) = header("scnt") {
            state.scnt = v;
        }
        if let Some(v) = header("X-Apple-Auth-Attributes") {
            state.auth_attributes = v;
        }
    }

    /// Sends, taking in every answer's cookies and headers, and following redirects
    /// itself, since the client does not.
    async fn execute<H: AsyncHttp>(&self, client: &Client<H>, request: Request) -> Result<Response> {
        let mut request = request;
        for _ in 0..=MAX_REDIRECTS {
            let response = client.send(request.clone()).await?;
            self.absorb(&response);
            let location = response.header("location");
            let Some(location) = location.filter(|_| matches!(response.status, 301 | 302 | 303 | 307 | 308)) else {
                return Ok(response);
            };
            let next = url::Url::parse(&request.url).and_then(|base| base.join(location)).map_err(|_| Fault::Io)?;
            request.url = next.to_string();
            if response.status == 303 || (matches!(response.status, 301 | 302) && request.method == "POST") {
                request.method = "GET";
                request.body = None;
            }
            // A session's cookies may have changed on the way.
            if let Some(cookie) = request.headers.iter_mut().find(|(k, _)| *k == "Cookie") {
                cookie.1 = self.cookie_string().expose().to_owned();
            }
        }
        Err(Fault::Io)
    }

    /// A request to an iCloud service, with the session's cookies.
    fn service(&self, request: Request) -> Request {
        let request = request.header("Origin", BASE).header("Referer", format!("{BASE}/")).header("User-Agent", USER_AGENT);
        let cookies = self.cookie_string();
        if cookies.is_empty() { request } else { request.header("Cookie", cookies.expose()) }
    }

    /// Runs a service request; a 2xx comes back, a dead session is [`CallError::Expired`].
    async fn send<H: AsyncHttp>(&self, client: &Client<H>, request: Request) -> std::result::Result<Response, CallError> {
        let response = self.execute(client, request).await?;
        match response.status {
            200..=299 => Ok(response),
            401 | 421 | 423 => Err(CallError::Expired),
            403 => Err(Fault::PermissionLost.into()),
            404 => Err(Fault::TargetMissing.into()),
            507 => Err(Fault::OutOfSpace.into()),
            _ => Err(Fault::Io.into()),
        }
    }

    /// The headers every call to Apple's auth service carries, as the iCloud.com sign-in
    /// frame sends them.
    fn auth(&self, request: Request) -> Request {
        let mut request = request
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .header("Origin", AUTH_ORIGIN)
            .header("Referer", format!("{AUTH_ORIGIN}/"))
            .header("X-Apple-Widget-Key", self.client_id.clone())
            .header("X-Apple-OAuth-Client-Id", self.client_id.clone())
            .header("X-Apple-OAuth-Client-Type", "firstPartyAuth")
            .header("X-Apple-OAuth-Redirect-URI", BASE)
            .header("X-Apple-OAuth-Require-Grant-Code", "true")
            .header("X-Apple-OAuth-Response-Mode", "web_message")
            .header("X-Apple-OAuth-Response-Type", "code")
            .header("X-Apple-OAuth-State", self.frame.clone())
            .header("X-Apple-Frame-Id", self.frame.clone())
            .header("X-Requested-With", "XMLHttpRequest")
            .header("X-Apple-Mandate-Security-Upgrade", "0")
            .header("X-Apple-I-Require-UE", "true")
            .header("X-Apple-I-FD-Client-Info", format!(r#"{{"U":"{USER_AGENT}","L":"en-US","Z":"GMT+00:00","V":"1.1","F":""}}"#));
        let state = lock(&self.state);
        if !state.auth_attributes.is_empty() {
            request = request.header("X-Apple-Auth-Attributes", state.auth_attributes.clone());
        }
        if !state.scnt.is_empty() {
            request = request.header("scnt", state.scnt.clone());
        }
        if !state.session_id.is_empty() {
            request = request.header("X-Apple-ID-Session-Id", state.session_id.clone());
        }
        request
    }

    /// The password sign-in: SRP-6a with the trust token offered, so a trusted session
    /// skips the code. `NeedsCode` when Apple wants one (HTTP 409), `Refused` for a
    /// wrong password.
    async fn sign_in<H: AsyncHttp>(&self, client: &Client<H>, password: &Secret) -> Result<SignIn> {
        let account = self.apple_id.trim().to_lowercase();
        let query = [
            ("frame_id", self.frame.as_str()),
            ("language", "en_US"),
            ("skVersion", "7"),
            ("iframeId", self.frame.as_str()),
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", BASE),
            ("response_type", "code"),
            ("response_mode", "web_message"),
            ("state", self.frame.as_str()),
            ("authVersion", "latest"),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={}", segment(v)))
        .collect::<Vec<_>>()
        .join("&");
        let start = Request::get(format!("{AUTH}/authorize/signin?{query}")).header("Accept", "*/*").header("User-Agent", USER_AGENT);
        self.execute(client, start).await?;

        let federate = json!({ "accountName": account, "rememberMe": true });
        self.execute(client, self.auth(json_body("POST", format!("{AUTH}/federate?isRememberMeEnabled=true"), &federate))).await?;

        let srp = ICloudSrp::new()?;
        let engine = base64::engine::general_purpose::STANDARD;
        let init = json!({
            "a": engine.encode(srp.public_a()),
            "accountName": account,
            "protocols": ["s2k", "s2k_fo"],
        });
        let answer = self.execute(client, self.auth(json_body("POST", format!("{AUTH}/signin/init"), &init))).await?;
        if !(200..300).contains(&answer.status) {
            return match answer.status {
                401 | 403 => Ok(SignIn::Refused),
                _ => Err(Fault::Io),
            };
        }
        let challenge = parse(&answer)?;
        let field = |key: &str| challenge.get(key).and_then(Value::as_str).and_then(|v| engine.decode(v).ok()).ok_or(Fault::Io);
        let salt = field("salt")?;
        let b = field("b")?;
        let iterations = challenge.get("iteration").and_then(Value::as_u64).ok_or(Fault::Io)? as u32;
        let protocol = challenge.get("protocol").and_then(Value::as_str).unwrap_or_default();
        let derived = ICloudSrp::derive_password(password.expose(), &salt, iterations, protocol).ok_or(Fault::Io)?;
        let (m1, m2) = srp.proofs(account.as_bytes(), &derived, &salt, &b).ok_or(Fault::Io)?;

        let trust = self.trust_token();
        let trust_tokens: Vec<&str> = if trust.is_empty() { Vec::new() } else { vec![trust.expose()] };
        let complete = json!({
            "accountName": account,
            "m1": engine.encode(m1),
            "m2": engine.encode(m2),
            "c": challenge.get("c").cloned().unwrap_or(json!("")),
            "rememberMe": true,
            "trustTokens": trust_tokens,
        });
        let done = self
            .execute(client, self.auth(json_body("POST", format!("{AUTH}/signin/complete?isRememberMeEnabled=true"), &complete)))
            .await?;
        Ok(match done.status {
            200..=299 => SignIn::Done,
            409 => SignIn::NeedsCode,
            412 => {
                // An account Apple wants something "repaired" on (terms, a security
                // upgrade); the web sign-in acknowledges it.
                let repaired = self.execute(client, self.auth(json_body("POST", format!("{AUTH}/repair/complete"), &json!({})))).await?;
                if (200..300).contains(&repaired.status) { SignIn::Done } else { SignIn::Refused }
            }
            401 | 403 => SignIn::Refused,
            _ => return Err(Fault::Io),
        })
    }

    /// Asks for the code on the trusted devices, or by text when there are none. True
    /// when one was sent by text.
    async fn request_code<H: AsyncHttp>(&self, client: &Client<H>) -> Result<bool> {
        let answer = self.execute(client, self.auth(Request::get(AUTH))).await?;
        let state: Option<Value> = serde_json::from_slice(&answer.body).ok();
        let inner = state.as_ref().map(|s| s.get("phoneNumberVerification").unwrap_or(s));
        let no_devices = inner.and_then(|i| i.get("noTrustedDevices")).and_then(Value::as_bool) == Some(true);
        let phone = inner.and_then(|i| {
            i.pointer("/trustedPhoneNumber/id")
                .and_then(Value::as_i64)
                .or_else(|| i.pointer("/trustedPhoneNumbers/0/id").and_then(Value::as_i64))
        });
        if let (true, Some(phone)) = (no_devices, phone) {
            let body = json!({ "phoneNumber": { "id": phone }, "mode": "sms" });
            self.execute(client, self.auth(json_body("PUT", format!("{AUTH}/verify/phone"), &body))).await?;
            lock(&self.state).sms_phone = Some(phone);
            return Ok(true);
        }
        self.execute(client, self.auth(empty_body("PUT", format!("{AUTH}/verify/trusteddevice/securitycode")))).await?;
        Ok(false)
    }

    /// Checks the code, then trusts this session, which is what hands back the trust
    /// token.
    async fn submit_code<H: AsyncHttp>(&self, client: &Client<H>, code: &str) -> Result<bool> {
        let digits: String = code.chars().filter(char::is_ascii_digit).collect();
        let phone = lock(&self.state).sms_phone;
        let (path, body) = match phone {
            Some(phone) => (
                "/verify/phone/securitycode",
                json!({ "securityCode": { "code": digits }, "phoneNumber": { "id": phone }, "mode": "sms" }),
            ),
            None => ("/verify/trusteddevice/securitycode", json!({ "securityCode": { "code": digits } })),
        };
        let answer = self.execute(client, self.auth(json_body("POST", format!("{AUTH}{path}"), &body))).await?;
        // A 409 that carries a session token is Apple accepting the code.
        let accepted = (200..300).contains(&answer.status) || (answer.status == 409 && !lock(&self.state).session_token.is_empty());
        if !accepted {
            return Ok(false);
        }
        let trusted = self.execute(client, self.auth(Request::get(format!("{AUTH}/2sv/trust")))).await?;
        Ok((200..300).contains(&trusted.status))
    }

    fn services(&self, response: &Response) -> Option<Services> {
        let root: Value = serde_json::from_slice(&response.body).ok()?;
        let web = root.get("webservices")?;
        let drive = web.get("drivews")?;
        let drive_url = drive.get("url")?.as_str()?.to_owned();
        let docs_url = web.pointer("/docws/url")?.as_str()?.to_owned();
        let pcs = drive.get("pcsRequired").and_then(Value::as_bool) == Some(true) && !self.has_cookie(PCS_COOKIE);
        Some(Services { drive: drive_url, docs: docs_url, needs_web_access_approval: pcs })
    }

    /// Trades the auth session for iCloud cookies and the service addresses.
    async fn account_login<H: AsyncHttp>(&self, client: &Client<H>) -> Option<Services> {
        let body = {
            let state = lock(&self.state);
            json!({
                "accountCountryCode": state.account_country,
                "dsWebAuthToken": state.session_token.expose(),
                "extended_login": true,
                "trustToken": state.trust_token.expose(),
            })
        };
        let response = self.send(client, self.service(json_body("POST", format!("{SETUP}/accountLogin"), &body))).await.ok()?;
        self.services(&response)
    }

    /// The stored cookies' session, if iCloud still honours it.
    async fn validate<H: AsyncHttp>(&self, client: &Client<H>) -> Option<Services> {
        if lock(&self.state).cookies.is_empty() {
            return None;
        }
        let response = self.send(client, self.service(empty_body("POST", format!("{SETUP}/validate")))).await.ok()?;
        self.services(&response)
    }

    /// With Advanced Data Protection, asks once for the web access cookies, which need an
    /// approval on a trusted device.
    async fn request_web_access<H: AsyncHttp>(&self, client: &Client<H>) -> bool {
        let body = json!({ "appName": "iclouddrive", "derivedFromUserAction": true });
        let Ok(response) = self.send(client, self.service(json_body("POST", format!("{SETUP}/requestPCS"), &body))).await else {
            return false;
        };
        let status = serde_json::from_slice::<Value>(&response.body).ok().and_then(|v| v.get("status").and_then(Value::as_str).map(str::to_owned));
        status.as_deref() == Some("success") && self.has_cookie(PCS_COOKIE)
    }
}

// ---------------------------------------------------------------------------------------
// Signing in from the settings screen.

/// Where a sign-in got to.
#[derive(Debug)]
pub enum ICloudOutcome {
    /// Signed in: what the location keeps.
    SignedIn {
        /// For the next sign-ins.
        trust_token: Secret,
        /// For the next runs.
        cookies: Secret,
    },
    /// A code is on its way: to the trusted devices, or by text when `by_sms`.
    NeedsCode {
        /// Goes back with the code.
        pending: ICloudPending,
        /// Sent by text.
        by_sms: bool,
    },
    /// With Advanced Data Protection: approve the sign-in on a trusted device, then
    /// [`approve`].
    NeedsApproval(ICloudPending),
    /// Refused or unreachable.
    Failed(Fault),
}

/// A sign-in between its steps. Opaque.
#[derive(Debug)]
pub struct ICloudPending(ICloudSession);

/// The password half; Apple sends a code when this device is not trusted yet.
pub async fn start<H: AsyncHttp>(client: &Client<H>, apple_id: &str, password: &Secret, client_id: &str, now_ms: u64) -> ICloudOutcome {
    let session = match ICloudSession::new(apple_id, client_id, &Secret::default(), &Secret::default(), now_ms) {
        Ok(session) => session,
        Err(fault) => return ICloudOutcome::Failed(fault),
    };
    match session.sign_in(client, password).await {
        Ok(SignIn::Refused) => ICloudOutcome::Failed(Fault::PermissionLost),
        Ok(SignIn::NeedsCode) => match session.request_code(client).await {
            Ok(by_sms) => ICloudOutcome::NeedsCode { pending: ICloudPending(session), by_sms },
            Err(fault) => ICloudOutcome::Failed(fault),
        },
        Ok(SignIn::Done) => finish_login(client, session).await,
        Err(fault) => ICloudOutcome::Failed(fault),
    }
}

/// The code from the trusted device or the text.
pub async fn finish<H: AsyncHttp>(client: &Client<H>, pending: ICloudPending, code: &str) -> ICloudOutcome {
    let session = pending.0;
    match session.submit_code(client, code).await {
        Ok(true) => finish_login(client, session).await,
        Ok(false) => ICloudOutcome::Failed(Fault::PermissionLost),
        Err(fault) => ICloudOutcome::Failed(fault),
    }
}

/// Asks again whether the web access was approved on a trusted device. Call it every
/// ten seconds or so while the screen waits; WM Keyboard waited two minutes in one call,
/// which an async caller cannot do.
pub async fn approve<H: AsyncHttp>(client: &Client<H>, pending: ICloudPending) -> ICloudOutcome {
    let session = pending.0;
    if !session.request_web_access(client).await {
        return ICloudOutcome::NeedsApproval(ICloudPending(session));
    }
    signed_in(session)
}

async fn finish_login<H: AsyncHttp>(client: &Client<H>, session: ICloudSession) -> ICloudOutcome {
    let Some(services) = session.account_login(client).await else { return ICloudOutcome::Failed(Fault::PermissionLost) };
    if services.needs_web_access_approval && !session.request_web_access(client).await {
        return ICloudOutcome::NeedsApproval(ICloudPending(session));
    }
    signed_in(session)
}

fn signed_in(session: ICloudSession) -> ICloudOutcome {
    let trust_token = session.trust_token();
    if trust_token.is_empty() {
        return ICloudOutcome::Failed(Fault::PermissionLost);
    }
    ICloudOutcome::SignedIn { trust_token, cookies: session.cookie_string() }
}

// ---------------------------------------------------------------------------------------
// The sink.

struct Item {
    id: String,
    etag: String,
    name: String,
    folder: bool,
    size: u64,
}

/// An iCloud Drive folder.
pub struct ICloud<H> {
    client: Client<H>,
    config: ICloudConfig,
    now_ms: u64,
    session: ICloudSession,
    on_session: Box<dyn Fn(&ICloudSessionUpdate) + Send + Sync>,
    services: Mutex<Option<Services>>,
    folder_id: Mutex<Option<String>>,
}

impl<H> std::fmt::Debug for ICloud<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ICloud").field("config", &self.config).finish_non_exhaustive()
    }
}

fn zone_of(id: &str) -> String {
    id.split("::").nth(1).filter(|z| !z.is_empty()).unwrap_or(DEFAULT_ZONE).to_owned()
}

fn after_last(id: &str) -> &str {
    id.rsplit("::").next().unwrap_or(id)
}

impl<H: AsyncHttp> ICloud<H> {
    /// A folder for a signed-in `config`. `on_session` stores the cookies a sign-in ends
    /// with, and the trust token, which Apple may hand back renewed. `now_ms` judges
    /// cookie expiry and dates new files.
    pub fn new(
        client: Client<H>,
        config: ICloudConfig,
        now_ms: u64,
        on_session: Box<dyn Fn(&ICloudSessionUpdate) + Send + Sync>,
    ) -> Result<Self> {
        let session = ICloudSession::new(&config.apple_id, &config.client_id, &config.cookies, &config.trust_token, now_ms)?;
        Ok(Self { client, config, now_ms, session, on_session, services: Mutex::new(None), folder_id: Mutex::new(None) })
    }

    /// Signed in, by the stored cookies if they still work and by password and trust
    /// token if not.
    async fn services(&self) -> Result<Services> {
        if let Some(services) = lock(&self.services).clone() {
            return Ok(services);
        }
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        let found = match self.session.validate(&self.client).await {
            Some(found) => found,
            None => {
                match self.session.sign_in(&self.client, &self.config.password).await? {
                    SignIn::Done => {}
                    // The trust token has run out: only a person can type the code.
                    SignIn::NeedsCode | SignIn::Refused => return Err(Fault::PermissionLost),
                }
                self.session.account_login(&self.client).await.ok_or(Fault::PermissionLost)?
            }
        };
        if found.needs_web_access_approval && !self.session.request_web_access(&self.client).await {
            return Err(Fault::PermissionLost);
        }
        (self.on_session)(&ICloudSessionUpdate { cookies: self.session.cookie_string(), trust_token: self.session.trust_token() });
        *lock(&self.services) = Some(found.clone());
        Ok(found)
    }

    /// One call with the session's cookies, signed in again once if iCloud says the
    /// session is gone.
    async fn drive(&self, make: impl Fn(&Services) -> Request) -> Result<Response> {
        let first = self.services().await?;
        match self.session.send(&self.client, self.session.service(make(&first))).await {
            Ok(response) => return Ok(response),
            Err(CallError::Fault(fault)) => return Err(fault),
            Err(CallError::Expired) => {}
        }
        *lock(&self.services) = None;
        self.session.drop_session();
        let again = self.services().await?;
        match self.session.send(&self.client, self.session.service(make(&again))).await {
            Ok(response) => Ok(response),
            Err(CallError::Expired) => Err(Fault::PermissionLost),
            Err(CallError::Fault(fault)) => Err(fault),
        }
    }

    async fn children(&self, folder: &str) -> Result<Vec<Item>> {
        let body = json!([{ "drivewsid": folder, "partialData": false, "includeHierarchy": false }]);
        let answer = parse(&self.drive(|s| json_body("POST", format!("{}/retrieveItemDetailsInFolders", s.drive), &body)).await?)?;
        let items = answer.pointer("/0/items").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|item| {
                let id = item.get("drivewsid")?.as_str()?.to_owned();
                let field = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
                let (name, extension, kind) = (field("name"), field("extension"), field("type"));
                let full = if extension.is_empty() { name.to_owned() } else { format!("{name}.{extension}") };
                Some(Item {
                    id,
                    etag: field("etag").to_owned(),
                    // iCloud keeps the extension apart and the name in NFD.
                    name: full.nfc().collect(),
                    folder: matches!(kind, "FOLDER" | "APP_CONTAINER" | "APP_LIBRARY"),
                    size: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                })
            })
            .collect())
    }

    async fn folder(&self) -> Result<String> {
        if let Some(id) = lock(&self.folder_id).clone() {
            return Ok(id);
        }
        let mut at = ROOT_ID.to_owned();
        for part in self.config.folder.split('/').map(str::trim).filter(|p| !p.is_empty()) {
            let existing = self.children(&at).await?.into_iter().find(|i| i.folder && i.name == part);
            at = match existing {
                Some(item) => item.id,
                None => self.make_folder(&at, part).await?,
            };
        }
        *lock(&self.folder_id) = Some(at.clone());
        Ok(at)
    }

    async fn make_folder(&self, parent: &str, name: &str) -> Result<String> {
        let body = json!({
            "destinationDrivewsId": parent,
            "folders": [{ "clientId": format!("FOLDER::UNKNOWN_ZONE::TempId-{}", uuid()?), "name": name }],
        });
        let answer = parse(&self.drive(|s| json_body("POST", format!("{}/createFolders", s.drive), &body)).await?)?;
        let made = answer.pointer("/folders/0");
        if made.and_then(|m| m.get("status")).and_then(Value::as_str).is_some_and(|s| s != "OK") {
            return Err(Fault::Io);
        }
        made.and_then(|m| m.get("drivewsid")).and_then(Value::as_str).map(str::to_owned).ok_or(Fault::Io)
    }

    async fn trash(&self, item: &Item) -> Result<()> {
        let body = json!({ "items": [{ "drivewsid": item.id, "etag": item.etag, "clientId": item.id }] });
        self.drive(|s| json_body("POST", format!("{}/moveItemsToTrash", s.drive), &body)).await.map(|_| ())
    }
}

impl<H: AsyncHttp> Sink for ICloud<H> {
    async fn readiness(&self) -> Result<()> {
        self.folder().await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let parent = self.folder().await?;
        let zone = zone_of(&parent);
        // iCloud would add a second file as "name 2" rather than replace one, so the
        // older copy goes to Recently Deleted first, as rclone does it. Backups never
        // share a name; sync files do.
        for item in self.children(&parent).await?.into_iter().filter(|i| !i.folder && i.name == name) {
            self.trash(&item).await?;
        }

        let request = json!({ "filename": name, "type": "FILE", "size": bytes.len().to_string(), "content_type": MIME });
        let upload = parse(&self.drive(|s| json_body("POST", format!("{}/ws/{zone}/upload/web", s.docs), &request)).await?)?;
        let upload = upload.get(0).ok_or(Fault::Io)?;
        let target = upload.get("url").and_then(Value::as_str).ok_or(Fault::Io)?.to_owned();
        let document = upload.get("document_id").and_then(Value::as_str).ok_or(Fault::Io)?.to_owned();

        let sent = parse(
            &self
                .drive(|_| {
                    let mut request = Request::verb("POST", target.clone());
                    request.body = Some((MIME, bytes.to_vec()));
                    request.timeout_ms = 300_000;
                    request
                })
                .await?,
        )?;
        let single = sent.get("singleFile").ok_or(Fault::Io)?;
        let mut data = serde_json::Map::new();
        for (from, to) in [
            ("receipt", "receipt"),
            ("referenceChecksum", "reference_signature"),
            ("fileChecksum", "signature"),
            ("size", "size"),
            ("wrappingKey", "wrapping_key"),
        ] {
            if let Some(value) = single.get(from) {
                data.insert(to.to_owned(), value.clone());
            }
        }
        let now = self.now_ms;
        let update = json!({
            "allow_conflict": true,
            "btime": now,
            "command": "add_file",
            "create_short_guid": true,
            "data": data,
            "document_id": document,
            "file_flags": { "is_executable": false, "is_hidden": false, "is_writable": true },
            "mtime": now,
            "path": { "path": name, "starting_document_id": after_last(&parent) },
        });
        self.drive(|s| json_body("POST", format!("{}/ws/{zone}/update/documents", s.docs), &update)).await?;
        Ok(Stored { id: format!("FILE::{zone}::{document}"), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let folder = self.folder().await?;
        Ok(self
            .children(&folder)
            .await?
            .into_iter()
            .filter(|i| !i.folder)
            .map(|i| Stored { id: i.id, name: i.name, size: i.size })
            .collect())
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let zone = zone_of(&file.id);
        let document = segment(after_last(&file.id));
        let token = parse(&self.drive(|s| Request::get(format!("{}/ws/{zone}/download/by_id?document_id={document}", s.docs))).await?)?;
        let url = token
            .pointer("/data_token/url")
            .or_else(|| token.pointer("/package_token/url"))
            .and_then(Value::as_str)
            .ok_or(Fault::TargetMissing)?
            .to_owned();
        let response = self
            .drive(|_| {
                let mut request = Request::get(url.clone());
                request.timeout_ms = 300_000;
                request
            })
            .await?;
        Ok(response.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let folder = self.folder().await?;
        match self.children(&folder).await?.into_iter().find(|i| i.id == file.id) {
            Some(item) => self.trash(&item).await,
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
