//! The async seam, with the same policy check as every other request.

use std::future::Future;

use wmpass_net::http::{self, NetError, NetPolicy, Request, Response};

use crate::{Fault, Result};

/// Sends requests, possibly over `fetch`.
pub trait AsyncHttp {
    /// Sends one request.
    fn send(&self, request: &Request) -> impl Future<Output = std::result::Result<Response, NetError>>;
}

/// Adapts a blocking client. Its futures are ready at once.
#[allow(missing_debug_implementations)]
pub struct Blocking<'a>(pub &'a dyn http::Http);

impl AsyncHttp for Blocking<'_> {
    async fn send(&self, request: &Request) -> std::result::Result<Response, NetError> {
        self.0.send(request)
    }
}

/// A client plus the policy it must obey. Destinations hold one of these, so none can
/// skip the check.
#[derive(Debug, Clone, Copy)]
pub struct Client<H> {
    http: H,
    policy: NetPolicy,
}

impl<H: AsyncHttp> Client<H> {
    /// Pairs a client with a policy.
    pub fn new(http: H, policy: NetPolicy) -> Self {
        Self { http, policy }
    }

    pub(crate) async fn send(&self, request: Request) -> Result<Response> {
        http::check(&request, self.policy).map_err(|error| match error {
            NetError::Blocked | NetError::Insecure => Fault::Blocked,
            _ => Fault::NotConfigured,
        })?;
        self.http.send(&request).await.map_err(|_| Fault::Io)
    }

    /// Sends and refuses anything but success.
    pub(crate) async fn ok(&self, request: Request) -> Result<Response> {
        let response = self.send(request).await?;
        if (200..300).contains(&response.status) {
            Ok(response)
        } else {
            Err(crate::status_fault(response.status))
        }
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
