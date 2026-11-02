//! Dropbox, inside this app's own folder.

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_net::Request;

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

const API: &str = "https://api.dropboxapi.com/2";
const CONTENT: &str = "https://content.dropboxapi.com/2";

/// A Dropbox app folder.
#[derive(Debug)]
pub struct Dropbox<H> {
    client: Client<H>,
    token: Secret,
}

impl<H: AsyncHttp> Dropbox<H> {
    /// With a current access token.
    pub fn new(client: Client<H>, token: Secret) -> Self {
        Self { client, token }
    }

    fn auth(&self, request: Request) -> Request {
        request.header("Authorization", format!("Bearer {}", self.token.expose()))
    }

    fn rpc(&self, path: &str, body: &Value) -> Request {
        self.auth(Request::json("POST", format!("{API}{path}"), body))
    }

    /// Dropbox reports most failures as 409 with a reason in the body.
    async fn checked(&self, request: Request) -> Result<wmpass_net::Response> {
        let response = self.client.send(request).await?;
        match response.status {
            200..=299 => Ok(response),
            409 => {
                let text = String::from_utf8_lossy(&response.body);
                if text.contains("insufficient_space") {
                    Err(Fault::OutOfSpace)
                } else if text.contains("not_found") {
                    Err(Fault::TargetMissing)
                } else {
                    Err(Fault::Io)
                }
            }
            status => Err(crate::status_fault(status)),
        }
    }
}

impl<H: AsyncHttp> Sink for Dropbox<H> {
    async fn readiness(&self) -> Result<()> {
        self.checked(self.rpc("/files/list_folder", &json!({"path": "", "limit": 1}))).await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let argument = json!({"path": format!("/{name}"), "mode": "overwrite", "mute": true}).to_string();
        let mut request = self.auth(Request::verb("POST", format!("{CONTENT}/files/upload"))).header("Dropbox-API-Arg", argument);
        request.body = Some(("application/octet-stream", bytes.to_vec()));
        request.timeout_ms = 300_000;
        self.checked(request).await?;
        Ok(Stored { id: format!("/{name}"), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let mut out = Vec::new();
        let mut request = self.rpc("/files/list_folder", &json!({"path": ""}));
        loop {
            let response = self.checked(request).await?;
            let root = response.json().map_err(|_| Fault::Io)?;
            for item in root.get("entries").and_then(Value::as_array).ok_or(Fault::Io)? {
                if item.get(".tag").and_then(Value::as_str) != Some("file") {
                    continue;
                }
                let name = item.get("name").and_then(Value::as_str).ok_or(Fault::Io)?;
                let path = item.get("path_lower").and_then(Value::as_str).unwrap_or(name);
                let size = item.get("size").and_then(Value::as_u64).unwrap_or(0);
                out.push(Stored { id: path.to_owned(), name: name.to_owned(), size });
            }
            if root.get("has_more").and_then(Value::as_bool) != Some(true) {
                return Ok(out);
            }
            let cursor = root.get("cursor").and_then(Value::as_str).ok_or(Fault::Io)?;
            request = self.rpc("/files/list_folder/continue", &json!({"cursor": cursor}));
        }
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let argument = json!({"path": file.id}).to_string();
        let mut request = self.auth(Request::verb("POST", format!("{CONTENT}/files/download"))).header("Dropbox-API-Arg", argument);
        request.timeout_ms = 300_000;
        Ok(self.checked(request).await?.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        match self.checked(self.rpc("/files/delete_v2", &json!({"path": file.id}))).await {
            Ok(_) | Err(Fault::TargetMissing) => Ok(()),
            Err(other) => Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Blocking, block_on};
    use wmpass_net::NetPolicy;
    use wmpass_net::http::fake::FakeHttp;

    fn sink(http: &FakeHttp) -> Dropbox<Blocking<'_>> {
        Dropbox::new(Client::new(Blocking(http), NetPolicy::Any), "tok".into())
    }

    #[test]
    fn list_pages_and_skips_folders() {
        // First matching prefix wins, so the continuation has to come first.
        let http = FakeHttp::new()
            .answer(&format!("{API}/files/list_folder/continue"), 200, r#"{"entries":[{".tag":"file","name":"b","path_lower":"/b","size":2}],"has_more":false}"#)
            .answer(&format!("{API}/files/list_folder"), 200, r#"{"entries":[{".tag":"folder","name":"d"},{".tag":"file","name":"A","path_lower":"/a","size":1}],"has_more":true,"cursor":"c"}"#);
        let files = block_on(sink(&http).list()).unwrap();
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["A", "b"]);
        assert_eq!(files[0].id, "/a");
    }

    #[test]
    fn reasons_in_409_bodies_are_read() {
        let full = FakeHttp::new().answer(CONTENT, 409, r#"{"error_summary":"path/insufficient_space/.."}"#);
        assert_eq!(block_on(sink(&full).write("v", b"x")).err(), Some(Fault::OutOfSpace));
        let gone = FakeHttp::new().answer(API, 409, r#"{"error_summary":"path_lookup/not_found/.."}"#);
        let file = Stored { id: "/v".into(), name: "v".into(), size: 0 };
        assert_eq!(block_on(sink(&gone).delete(&file)), Ok(()));
        let expired = FakeHttp::new().answer(API, 401, "");
        assert_eq!(block_on(sink(&expired).readiness()), Err(Fault::PermissionLost));
    }

    #[test]
    fn upload_overwrites_by_name() {
        let http = FakeHttp::new().answer(CONTENT, 200, "{}");
        block_on(sink(&http).write("v", b"data")).unwrap();
        let asked = http.last().unwrap();
        let argument = &asked.headers.iter().find(|(n, _)| *n == "Dropbox-API-Arg").unwrap().1;
        assert!(argument.contains("overwrite") && argument.contains("/v"));
        assert_eq!(asked.body.unwrap().1, b"data");
    }
}
