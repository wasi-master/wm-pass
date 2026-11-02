//! OneDrive, inside this app's own folder. Large files go up in pieces, because Graph
//! caps a single request.

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_net::Request;

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored, segment};

const ROOT: &str = "https://graph.microsoft.com/v1.0/me/drive/special/approot";
const SIMPLE_LIMIT: usize = 4 * 1024 * 1024;
/// Must be a multiple of 320 KiB.
const PIECE: usize = 10 * 1024 * 1024;

/// A OneDrive app folder.
#[derive(Debug)]
pub struct OneDrive<H> {
    client: Client<H>,
    token: Secret,
}

impl<H: AsyncHttp> OneDrive<H> {
    /// With a current access token.
    pub fn new(client: Client<H>, token: Secret) -> Self {
        Self { client, token }
    }

    fn auth(&self, request: Request) -> Request {
        request.header("Authorization", format!("Bearer {}", self.token.expose()))
    }
}

impl<H: AsyncHttp> Sink for OneDrive<H> {
    async fn readiness(&self) -> Result<()> {
        self.client.ok(self.auth(Request::get(ROOT))).await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let id = format!("{ROOT}:/{}:", segment(name));
        if bytes.len() <= SIMPLE_LIMIT {
            let mut put = self.auth(Request::verb("PUT", format!("{id}/content")));
            put.body = Some(("application/octet-stream", bytes.to_vec()));
            put.timeout_ms = 120_000;
            self.client.ok(put).await?;
        } else {
            let body = json!({"item": {"@microsoft.graph.conflictBehavior": "replace"}});
            let session = self.client.ok(self.auth(Request::json("POST", format!("{id}/createUploadSession"), &body))).await?;
            let root = session.json().map_err(|_| Fault::Io)?;
            let target = root.get("uploadUrl").and_then(Value::as_str).ok_or(Fault::Io)?;
            let total = bytes.len();
            for (index, piece) in bytes.chunks(PIECE).enumerate() {
                let start = index * PIECE;
                let end = start + piece.len() - 1;
                // The upload address is pre-authorised, and sending the token to it is
                // documented as wrong.
                let mut put = Request::verb("PUT", target).header("Content-Range", format!("bytes {start}-{end}/{total}"));
                put.body = Some(("application/octet-stream", piece.to_vec()));
                put.timeout_ms = 300_000;
                self.client.ok(put).await?;
            }
        }
        Ok(Stored { id, name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let mut out = Vec::new();
        let mut next = format!("{ROOT}/children?$select=name,size,file");
        loop {
            let response = self.client.ok(self.auth(Request::get(next))).await?;
            let root = response.json().map_err(|_| Fault::Io)?;
            for item in root.get("value").and_then(Value::as_array).ok_or(Fault::Io)? {
                if item.get("file").is_none() {
                    continue;
                }
                let name = item.get("name").and_then(Value::as_str).ok_or(Fault::Io)?;
                out.push(Stored {
                    id: format!("{ROOT}:/{}:", segment(name)),
                    name: name.to_owned(),
                    size: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                });
            }
            match root.get("@odata.nextLink").and_then(Value::as_str) {
                Some(link) => next = link.to_owned(),
                None => return Ok(out),
            }
        }
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        // Graph answers with a redirect to a pre-authorised address, and clients here
        // do not follow redirects, so the second request goes without the token.
        let mut request = self.auth(Request::get(format!("{}/content", file.id)));
        request.timeout_ms = 300_000;
        let first = self.client.send(request).await?;
        if (200..300).contains(&first.status) {
            return Ok(first.body);
        }
        if first.status != 302 {
            return Err(crate::status_fault(first.status));
        }
        let meta = self.client.ok(self.auth(Request::get(format!("{}?select=@microsoft.graph.downloadUrl", file.id)))).await?;
        let root = meta.json().map_err(|_| Fault::Io)?;
        let url = root.get("@microsoft.graph.downloadUrl").and_then(Value::as_str).ok_or(Fault::Io)?;
        let mut download = Request::get(url);
        download.timeout_ms = 300_000;
        Ok(self.client.ok(download).await?.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let response = self.client.send(self.auth(Request::verb("DELETE", file.id.clone()))).await?;
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

    fn sink(http: &FakeHttp) -> OneDrive<Blocking<'_>> {
        OneDrive::new(Client::new(Blocking(http), NetPolicy::Any), "tok".into())
    }

    #[test]
    fn small_files_go_straight_up() {
        let http = FakeHttp::new().answer(ROOT, 201, "{}");
        block_on(sink(&http).write("v", b"data")).unwrap();
        assert!(http.last().unwrap().url.ends_with(":/v:/content"));
    }

    #[test]
    fn large_files_use_a_session_in_ranges_without_the_token() {
        let http = FakeHttp::new()
            .answer("https://upload.example/", 202, "{}")
            .answer(ROOT, 200, r#"{"uploadUrl":"https://upload.example/s"}"#);
        let bytes = vec![0u8; PIECE + 5];
        block_on(sink(&http).write("v", &bytes)).unwrap();
        let asked = http.asked.lock().unwrap();
        assert_eq!(asked.len(), 3);
        let range = |i: usize| asked[i].headers.iter().find(|(n, _)| *n == "Content-Range").unwrap().1.clone();
        assert_eq!(range(1), format!("bytes 0-{}/{}", PIECE - 1, PIECE + 5));
        assert_eq!(range(2), format!("bytes {PIECE}-{}/{}", PIECE + 4, PIECE + 5));
        assert!(asked[1].headers.iter().all(|(n, _)| *n != "Authorization"), "the token must not go to the upload host");
        assert_eq!(PIECE % (320 * 1024), 0);
    }

    #[test]
    fn listing_pages_and_skips_folders() {
        let http = FakeHttp::new()
            .answer("https://next.example", 200, r#"{"value":[{"name":"b","size":2,"file":{}}]}"#)
            .answer(ROOT, 200, r#"{"value":[{"name":"d","folder":{}},{"name":"a","size":1,"file":{}}],"@odata.nextLink":"https://next.example/p"}"#);
        let files = block_on(sink(&http).list()).unwrap();
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    }
}
