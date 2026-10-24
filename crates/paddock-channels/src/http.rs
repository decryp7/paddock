//! The port to the network for webhook and ntfy: one POST. [`EgressPoster`] is the real one, on `paddock-net`.

use paddock_download::{Header, TransportError, TransportErrorKind, parse_retry_after};
use paddock_jobs::CancelToken;
use paddock_net::{Egress, NetError, Policy, Reach, Request};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Longest header value sent (an `Authorization` token is the longest in practice).
pub const MAX_HEADER_VALUE: usize = 8 * 1024;

/// One POST. Contract for implementations: send exactly these headers and body, never follow a redirect (the answer of the
/// first hop is returned, so a 3xx counts as a rejection and neither the message nor the token reaches another host), give up
/// after `timeout` in total, return any status as a [`PostResponse`] (only a failure without a status line is an error), and
/// do not read the answer's body. Called from several threads at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRequest {
    /// The address (http or https).
    pub url: String,
    /// The headers; `Debug` shows names only because values are often secrets.
    pub headers: Vec<Header>,
    /// The body, already bounded.
    pub body: Vec<u8>,
    /// The longest the whole exchange may take.
    pub timeout: Duration,
}

/// The status and the one header the dispatcher uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostResponse {
    /// The HTTP status.
    pub status: u16,
    /// What `Retry-After` asked for, if it was sent and could be read.
    pub retry_after: Option<Duration>,
}

/// The network port for POST requests. See [`PostRequest`] for the contract.
pub trait HttpPoster: Send + Sync {
    /// Sends the request.
    fn post(&self, request: &PostRequest) -> Result<PostResponse, TransportError>;
}

/// The policy for the network calls of notification channels: https only (plain http to this computer when `reach` is
/// [`Reach::Loopback`]), no redirects (a redirect could leak the message or token to another host, so it counts as a
/// rejection), the given reach (a webhook on the local network needs [`Reach::Lan`], a test against this computer
/// [`Reach::Loopback`]).
pub fn channel_policy(reach: Reach) -> Policy {
    Policy {
        user_agent: concat!("paddock-channels/", env!("CARGO_PKG_VERSION")).to_string(),
        ..Policy::default().reach(reach).max_redirects(0)
    }
}

/// [`HttpPoster`] over the app's [`Egress`]: every request obeys the family's outbound policy (reach, scheme, offline mode,
/// connection reuse). Build the egress with [`channel_policy`].
pub struct EgressPoster {
    egress: Arc<Egress>,
}

impl EgressPoster {
    /// A poster over `egress`.
    pub fn new(egress: Arc<Egress>) -> EgressPoster {
        EgressPoster { egress }
    }
}

impl HttpPoster for EgressPoster {
    fn post(&self, request: &PostRequest) -> Result<PostResponse, TransportError> {
        let mut req = Request::post(&request.url, request.body.clone());
        req.headers = request.headers.clone();
        // The answer's body is never read: a chatty server cannot make a notification expensive.
        let answer = self
            .egress
            .stream_within(
                &req,
                &CancelToken::new(),
                request.timeout,
                Some(request.timeout),
            )
            .map_err(to_transport)?;
        let retry_after = answer
            .headers
            .iter()
            .find(|(n, _)| n == "retry-after")
            .and_then(|(_, v)| parse_retry_after(v, SystemTime::now()));
        Ok(PostResponse {
            status: answer.status,
            retry_after,
        })
    }
}

fn to_transport(e: NetError) -> TransportError {
    let kind = match &e {
        NetError::Connect(_) => TransportErrorKind::Connect,
        NetError::Timeout(_) => TransportErrorKind::Timeout,
        NetError::Tls(_) => TransportErrorKind::Tls,
        NetError::Proxy(_) => TransportErrorKind::Proxy,
        _ => TransportErrorKind::Invalid,
    };
    TransportError::new(kind, e.to_string())
}
