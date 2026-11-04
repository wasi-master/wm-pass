//! The async seam, with the same policy check as every other request.

use wmpass_net::http::{self, NetError, NetPolicy, Request, Response};

pub use wmpass_net::http::{AsyncHttp, Blocking, block_on};

use crate::{Fault, Result};

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
        self.http.send(&request).await.map_err(|error| match error {
            // A certificate refused is not a bad minute: the server is not the one first seen.
            NetError::Untrusted => Fault::Unsafe,
            _ => Fault::Io,
        })
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
