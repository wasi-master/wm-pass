//! The requests, each one a [`Call`] so any client can run it.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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
    let mut request = Request::json("POST", format!("{}/accounts/prelogin", endpoints.identity), &body);
    client_headers(&mut request, 0);
    Call::new(request, |response| {
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

/// The client headers Bitwarden's own apps send with each call. Its cloud service expects them,
/// and `Auth-Email` on a password login, so a login looks like one of theirs.
fn client_headers(request: &mut Request, device: u32) {
    request.headers.push(("Bitwarden-Client-Name", "cli".to_owned()));
    request.headers.push(("Bitwarden-Client-Version", CLIENT_VERSION.to_owned()));
    request.headers.push(("Device-Type", device.to_string()));
}

/// The client version WM Pass reports. Bitwarden turns away clients too old for what an account
/// uses, so this follows a recent release of its command line client.
const CLIENT_VERSION: &str = "2025.10.0";

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

/// What a login sends besides the password, once the server has asked for it.
#[derive(Clone, Debug)]
pub enum SecondFactor {
    /// A two-step login code.
    Code {
        /// Provider number.
        provider: u32,
        /// The code.
        code: String,
    },
    /// The code Bitwarden emails when a device it has not seen before signs in to an account
    /// without two-step login.
    NewDevice(String),
}

/// Why the login service said no. Bitwarden refuses a device it does not know yet with the
/// same status as a wrong password, and only a wrong password may count as a guess.
fn refusal(root: &Value) -> BwError {
    if let Some(providers) = prop(root, "TwoFactorProviders").and_then(Value::as_array) {
        let numbers = providers
            .iter()
            .filter_map(|p| p.as_u64().or_else(|| p.as_str().and_then(|s| s.parse().ok())))
            .filter_map(|p| u32::try_from(p).ok())
            .collect();
        return BwError::TwoFactor(numbers);
    }
    // The message is what Bitwarden's own clients act on. A failed two-step code carries the
    // same description as a failed password, so the message is read first.
    let message = prop(root, "ErrorModel").and_then(|model| text(model, "Message")).unwrap_or_default();
    let description = text(root, "error_description").unwrap_or_default();
    let says = |start: &str| {
        message.to_lowercase().starts_with(start) || description.to_lowercase().starts_with(start)
    };
    if says("two-step token is invalid") || says("invalid new device otp") {
        BwError::BadCode
    } else if says("new device verification required") {
        BwError::NewDevice
    } else if says("username or password is incorrect") || description == "invalid_username_or_password" {
        BwError::BadLogin
    } else if !message.is_empty() {
        BwError::Refused(message.to_owned())
    } else if !description.is_empty() {
        BwError::Refused(description.to_owned())
    } else {
        BwError::BadLogin
    }
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
    let provider = match factor {
        Some(SecondFactor::Code { provider, .. }) => provider.to_string(),
        _ => String::new(),
    };
    match factor {
        Some(SecondFactor::Code { code, .. }) => {
            fields.push(("twoFactorToken", code.as_str()));
            fields.push(("twoFactorProvider", provider.as_str()));
            fields.push(("twoFactorRemember", "0"));
        }
        Some(SecondFactor::NewDevice(code)) => fields.push(("newDeviceOtp", code.as_str())),
        None => {}
    }
    let mut request = Request::verb("POST", format!("{}/connect/token", endpoints.identity));
    request.body = Some(("application/x-www-form-urlencoded; charset=utf-8", form(&fields)));
    client_headers(&mut request, device.kind);
    request.headers.push(("Auth-Email", URL_SAFE_NO_PAD.encode(email.as_bytes())));
    Call::new(request, |response| {
        if response.status == 400 || response.status == 401 {
            return Ok(Err(refusal(&response.json().unwrap_or(Value::Null))));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_refused_password_reads_as_one() {
        let wrong = json!({
            "error": "invalid_grant",
            "error_description": "invalid_username_or_password",
            "ErrorModel": {"Message": "Username or password is incorrect. Try again.", "Object": "error"},
        });
        assert_eq!(refusal(&wrong), BwError::BadLogin);
        // Vaultwarden leaves the description empty and the full stop off.
        let vaultwarden = json!({"error": "", "error_description": "", "ErrorModel": {"Message": "Username or password is incorrect. Try again"}});
        assert_eq!(refusal(&vaultwarden), BwError::BadLogin);

        let new_device = json!({
            "error": "device_error",
            "error_description": "New device verification required",
            "ErrorModel": {"Message": "new device verification required", "Object": "error"},
        });
        assert_eq!(refusal(&new_device), BwError::NewDevice);
        let wrong_device_code = json!({"error": "device_error", "error_description": "Invalid New Device OTP", "ErrorModel": {"Message": "invalid new device otp"}});
        assert_eq!(refusal(&wrong_device_code), BwError::BadCode);
        let wrong_two_step = json!({
            "error": "invalid_grant",
            "error_description": "invalid_username_or_password",
            "ErrorModel": {"Message": "Two-step token is invalid. Try again."},
        });
        assert_eq!(refusal(&wrong_two_step), BwError::BadCode);

        let two_factor = json!({"error": "invalid_grant", "error_description": "Two factor required.", "TwoFactorProviders": ["0", 1]});
        assert_eq!(refusal(&two_factor), BwError::TwoFactor(vec![0, 1]));
        let other = json!({"error": "invalid_grant", "ErrorModel": {"Message": "Something else entirely."}});
        assert_eq!(refusal(&other), BwError::Refused("Something else entirely.".into()));
    }

    #[test]
    fn the_emailed_device_code_goes_with_the_password() {
        let device = Device { identifier: "d".into(), name: "WM Pass".into(), kind: 0 };
        let code = SecondFactor::NewDevice("123456".into());
        let call = login(&Endpoints::official(), "Me@Example.com", "hash", &device, Some(&code));
        let auth = call.request.headers.iter().find(|(name, _)| *name == "Auth-Email").map(|(_, value)| value.clone());
        assert_eq!(auth.as_deref(), Some("bWVAZXhhbXBsZS5jb20"));
        let body = String::from_utf8(call.request.body.unwrap().1).unwrap();
        assert!(body.contains("newDeviceOtp=123456"), "{body}");
        assert!(!body.contains("twoFactor"), "{body}");

        let code = SecondFactor::Code { provider: 0, code: "654321".into() };
        let call = login(&Endpoints::official(), "me@example.com", "hash", &device, Some(&code));
        let body = String::from_utf8(call.request.body.unwrap().1).unwrap();
        assert!(body.contains("twoFactorToken=654321&twoFactorProvider=0"), "{body}");
        assert!(!body.contains("newDeviceOtp"), "{body}");
    }
}
