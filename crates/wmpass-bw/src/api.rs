//! The requests, each one a [`Call`] so any client can run it.

use serde_json::{Value, json};
use wmpass_core::otp::percent_encode;
use wmpass_net::{Call, NetError, Request};

use crate::crypto::Kdf;
use crate::{BwError, Result};

/// Where an account's services are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    /// Vault API.
    pub api: String,
    /// Login service.
    pub identity: String,
}

impl Endpoints {
    /// bitwarden.com.
    pub fn official() -> Self {
        Self { api: "https://api.bitwarden.com".into(), identity: "https://identity.bitwarden.com".into() }
    }

    /// bitwarden.eu.
    pub fn official_eu() -> Self {
        Self { api: "https://api.bitwarden.eu".into(), identity: "https://identity.bitwarden.eu".into() }
    }

    /// A self-hosted server.
    pub fn self_hosted(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self { api: format!("{base}/api"), identity: format!("{base}/identity") }
    }
}

/// Reads a property whatever its case, since servers differ: Bitwarden moved to camelCase,
/// older servers and some token fields still use PascalCase.
pub fn prop<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    let object = value.as_object()?;
    object.get(name).or_else(|| object.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value))
}

/// A property as text, treating null as absent.
pub fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    prop(value, name).and_then(Value::as_str)
}

fn number(value: &Value, name: &str) -> Option<u64> {
    prop(value, name).and_then(Value::as_u64)
}

/// Asks for an account's stretching settings.
pub fn prelogin(endpoints: &Endpoints, email: &str) -> Call<Kdf> {
    let body = json!({"email": email.trim().to_lowercase()});
    Call::new(Request::json("POST", format!("{}/accounts/prelogin", endpoints.identity), &body), |response| {
        let root = response.ok()?.json()?;
        let kdf = Kdf::from_api(
            number(&root, "kdf").unwrap_or(0),
            number(&root, "kdfIterations").ok_or(NetError::BadAnswer)?,
            number(&root, "kdfMemory"),
            number(&root, "kdfParallelism"),
        )
        .map_err(|_| NetError::BadAnswer)?;
        Ok(kdf)
    })
}

/// What a new account sends.
#[derive(Clone, Debug)]
pub struct Registration {
    /// Email.
    pub email: String,
    /// Display name.
    pub name: String,
    /// Stretching settings.
    pub kdf: Kdf,
    /// See [`crate::crypto::MasterKey::login_hash`].
    pub login_hash: String,
    /// Account key wrapped under the stretched master key.
    pub key: String,
    /// Public key.
    pub public_key: String,
    /// Private key wrapped under the account key.
    pub private_key: String,
}

/// Creates an account. bitwarden.com also requires a captcha and an emailed link, so this
/// is for self-hosted servers.
pub fn register(endpoints: &Endpoints, form: &Registration) -> Call<()> {
    let (kind, iterations, memory, parallelism) = match form.kdf {
        Kdf::Pbkdf2 { iterations } => (0, iterations, None, None),
        Kdf::Argon2id { iterations, memory_mib, parallelism } => (1, iterations, Some(memory_mib), Some(parallelism)),
    };
    let body = json!({
        "email": form.email.trim().to_lowercase(),
        "name": form.name,
        "masterPasswordHash": form.login_hash,
        "key": form.key,
        "kdf": kind,
        "kdfIterations": iterations,
        "kdfMemory": memory,
        "kdfParallelism": parallelism,
        "keys": {"publicKey": form.public_key, "encryptedPrivateKey": form.private_key},
    });
    Call::new(Request::json("POST", format!("{}/accounts/register", endpoints.identity), &body), |response| {
        response.ok()?;
        Ok(())
    })
}

/// What identifies this install to the server.
#[derive(Clone, Debug)]
pub struct Device {
    /// Random and stable per install.
    pub identifier: String,
    /// Shown in the account's device list.
    pub name: String,
    /// Bitwarden's device type number. 14 is an unknown browser, 8 is the Linux CLI.
    pub kind: u32,
}

/// What a login gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tokens {
    /// For requests.
    pub access: wmpass_core::Secret,
    /// For getting more.
    pub refresh: wmpass_core::Secret,
    /// Seconds the access token lasts.
    pub expires_in: u64,
    /// Account key, wrapped.
    pub key: Option<String>,
    /// Private key, wrapped.
    pub private_key: Option<String>,
}

fn form(fields: &[(&str, &str)]) -> Vec<u8> {
    fields.iter().map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v))).collect::<Vec<_>>().join("&").into_bytes()
}

fn read_tokens(root: &Value) -> std::result::Result<Tokens, NetError> {
    Ok(Tokens {
        access: text(root, "access_token").ok_or(NetError::BadAnswer)?.into(),
        refresh: text(root, "refresh_token").ok_or(NetError::BadAnswer)?.into(),
        expires_in: number(root, "expires_in").unwrap_or(3600),
        key: text(root, "Key").map(str::to_owned),
        private_key: text(root, "PrivateKey").map(str::to_owned),
    })
}

/// A second factor to send along.
#[derive(Clone, Debug)]
pub struct SecondFactor {
    /// Provider number.
    pub provider: u32,
    /// The code.
    pub code: String,
}

/// Logs in, or reports what is needed instead.
pub fn login(
    endpoints: &Endpoints,
    email: &str,
    login_hash: &str,
    device: &Device,
    factor: Option<&SecondFactor>,
) -> Call<std::result::Result<Tokens, BwError>> {
    let email = email.trim().to_lowercase();
    let kind = device.kind.to_string();
    let mut fields = vec![
        ("grant_type", "password"),
        ("username", email.as_str()),
        ("password", login_hash),
        ("scope", "api offline_access"),
        ("client_id", "cli"),
        ("deviceType", kind.as_str()),
        ("deviceIdentifier", device.identifier.as_str()),
        ("deviceName", device.name.as_str()),
    ];
    let provider = factor.map(|f| f.provider.to_string());
    if let (Some(factor), Some(provider)) = (factor, provider.as_deref()) {
        fields.push(("twoFactorToken", factor.code.as_str()));
        fields.push(("twoFactorProvider", provider));
        fields.push(("twoFactorRemember", "0"));
    }
    let mut request = Request::verb("POST", format!("{}/connect/token", endpoints.identity));
    request.body = Some(("application/x-www-form-urlencoded; charset=utf-8", form(&fields)));
    Call::new(request, |response| {
        if response.status == 400 || response.status == 401 {
            let root = response.json().unwrap_or(Value::Null);
            if let Some(providers) = prop(&root, "TwoFactorProviders").and_then(Value::as_array) {
                let numbers = providers
                    .iter()
                    .filter_map(|p| p.as_u64().or_else(|| p.as_str().and_then(|s| s.parse().ok())))
                    .filter_map(|p| u32::try_from(p).ok())
                    .collect();
                return Ok(Err(BwError::TwoFactor(numbers)));
            }
            return Ok(Err(BwError::BadLogin));
        }
        Ok(Ok(read_tokens(&response.ok()?.json()?)?))
    })
}

/// Gets a new access token.
pub fn refresh(endpoints: &Endpoints, token: &wmpass_core::Secret) -> Call<Tokens> {
    let mut request = Request::verb("POST", format!("{}/connect/token", endpoints.identity));
    request.body = Some((
        "application/x-www-form-urlencoded; charset=utf-8",
        form(&[("grant_type", "refresh_token"), ("client_id", "cli"), ("refresh_token", token.expose())]),
    ));
    Call::new(request, |response| read_tokens(&response.ok()?.json()?))
}

fn authed(mut request: Request, access: &wmpass_core::Secret) -> Request {
    request.headers.push(("Authorization", format!("Bearer {}", access.expose())));
    request.timeout_ms = 120_000;
    request
}

/// Fetches everything.
pub fn sync(endpoints: &Endpoints, access: &wmpass_core::Secret) -> Call<Value> {
    let request = authed(Request::get(format!("{}/sync?excludeDomains=true", endpoints.api)), access);
    Call::new(request, |response| response.ok()?.json())
}

/// Sets the account's key pair, for accounts created without one.
pub fn set_keys(endpoints: &Endpoints, access: &wmpass_core::Secret, public_key: &str, private_key: &str) -> Call<()> {
    let body = json!({"publicKey": public_key, "encryptedPrivateKey": private_key});
    let request = authed(Request::json("POST", format!("{}/accounts/keys", endpoints.api), &body), access);
    Call::new(request, |response| {
        response.ok()?;
        Ok(())
    })
}

/// What saving a cipher came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Saved {
    /// Stored. Holds the server's copy.
    Stored(Value),
    /// Someone changed it first. Fetch, merge, try again.
    Conflict,
}

fn saving(request: Request) -> Call<Saved> {
    Call::new(request, |response| {
        if response.status == 400 {
            // Bitwarden answers this way when lastKnownRevisionDate is stale.
            let text = String::from_utf8_lossy(&response.body).to_lowercase();
            if text.contains("out of date") {
                return Ok(Saved::Conflict);
            }
        }
        Ok(Saved::Stored(response.ok()?.json()?))
    })
}

/// Creates a cipher.
pub fn create_cipher(endpoints: &Endpoints, access: &wmpass_core::Secret, cipher: &Value) -> Call<Saved> {
    saving(authed(Request::json("POST", format!("{}/ciphers", endpoints.api), cipher), access))
}

/// Replaces a cipher.
pub fn update_cipher(endpoints: &Endpoints, access: &wmpass_core::Secret, id: &str, cipher: &Value) -> Call<Saved> {
    let url = format!("{}/ciphers/{}", endpoints.api, percent_encode(id));
    saving(authed(Request::json("PUT", url, cipher), access))
}

/// Moves a cipher to the trash.
pub fn trash_cipher(endpoints: &Endpoints, access: &wmpass_core::Secret, id: &str) -> Call<()> {
    let url = format!("{}/ciphers/{}/delete", endpoints.api, percent_encode(id));
    Call::new(authed(Request::verb("PUT", url), access), |response| {
        if response.status != 404 {
            response.ok()?;
        }
        Ok(())
    })
}

/// Creates a folder and returns its id. The name is already encrypted.
pub fn create_folder(endpoints: &Endpoints, access: &wmpass_core::Secret, encrypted_name: &str) -> Call<String> {
    let body = json!({"name": encrypted_name});
    let request = authed(Request::json("POST", format!("{}/folders", endpoints.api), &body), access);
    Call::new(request, |response| {
        let root = response.ok()?.json()?;
        text(&root, "id").map(str::to_owned).ok_or(NetError::BadAnswer)
    })
}

/// Treats a call's outcome as this crate's error.
pub fn run<T>(call: Call<T>, http: &dyn wmpass_net::Http, policy: wmpass_net::NetPolicy) -> Result<T> {
    Ok(call.run(http, policy)?)
}
