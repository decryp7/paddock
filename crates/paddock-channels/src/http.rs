//! The port to the network for webhook and ntfy: one POST. [`UreqPoster`] (feature `http`) is the real one.

use paddock_download::{Header, TransportError};
use std::time::Duration;

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

#[cfg(feature = "http")]
mod real {
    use super::{HttpPoster, PostRequest, PostResponse};
    use crate::SendError;
    use paddock_download::{
        HttpConfig, ProxyMode, ProxyRules, TransportError, TransportErrorKind, parse_retry_after,
        ureq_agent,
    };
    use std::sync::{Arc, RwLock};
    use std::time::{Duration, SystemTime};

    /// [`HttpPoster`] over HTTPS with ureq: certificates are checked against the operating system's trust store (a corporate
    /// TLS-inspecting proxy whose root IT installed works as in the browser; checking is never switched off), the proxy is
    /// chosen like `paddock-download` does ([`ProxyMode`]), redirects are never followed.
    ///
    /// Plan X16 (`paddock-net`) will own the one outbound policy; this poster is the part that moves there.
    pub struct UreqPoster {
        mode: ProxyMode,
        rules: RwLock<Arc<ProxyRules>>,
    }

    impl UreqPoster {
        /// Reads the proxy rules once (for [`ProxyMode::System`] from the environment and the operating system). An unusable
        /// proxy setting is an error that names it.
        pub fn new(mode: ProxyMode) -> Result<UreqPoster, SendError> {
            let rules = Self::rules_for(&mode)?;
            Ok(UreqPoster {
                mode,
                rules: RwLock::new(Arc::new(rules)),
            })
        }

        fn rules_for(mode: &ProxyMode) -> Result<ProxyRules, SendError> {
            match mode {
                ProxyMode::System => {
                    ProxyRules::detect().map_err(|e| SendError::Proxy(e.to_string()))
                }
                ProxyMode::Direct => Ok(ProxyRules::none()),
                ProxyMode::Rules(r) => Ok(r.clone()),
            }
        }

        /// Reads the system's proxy settings again; only [`ProxyMode::System`] changes, and the old rules stay when the new
        /// ones are unusable.
        pub fn reload_proxy(&self) -> Result<(), SendError> {
            if self.mode == ProxyMode::System {
                let rules = Arc::new(Self::rules_for(&self.mode)?);
                *self.rules.write().unwrap_or_else(|p| p.into_inner()) = rules;
            }
            Ok(())
        }

        fn agent(&self, url: &str) -> Result<ureq::Agent, TransportError> {
            let rules = self.rules.read().unwrap_or_else(|p| p.into_inner()).clone();
            // Redirects are never followed: one could leak the message or token to another host.
            let config = HttpConfig {
                proxy: ProxyMode::Direct,
                user_agent: concat!("paddock-channels/", env!("CARGO_PKG_VERSION")).to_string(),
                connect_timeout: Duration::from_secs(30),
                response_timeout: Duration::from_secs(60),
                max_redirects: 0,
            };
            ureq_agent(&config, rules.for_url(url))
                .map_err(|e| TransportError::new(TransportErrorKind::Proxy, e.to_string()))
        }
    }

    impl HttpPoster for UreqPoster {
        fn post(&self, request: &PostRequest) -> Result<PostResponse, TransportError> {
            let agent = self.agent(&request.url)?;
            let mut req = agent.post(&request.url);
            for h in &request.headers {
                req = req.header(h.name.as_str(), h.value.as_str());
            }
            let resp = req
                .config()
                .timeout_global(Some(request.timeout))
                .build()
                .send(&request.body[..])
                .map_err(classify)?;
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| parse_retry_after(v, SystemTime::now()));
            Ok(PostResponse {
                status: resp.status().as_u16(),
                retry_after,
            })
        }
    }

    fn classify(e: ureq::Error) -> TransportError {
        use TransportErrorKind as K;
        let kind = match &e {
            ureq::Error::Timeout(_) => K::Timeout,
            ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => K::Timeout,
            ureq::Error::Io(io)
                if io
                    .get_ref()
                    .is_some_and(|inner| inner.is::<rustls::Error>()) =>
            {
                K::Tls
            }
            ureq::Error::Io(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Protocol(_) => K::Connect,
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) => K::Tls,
            ureq::Error::InvalidProxyUrl | ureq::Error::ConnectProxyFailed(_) => K::Proxy,
            _ => K::Invalid,
        };
        TransportError::new(kind, e.to_string())
    }
}

#[cfg(feature = "http")]
pub use real::UreqPoster;
