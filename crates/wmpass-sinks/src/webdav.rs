//! WebDAV: Nextcloud, ownCloud, most NAS boxes.

use base64::Engine;
use quick_xml::Reader;
use quick_xml::events::Event;
use wmpass_core::Secret;
use wmpass_core::otp::percent_decode;
use wmpass_net::Request;

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored, segment};

/// Suffix while uploading. Nextcloud 30 and later refuse `.part` names outright, which
/// shows up as a 400 on every upload while the connection test still passes.
const UPLOADING: &str = ".upload";

/// A WebDAV folder.
#[derive(Debug)]
pub struct WebDav<H> {
    client: Client<H>,
    base: String,
    authorization: Secret,
}

impl<H: AsyncHttp> WebDav<H> {
    /// A folder at `url`.
    pub fn new(client: Client<H>, url: &str, user: &str, password: &Secret) -> Self {
        let pair = format!("{user}:{}", password.expose());
        let encoded = base64::engine::general_purpose::STANDARD.encode(pair.as_bytes());
        Self {
            client,
            base: url.trim_end_matches('/').to_owned(),
            authorization: Secret::from(format!("Basic {encoded}")),
        }
    }

    fn request(&self, method: &'static str, url: String) -> Request {
        let mut request = Request::verb(method, url).header("Authorization", self.authorization.expose());
        request.timeout_ms = 120_000;
        request
    }

    fn url_of(&self, name: &str) -> String {
        format!("{}/{}", self.base, segment(name))
    }
}

/// Reads a PROPFIND answer: every `response` with its href and length. Tag prefixes vary
/// by server, so only local names are compared. DOCTYPE content is ignored, not processed.
pub fn parse_propfind(xml: &str) -> Vec<(String, u64, bool)> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let (mut href, mut size, mut is_dir) = (String::new(), 0u64, false);
    let mut current = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                let name = tag.local_name().as_ref().to_ascii_lowercase();
                if name == "response" {
                    href.clear();
                    size = 0;
                    is_dir = false;
                } else if name == "collection" {
                    is_dir = true;
                }
                current = name;
            }
            Ok(Event::Empty(tag)) => {
                if tag.local_name().as_ref().eq_ignore_ascii_case("collection") {
                    is_dir = true;
                }
            }
            // A reference such as `&amp;` arrives as its own event between runs of text.
            Ok(event @ (Event::Text(_) | Event::GeneralRef(_))) => {
                let value = crate::xml_text(&event);
                match current.as_str() {
                    "href" => href.push_str(&value),
                    "getcontentlength" => size = value.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
            Ok(Event::End(tag)) => {
                if tag.local_name().as_ref().eq_ignore_ascii_case("response") && !href.is_empty() {
                    out.push((std::mem::take(&mut href), size, is_dir));
                }
                current.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

impl<H: AsyncHttp> Sink for WebDav<H> {
    async fn readiness(&self) -> Result<()> {
        let mut probe = self.request("PROPFIND", format!("{}/", self.base));
        probe.headers.push(("Depth", "0".to_owned()));
        let response = self.client.send(probe).await?;
        match response.status {
            200..=299 => Ok(()),
            404 => {
                let made = self.client.send(self.request("MKCOL", format!("{}/", self.base))).await?;
                if (200..300).contains(&made.status) { Ok(()) } else { Err(crate::status_fault(made.status)) }
            }
            status => Err(crate::status_fault(status)),
        }
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let staged = self.url_of(&format!("{name}{UPLOADING}"));
        let target = self.url_of(name);
        let mut put = self.request("PUT", staged.clone());
        put.body = Some(("application/octet-stream", bytes.to_vec()));
        let mut moved = self.request("MOVE", staged.clone());
        moved.headers.push(("Destination", target.clone()));
        moved.headers.push(("Overwrite", "T".to_owned()));
        let done = match self.client.ok(put).await {
            Ok(_) => self.client.ok(moved).await.map(|_| ()),
            Err(fault) => Err(fault),
        };
        if let Err(fault) = done {
            // A half-written or unmoved upload is hidden from the listing but still takes
            // space, which is the very thing a full account is short of.
            if fault != Fault::Io {
                let _ = self.client.send(self.request("DELETE", staged)).await;
            }
            return Err(fault);
        }
        Ok(Stored { id: target, name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let mut request = self.request("PROPFIND", format!("{}/", self.base));
        request.headers.push(("Depth", "1".to_owned()));
        let response = self.client.ok(request).await?;
        let xml = std::str::from_utf8(&response.body).map_err(|_| Fault::Io)?;
        let mut out = Vec::new();
        for (href, size, is_dir) in parse_propfind(xml) {
            if is_dir {
                continue;
            }
            let trimmed = href.trim_end_matches('/');
            let name = percent_decode(trimmed.rsplit('/').next().unwrap_or(trimmed).replace('+', "%2B").as_str());
            if name.is_empty() || name.ends_with(UPLOADING) {
                continue;
            }
            out.push(Stored { id: self.url_of(&name), name, size });
        }
        Ok(out)
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        Ok(self.client.ok(self.request("GET", file.id.clone())).await?.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let response = self.client.send(self.request("DELETE", file.id.clone())).await?;
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

    const LISTING: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/wm/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
<d:response><d:href>/dav/wm/a%20b.bin</d:href><d:propstat><d:prop><d:getcontentlength>12</d:getcontentlength><d:resourcetype/></d:prop></d:propstat></d:response>
<D:response xmlns:D="DAV:"><D:href>/dav/wm/half.bin.upload</D:href><D:propstat><D:prop><D:getcontentlength>1</D:getcontentlength></D:prop></D:propstat></D:response></d:multistatus>"#;

    #[test]
    fn listing_reads_any_prefix_and_hides_uploads_and_folders() {
        let http = FakeHttp::new().answer("https://x.example/dav/wm/", 207, LISTING);
        let sink = WebDav::new(Client::new(Blocking(&http), NetPolicy::Any), "https://x.example/dav/wm/", "u", &"p".into());
        let files = block_on(sink.list()).unwrap();
        assert_eq!(files, vec![Stored {
            id: "https://x.example/dav/wm/a%20b.bin".into(),
            name: "a b.bin".into(),
            size: 12,
        }]);
    }

    #[test]
    fn write_stages_then_moves_and_never_uses_part() {
        let http = FakeHttp::new().answer("https://x.example/", 201, "");
        let sink = WebDav::new(Client::new(Blocking(&http), NetPolicy::Any), "https://x.example", "u", &"p".into());
        block_on(sink.write("v.bin", b"data")).unwrap();
        let asked = http.asked.lock().unwrap();
        assert_eq!(asked[0].method, "PUT");
        assert!(asked[0].url.ends_with("v.bin.upload"));
        assert_eq!(asked[1].method, "MOVE");
        assert!(asked[1].headers.iter().any(|(n, v)| *n == "Destination" && v.ends_with("/v.bin")));
        assert!(asked[0].headers.iter().any(|(n, v)| *n == "Authorization" && v == "Basic dTpw"));
    }

    #[test]
    fn an_upload_that_cannot_be_moved_is_removed_and_the_reason_named() {
        let staged = "https://x.example/v.bin.upload";
        let http = FakeHttp::new().answer_once(staged, 201, "").answer_once(staged, 507, "").answer("https://x.example/", 204, "");
        let sink = WebDav::new(Client::new(Blocking(&http), NetPolicy::Any), "https://x.example", "u", &"p".into());
        assert_eq!(block_on(sink.write("v.bin", b"data")), Err(Fault::OutOfSpace));
        let asked = http.asked.lock().unwrap();
        assert_eq!(asked.iter().map(|r| r.method).collect::<Vec<_>>(), ["PUT", "MOVE", "DELETE"]);
        assert_eq!(asked[2].url, staged);
    }

    #[test]
    fn a_name_with_an_escaped_character_is_listed_whole() {
        let xml = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/dav/Tom&amp;Jerry&#39;s.bin</d:href><d:propstat><d:prop><d:getcontentlength>3</d:getcontentlength></d:prop></d:propstat></d:response></d:multistatus>"#;
        assert_eq!(parse_propfind(xml), vec![("/dav/Tom&Jerry's.bin".to_owned(), 3, false)]);
    }

    #[test]
    fn plain_http_with_a_password_is_refused() {
        let http = FakeHttp::new();
        let sink = WebDav::new(Client::new(Blocking(&http), NetPolicy::Any), "http://nas.example/dav", "u", &"p".into());
        assert_eq!(block_on(sink.readiness()), Err(Fault::Blocked));
        assert!(http.asked.lock().unwrap().is_empty(), "the password must not leave at all");
    }

    #[test]
    fn missing_folder_is_created_and_refusal_is_named() {
        let made = FakeHttp::new().answer("https://x.example/dav/", 404, "");
        let sink = WebDav::new(Client::new(Blocking(&made), NetPolicy::Any), "https://x.example/dav", "u", &"p".into());
        assert_eq!(block_on(sink.readiness()), Err(Fault::TargetMissing));
        assert_eq!(made.asked.lock().unwrap()[1].method, "MKCOL");
        let refused = FakeHttp::new().answer("https://x.example/", 401, "");
        let sink = WebDav::new(Client::new(Blocking(&refused), NetPolicy::Any), "https://x.example/dav", "u", &"p".into());
        assert_eq!(block_on(sink.readiness()), Err(Fault::PermissionLost));
    }
}
