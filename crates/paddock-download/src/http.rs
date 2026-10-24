//! [`Transport`] over HTTPS with ureq (feature `http`): certificates are checked against the operating system's trust store (so
//! a corporate TLS-inspecting proxy whose root certificate IT installed works as in the browser; checking is never switched
//! off), a proxy as [`ProxyMode`] says (by default the system's: environment, then the operating system), bodies as sent
//! (`Accept-Encoding: identity`).
//!
//! One `ureq` agent is kept per proxy in use, so connections are reused. Redirects stay on the agent that made the request: a
//! redirect to a host on the bypass list still goes through the proxy the first request chose.

use crate::error::{DownloadError, TransportError, TransportErrorKind};
use crate::proxy::{ProxyRules, ProxyScheme, ProxyServer};
use crate::transport::{Get, Headers, Response, Transport};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// Where [`HttpTransport`] gets its proxy from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProxyMode {
    /// The system's settings: `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`/`NO_PROXY` (any case) first, then the operating system's
    /// (macOS system configuration, Windows Internet Settings or WinHTTP). Loopback always goes direct. Read when the transport
    /// is built and again on [`HttpTransport::reload_proxy`]. See [`ProxyRules::detect`].
    #[default]
    System,
    /// Never a proxy, whatever the system says.
    Direct,
    /// Exactly these rules (an address from the app's Settings, with credentials if the proxy needs a login).
    Rules(ProxyRules),
}

impl ProxyMode {
    /// One explicit proxy for every request, e.g. `ProxyMode::explicit("http://proxy.corp:3128")`; a login may be part of the
    /// address or added with [`ProxyServer::with_credentials`] and [`ProxyMode::Rules`].
    pub fn explicit(address: &str) -> Result<Self, DownloadError> {
        Ok(ProxyMode::Rules(ProxyRules::all(ProxyServer::parse(
            address,
        )?)))
    }
}

/// How [`HttpTransport`] connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfig {
    /// Where the proxy comes from (default: the system's).
    pub proxy: ProxyMode,
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
            proxy: ProxyMode::System,
            user_agent: concat!("paddock-download/", env!("CARGO_PKG_VERSION")).to_string(),
            connect_timeout: Duration::from_secs(30),
            response_timeout: Duration::from_secs(60),
            max_redirects: 10,
        }
    }
}

/// The real [`Transport`].
pub struct HttpTransport {
    config: HttpConfig,
    rules: RwLock<Arc<ProxyRules>>,
    /// One agent per proxy in use (`None`: direct), so each keeps its own connection pool.
    agents: Mutex<HashMap<Option<ProxyServer>, ureq::Agent>>,
}

impl HttpTransport {
    /// Reads the proxy rules (for [`ProxyMode::System`], from the environment and the operating system) and builds the direct
    /// agent. Refuses an unusable proxy address, and an unusable proxy variable, with a message that names it.
    pub fn new(config: HttpConfig) -> Result<Self, DownloadError> {
        let rules = Self::rules_for(&config.proxy)?;
        let t = HttpTransport {
            config,
            rules: RwLock::new(Arc::new(rules)),
            agents: Mutex::new(HashMap::new()),
        };
        t.agent_for(None)?;
        Ok(t)
    }

    fn rules_for(mode: &ProxyMode) -> Result<ProxyRules, DownloadError> {
        match mode {
            ProxyMode::System => ProxyRules::detect(),
            ProxyMode::Direct => Ok(ProxyRules::none()),
            ProxyMode::Rules(r) => Ok(r.clone()),
        }
    }

    /// Reads the system's proxy settings again (after the user changed them, say). Only [`ProxyMode::System`] changes; keeps the
    /// old rules when the new ones are unusable.
    pub fn reload_proxy(&self) -> Result<(), DownloadError> {
        if self.config.proxy != ProxyMode::System {
            return Ok(());
        }
        let rules = Arc::new(ProxyRules::detect()?);
        *self.rules.write().unwrap_or_else(|p| p.into_inner()) = rules;
        self.agents().retain(|k, _| k.is_none());
        Ok(())
    }

    /// The rules in force.
    pub fn proxy_rules(&self) -> Arc<ProxyRules> {
        self.rules.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn agents(&self) -> MutexGuard<'_, HashMap<Option<ProxyServer>, ureq::Agent>> {
        self.agents.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn agent_for(&self, proxy: Option<&ProxyServer>) -> Result<ureq::Agent, DownloadError> {
        let mut agents = self.agents();
        if let Some(a) = agents.get(&proxy.cloned()) {
            return Ok(a.clone());
        }
        let agent = build_agent(&self.config, proxy)?;
        agents.insert(proxy.cloned(), agent.clone());
        Ok(agent)
    }
}

/// A ureq agent configured like [`HttpTransport`]'s (certificates from the operating system's store, `proxy` or direct, the
/// config's timeouts, user agent and redirect limit), for other Paddock crates that make requests of their own (a POST for a
/// notification channel) and must not duplicate the TLS and proxy set-up. The proxy's login is applied here and never appears
/// in the error.
pub fn ureq_agent(
    config: &HttpConfig,
    proxy: Option<&ProxyServer>,
) -> Result<ureq::Agent, DownloadError> {
    build_agent(config, proxy)
}

fn build_agent(
    config: &HttpConfig,
    proxy: Option<&ProxyServer>,
) -> Result<ureq::Agent, DownloadError> {
    let proxy = proxy.map(to_ureq).transpose()?;
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    Ok(ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(config.max_redirects)
        .timeout_connect(Some(config.connect_timeout))
        .timeout_recv_response(Some(config.response_timeout))
        .user_agent(config.user_agent.as_str())
        .accept_encoding("identity")
        .proxy(proxy)
        .tls_config(tls)
        .build()
        .into())
}

/// The proxy for ureq. The login goes in through the builder (percent-encoded there); the error never repeats it.
fn to_ureq(p: &ProxyServer) -> Result<ureq::Proxy, DownloadError> {
    let protocol = match p.scheme() {
        ProxyScheme::Http => ureq::ProxyProtocol::Http,
        ProxyScheme::Https => ureq::ProxyProtocol::Https,
    };
    let mut b = ureq::Proxy::builder(protocol).host(p.host()).port(p.port());
    if let Some(c) = p.credentials() {
        b = b.username(c.username()).password(c.password());
    }
    b.build().map_err(|_| {
        DownloadError::InvalidRequest(format!("invalid proxy address: {p} cannot be used"))
    })
}

impl Transport for HttpTransport {
    fn get(&self, request: &Get<'_>) -> Result<Response, TransportError> {
        let rules = self.proxy_rules();
        let agent = self
            .agent_for(rules.for_url(request.url))
            .map_err(|e| TransportError::new(TransportErrorKind::Proxy, e.to_string()))?;
        let mut req = agent.get(request.url);
        for h in request.headers {
            req = req.header(h.name.as_str(), h.value.as_str());
        }
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
