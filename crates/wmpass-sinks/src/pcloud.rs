//! A folder in pCloud, through pCloud's own HTTP API.
//!
//! pCloud also has a WebDAV door, but that takes the account password on every request
//! and has no answer for an account with two-step verification. This signs in once
//! ([`sign_in`], then [`submit_code`] when asked) and keeps the token that hands back.
//!
//! Every call answers HTTP 200 with a `result` number in the body: 0 is success,
//! anything else an error, which [`result_fault`] reads.

use serde_json::Value;
use wmpass_core::Secret;
use wmpass_core::otp::percent_encode;
use wmpass_net::Request;

use crate::boxcloud::{FORM_DATA, form_data};
use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

/// Which of pCloud's two data regions an account lives in. Fixed when the account is made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PCloudRegion {
    /// `api.pcloud.com`.
    #[default]
    Us,
    /// `eapi.pcloud.com`.
    Eu,
}

impl PCloudRegion {
    /// The API host.
    pub fn api_host(self) -> &'static str {
        match self {
            Self::Us => "api.pcloud.com",
            Self::Eu => "eapi.pcloud.com",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Us => Self::Eu,
            Self::Eu => Self::Us,
        }
    }
}

/// A pCloud account and folder.
#[derive(Clone, PartialEq, Eq)]
pub struct PCloudConfig {
    /// Where the account lives.
    pub region: PCloudRegion,
    /// The account's email, for the screen.
    pub email: String,
    /// What the sign-in handed back. Empty means signed out.
    pub auth: Secret,
    /// Folder from the top, made on first use.
    pub folder: String,
}

impl Default for PCloudConfig {
    fn default() -> Self {
        Self {
            region: PCloudRegion::Us,
            email: String::new(),
            auth: Secret::default(),
            folder: "WM Pass backups".to_owned(),
        }
    }
}

impl std::fmt::Debug for PCloudConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PCloudConfig").field("region", &self.region).finish_non_exhaustive()
    }
}

/// What a pCloud `result` means. 1000 is "log in required", 2000 "log in failed", 2094 an
/// invalid token and 2297 a two-step code wanted; 2005 and 2009 are a missing folder and
/// file; 2008 is a full account.
pub fn result_fault(result: i64) -> Fault {
    match result {
        1000 | 2000 | 2094 | 2095 | 2297 | 2064 => Fault::PermissionLost,
        2005 | 2009 | 2002 => Fault::TargetMissing,
        2008 => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

/// A non-zero `result`, with the body that carried it.
enum Refusal {
    Result(i64, Value),
    Fault(Fault),
}

/// The one call every pCloud request is: 200 always, a `result` in the body.
async fn call<H: AsyncHttp>(client: &Client<H>, request: Request) -> std::result::Result<Value, Refusal> {
    let response = client.send(request).await.map_err(Refusal::Fault)?;
    if !(200..300).contains(&response.status) {
        return Err(Refusal::Fault(Fault::Io));
    }
    let root = response.json().map_err(|_| Refusal::Fault(Fault::Io))?;
    match root.get("result").and_then(Value::as_i64).unwrap_or(0) {
        0 => Ok(root),
        result => Err(Refusal::Result(result, root)),
    }
}

async fn checked<H: AsyncHttp>(client: &Client<H>, request: Request) -> Result<Value> {
    call(client, request).await.map_err(|refusal| match refusal {
        Refusal::Result(result, _) => result_fault(result),
        Refusal::Fault(fault) => fault,
    })
}

/// A pCloud folder.
pub struct PCloud<H> {
    client: Client<H>,
    config: PCloudConfig,
    /// With a leading slash and none at the end; `/` for the top.
    folder: String,
}

impl<H> std::fmt::Debug for PCloud<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PCloud").field("config", &self.config).finish_non_exhaustive()
    }
}

impl<H: AsyncHttp> PCloud<H> {
    /// A folder in a signed-in account.
    pub fn new(client: Client<H>, config: PCloudConfig) -> Self {
        let parts: Vec<&str> = config.folder.split('/').map(str::trim).filter(|part| !part.is_empty()).collect();
        let folder = format!("/{}", parts.join("/"));
        Self { client, config, folder }
    }

    fn path_of(&self, name: &str) -> String {
        if self.folder == "/" { format!("/{name}") } else { format!("{}/{name}", self.folder) }
    }

    /// The token goes in the query, as pCloud's API is designed; addresses are never
    /// printed or logged here.
    fn url(&self, method: &str, query: &[(&str, &str)]) -> String {
        let mut url = format!("https://{}/{method}?auth={}", self.config.region.api_host(), percent_encode(self.config.auth.expose()));
        for (key, value) in query {
            url.push('&');
            url.push_str(key);
            url.push('=');
            url.push_str(&percent_encode(value));
        }
        url
    }

    async fn ensure_folder(&self) -> Result<()> {
        if self.config.auth.is_empty() {
            return Err(Fault::NotConfigured);
        }
        let mut at = String::new();
        for part in self.folder.split('/').filter(|part| !part.is_empty()) {
            at.push('/');
            at.push_str(part);
            checked(&self.client, Request::get(self.url("createfolderifnotexists", &[("path", &at)]))).await?;
        }
        Ok(())
    }

    fn stored_of(&self, item: &Value) -> Option<Stored> {
        let name = item.get("name").and_then(Value::as_str)?;
        Some(Stored { id: self.path_of(name), name: name.to_owned(), size: item.get("size").and_then(Value::as_u64).unwrap_or(0) })
    }
}

impl<H: AsyncHttp> Sink for PCloud<H> {
    async fn readiness(&self) -> Result<()> {
        self.ensure_folder().await
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        self.ensure_folder().await?;
        // nopartial: a cut-off upload leaves nothing behind rather than a short file under
        // the real name. A file already of this name is replaced, which leaving
        // renameifexists off does.
        let mut request = Request::verb("POST", self.url("uploadfile", &[("path", &self.folder), ("filename", name), ("nopartial", "1")]));
        request.body = Some((FORM_DATA, form_data(&[], "file", name, bytes)));
        request.timeout_ms = 300_000;
        let answer = checked(&self.client, request).await?;
        Ok(answer
            .pointer("/metadata/0")
            .and_then(|item| self.stored_of(item))
            .unwrap_or_else(|| Stored { id: self.path_of(name), name: name.to_owned(), size: bytes.len() as u64 }))
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let answer = checked(&self.client, Request::get(self.url("listfolder", &[("path", &self.folder)]))).await?;
        let contents = answer.pointer("/metadata/contents").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(contents
            .iter()
            .filter(|item| item.get("isfolder").and_then(Value::as_bool) != Some(true))
            .filter_map(|item| self.stored_of(item))
            .collect())
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let link = checked(&self.client, Request::get(self.url("getfilelink", &[("path", &self.path_of(&file.name))]))).await?;
        let host = link.pointer("/hosts/0").and_then(Value::as_str).ok_or(Fault::Io)?;
        let path = link.get("path").and_then(Value::as_str).ok_or(Fault::Io)?;
        let mut download = Request::get(format!("https://{host}{path}"));
        download.timeout_ms = 300_000;
        let response = self.client.send(download).await?;
        if (200..300).contains(&response.status) { Ok(response.body) } else { Err(Fault::Io) }
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        match checked(&self.client, Request::get(self.url("deletefile", &[("path", &self.path_of(&file.name))]))).await {
            Ok(_) | Err(Fault::TargetMissing) => Ok(()),
            Err(fault) => Err(fault),
        }
    }
}

/// Where a pCloud sign-in stands, for the settings screen.
#[derive(Debug, PartialEq, Eq)]
pub enum PCloudLogin {
    /// Signed in: store this as [`PCloudConfig::auth`].
    SignedIn(Secret),
    /// The account has two-step verification. Hand this back with the code to [`submit_code`].
    NeedsCode(PCloudPending),
    /// The account lives in the other region; sign in there instead.
    OtherRegion(PCloudRegion),
    /// Refused, or the service could not be reached.
    Failed(Fault),
}

/// A sign-in waiting for its two-step code. Opaque.
#[derive(Clone, PartialEq, Eq)]
pub struct PCloudPending {
    region: PCloudRegion,
    token: Secret,
}

impl std::fmt::Debug for PCloudPending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PCloudPending").field("region", &self.region).finish_non_exhaustive()
    }
}

const TFA_REQUIRED: i64 = 2297;
const OTHER_LOCATION: i64 = 2321;

/// Signs in with the account password. POST, so the password is in the body and never in
/// an address that something might log.
pub async fn sign_in<H: AsyncHttp>(client: &Client<H>, region: PCloudRegion, email: &str, password: &Secret) -> PCloudLogin {
    let body = crate::oauth::form_body(&[
        ("getauth", "1"),
        ("logout", "1"),
        ("username", email.trim()),
        ("password", password.expose()),
        ("device", "WM Pass"),
    ]);
    let mut request = Request::verb("POST", format!("https://{}/userinfo", region.api_host()));
    request.body = Some(("application/x-www-form-urlencoded", body));
    answer(client, region, request).await
}

/// Finishes a sign-in with the two-step code, trusting this device so later sign-ins skip it.
pub async fn submit_code<H: AsyncHttp>(client: &Client<H>, pending: &PCloudPending, code: &str) -> PCloudLogin {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let body = crate::oauth::form_body(&[("token", pending.token.expose()), ("code", &code), ("trustdevice", "1")]);
    let mut request = Request::verb("POST", format!("https://{}/tfa_login", pending.region.api_host()));
    request.body = Some(("application/x-www-form-urlencoded", body));
    answer(client, pending.region, request).await
}

async fn answer<H: AsyncHttp>(client: &Client<H>, region: PCloudRegion, request: Request) -> PCloudLogin {
    match call(client, request).await {
        Ok(root) => match root.get("auth").and_then(Value::as_str) {
            Some(auth) => PCloudLogin::SignedIn(Secret::new(auth)),
            None => PCloudLogin::Failed(Fault::Io),
        },
        Err(Refusal::Result(TFA_REQUIRED, root)) => match root.get("token").and_then(Value::as_str) {
            Some(token) => PCloudLogin::NeedsCode(PCloudPending { region, token: Secret::new(token) }),
            None => PCloudLogin::Failed(Fault::Io),
        },
        // "This user is in another location": the answer names the region's API host.
        Err(Refusal::Result(OTHER_LOCATION, root)) => {
            let host = root.pointer("/location/api").and_then(Value::as_str);
            let named = [PCloudRegion::Us, PCloudRegion::Eu].into_iter().find(|r| Some(r.api_host()) == host);
            PCloudLogin::OtherRegion(named.unwrap_or(region.other()))
        }
        Err(Refusal::Result(result, _)) => PCloudLogin::Failed(result_fault(result)),
        Err(Refusal::Fault(fault)) => PCloudLogin::Failed(fault),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Blocking, block_on};
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    const EU: &str = "https://eapi.pcloud.com";

    fn sink(http: &FakeHttp) -> PCloud<Blocking<'_>> {
        let config = PCloudConfig { region: PCloudRegion::Eu, auth: "tok".into(), folder: "/A/ B /".into(), ..PCloudConfig::default() };
        PCloud::new(Client::new(Blocking(http), NetPolicy::Any), config)
    }

    #[test]
    fn folders_are_made_level_by_level_and_uploads_leave_no_partial() {
        let http = FakeHttp::new()
            .answer(&format!("{EU}/uploadfile"), 200, r#"{"result":0,"metadata":[{"name":"v.bin","size":4}]}"#)
            .answer(EU, 200, r#"{"result":0}"#);
        let stored = block_on(sink(&http).write("v.bin", b"data")).unwrap();
        assert_eq!(stored, Stored { id: "/A/B/v.bin".into(), name: "v.bin".into(), size: 4 });
        let asked = http.asked.lock().unwrap();
        assert!(asked[0].url.ends_with("createfolderifnotexists?auth=tok&path=%2FA"));
        assert!(asked[1].url.ends_with("path=%2FA%2FB"));
        assert!(asked[2].url.contains("filename=v.bin&nopartial=1"));
    }

    #[test]
    fn results_in_a_200_are_read() {
        let gone = FakeHttp::new().answer(EU, 200, r#"{"result":2009,"error":"File not found."}"#);
        let file = Stored { id: "/A/B/v".into(), name: "v".into(), size: 0 };
        assert_eq!(block_on(sink(&gone).delete(&file)), Ok(()));
        let full = FakeHttp::new().answer(EU, 200, r#"{"result":2008}"#);
        assert_eq!(block_on(sink(&full).readiness()), Err(Fault::OutOfSpace));
        assert_eq!(result_fault(2094), Fault::PermissionLost);
        assert_eq!(result_fault(2005), Fault::TargetMissing);
        let signed_out = PCloud::new(Client::new(Blocking(&gone), NetPolicy::Any), PCloudConfig::default());
        assert_eq!(block_on(signed_out.readiness()), Err(Fault::NotConfigured));
    }

    #[test]
    fn listing_skips_folders_and_reading_follows_the_link() {
        let http = FakeHttp::new()
            .answer(&format!("{EU}/listfolder"), 200, r#"{"result":0,"metadata":{"contents":[{"name":"d","isfolder":true},{"name":"v","size":3,"isfolder":false}]}}"#)
            .answer(&format!("{EU}/getfilelink"), 200, r#"{"result":0,"hosts":["c1.pcloud.example"],"path":"/x/v"}"#)
            .answer("https://c1.pcloud.example/x/v", 200, "abc");
        let files = block_on(sink(&http).list()).unwrap();
        assert_eq!(files, vec![Stored { id: "/A/B/v".into(), name: "v".into(), size: 3 }]);
        assert_eq!(block_on(sink(&http).read(&files[0])).unwrap(), b"abc");
    }

    #[test]
    fn sign_in_asks_for_a_code_then_signs_in_and_finds_the_region() {
        let client_http = FakeHttp::new()
            .answer("https://api.pcloud.com/userinfo", 200, r#"{"result":2297,"token":"pending"}"#)
            .answer("https://api.pcloud.com/tfa_login", 200, r#"{"result":0,"auth":"A"}"#);
        let client = Client::new(Blocking(&client_http), NetPolicy::Any);
        let PCloudLogin::NeedsCode(pending) = block_on(sign_in(&client, PCloudRegion::Us, " me@x ", &"pw".into())) else {
            panic!("a code was due");
        };
        let asked = client_http.last().unwrap();
        assert!(!asked.url.contains("pw"), "the password stays out of the address");
        assert!(String::from_utf8(asked.body.unwrap().1).unwrap().contains("username=me%40x&password=pw"));
        assert!(!format!("{pending:?}").contains("pending\""));
        assert_eq!(block_on(submit_code(&client, &pending, "123 456")), PCloudLogin::SignedIn("A".into()));
        assert!(String::from_utf8(client_http.last().unwrap().body.unwrap().1).unwrap().contains("code=123456"));

        let moved = FakeHttp::new().answer("https://api.pcloud.com/userinfo", 200, r#"{"result":2321,"location":{"api":"eapi.pcloud.com"}}"#);
        let client = Client::new(Blocking(&moved), NetPolicy::Any);
        assert_eq!(block_on(sign_in(&client, PCloudRegion::Us, "me", &"pw".into())), PCloudLogin::OtherRegion(PCloudRegion::Eu));
        let wrong = FakeHttp::new().answer("https://api.pcloud.com/userinfo", 200, r#"{"result":2000}"#);
        let client = Client::new(Blocking(&wrong), NetPolicy::Any);
        assert_eq!(block_on(sign_in(&client, PCloudRegion::Us, "me", &"pw".into())), PCloudLogin::Failed(Fault::PermissionLost));
    }
}
