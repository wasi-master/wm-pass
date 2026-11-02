//! The one seam to the network, and the one policy every request passes.

use url::Url;

/// Why a request did not produce a usable answer. Never holds request contents, so
/// an error shown or logged cannot leak a key or an email address.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum NetError {
    /// Offline mode, or Wi-Fi only without Wi-Fi.
    #[error("this needs the network, and network use is off")]
    Blocked,
    /// A plain http address outside this device.
    #[error("that service must use https")]
    Insecure,
    /// No connection, or a timeout.
    #[error("could not reach the service")]
    Unreachable,
    /// The key was refused.
    #[error("the service refused the key")]
    Unauthorised,
    /// Too many requests. Holds the seconds to wait, if the service said.
    #[error("too many requests, try again later")]
    RateLimited(Option<u64>),
    /// Any other error status.
    #[error("the service answered with an error ({0})")]
    Status(u16),
    /// The answer was not what the service documents.
    #[error("the service's answer could not be read")]
    BadAnswer,
    /// Input that must not be sent.
    #[error("{0}")]
    Invalid(String),
}

/// Whether network use is allowed at all. Passed to every client, so no feature can
/// forget to check it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetPolicy {
    /// Nothing leaves the device. The default.
    #[default]
    Offline,
    /// Only while on an unmetered network.
    WifiOnly {
        /// What the platform reports right now.
        on_wifi: bool,
    },
    /// Any connection.
    Any,
}

/// An outgoing request.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// Verb.
    pub method: &'static str,
    /// Full address.
    pub url: String,
    /// Headers. API keys go here.
    pub headers: Vec<(&'static str, String)>,
    /// Body and its type.
    pub body: Option<(&'static str, Vec<u8>)>,
    /// Time limit.
    pub timeout_ms: u64,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Addresses can hold emails and headers hold keys, so neither is printed.
        f.debug_struct("Request")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl Request {
    /// A GET.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET",
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout_ms: 15_000,
        }
    }

    /// A body-carrying request with JSON.
    pub fn json(method: &'static str, url: impl Into<String>, body: &serde_json::Value) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Some(("application/json", body.to_string().into_bytes())),
            timeout_ms: 15_000,
        }
    }

    /// A body-less request with another verb.
    pub fn verb(method: &'static str, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout_ms: 15_000,
        }
    }

    /// Adds a header.
    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

/// An answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// `Retry-After`, in seconds, when present.
    pub retry_after: Option<u64>,
    /// Body.
    pub body: Vec<u8>,
}

impl Response {
    /// Body as JSON.
    pub fn json(&self) -> Result<serde_json::Value, NetError> {
        serde_json::from_slice(&self.body).map_err(|_| NetError::BadAnswer)
    }

    /// Body as text.
    pub fn text(&self) -> Result<&str, NetError> {
        std::str::from_utf8(&self.body).map_err(|_| NetError::BadAnswer)
    }

    /// Turns the statuses every service means the same by into errors.
    pub fn ok(&self) -> Result<&Self, NetError> {
        match self.status {
            200..=299 => Ok(self),
            401 | 403 => Err(NetError::Unauthorised),
            429 => Err(NetError::RateLimited(self.retry_after)),
            status => Err(NetError::Status(status)),
        }
    }
}

/// A request and how to read its answer.
pub struct Call<T> {
    /// What to send.
    pub request: Request,
    /// Reads the answer.
    pub parse: Box<dyn FnOnce(&Response) -> Result<T, NetError> + Send>,
}

impl<T> std::fmt::Debug for Call<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Call")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

impl<T> Call<T> {
    /// Pairs a request with its reader.
    pub fn new(
        request: Request,
        parse: impl FnOnce(&Response) -> Result<T, NetError> + Send + 'static,
    ) -> Self {
        Self {
            request,
            parse: Box::new(parse),
        }
    }

    /// Sends it and reads the answer.
    pub fn run(self, http: &dyn Http, policy: NetPolicy) -> Result<T, NetError> {
        check(&self.request, policy)?;
        let response = http.send(&self.request)?;
        (self.parse)(&response)
    }
}

fn is_local(host: &str) -> bool {
    host == "localhost"
        || host == "[::1]"
        || host == "::1"
        || host.starts_with("127.")
        || host.ends_with(".local")
}

/// The rule every request passes. A browser or platform client must call this too.
pub fn check(request: &Request, policy: NetPolicy) -> Result<(), NetError> {
    let allowed = match policy {
        NetPolicy::Offline => false,
        NetPolicy::WifiOnly { on_wifi } => on_wifi,
        NetPolicy::Any => true,
    };
    if !allowed {
        return Err(NetError::Blocked);
    }
    let url = Url::parse(&request.url)
        .map_err(|_| NetError::Invalid("that address is not valid".into()))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if url.host_str().is_some_and(is_local) => Ok(()),
        _ => Err(NetError::Insecure),
    }
}

/// Sends requests. Clients must not follow redirects: a key sent to a service must
/// never travel on to wherever that service points.
pub trait Http: Send + Sync {
    /// Sends one request.
    fn send(&self, request: &Request) -> Result<Response, NetError>;
}

/// A blocking client over rustls.
#[cfg(feature = "native")]
#[derive(Debug)]
pub struct NativeHttp {
    agent: ureq::Agent,
}

#[cfg(feature = "native")]
impl Default for NativeHttp {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent("WM-Pass")
            .build();
        Self {
            agent: config.into(),
        }
    }
}

#[cfg(feature = "native")]
impl Http for NativeHttp {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        const MAX_BODY: u64 = 16 * 1024 * 1024;
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(&request.url);
        for (name, value) in &request.headers {
            builder = builder.header(*name, value.as_str());
        }
        if let Some((kind, _)) = &request.body {
            builder = builder.header("content-type", *kind);
        }
        let body = request
            .body
            .as_ref()
            .map_or(&[][..], |(_, bytes)| bytes.as_slice());
        let built = builder
            .body(body)
            .map_err(|_| NetError::Invalid("that request is not valid".into()))?;
        let timeout = std::time::Duration::from_millis(request.timeout_ms);
        let configured = self
            .agent
            .configure_request(built)
            .timeout_global(Some(timeout))
            .build();
        let mut answer = self
            .agent
            .run(configured)
            .map_err(|_| NetError::Unreachable)?;
        let status = answer.status().as_u16();
        let retry_after = answer
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok());
        let body = answer
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|_| NetError::Unreachable)?;
        Ok(Response {
            status,
            retry_after,
            body,
        })
    }
}

/// Canned answers, for tests of this crate and others.
pub mod fake {
    use std::sync::Mutex;

    use super::{Http, NetError, Request, Response};

    /// Answers by address prefix and records what was asked.
    #[derive(Debug, Default)]
    pub struct FakeHttp {
        answers: Vec<(String, u16, Vec<u8>)>,
        /// Every request made.
        pub asked: Mutex<Vec<Request>>,
    }

    impl FakeHttp {
        /// A client that knows nothing yet.
        pub fn new() -> Self {
            Self::default()
        }

        /// Adds an answer. The first matching prefix wins.
        #[must_use]
        pub fn answer(mut self, prefix: &str, status: u16, body: &str) -> Self {
            self.answers
                .push((prefix.to_owned(), status, body.as_bytes().to_vec()));
            self
        }

        /// The most recent request.
        pub fn last(&self) -> Option<Request> {
            self.asked
                .lock()
                .ok()
                .and_then(|asked| asked.last().cloned())
        }
    }

    impl Http for FakeHttp {
        fn send(&self, request: &Request) -> Result<Response, NetError> {
            if let Ok(mut asked) = self.asked.lock() {
                asked.push(request.clone());
            }
            self.answers
                .iter()
                .find(|(prefix, _, _)| request.url.starts_with(prefix.as_str()))
                .map(|(_, status, body)| Response {
                    status: *status,
                    retry_after: None,
                    body: body.clone(),
                })
                .ok_or(NetError::Unreachable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHttp;
    use super::*;

    fn call(url: &str) -> Call<u16> {
        Call::new(Request::get(url), |response| Ok(response.status))
    }

    #[test]
    fn offline_sends_nothing_at_all() {
        let http = FakeHttp::new().answer("https://x.example", 200, "");
        let result = call("https://x.example/").run(&http, NetPolicy::Offline);
        assert_eq!(result, Err(NetError::Blocked));
        assert!(http.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn wifi_only_follows_the_platform() {
        let http = FakeHttp::new().answer("https://x.example", 200, "");
        assert_eq!(
            call("https://x.example/").run(&http, NetPolicy::WifiOnly { on_wifi: false }),
            Err(NetError::Blocked)
        );
        assert_eq!(
            call("https://x.example/").run(&http, NetPolicy::WifiOnly { on_wifi: true }),
            Ok(200)
        );
    }

    #[test]
    fn plain_http_only_stays_on_this_device() {
        let http = FakeHttp::new().answer("http://", 200, "");
        assert_eq!(
            call("http://x.example/").run(&http, NetPolicy::Any),
            Err(NetError::Insecure)
        );
        assert_eq!(
            call("ftp://x.example/").run(&http, NetPolicy::Any),
            Err(NetError::Insecure)
        );
        assert_eq!(
            call("http://localhost:8080/").run(&http, NetPolicy::Any),
            Ok(200)
        );
        assert_eq!(
            call("http://127.0.0.1/").run(&http, NetPolicy::Any),
            Ok(200)
        );
    }

    #[test]
    fn debug_output_hides_keys_and_addresses() {
        let request = Request::get("https://x.example/me@example.com")
            .header("authorization", "Bearer key123");
        let printed = format!("{request:?}");
        assert!(!printed.contains("key123") && !printed.contains("me@example.com"));
    }

    #[test]
    fn common_statuses_map_to_errors() {
        let status = |status| Response {
            status,
            ..Response::default()
        };
        assert!(status(204).ok().is_ok());
        assert_eq!(status(401).ok().err(), Some(NetError::Unauthorised));
        assert_eq!(status(500).ok().err(), Some(NetError::Status(500)));
        let limited = Response {
            status: 429,
            retry_after: Some(6),
            body: Vec::new(),
        };
        assert_eq!(limited.ok().err(), Some(NetError::RateLimited(Some(6))));
    }
}
