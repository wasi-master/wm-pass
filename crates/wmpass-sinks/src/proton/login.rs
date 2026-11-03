//! Signing in to Proton from the settings screen: the account password, the 2FA code
//! when the account has one, and the second (mailbox) password on an account that still
//! has two. What is kept is the session and the salted key password; the passwords are
//! not.

use std::sync::Mutex;

use base64::Engine;
use serde_json::{Value, json};
use wmpass_core::Secret;

use super::api::{Api, ApiError, Session, lock};
use super::{pgp, srp};
use crate::Fault;
use crate::http::{AsyncHttp, Client};

const TWO_PASSWORD_MODE: i64 = 2;
const TOTP: i64 = 1;
const HUMAN_VERIFICATION: i64 = 9001;

/// Where a sign-in got to.
#[derive(Debug)]
pub enum ProtonOutcome {
    /// Signed in: what the location keeps.
    SignedIn(ProtonSignedIn),
    /// The account has two-factor sign-in. The state goes back with the code.
    NeedsCode(ProtonPending),
    /// The account has a separate mailbox password; sign in again with it.
    NeedsMailboxPassword,
    /// Proton wants a CAPTCHA, which only its own sign-in page can show.
    NeedsHumanVerification,
    /// Refused or unreachable.
    Failed(Fault),
}

/// A finished sign-in: the session and the salted key password, for [`super::ProtonConfig`].
#[derive(Clone)]
pub struct ProtonSignedIn {
    /// Session id.
    pub uid: String,
    /// Bearer token.
    pub access_token: Secret,
    /// Refresh token; rotated on every refresh.
    pub refresh_token: Secret,
    /// What unlocks the account's keys.
    pub salted_key_pass: Secret,
}

impl std::fmt::Debug for ProtonSignedIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProtonSignedIn(***)")
    }
}

/// A sign-in waiting for its 2FA code. Opaque.
pub struct ProtonPending {
    uid: String,
    access: Secret,
    refresh: Secret,
    key_password: Secret,
}

#[cfg(test)]
impl ProtonPending {
    pub(crate) fn for_tests(uid: &str, access: &str, refresh: &str, key_password: &str) -> Self {
        Self { uid: uid.into(), access: access.into(), refresh: refresh.into(), key_password: key_password.into() }
    }
}

impl std::fmt::Debug for ProtonPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProtonPending(***)")
    }
}

fn b64(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

fn guarded(result: Result<ProtonOutcome, ApiError>) -> ProtonOutcome {
    match result {
        Ok(outcome) => outcome,
        Err(ApiError { code: HUMAN_VERIFICATION, .. }) => ProtonOutcome::NeedsHumanVerification,
        Err(error) => ProtonOutcome::Failed(error.fault),
    }
}

/// The password half: SRP, then the code or the keys. `mailbox_password` is empty unless
/// a first try answered [`ProtonOutcome::NeedsMailboxPassword`].
pub async fn sign_in<H: AsyncHttp>(client: &Client<H>, user: &str, password: &Secret, mailbox_password: &Secret) -> ProtonOutcome {
    guarded(sign_in_inner(client, user, password, mailbox_password).await)
}

async fn sign_in_inner<H: AsyncHttp>(
    client: &Client<H>,
    user: &str,
    password: &Secret,
    mailbox_password: &Secret,
) -> Result<ProtonOutcome, ApiError> {
    let session = Mutex::new(Session::default());
    let api = Api { client, session: &session, on_tokens: None };
    let name = user.trim();
    let info = api.post("auth/v4/info", &json!({ "Username": name })).await?;
    let version = info.get("Version").and_then(Value::as_i64).unwrap_or(0);
    if version < 3 {
        return Ok(ProtonOutcome::Failed(Fault::Io));
    }
    let field = |key: &str| info.get(key).and_then(Value::as_str);
    let Some(modulus) = field("Modulus").and_then(srp::modulus_of) else { return Ok(ProtonOutcome::Failed(Fault::Io)) };
    let Some(salt) = field("Salt").and_then(b64) else { return Ok(ProtonOutcome::Failed(Fault::Io)) };
    let Some(server_b) = field("ServerEphemeral").and_then(b64) else { return Ok(ProtonOutcome::Failed(Fault::Io)) };
    let srp_session = field("SRPSession").unwrap_or_default();
    let Some(proofs) = srp::proofs(password.expose().as_bytes(), &salt, &modulus, &server_b) else {
        return Ok(ProtonOutcome::Failed(Fault::Unsafe));
    };

    let engine = base64::engine::general_purpose::STANDARD;
    let auth = api
        .post(
            "auth/v4",
            &json!({
                "Username": name,
                "ClientEphemeral": engine.encode(&proofs.client_ephemeral),
                "ClientProof": engine.encode(&proofs.client_proof),
                "SRPSession": srp_session,
            }),
        )
        .await?;
    let server_proof = auth.get("ServerProof").and_then(Value::as_str).and_then(b64);
    // The server proving it knew the verifier: without this match a machine in the
    // middle could have answered.
    if server_proof.as_deref() != Some(proofs.expected_server_proof.as_slice()) {
        return Ok(ProtonOutcome::Failed(Fault::Unsafe));
    }

    let two_passwords = auth.get("PasswordMode").and_then(Value::as_i64) == Some(TWO_PASSWORD_MODE);
    if two_passwords && mailbox_password.is_empty() {
        return Ok(ProtonOutcome::NeedsMailboxPassword);
    }
    let text = |key: &str| auth.get(key).and_then(Value::as_str).map(str::to_owned);
    let (Some(uid), Some(access), Some(refresh)) = (text("UID"), text("AccessToken"), text("RefreshToken")) else {
        return Ok(ProtonOutcome::Failed(Fault::Io));
    };
    let pending = ProtonPending {
        uid,
        access: Secret::new(access),
        refresh: Secret::new(refresh),
        key_password: if two_passwords { mailbox_password.clone() } else { password.clone() },
    };
    let enabled = auth.pointer("/2FA/Enabled").and_then(Value::as_i64).unwrap_or(0);
    if enabled & TOTP != 0 {
        Ok(ProtonOutcome::NeedsCode(pending))
    } else {
        finish(client, pending).await
    }
}

/// The 2FA code for a sign-in that asked for one.
pub async fn submit_code<H: AsyncHttp>(client: &Client<H>, pending: ProtonPending, code: &str) -> ProtonOutcome {
    guarded(submit_inner(client, pending, code).await)
}

async fn submit_inner<H: AsyncHttp>(client: &Client<H>, pending: ProtonPending, code: &str) -> Result<ProtonOutcome, ApiError> {
    let session = session_of(&pending);
    let api = Api { client, session: &session, on_tokens: None };
    let digits: String = code.chars().filter(char::is_ascii_digit).collect();
    api.post("auth/v4/2fa", &json!({ "TwoFactorCode": digits })).await?;
    finish(client, pending).await
}

fn session_of(pending: &ProtonPending) -> Mutex<Session> {
    Mutex::new(Session { uid: pending.uid.clone(), access: pending.access.clone(), refresh: pending.refresh.clone() })
}

/// Works out the salted key password and proves it by unlocking a user key.
async fn finish<H: AsyncHttp>(client: &Client<H>, pending: ProtonPending) -> Result<ProtonOutcome, ApiError> {
    let session = session_of(&pending);
    let api = Api { client, session: &session, on_tokens: None };
    let user = api.get("core/v4/users").await?;
    let keys: Vec<&Value> = user.pointer("/User/Keys").and_then(Value::as_array).map(|k| k.iter().collect()).unwrap_or_default();
    let Some(primary) = keys
        .iter()
        .find(|k| k.get("Primary").and_then(Value::as_i64) == Some(1))
        .or_else(|| keys.first())
    else {
        return Ok(ProtonOutcome::Failed(Fault::Io));
    };
    let salts = api.get("core/v4/keys/salts").await?;
    let primary_id = primary.get("ID").and_then(Value::as_str);
    let salt = salts
        .get("KeySalts")
        .and_then(Value::as_array)
        .and_then(|all| all.iter().find(|s| s.get("ID").and_then(Value::as_str) == primary_id))
        .and_then(|s| s.get("KeySalt").and_then(Value::as_str))
        .and_then(b64);
    let (Some(salt), Some(armored)) = (salt, primary.get("PrivateKey").and_then(Value::as_str)) else {
        return Ok(ProtonOutcome::Failed(Fault::Io));
    };
    let salted = srp::salted_key_pass(pending.key_password.expose().as_bytes(), &salt);
    if pgp::Key::unlock(armored, salted.as_bytes()).is_none() {
        return Ok(ProtonOutcome::Failed(Fault::PermissionLost));
    }
    let session = lock(&session);
    Ok(ProtonOutcome::SignedIn(ProtonSignedIn {
        uid: session.uid.clone(),
        access_token: session.access.clone(),
        refresh_token: session.refresh.clone(),
        salted_key_pass: Secret::new(salted),
    }))
}
