//! S3 and everything that speaks it: Backblaze, R2, Wasabi, MinIO. Requests are signed
//! with AWS Signature Version 4, written here against AWS's published example rather
//! than pulling in an SDK.

use hmac::{Hmac, KeyInit, Mac};
use quick_xml::Reader;
use quick_xml::events::Event;
use sha2::{Digest, Sha256};
use url::Url;
use wmpass_core::Secret;
use wmpass_core::vault::hex;
use wmpass_net::Request;

use crate::http::{AsyncHttp, Client};
use crate::{Fault, Result, Sink, Stored};

/// Where a bucket is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct S3Config {
    /// Service address, such as `https://s3.us-east-1.amazonaws.com`.
    pub endpoint: String,
    /// Region.
    pub region: String,
    /// Bucket.
    pub bucket: String,
    /// Key prefix, without a leading slash.
    pub prefix: String,
    /// Access key id.
    pub access_key: String,
}

/// An S3 bucket.
#[derive(Debug)]
pub struct S3<H> {
    client: Client<H>,
    config: S3Config,
    secret_key: Secret,
    now_ms: u64,
}

/// Encodes per AWS's rules: unreserved characters stay, `/` stays only in paths.
pub fn aws_encode(value: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
    // HMAC accepts any key length, so this cannot fail.
    let Ok(mut mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(key) else { return Vec::new() };
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// `YYYYMMDDTHHMMSSZ` for a moment. Civil-date arithmetic after Howard Hinnant.
pub fn amz_date(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// The inputs to one signature.
#[derive(Debug)]
pub struct Signing<'a> {
    /// Verb.
    pub method: &'a str,
    /// Encoded path.
    pub path: &'a str,
    /// Encoded query, sorted.
    pub query: &'a str,
    /// Header names, lower case and sorted, with their values.
    pub headers: &'a [(&'a str, &'a str)],
    /// Hex SHA-256 of the body.
    pub payload_hash: &'a str,
    /// `YYYYMMDDTHHMMSSZ`.
    pub amz_date: &'a str,
    /// Region.
    pub region: &'a str,
    /// Access key id.
    pub access_key: &'a str,
    /// Secret key.
    pub secret_key: &'a str,
}

/// The `Authorization` header for a request.
pub fn authorization(s: &Signing<'_>) -> String {
    let canonical_headers: String = s.headers.iter().map(|(k, v)| format!("{k}:{}\n", v.trim())).collect();
    let signed: Vec<&str> = s.headers.iter().map(|(k, _)| *k).collect();
    let signed = signed.join(";");
    let canonical = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed}\n{}",
        s.method, s.path, s.query, s.payload_hash
    );
    let date = &s.amz_date[..8];
    let scope = format!("{date}/{}/s3/aws4_request", s.region);
    let to_sign = format!("AWS4-HMAC-SHA256\n{}\n{scope}\n{}", s.amz_date, sha256_hex(canonical.as_bytes()));
    let k_date = mac(format!("AWS4{}", s.secret_key).as_bytes(), date.as_bytes());
    let k_region = mac(&k_date, s.region.as_bytes());
    let k_service = mac(&k_region, b"s3");
    let k_signing = mac(&k_service, b"aws4_request");
    let signature = hex(&mac(&k_signing, to_sign.as_bytes()));
    format!("AWS4-HMAC-SHA256 Credential={}/{scope},SignedHeaders={signed},Signature={signature}", s.access_key)
}

/// Reads ListObjectsV2: keys with sizes, and the continuation token if there is more.
pub fn parse_list(xml: &str) -> (Vec<(String, u64)>, Option<String>) {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut next = None;
    let (mut key, mut size) = (String::new(), 0u64);
    let mut current = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                current = tag.local_name().as_ref().to_owned();
                if current == "Contents" {
                    key.clear();
                    size = 0;
                }
            }
            Ok(Event::Text(text)) => {
                let value = quick_xml::escape::unescape(&text).map(|v| v.into_owned()).unwrap_or_default();
                match current.as_str() {
                    "Key" => key.push_str(&value),
                    "Size" => size = value.trim().parse().unwrap_or(0),
                    "NextContinuationToken" => next = Some(value),
                    _ => {}
                }
            }
            Ok(Event::End(tag)) => {
                if tag.local_name().as_ref() == "Contents" && !key.is_empty() {
                    out.push((std::mem::take(&mut key), size));
                }
                current.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (out, next)
}

impl<H: AsyncHttp> S3<H> {
    /// A bucket. `now_ms` goes into signatures, and a skewed clock gets refused.
    pub fn new(client: Client<H>, config: S3Config, secret_key: Secret, now_ms: u64) -> Self {
        Self { client, config, secret_key, now_ms }
    }

    fn key_of(&self, name: &str) -> String {
        let prefix = self.config.prefix.trim_matches('/');
        if prefix.is_empty() { name.to_owned() } else { format!("{prefix}/{name}") }
    }

    fn signed(&self, method: &'static str, key: &str, query: &str, body: Option<Vec<u8>>) -> Result<Request> {
        let base = Url::parse(&self.config.endpoint).map_err(|_| Fault::NotConfigured)?;
        let host = match (base.host_str(), base.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            _ => return Err(Fault::NotConfigured),
        };
        let path = format!("/{}/{}", aws_encode(&self.config.bucket, false), aws_encode(key, true));
        let path = if key.is_empty() { format!("/{}", aws_encode(&self.config.bucket, false)) } else { path };
        let payload = body.as_deref().unwrap_or(&[]);
        let payload_hash = sha256_hex(payload);
        let date = amz_date(self.now_ms);
        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", payload_hash.as_str()),
            ("x-amz-date", date.as_str()),
        ];
        let auth = authorization(&Signing {
            method,
            path: &path,
            query,
            headers: &headers,
            payload_hash: &payload_hash,
            amz_date: &date,
            region: &self.config.region,
            access_key: &self.config.access_key,
            secret_key: self.secret_key.expose(),
        });
        let origin = format!("{}://{host}", base.scheme());
        let url = if query.is_empty() { format!("{origin}{path}") } else { format!("{origin}{path}?{query}") };
        let mut request = Request::verb(method, url)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", date)
            .header("Authorization", auth);
        request.timeout_ms = 120_000;
        request.body = body.map(|bytes| ("application/octet-stream", bytes));
        Ok(request)
    }
}

impl<H: AsyncHttp> Sink for S3<H> {
    async fn readiness(&self) -> Result<()> {
        let request = self.signed("GET", "", "list-type=2&max-keys=1", None)?;
        self.client.ok(request).await.map(|_| ())
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let key = self.key_of(name);
        let request = self.signed("PUT", &key, "", Some(bytes.to_vec()))?;
        self.client.ok(request).await?;
        Ok(Stored { id: key, name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let prefix = self.config.prefix.trim_matches('/');
        let prefix = if prefix.is_empty() { String::new() } else { format!("{prefix}/") };
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query = Vec::new();
            if let Some(token) = &token {
                query.push(format!("continuation-token={}", aws_encode(token, false)));
            }
            query.push("list-type=2".to_owned());
            if !prefix.is_empty() {
                query.push(format!("prefix={}", aws_encode(&prefix, false)));
            }
            let request = self.signed("GET", "", &query.join("&"), None)?;
            let response = self.client.ok(request).await?;
            let xml = std::str::from_utf8(&response.body).map_err(|_| Fault::Io)?;
            let (files, next) = parse_list(xml);
            for (key, size) in files {
                let name = key.strip_prefix(&prefix).unwrap_or(&key).to_owned();
                if !name.is_empty() && !name.contains('/') {
                    out.push(Stored { id: key, name, size });
                }
            }
            match next {
                Some(more) => token = Some(more),
                None => return Ok(out),
            }
        }
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let request = self.signed("GET", &file.id, "", None)?;
        Ok(self.client.ok(request).await?.body)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        let request = self.signed("DELETE", &file.id, "", None)?;
        let response = self.client.send(request).await?;
        match response.status {
            200..=299 | 404 => Ok(()),
            status => Err(crate::status_fault(status)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_awss_published_example() {
        // "Example: GET Object" in the Amazon S3 API Reference, Signature Version 4.
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let headers = [
            ("host", "examplebucket.s3.amazonaws.com"),
            ("range", "bytes=0-9"),
            ("x-amz-content-sha256", empty),
            ("x-amz-date", "20130524T000000Z"),
        ];
        let header = authorization(&Signing {
            method: "GET",
            path: "/test.txt",
            query: "",
            headers: &headers,
            payload_hash: empty,
            amz_date: "20130524T000000Z",
            region: "us-east-1",
            access_key: "AKIAIOSFODNN7EXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        });
        assert_eq!(
            header,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,\
SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,\
Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn dates_format_correctly() {
        assert_eq!(amz_date(1_369_353_600_000), "20130524T000000Z");
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_709_210_096_000), "20240229T123456Z");
    }

    #[test]
    fn listing_and_encoding() {
        let xml = "<ListBucketResult><Contents><Key>p/a.bin</Key><Size>5</Size></Contents><IsTruncated>true</IsTruncated><NextContinuationToken>t+1</NextContinuationToken></ListBucketResult>";
        assert_eq!(parse_list(xml), (vec![("p/a.bin".to_owned(), 5)], Some("t+1".to_owned())));
        assert_eq!(aws_encode("a b/c+d~", true), "a%20b/c%2Bd~");
        assert_eq!(aws_encode("a/b", false), "a%2Fb");
    }
}
