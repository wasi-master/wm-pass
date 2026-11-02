//! OAuth for Dropbox, OneDrive and Drive: PKCE, so no client secret needs to ship, and
//! refresh tokens, because access tokens last hours and backups run for years.

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use wmpass_core::Secret;
use wmpass_core::otp::percent_encode;
use wmpass_net::{Call, NetError, Request};

/// A service's addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Service {
    /// Sign-in page.
    pub authorize: &'static str,
    /// Token endpoint.
    pub token: &'static str,
    /// Scope to ask for.
    pub scope: &'static str,
    /// Extra query for the sign-in page. Without these two, no refresh token is issued.
    pub extra: &'static str,
}

/// Dropbox, app folder only.
pub const DROPBOX: Service = Service {
    authorize: "https://www.dropbox.com/oauth2/authorize",
    token: "https://api.dropboxapi.com/oauth2/token",
    scope: "files.content.write files.content.read files.metadata.read",
    extra: "token_access_type=offline",
};

/// OneDrive, app folder only.
pub const ONEDRIVE: Service = Service {
    authorize: "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize",
    token: "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
    scope: "Files.ReadWrite.AppFolder offline_access",
    extra: "",
};

/// Google Drive, this app's own hidden folder only.
pub const DRIVE: Service = Service {
    authorize: "https://accounts.google.com/o/oauth2/v2/auth",
    token: "https://oauth2.googleapis.com/token",
    scope: "https://www.googleapis.com/auth/drive.appdata",
    extra: "access_type=offline&prompt=consent",
};

/// A PKCE pair. Keep the verifier until the redirect comes back.
#[derive(Clone, Debug)]
pub struct Pkce {
    /// Stays here.
    pub verifier: Secret,
    /// Goes in the sign-in address.
    pub challenge: String,
    /// Ties the redirect to this request, so another app cannot inject a code.
    pub state: String,
}

/// Makes a pair from the OS random source.
pub fn pkce() -> crate::Result<Pkce> {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let mut random = [0u8; 48];
    getrandom::fill(&mut random).map_err(|_| crate::Fault::Io)?;
    let verifier = engine.encode(&random[..32]);
    let state = engine.encode(&random[32..]);
    let challenge = engine.encode(Sha256::digest(verifier.as_bytes()));
    Ok(Pkce { verifier: Secret::from(verifier), challenge, state })
}

/// The address to open for sign-in.
pub fn authorize_url(service: Service, client_id: &str, redirect: &str, pkce: &Pkce) -> String {
    let mut url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        service.authorize,
        percent_encode(client_id),
        percent_encode(redirect),
        percent_encode(service.scope),
        percent_encode(&pkce.state),
        pkce.challenge,
    );
    if !service.extra.is_empty() {
        url.push('&');
        url.push_str(service.extra);
    }
    url
}

/// What a token endpoint gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tokens {
    /// Short-lived.
    pub access: Secret,
    /// Long-lived, when issued. Microsoft replaces it on every refresh and each one
    /// lasts 90 days, so a new value must always be saved over the old.
    pub refresh: Option<Secret>,
}

fn form(fields: &[(&str, &str)]) -> Vec<u8> {
    fields
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes()
}

fn token_call(service: Service, fields: &[(&str, &str)]) -> Call<Tokens> {
    let mut request = Request::verb("POST", service.token);
    request.body = Some(("application/x-www-form-urlencoded", form(fields)));
    Call::new(request, |response| {
        let root: Value = response.ok()?.json()?;
        let access = root.get("access_token").and_then(Value::as_str).ok_or(NetError::BadAnswer)?;
        Ok(Tokens {
            access: Secret::new(access),
            refresh: root.get("refresh_token").and_then(Value::as_str).map(Secret::new),
        })
    })
}

/// Swaps the code from the redirect for tokens.
pub fn exchange_code(service: Service, client_id: &str, redirect: &str, code: &Secret, verifier: &Secret) -> Call<Tokens> {
    token_call(service, &[
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("redirect_uri", redirect),
        ("code", code.expose()),
        ("code_verifier", verifier.expose()),
    ])
}

/// Gets a fresh access token.
pub fn refresh(service: Service, client_id: &str, token: &Secret) -> Call<Tokens> {
    token_call(service, &[
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", token.expose()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    #[test]
    fn pkce_challenge_is_the_s256_of_the_verifier() {
        let pair = pkce().unwrap();
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        assert_eq!(pair.challenge, engine.encode(Sha256::digest(pair.verifier.expose().as_bytes())));
        assert!(pair.verifier.expose().len() >= 43);
        assert_ne!(pkce().unwrap().state, pair.state);
    }

    #[test]
    fn rfc_7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        assert_eq!(engine.encode(Sha256::digest(verifier)), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn sign_in_addresses_ask_for_a_refresh_token_and_no_secret() {
        let pair = pkce().unwrap();
        let url = authorize_url(DROPBOX, "id", "wmpass://oauth", &pair);
        assert!(url.contains("token_access_type=offline"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("redirect_uri=wmpass%3A%2F%2Foauth"));
        assert!(!url.contains(pair.verifier.expose()));
        assert!(ONEDRIVE.scope.contains("offline_access"));
    }

    #[test]
    fn token_answers_are_read_and_rotation_is_visible() {
        let http = FakeHttp::new().answer(ONEDRIVE.token, 200, r#"{"access_token":"a","refresh_token":"r2"}"#);
        let tokens = refresh(ONEDRIVE, "id", &"r1".into()).run(&http, NetPolicy::Any).unwrap();
        assert_eq!(tokens.access.expose(), "a");
        assert_eq!(tokens.refresh.unwrap().expose(), "r2");
        let body = http.last().unwrap().body.unwrap().1;
        assert!(String::from_utf8(body).unwrap().contains("grant_type=refresh_token"));
        let expired = FakeHttp::new().answer(DROPBOX.token, 401, "");
        assert_eq!(refresh(DROPBOX, "id", &"r".into()).run(&expired, NetPolicy::Any).err(), Some(NetError::Unauthorised));
    }
}
