//! [`Transport`] over HTTPS with ureq (feature `http`): certificates are checked against the operating system's trust store (so
//! a corporate TLS-inspecting proxy whose root certificate IT installed works as in the browser; checking is never switched
//! off), a proxy only when configured (never from the environment), bodies as sent (`Accept-Encoding: identity`).

use crate::error::{DownloadError, TransportError, TransportErrorKind};
use crate::transport::{Get, Headers, Response, Transport};
use std::sync::Arc;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// How [`HttpTransport`] connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfig {
    /// HTTP(S) proxy, e.g. `http://proxy.corp:3128` (credentials as `http://user:pass@host:port`).
    pub proxy: Option<String>,
    pub user_agent: String,
    pub connect_timeout: Duration,
    /// Longest wait for the status line and headers.
    pub response_timeout: Duration,
    /// Redirects followed (GitHub release assets, CDNs).
    pub max_redirects: u32,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            proxy: None,
            user_agent: concat!("paddock-download/", env!("CARGO_PKG_VERSION")).to_string(),
            connect_timeout: Duration::from_secs(30),
            response_timeout: Duration::from_secs(60),
            max_redirects: 10,
        }
    }
}

/// The real [`Transport`].
pub struct HttpTransport {
    agent: ureq::Agent,
}

impl HttpTransport {
    /// Refuses an unusable proxy address.
    pub fn new(config: HttpConfig) -> Result<Self, DownloadError> {
        let proxy = match &config.proxy {
            Some(p) => Some(ureq::Proxy::new(p).map_err(|e| {
                DownloadError::InvalidRequest(format!("invalid proxy address: {e}"))
            })?),
            None => None,
        };
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(config.max_redirects)
            .timeout_connect(Some(config.connect_timeout))
            .timeout_recv_response(Some(config.response_timeout))
            .user_agent(config.user_agent.as_str())
            .accept_encoding("identity")
            .proxy(proxy)
            .tls_config(tls)
            .build()
            .into();
        Ok(HttpTransport { agent })
    }
}

impl Transport for HttpTransport {
    fn get(&self, request: &Get<'_>) -> Result<Response, TransportError> {
        let mut req = self.agent.get(request.url);
        if let Some(r) = request.range {
            req = req.header("Range", r.header());
        }
        if let Some(v) = request.if_range {
            req = req.header("If-Range", v);
        }
        let resp = req
            .config()
            .timeout_recv_body(request.body_timeout)
            .build()
            .call()
            .map_err(classify)?;
        let h = resp.headers();
        let get = |name: &str| {
            h.get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let headers = Headers {
            content_length: get("content-length"),
            content_range: get("content-range"),
            content_encoding: get("content-encoding"),
            etag: get("etag"),
            last_modified: get("last-modified"),
            retry_after: get("retry-after"),
        };
        let status = resp.status().as_u16();
        Ok(Response {
            status,
            headers,
            body: Box::new(resp.into_body().into_reader()),
        })
    }
}

/// What a failed request means.
pub(crate) fn classify(e: ureq::Error) -> TransportError {
    use TransportErrorKind as K;
    let kind = match &e {
        ureq::Error::Timeout(_) => K::Timeout,
        // ureq hands TLS failures over as I/O errors around the rustls error.
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
