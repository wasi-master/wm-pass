//! A folder in the user's Box, through the Box REST API.
//!
//! Box has no app-folder scope like Dropbox's and OneDrive's, so the folder is an
//! ordinary one ([`BoxConfig::folder`], made on first use) and the grant is the whole
//! account's. Everything is addressed by id: the folder's is looked up by name from the
//! top once per sink, and a file's comes back with the listing.
//!
//! Box's refresh tokens are single use and last 60 days, so every refresh hands back a
//! new one, which the rotation callback stores before anything else happens. A location
//! left unused for two months has to be signed in to again.

use std::sync::Mutex;

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_net::{Call, Request, Response};

use crate::http::{AsyncHttp, Client};
use crate::oauth::{self, Tokens};
use crate::{Fault, Result, Sink, Stored};

const API: &str = "https://api.box.com/2.0";
const UPLOAD: &str = "https://upload.box.com/api/2.0";
const ROOT_ID: &str = "0";
const PAGE: usize = 1000;

/// Separates the parts of a form upload. Fixed, as the body type has to be: what goes
/// between is encrypted, and 40 random-looking characters do not turn up in it by chance.
pub(crate) const FORM_BOUNDARY: &str = "wmpass-form-7f3a9c2e41d8b6055e0c9a71d4f2b83e";
/// The type of a body made by [`form_data`].
pub(crate) const FORM_DATA: &str = "multipart/form-data; boundary=wmpass-form-7f3a9c2e41d8b6055e0c9a71d4f2b83e";

/// A `multipart/form-data` body: the text fields in order, then the file. Box, pCloud and
/// both NAS vendors read the parts in order and need to know where a file goes before it
/// starts arriving, so the file is always last.
pub(crate) fn form_data(fields: &[(&str, &str)], file_field: &str, file_name: &str, bytes: &[u8]) -> Vec<u8> {
    let clean = |text: &str| text.replace(['"', '\r', '\n'], "_");
    let mut body = Vec::with_capacity(bytes.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{FORM_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{value}\r\n", clean(name)).as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{FORM_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
            clean(file_field),
            clean(file_name),
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{FORM_BOUNDARY}--\r\n").as_bytes());
    body
}

/// Where in Box.
#[derive(Clone, PartialEq, Eq)]
pub struct BoxConfig {
    /// Path from the top of All Files, made level by level on first use.
    pub folder: String,
}

impl Default for BoxConfig {
    fn default() -> Self {
        Self { folder: "WM Pass backups".to_owned() }
    }
}

impl std::fmt::Debug for BoxConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxConfig").finish_non_exhaustive()
    }
}

/// The app's credentials and the account's refresh token.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct BoxCredentials {
    /// The app's client id.
    pub client_id: String,
    /// The app's client secret. Box wants it on every token call.
    pub client_secret: Secret,
    /// The account's current refresh token.
    pub refresh_token: Secret,
}

impl std::fmt::Debug for BoxCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxCredentials").finish_non_exhaustive()
    }
}

/// Told the new refresh token whenever Box rotates it, which is on every refresh.
pub type OnRotated = Box<dyn Fn(&Secret) + Send + Sync>;

/// A folder in Box.
pub struct BoxCloud<H> {
    client: Client<H>,
    config: BoxConfig,
    client_id: String,
    client_secret: Secret,
    refresh_token: Mutex<Secret>,
    on_rotated: OnRotated,
    access: Mutex<Option<Secret>>,
    folder_id: Mutex<Option<String>>,
}

impl<H> std::fmt::Debug for BoxCloud<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxCloud").finish_non_exhaustive()
    }
}

/// What a Box failure means. `storage_limit_exceeded` comes as a 403, so the body is read
/// before the status.
pub fn status_fault(status: u16, body: &str) -> Fault {
    match status {
        _ if body.contains("storage_limit_exceeded") => Fault::OutOfSpace,
        413 | 507 => Fault::OutOfSpace,
        401 | 403 => Fault::PermissionLost,
        404 => Fault::TargetMissing,
        _ => Fault::Io,
    }
}

/// Why a call failed: a name already taken, with the id of what has it when Box said,
/// or anything else.
enum Failure {
    Conflict(Option<String>),
    Fault(Fault),
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Fault(fault)
    }
}

fn flatten<T>(result: std::result::Result<T, Failure>) -> Result<T> {
    result.map_err(|failure| match failure {
        Failure::Conflict(_) => Fault::Io,
        Failure::Fault(fault) => fault,
    })
}

/// The id in a 409's `context_info.conflicts`: an object for a file, an array of one for a folder.
fn conflict_id(body: &[u8]) -> Option<String> {
    let root: Value = serde_json::from_slice(body).ok()?;
    let conflicts = root.pointer("/context_info/conflicts")?;
    let item = conflicts.as_array().and_then(|all| all.first()).unwrap_or(conflicts);
    item.get("id").and_then(Value::as_str).map(str::to_owned)
}

fn stored_of(item: &Value) -> Option<Stored> {
    Some(Stored {
        id: item.get("id").and_then(Value::as_str)?.to_owned(),
        name: item.get("name").and_then(Value::as_str)?.to_owned(),
        size: item.get("size").and_then(Value::as_u64).unwrap_or(0),
    })
}

impl<H: AsyncHttp> BoxCloud<H> {
    /// A folder in the account `credentials` sign in to. `on_rotated` must store the new
    /// refresh token, or the next run signs in with a spent one.
    pub fn new(client: Client<H>, config: BoxConfig, credentials: BoxCredentials, on_rotated: OnRotated) -> Self {
        Self {
            client,
            config,
            client_id: credentials.client_id,
            client_secret: credentials.client_secret,
            refresh_token: Mutex::new(credentials.refresh_token),
            on_rotated,
            access: Mutex::new(None),
            folder_id: Mutex::new(None),
        }
    }

    /// An access token, refreshed once per sink. A refused refresh is not transient: the
    /// grant was revoked or has lapsed, and only signing in again helps.
    async fn bearer(&self) -> Result<Secret> {
        if let Some(token) = self.access.lock().map_err(|_| Fault::Io)?.clone() {
            return Ok(token);
        }
        let refresh = self.refresh_token.lock().map_err(|_| Fault::Io)?.clone();
        if refresh.is_empty() || self.client_id.is_empty() {
            return Err(Fault::NotConfigured);
        }
        let Call { request, parse } = oauth::refresh_with_secret(oauth::BOX, &self.client_id, &self.client_secret, &refresh);
        let response = self.client.send(request).await?;
        match response.status {
            200..=299 => {}
            // OAuth reports invalid_grant as 400, some servers as 401.
            400 | 401 => return Err(Fault::PermissionLost),
            _ => return Err(Fault::Io),
        }
        let Tokens { access, refresh: rotated } = parse(&response).map_err(|_| Fault::Io)?;
        if let Some(rotated) = rotated.filter(|next| !next.is_empty() && *next != refresh) {
            *self.refresh_token.lock().map_err(|_| Fault::Io)? = rotated.clone();
            (self.on_rotated)(&rotated);
        }
        *self.access.lock().map_err(|_| Fault::Io)? = Some(access.clone());
        Ok(access)
    }

    async fn authorized(&self, mut request: Request) -> Result<Request> {
        let token = self.bearer().await?;
        request.headers.push(("Authorization", format!("Bearer {}", token.expose())));
        Ok(request)
    }

    async fn call(&self, request: Request, allow_missing: bool) -> std::result::Result<Response, Failure> {
        let response = self.client.send(self.authorized(request).await?).await?;
        match response.status {
            200..=299 => Ok(response),
            404 if allow_missing => Ok(response),
            409 => Err(Failure::Conflict(conflict_id(&response.body))),
            status => Err(Failure::Fault(status_fault(status, &String::from_utf8_lossy(&response.body)))),
        }
    }

    /// Every item in a folder, with its Box type: `file`, `folder` or `web_link`.
    async fn items(&self, folder: &str) -> Result<Vec<(Stored, String)>> {
        let mut out = Vec::new();
        let mut offset = 0usize;
        loop {
            let url = format!("{API}/folders/{folder}/items?fields=id,name,size,modified_at,type&limit={PAGE}&offset={offset}");
            let response = flatten(self.call(Request::get(url), false).await)?;
            let root = response.json().map_err(|_| Fault::Io)?;
            let entries = root.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
            for item in &entries {
                if let Some(stored) = stored_of(item) {
                    let kind = item.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
                    out.push((stored, kind));
                }
            }
            offset += entries.len();
            let total = root.get("total_count").and_then(Value::as_u64).unwrap_or(0);
            if entries.is_empty() || offset as u64 >= total {
                return Ok(out);
            }
        }
    }

    async fn make_folder(&self, parent: &str, name: &str) -> Result<String> {
        let body = json!({"name": name, "parent": {"id": parent}});
        match self.call(Request::json("POST", format!("{API}/folders"), &body), false).await {
            Ok(response) => {
                let root = response.json().map_err(|_| Fault::Io)?;
                root.get("id").and_then(Value::as_str).map(str::to_owned).ok_or(Fault::Io)
            }
            // Made meanwhile, by another run or by hand.
            Err(Failure::Conflict(id)) => id.ok_or(Fault::Io),
            Err(Failure::Fault(fault)) => Err(fault),
        }
    }

    /// The backup folder's id, made level by level where missing.
    async fn folder(&self) -> Result<String> {
        if let Some(id) = self.folder_id.lock().map_err(|_| Fault::Io)?.clone() {
            return Ok(id);
        }
        let mut at = ROOT_ID.to_owned();
        for part in self.config.folder.split('/').map(str::trim).filter(|part| !part.is_empty()) {
            let found = self
                .items(&at)
                .await?
                .into_iter()
                .find(|(item, kind)| kind == "folder" && item.name == part)
                .map(|(item, _)| item.id);
            at = match found {
                Some(id) => id,
                None => self.make_folder(&at, part).await?,
            };
        }
        *self.folder_id.lock().map_err(|_| Fault::Io)? = Some(at.clone());
        Ok(at)
    }

    async fn upload(&self, url: String, attributes: &Value, name: &str, bytes: &[u8]) -> std::result::Result<Stored, Failure> {
        // Attributes first: Box needs to know where the file goes before it arrives.
        let mut request = Request::verb("POST", url);
        request.body = Some((FORM_DATA, form_data(&[("attributes", &attributes.to_string())], "file", name, bytes)));
        request.timeout_ms = 300_000;
        let response = self.call(request, false).await?;
        let root = response.json().map_err(|_| Fault::Io)?;
        root.pointer("/entries/0").and_then(stored_of).ok_or(Failure::Fault(Fault::Io))
    }
}

impl<H: AsyncHttp> Sink for BoxCloud<H> {
    async fn readiness(&self) -> Result<()> {
        self.folder().await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let parent = self.folder().await?;
        let attributes = json!({"name": name, "parent": {"id": parent}});
        match self.upload(format!("{UPLOAD}/files/content"), &attributes, name, bytes).await {
            Ok(stored) => Ok(stored),
            // A file of this name is an older copy: a new version of it rather than a
            // second file, which Box would refuse.
            Err(Failure::Conflict(Some(existing))) => {
                let attributes = json!({"name": name});
                flatten(self.upload(format!("{UPLOAD}/files/{existing}/content"), &attributes, name, bytes).await)
            }
            Err(failure) => flatten(Err(failure)),
        }
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let folder = self.folder().await?;
        Ok(self.items(&folder).await?.into_iter().filter(|(_, kind)| kind == "file").map(|(item, _)| item).collect())
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let mut request = Request::get(format!("{API}/files/{}/content", file.id));
        request.timeout_ms = 300_000;
        let first = self.client.send(self.authorized(request).await?).await?;
        match first.status {
            200..=299 => return Ok(first.body),
            301 | 302 | 303 | 307 | 308 => {}
            status => return Err(status_fault(status, &String::from_utf8_lossy(&first.body))),
        }
        // A redirect to a signed download address. Clients here do not follow
        // redirects, and the token must not go along, so this one is sent without it.
        let location = first.header("location").ok_or(Fault::Io)?;
        let mut download = Request::get(location);
        download.timeout_ms = 300_000;
        Ok(self.client.ok(download).await?.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        flatten(self.call(Request::verb("DELETE", format!("{API}/files/{}", file.id)), true).await).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::http::{Blocking, block_on};
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    const TOKEN: &str = "https://api.box.com/oauth2/token";

    fn sink(http: &FakeHttp, rotated: Arc<Mutex<Vec<String>>>) -> BoxCloud<Blocking<'_>> {
        let credentials = BoxCredentials {
            client_id: "id".into(),
            client_secret: "shh".into(),
            refresh_token: "r1".into(),
        };
        let config = BoxConfig { folder: "Backups/WM".into() };
        BoxCloud::new(
            Client::new(Blocking(http), NetPolicy::Any),
            config,
            credentials,
            Box::new(move |token| rotated.lock().unwrap().push(token.expose().to_owned())),
        )
    }

    #[test]
    fn the_folder_is_found_or_made_and_the_rotated_token_is_kept() {
        let rotated = Arc::new(Mutex::new(Vec::new()));
        let http = FakeHttp::new()
            .answer(TOKEN, 200, r#"{"access_token":"a1","refresh_token":"r2"}"#)
            .answer(&format!("{API}/folders/0/items"), 200, r#"{"entries":[{"type":"folder","id":"7","name":"Backups"}],"total_count":1}"#)
            .answer(&format!("{API}/folders/7/items"), 200, r#"{"entries":[{"type":"file","id":"1","name":"WM"}],"total_count":1}"#)
            .answer(&format!("{API}/folders"), 409, r#"{"code":"item_name_in_use","context_info":{"conflicts":[{"type":"folder","id":"9"}]}}"#);
        let sink = sink(&http, rotated.clone());
        block_on(sink.readiness()).unwrap();
        assert_eq!(sink.folder_id.lock().unwrap().as_deref(), Some("9"));
        assert_eq!(*rotated.lock().unwrap(), vec!["r2".to_owned()]);
        let asked = http.asked.lock().unwrap();
        assert_eq!(asked.iter().filter(|r| r.url == TOKEN).count(), 1, "one refresh per sink");
        let made = asked.iter().find(|r| r.method == "POST" && r.url.ends_with("/folders")).unwrap();
        assert!(String::from_utf8_lossy(&made.body.as_ref().unwrap().1).contains(r#""parent":{"id":"7"}"#));
        assert!(made.headers.iter().any(|(n, v)| *n == "Authorization" && v == "Bearer a1"));
    }

    #[test]
    fn a_name_taken_becomes_a_new_version_and_the_attributes_come_first() {
        let http = FakeHttp::new()
            .answer(TOKEN, 200, r#"{"access_token":"a1","refresh_token":"r1"}"#)
            .answer(&format!("{API}/folders/0/items"), 200, r#"{"entries":[{"type":"folder","id":"7","name":"Backups"}],"total_count":1}"#)
            .answer(&format!("{API}/folders/7/items"), 200, r#"{"entries":[{"type":"folder","id":"8","name":"WM"}],"total_count":1}"#)
            .answer(&format!("{UPLOAD}/files/content"), 409, r#"{"context_info":{"conflicts":{"type":"file","id":"42"}}}"#)
            .answer(&format!("{UPLOAD}/files/42/content"), 201, r#"{"entries":[{"type":"file","id":"42","name":"v.bin","size":4}]}"#);
        let rotated = Arc::new(Mutex::new(Vec::new()));
        let stored = block_on(sink(&http, rotated.clone()).write("v.bin", b"data")).unwrap();
        assert_eq!(stored, Stored { id: "42".into(), name: "v.bin".into(), size: 4 });
        assert!(rotated.lock().unwrap().is_empty(), "an unchanged token is not reported");
        let asked = http.asked.lock().unwrap();
        let first = asked.iter().find(|r| r.url == format!("{UPLOAD}/files/content")).unwrap();
        let body = String::from_utf8_lossy(&first.body.as_ref().unwrap().1).into_owned();
        assert!(body.find("name=\"attributes\"").unwrap() < body.find("filename=\"v.bin\"").unwrap());
        assert!(body.contains(r#"{"name":"v.bin","parent":{"id":"8"}}"#));
    }

    #[test]
    fn a_download_redirect_is_followed_without_the_token() {
        let http = FakeHttp::new()
            .answer(TOKEN, 200, r#"{"access_token":"a1"}"#)
            .answer_with(&format!("{API}/files/5/content"), 302, b"", &[("Location", "https://dl.boxcloud.example/x")])
            .answer("https://dl.boxcloud.example/", 200, "bytes");
        let file = Stored { id: "5".into(), name: "v".into(), size: 5 };
        let rotated = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(block_on(sink(&http, rotated).read(&file)).unwrap(), b"bytes");
        let last = http.last().unwrap();
        assert!(last.headers.iter().all(|(n, _)| *n != "Authorization"));
    }

    #[test]
    fn a_refused_refresh_is_a_lost_permission_and_failures_are_read() {
        let http = FakeHttp::new().answer(TOKEN, 400, r#"{"error":"invalid_grant"}"#);
        let rotated = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(block_on(sink(&http, rotated.clone()).readiness()), Err(Fault::PermissionLost));
        assert_eq!(status_fault(403, r#"{"code":"storage_limit_exceeded"}"#), Fault::OutOfSpace);
        assert_eq!(status_fault(401, ""), Fault::PermissionLost);
        assert_eq!(status_fault(404, ""), Fault::TargetMissing);

        let gone = FakeHttp::new().answer(TOKEN, 200, r#"{"access_token":"a"}"#).answer(API, 404, "");
        let file = Stored { id: "5".into(), name: "v".into(), size: 0 };
        assert_eq!(block_on(sink(&gone, rotated).delete(&file)), Ok(()));
    }

    #[test]
    fn debug_shows_no_secret() {
        let credentials = BoxCredentials { client_id: "id".into(), client_secret: "shh".into(), refresh_token: "rt".into() };
        let printed = format!("{credentials:?} {:?}", BoxConfig { folder: "Private".into() });
        assert!(!printed.contains("shh") && !printed.contains("rt\"") && !printed.contains("Private"));
    }
}
