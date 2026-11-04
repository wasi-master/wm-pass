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
    /// The TLS certificate was refused: none trusted vouches for it, or it is not the
    /// one pinned for this server. Not transient, and not a password problem.
    #[error("the service's certificate is not the trusted one")]
    Untrusted,
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
    /// Headers, names in lower case. Only what a protocol needs reads them: cookies and
    /// session tokens for the services that sign in with a password.
    pub headers: Vec<(String, String)>,
}

impl Response {
    /// The first header of that name, ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Every header of that name, such as each `Set-Cookie`.
    pub fn headers_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

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

type Parser<T> = Box<dyn FnOnce(&Response) -> Result<T, NetError> + Send>;

/// A request and how to read its answer.
pub struct Call<T> {
    /// What to send.
    pub request: Request,
    /// Reads the answer.
    pub parse: Parser<T>,
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

    /// The same, over a client that may wait, such as the browser's `fetch`.
    pub async fn run_async(self, http: &impl AsyncHttp, policy: NetPolicy) -> Result<T, NetError> {
        check(&self.request, policy)?;
        let response = http.send(&self.request).await?;
        (self.parse)(&response)
    }
}

/// Where plain http is acceptable, because nothing readable crosses a network: this
/// device, or Tailscale, which encrypts below this layer. `.local` names are deliberately
/// not here. Anyone on the same network can answer for one, and would be handed the
/// credentials in the clear. Compared as parsed addresses, since a text prefix check
/// would also accept a domain such as `127.example.com`.
fn is_local(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost") || name.ends_with(".ts.net")
        }
        url::Host::Ipv4(ip) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback() || (a == 100 && (64..128).contains(&b))
        }
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
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
        "http" if url.host().is_some_and(|host| is_local(&host)) => Ok(()),
        _ => Err(NetError::Insecure),
    }
}

/// The rule for a raw connection (FTP, SFTP, SMB, NFS, IMAP), which has no URL to judge.
/// Only the policy applies; whether the protocol encrypts is the destination's own rule.
pub fn check_socket(policy: NetPolicy) -> Result<(), NetError> {
    match policy {
        NetPolicy::Any | NetPolicy::WifiOnly { on_wifi: true } => Ok(()),
        NetPolicy::Offline | NetPolicy::WifiOnly { on_wifi: false } => Err(NetError::Blocked),
    }
}

/// Sends requests. Clients must not follow redirects: a key sent to a service must
/// never travel on to wherever that service points.
pub trait Http: Send + Sync {
    /// Sends one request.
    fn send(&self, request: &Request) -> Result<Response, NetError>;
}

/// Sends requests, possibly over `fetch`. The same rule about redirects applies.
pub trait AsyncHttp {
    /// Sends one request.
    fn send(&self, request: &Request) -> impl Future<Output = Result<Response, NetError>>;
}

/// Adapts a blocking client. Its futures are ready at once.
#[allow(missing_debug_implementations)]
pub struct Blocking<'a>(pub &'a dyn Http);

impl AsyncHttp for Blocking<'_> {
    async fn send(&self, request: &Request) -> Result<Response, NetError> {
        self.0.send(request)
    }
}

/// Runs a future built on a blocking client.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
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
        Self {
            agent: agent_config().into(),
        }
    }
}

#[cfg(feature = "native")]
impl Http for NativeHttp {
    fn send(&self, request: &Request) -> Result<Response, NetError> {
        send_with(&self.agent, request)
    }
}

/// The agent settings both native clients share: no redirects, any verb, statuses as answers.
#[cfg(feature = "native")]
fn agent_config() -> ureq::config::Config {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .allow_non_standard_methods(true)
        .http_status_as_error(false)
        .user_agent("WM-Pass")
        .build()
}

/// Whether a failure was the certificate being refused, wherever in the chain rustls
/// reported it: directly, or inside the I/O error a handshake returns.
#[cfg(feature = "native")]
fn is_certificate_refusal(error: &ureq::Error) -> bool {
    let refused = |inner: &rustls::Error| matches!(inner, rustls::Error::InvalidCertificate(_));
    match error {
        ureq::Error::Rustls(inner) => refused(inner),
        ureq::Error::Io(io) => io
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
            .is_some_and(refused),
        _ => false,
    }
}

#[cfg(feature = "native")]
fn send_with(agent: &ureq::Agent, request: &Request) -> Result<Response, NetError> {
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
    let configured = agent
        .configure_request(built)
        .timeout_global(Some(timeout))
        .build();
    let mut answer = agent.run(configured).map_err(|error| {
        if is_certificate_refusal(&error) {
            NetError::Untrusted
        } else {
            NetError::Unreachable
        }
    })?;
    let status = answer.status().as_u16();
    let headers = answer
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_ascii_lowercase(), value.to_str().ok()?.to_owned())))
        .collect();
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
        headers,
    })
}

#[cfg(feature = "native")]
pub use pinned::{PinSeen, PinnedHttp, certificate_pin};

/// Trust on first use for a server's own certificate, the kind almost every NAS on a
/// home network makes for itself and no authority vouches for.
#[cfg(feature = "native")]
mod pinned {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
    use sha2::{Digest, Sha256};
    use ureq::unversioned::resolver::DefaultResolver;
    use ureq::unversioned::transport::{
        Buffers, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout, TcpConnector, Transport,
        TransportAdapter,
    };

    use super::{Http, NetError, Request, Response};

    /// Called once, with the SHA-256 of the certificate the first connection saw.
    pub type PinSeen = Box<dyn Fn([u8; 32]) + Send + Sync>;

    /// SHA-256 of a certificate's DER encoding: what a pin holds.
    pub fn certificate_pin(der: &[u8]) -> [u8; 32] {
        Sha256::digest(der).into()
    }

    struct Pin {
        /// None until a connection has seen one.
        expected: Mutex<Option<[u8; 32]>>,
        on_first_seen: PinSeen,
    }

    impl std::fmt::Debug for Pin {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Pin").finish_non_exhaustive()
        }
    }

    /// Accepts exactly one certificate. The name on it is not checked: the pin is the
    /// whole certificate, and a NAS's own one names whatever the box was called the day
    /// it made it, rarely the address typed in. The signature over the handshake still
    /// is, so only whoever holds that certificate's key can answer.
    #[derive(Debug)]
    struct PinVerifier {
        pin: Arc<Pin>,
        algorithms: rustls::crypto::WebPkiSupportedAlgorithms,
    }

    impl ServerCertVerifier for PinVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            let digest = certificate_pin(end_entity.as_ref());
            let mut expected = self
                .pin
                .expected
                .lock()
                .map_err(|_| rustls::Error::General("pin lock".into()))?;
            match *expected {
                None => {
                    *expected = Some(digest);
                    (self.pin.on_first_seen)(digest);
                    Ok(ServerCertVerified::assertion())
                }
                Some(pinned) if pinned == digest => Ok(ServerCertVerified::assertion()),
                Some(_) => Err(rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)),
            }
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.algorithms.supported_schemes()
        }
    }

    /// Wraps the TCP connection in TLS checked against the pin.
    #[derive(Debug)]
    struct PinnedConnector {
        config: Arc<ClientConfig>,
    }

    impl<In: Transport> Connector<In> for PinnedConnector {
        type Out = Either<In, PinnedTransport>;

        fn connect(&self, details: &ConnectionDetails, chained: Option<In>) -> Result<Option<Self::Out>, ureq::Error> {
            let Some(transport) = chained else {
                return Err(ureq::Error::Tls("no connection to wrap"));
            };
            if !details.needs_tls() || transport.is_tls() {
                return Ok(Some(Either::A(transport)));
            }
            let host = details
                .uri
                .host()
                .ok_or(ureq::Error::Tls("no host"))?
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned();
            let name = ServerName::try_from(host).map_err(|_| ureq::Error::Tls("not a host name"))?;
            let mut connection = ClientConnection::new(self.config.clone(), name)?;
            let mut socket = TransportAdapter::new(transport.boxed(), details.current_time.clone());
            socket.set_timeout(details.timeout);
            connection.complete_io(&mut socket)?;
            let buffers = LazyBuffers::new(details.config.input_buffer_size(), details.config.output_buffer_size());
            Ok(Some(Either::B(PinnedTransport {
                buffers,
                stream: StreamOwned { conn: connection, sock: socket },
            })))
        }
    }

    struct PinnedTransport {
        buffers: LazyBuffers,
        stream: StreamOwned<ClientConnection, TransportAdapter>,
    }

    impl std::fmt::Debug for PinnedTransport {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PinnedTransport").finish_non_exhaustive()
        }
    }

    impl Transport for PinnedTransport {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }

        fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
            self.stream.get_mut().set_timeout(timeout);
            let output = &self.buffers.output()[..amount];
            self.stream.write_all(output)?;
            Ok(())
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
            self.stream.get_mut().set_timeout(timeout);
            let input = self.buffers.input_append_buf();
            let amount = self.stream.read(input)?;
            self.buffers.input_appended(amount);
            Ok(amount > 0)
        }

        fn is_open(&mut self) -> bool {
            self.stream.get_mut().get_mut().is_open()
        }

        fn is_tls(&self) -> bool {
            true
        }
    }

    /// A blocking client that trusts one certificate, recorded the first time one is seen
    /// when there is no pin yet. Otherwise the same as [`super::NativeHttp`]: no redirects,
    /// and the same policy check by whoever holds it.
    pub struct PinnedHttp {
        agent: ureq::Agent,
    }

    impl std::fmt::Debug for PinnedHttp {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PinnedHttp").finish_non_exhaustive()
        }
    }

    impl PinnedHttp {
        /// Trusts `expected`, or the first certificate seen when it is None, which
        /// `on_first_seen` is then told about to store. Fails only when the TLS library
        /// offers no protocol version, which is a build problem.
        pub fn new(expected: Option<[u8; 32]>, on_first_seen: PinSeen) -> Result<Self, NetError> {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let verifier = PinVerifier {
                pin: Arc::new(Pin {
                    expected: Mutex::new(expected),
                    on_first_seen,
                }),
                algorithms: provider.signature_verification_algorithms,
            };
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|_| NetError::Invalid("no TLS version is available".into()))?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier))
                .with_no_client_auth();
            let connector = PinnedConnector { config: Arc::new(config) };
            let agent = ureq::Agent::with_parts(
                super::agent_config(),
                TcpConnector::default().chain(connector),
                DefaultResolver::default(),
            );
            Ok(Self { agent })
        }
    }

    impl Http for PinnedHttp {
        fn send(&self, request: &Request) -> Result<Response, NetError> {
            super::send_with(&self.agent, request)
        }
    }
}

/// Canned answers, for tests of this crate and others.
pub mod fake {
    use std::sync::Mutex;

    use super::{Http, NetError, Request, Response};

    #[derive(Debug)]
    struct Canned {
        prefix: String,
        response: Response,
        /// Answers one request, then is gone, so a flow can see a different answer the
        /// second time it asks the same address.
        once: bool,
    }

    /// Answers by address prefix and records what was asked.
    #[derive(Debug, Default)]
    pub struct FakeHttp {
        answers: Mutex<Vec<Canned>>,
        /// Every request made.
        pub asked: Mutex<Vec<Request>>,
    }

    impl FakeHttp {
        /// A client that knows nothing yet.
        pub fn new() -> Self {
            Self::default()
        }

        fn push(self, prefix: &str, status: u16, body: &[u8], headers: &[(&str, &str)], once: bool) -> Self {
            let response = Response {
                status,
                retry_after: None,
                body: body.to_vec(),
                headers: headers
                    .iter()
                    .map(|(name, value)| (name.to_ascii_lowercase(), (*value).to_owned()))
                    .collect(),
            };
            if let Ok(mut answers) = self.answers.lock() {
                answers.push(Canned { prefix: prefix.to_owned(), response, once });
            }
            self
        }

        /// Adds an answer. The first matching prefix wins.
        #[must_use]
        pub fn answer(self, prefix: &str, status: u16, body: &str) -> Self {
            self.push(prefix, status, body.as_bytes(), &[], false)
        }

        /// Adds an answer with headers, such as a `Location` or a `Content-Type`.
        #[must_use]
        pub fn answer_with(self, prefix: &str, status: u16, body: &[u8], headers: &[(&str, &str)]) -> Self {
            self.push(prefix, status, body, headers, false)
        }

        /// Adds an answer that is used for one request and then dropped. Put it before
        /// the lasting answer for the same prefix.
        #[must_use]
        pub fn answer_once(self, prefix: &str, status: u16, body: &str) -> Self {
            self.push(prefix, status, body.as_bytes(), &[], true)
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
            let mut answers = self.answers.lock().map_err(|_| NetError::Unreachable)?;
            let index = answers
                .iter()
                .position(|canned| request.url.starts_with(canned.prefix.as_str()))
                .ok_or(NetError::Unreachable)?;
            if answers[index].once {
                Ok(answers.remove(index).response)
            } else {
                Ok(answers[index].response.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHttp;
    use super::*;

    #[test]
    fn plain_http_only_where_nothing_readable_crosses_a_network() {
        let allowed = |url: &str| check(&Request::get(url), NetPolicy::Any).is_ok();
        for good in [
            "http://localhost:80/",
            "http://LOCALHOST/",
            "http://app.localhost/",
            "http://127.0.0.1:9/",
            "http://127.8.8.8/",
            "http://[::1]:9/",
            "http://100.64.0.1/",
            "http://100.127.255.254/",
            "http://box.tail1234.ts.net/",
            "https://anything.example/",
        ] {
            assert!(allowed(good), "{good}");
        }
        for bad in [
            "http://127.example.com/",
            "http://127.0.0.1.example.com/",
            "http://localhost.example.com/",
            "http://nas.local/",
            "http://192.168.1.10/",
            "http://10.0.0.1/",
            "http://100.63.0.1/",
            "http://100.128.0.1/",
            "http://example.ts.net.evil.com/",
            "ftp://localhost/",
        ] {
            assert!(!allowed(bad), "{bad}");
        }
    }

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
            ..Response::default()
        };
        assert_eq!(limited.ok().err(), Some(NetError::RateLimited(Some(6))));
    }
}

#[cfg(test)]
#[cfg(feature = "native")]
mod pinned_tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    use super::*;

    const CERT_A: &[u8] = include_bytes!("../testdata/cert-a.der");
    const KEY_A: &[u8] = include_bytes!("../testdata/key-a.der");
    const CERT_B: &[u8] = include_bytes!("../testdata/cert-b.der");
    const KEY_B: &[u8] = include_bytes!("../testdata/key-b.der");

    /// A TLS server on this device that answers every request with `ok`.
    fn serve(cert: &'static [u8], key: &'static [u8]) -> String {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
            )
            .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for socket in listener.incoming().flatten() {
                let Ok(connection) = rustls::ServerConnection::new(config.clone()) else { continue };
                let mut tls = rustls::StreamOwned::new(connection, socket);
                let mut seen = Vec::new();
                let mut buffer = [0u8; 1024];
                while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => seen.extend_from_slice(&buffer[..n]),
                    }
                }
                let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });
        format!("https://127.0.0.1:{port}/")
    }

    #[test]
    fn the_first_certificate_is_recorded_and_then_only_it_is_trusted() {
        let url = serve(CERT_A, KEY_A);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let http = PinnedHttp::new(None, Box::new(move |pin| record.lock().unwrap().push(pin))).unwrap();
        assert_eq!(http.send(&Request::get(&url)).unwrap().body, b"ok");
        assert_eq!(http.send(&Request::get(&url)).unwrap().status, 200);
        assert_eq!(*seen.lock().unwrap(), vec![certificate_pin(CERT_A)], "recorded once, on first use");

        let pinned = PinnedHttp::new(Some(certificate_pin(CERT_A)), Box::new(|_| panic!("already pinned"))).unwrap();
        assert_eq!(pinned.send(&Request::get(&url)).unwrap().body, b"ok");
    }

    #[test]
    fn a_changed_certificate_is_refused_as_untrusted() {
        let url = serve(CERT_B, KEY_B);
        let pinned = PinnedHttp::new(Some(certificate_pin(CERT_A)), Box::new(|_| panic!("already pinned"))).unwrap();
        assert_eq!(pinned.send(&Request::get(&url)).err(), Some(NetError::Untrusted));
    }

    #[test]
    fn the_ordinary_client_refuses_a_self_made_certificate_as_untrusted() {
        let url = serve(CERT_A, KEY_A);
        assert_eq!(NativeHttp::default().send(&Request::get(&url)).err(), Some(NetError::Untrusted));
    }
}
