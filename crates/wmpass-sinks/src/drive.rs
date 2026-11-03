//! Google Drive, in one of two places. The app data folder (scope `drive.appdata`) is seen by
//! nothing else, including the owner in Drive's own interface, so restoring always goes
//! through WM Pass. A folder in My Drive (scope `drive.file`) is one the owner can open, and
//! the app sees only the files it made there, so it makes its own folder rather than use one
//! of the same name made by hand.

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_net::Request;

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

const FILES: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
const BOUNDARY: &str = "wmpass-part-boundary-7f3a";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

/// Scope for the hidden app folder.
pub const SCOPE_APP_DATA: &str = "https://www.googleapis.com/auth/drive.appdata";
/// Scope for a visible folder: the files this app made, and nothing else.
pub const SCOPE_FILE: &str = "https://www.googleapis.com/auth/drive.file";

/// Where in Drive the copies go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Space {
    /// The hidden app data folder.
    AppData,
    /// A folder by path from the top of My Drive, made on first use.
    Folder(String),
}

impl Space {
    /// The scope a sign-in must ask for to reach this space.
    pub fn scope(&self) -> &'static str {
        match self {
            Self::AppData => SCOPE_APP_DATA,
            Self::Folder(_) => SCOPE_FILE,
        }
    }
}

/// A Drive folder.
#[derive(Debug)]
pub struct Drive<H> {
    client: Client<H>,
    token: Secret,
    space: Space,
    /// The visible folder's id once found or made.
    folder_id: std::sync::Mutex<Option<String>>,
}

fn escape(name: &str) -> String {
    name.replace('\\', "\\\\").replace('\'', "\\'")
}

impl<H: AsyncHttp> Drive<H> {
    /// The app data folder, with a current access token.
    pub fn new(client: Client<H>, token: Secret) -> Self {
        Self::in_space(client, token, Space::AppData)
    }

    /// Either space.
    pub fn in_space(client: Client<H>, token: Secret, space: Space) -> Self {
        Self { client, token, space, folder_id: std::sync::Mutex::new(None) }
    }

    fn auth(&self, request: Request) -> Request {
        request.header("Authorization", format!("Bearer {}", self.token.expose()))
    }

    fn spaces(&self) -> &'static str {
        match self.space {
            Space::AppData => "appDataFolder",
            Space::Folder(_) => "drive",
        }
    }

    /// `appDataFolder`, or the visible folder's id, found or made segment by segment.
    async fn parent(&self) -> Result<String> {
        let path = match &self.space {
            Space::AppData => return Ok("appDataFolder".to_owned()),
            Space::Folder(path) => path,
        };
        if let Some(id) = self.folder_id.lock().map_err(|_| Fault::Io)?.clone() {
            return Ok(id);
        }
        let mut parent = "root".to_owned();
        let segments: Vec<&str> = path.split('/').map(str::trim).filter(|s| !s.is_empty()).collect();
        for segment in if segments.is_empty() { vec!["WM Pass"] } else { segments } {
            parent = match self.find_folder(segment, &parent).await? {
                Some(id) => id,
                None => self.make_folder(segment, &parent).await?,
            };
        }
        *self.folder_id.lock().map_err(|_| Fault::Io)? = Some(parent.clone());
        Ok(parent)
    }

    async fn find_folder(&self, name: &str, parent: &str) -> Result<Option<String>> {
        let query = wmpass_core::otp::percent_encode(&format!(
            "name = '{}' and mimeType = '{FOLDER_MIME}' and '{}' in parents and trashed = false",
            escape(name),
            escape(parent),
        ));
        let url = format!("{FILES}?spaces=drive&pageSize=1&fields=files(id)&q={query}");
        let root = self.client.ok(self.auth(Request::get(url))).await?.json().map_err(|_| Fault::Io)?;
        Ok(root.pointer("/files/0/id").and_then(Value::as_str).map(str::to_owned))
    }

    async fn make_folder(&self, name: &str, parent: &str) -> Result<String> {
        let body = json!({"name": name, "mimeType": FOLDER_MIME, "parents": [parent]});
        let response = self.client.ok(self.auth(Request::json("POST", format!("{FILES}?fields=id"), &body))).await?;
        let root = response.json().map_err(|_| Fault::Io)?;
        root.get("id").and_then(Value::as_str).map(str::to_owned).ok_or(Fault::Io)
    }

    /// Drive allows several files of one name, so an existing one must be found to
    /// replace it.
    async fn find(&self, name: &str) -> Result<Option<String>> {
        let parent = self.parent().await?;
        let query = wmpass_core::otp::percent_encode(&format!("name = '{}' and '{}' in parents and trashed = false", escape(name), escape(&parent)));
        let url = format!("{FILES}?spaces={}&fields=files(id)&q={query}", self.spaces());
        let response = self.client.ok(self.auth(Request::get(url))).await?;
        let root = response.json().map_err(|_| Fault::Io)?;
        Ok(root.pointer("/files/0/id").and_then(Value::as_str).map(str::to_owned))
    }
}

fn multipart(metadata: &Value, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n").as_bytes());
    body.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

impl<H: AsyncHttp> Sink for Drive<H> {
    async fn readiness(&self) -> Result<()> {
        if let Space::Folder(_) = self.space {
            // Finding or making the folder proves the token and the network, and gives a
            // first backup somewhere to go.
            return self.parent().await.map(|_| ());
        }
        let url = format!("{FILES}?spaces=appDataFolder&pageSize=1&fields=files(id)");
        self.client.ok(self.auth(Request::get(url))).await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let existing = self.find(name).await?;
        let (method, url, metadata) = match &existing {
            Some(id) => ("PATCH", format!("{UPLOAD}/{id}?uploadType=multipart&fields=id"), json!({})),
            None => (
                "POST",
                format!("{UPLOAD}?uploadType=multipart&fields=id"),
                json!({"name": name, "parents": [self.parent().await?]}),
            ),
        };
        let mut request = self.auth(Request::verb(method, url));
        request.body = Some(("multipart/related; boundary=wmpass-part-boundary-7f3a", multipart(&metadata, bytes)));
        request.timeout_ms = 300_000;
        let response = self.client.ok(request).await?;
        let root = response.json().map_err(|_| Fault::Io)?;
        let id = root.get("id").and_then(Value::as_str).ok_or(Fault::Io)?;
        Ok(Stored { id: id.to_owned(), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        let within = match self.space {
            Space::AppData => String::new(),
            Space::Folder(_) => format!("&q={}", wmpass_core::otp::percent_encode(&format!("'{}' in parents and trashed = false", escape(&self.parent().await?)))),
        };
        loop {
            let mut url = format!("{FILES}?spaces={}&pageSize=200&fields=nextPageToken,files(id,name,size){within}", self.spaces());
            if let Some(token) = &token {
                url.push_str("&pageToken=");
                url.push_str(&wmpass_core::otp::percent_encode(token));
            }
            let response = self.client.ok(self.auth(Request::get(url))).await?;
            let root = response.json().map_err(|_| Fault::Io)?;
            for item in root.get("files").and_then(Value::as_array).ok_or(Fault::Io)? {
                out.push(Stored {
                    id: item.get("id").and_then(Value::as_str).ok_or(Fault::Io)?.to_owned(),
                    name: item.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    // Drive sends sizes as strings.
                    size: item.get("size").and_then(Value::as_str).and_then(|s| s.parse().ok()).unwrap_or(0),
                });
            }
            match root.get("nextPageToken").and_then(Value::as_str) {
                Some(more) => token = Some(more.to_owned()),
                None => return Ok(out),
            }
        }
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let mut request = self.auth(Request::get(format!("{FILES}/{}?alt=media", file.id)));
        request.timeout_ms = 300_000;
        Ok(self.client.ok(request).await?.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let response = self.client.send(self.auth(Request::verb("DELETE", format!("{FILES}/{}", file.id)))).await?;
        match response.status {
            200..=299 | 404 => Ok(()),
            status => Err(crate::status_fault(status)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Blocking, block_on};
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    fn sink(http: &FakeHttp) -> Drive<Blocking<'_>> {
        Drive::new(Client::new(Blocking(http), NetPolicy::Any), "tok".into())
    }

    #[test]
    fn a_new_file_goes_in_the_hidden_folder_and_an_existing_one_is_replaced() {
        let fresh = FakeHttp::new().answer(UPLOAD, 200, r#"{"id":"new"}"#).answer(FILES, 200, r#"{"files":[]}"#);
        let stored = block_on(sink(&fresh).write("v", b"secret-bytes")).unwrap();
        assert_eq!(stored.id, "new");
        let asked = fresh.last().unwrap();
        assert_eq!(asked.method, "POST");
        let body = String::from_utf8(asked.body.unwrap().1).unwrap();
        assert!(body.contains("appDataFolder") && body.contains("secret-bytes"));

        let replace = FakeHttp::new().answer(UPLOAD, 200, r#"{"id":"old"}"#).answer(FILES, 200, r#"{"files":[{"id":"old"}]}"#);
        block_on(sink(&replace).write("v", b"x")).unwrap();
        let asked = replace.last().unwrap();
        assert_eq!(asked.method, "PATCH");
        assert!(asked.url.contains("/old?"));
    }

    #[test]
    fn a_name_cannot_break_out_of_the_query() {
        let http = FakeHttp::new().answer(UPLOAD, 200, r#"{"id":"x"}"#).answer(FILES, 200, r#"{"files":[]}"#);
        block_on(sink(&http).write("a' or name != 'b", b"x")).unwrap();
        let first = http.asked.lock().unwrap()[0].url.clone();
        assert!(first.contains("a%5C%27%20or"), "{first}");
    }

    #[test]
    fn a_visible_folder_is_made_once_and_holds_the_files() {
        let http = FakeHttp::new()
            .answer(UPLOAD, 200, r#"{"id":"file"}"#)
            .answer(&format!("{FILES}?fields=id"), 200, r#"{"id":"made"}"#)
            .answer(FILES, 200, r#"{"files":[]}"#);
        let drive = Drive::in_space(Client::new(Blocking(&http), NetPolicy::Any), "tok".into(), Space::Folder("Backups/WM Pass".into()));
        block_on(drive.write("v", b"x")).unwrap();
        let asked = http.asked.lock().unwrap().clone();
        let made: Vec<_> = asked.iter().filter(|r| r.url.ends_with("?fields=id")).collect();
        assert_eq!(made.len(), 2, "one folder per segment");
        let upload = asked.last().unwrap();
        assert!(String::from_utf8(upload.body.clone().unwrap().1).unwrap().contains(r#""parents":["made"]"#));
        block_on(drive.list()).unwrap();
        let listed = http.last().unwrap();
        assert!(listed.url.contains("spaces=drive") && listed.url.contains("%27made%27%20in%20parents"), "{}", listed.url);
        assert_eq!(http.asked.lock().unwrap().iter().filter(|r| r.url.ends_with("?fields=id")).count(), 2, "the folder id is kept");
    }

    #[test]
    fn listing_reads_string_sizes_and_pages() {
        let http = FakeHttp::new()
            .answer(&format!("{FILES}?spaces=appDataFolder&pageSize=200&fields=nextPageToken,files(id,name,size)&pageToken="), 200, r#"{"files":[{"id":"2","name":"b","size":"7"}]}"#)
            .answer(FILES, 200, r#"{"nextPageToken":"t","files":[{"id":"1","name":"a","size":"5"}]}"#);
        let files = block_on(sink(&http).list()).unwrap();
        assert_eq!((files[0].size, files[1].size), (5, 7));
    }
}
