//! [`Wire`] over HTTPS with ureq: certificates from the operating system's store (never switched off), the proxy of
//! `paddock-download`, no redirects (the egress follows them with its checks), the address guard on the very addresses the
//! socket connects to (so a name that answers differently the second time is refused), one agent per proxy and reach so
//! connections are reused.

use crate::error::NetError;
use crate::policy::Policy;
use crate::reach::{self, Reach, Refusal};
use crate::request::Method;
use crate::wire::{Wire, WireRequest, WireResponse};
use paddock_download::{HttpConfig, ProxyMode, ProxyRules, ProxyServer, ureq_config};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

/// The real [`Wire`].
pub struct UreqWire {
    mode: ProxyMode,
    rules: RwLock<Arc<ProxyRules>>,
    user_agent: String,
    connect_timeout: Duration,
    response_timeout: Duration,
    agents: Mutex<HashMap<(Option<ProxyServer>, Reach), ureq::Agent>>,
}

impl UreqWire {
    /// Reads the proxy rules (for [`ProxyMode::System`] from the environment and the operating system). An unusable proxy
    /// setting is an error that names it.
    pub fn new(policy: &Policy, mode: ProxyMode) -> Result<UreqWire, NetError> {
        let rules = Self::rules_for(&mode)?;
        Ok(UreqWire {
            mode,
            rules: RwLock::new(Arc::new(rules)),
            user_agent: policy.user_agent.clone(),
            connect_timeout: policy.connect_timeout,
            response_timeout: policy.timeout,
            agents: Mutex::new(HashMap::new()),
        })
    }

    fn rules_for(mode: &ProxyMode) -> Result<ProxyRules, NetError> {
        match mode {
            ProxyMode::System => ProxyRules::detect().map_err(|e| NetError::Proxy(e.to_string())),
            ProxyMode::Direct => Ok(ProxyRules::none()),
            ProxyMode::Rules(r) => Ok(r.clone()),
        }
    }

    /// Reads the system's proxy settings again; only [`ProxyMode::System`] changes, and the old rules stay when the new ones are
    /// unusable.
    pub fn reload_proxy(&self) -> Result<(), NetError> {
        if self.mode == ProxyMode::System {
            let rules = Arc::new(Self::rules_for(&self.mode)?);
            *self.rules.write().unwrap_or_else(PoisonError::into_inner) = rules;
            self.agents
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
        }
        Ok(())
    }

    /// How many agents (connection pools) exist now.
    pub fn agents(&self) -> usize {
        self.agents
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn agent(&self, url: &str, reach: Reach) -> Result<ureq::Agent, NetError> {
        let rules = self
            .rules
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let proxy = rules.for_url(url).cloned();
        let key = (proxy.clone(), reach);
        let mut agents = self.agents.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(a) = agents.get(&key) {
            return Ok(a.clone());
        }
        let http = HttpConfig {
            proxy: ProxyMode::Direct,
            user_agent: self.user_agent.clone(),
            connect_timeout: self.connect_timeout,
            response_timeout: self.response_timeout,
            max_redirects: 0,
        };
        let config: Config =
            ureq_config(&http, proxy.as_ref()).map_err(|e| NetError::Proxy(e.to_string()))?;
        // Behind a proxy the socket goes to the proxy, which is allowed to be on the local network; the target's addresses were
        // checked by the egress before the hop.
        let guard = Guard {
            reach: proxy.is_none().then_some(reach),
        };
        let agent = ureq::Agent::with_parts(config, DefaultConnector::default(), guard);
        agents.insert(key, agent.clone());
        Ok(agent)
    }
}

/// Applies the reach to the addresses of every connection.
#[derive(Debug)]
struct Guard {
    reach: Option<Reach>,
}

impl Resolver for Guard {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addrs = DefaultResolver::default().resolve(uri, config, timeout)?;
        if let Some(r) = self.reach {
            let ips: Vec<IpAddr> = addrs.iter().map(|a| a.ip()).collect();
            reach::vet(&ips, r).map_err(|why| {
                ureq::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    why,
                ))
            })?;
        }
        Ok(addrs)
    }
}

fn with_headers<B>(
    mut rb: ureq::RequestBuilder<B>,
    r: &WireRequest<'_>,
) -> ureq::RequestBuilder<B> {
    for h in r.headers {
        rb = rb.header(h.name.as_str(), h.value.as_str());
    }
    rb
}

impl Wire for UreqWire {
    fn send(&self, r: &WireRequest<'_>) -> Result<WireResponse, NetError> {
        let agent = self.agent(r.url, r.reach)?;
        let host = ureq::http::Uri::try_from(r.url)
            .ok()
            .and_then(|u| u.host().map(str::to_string))
            .unwrap_or_default();
        let resp = match r.method {
            Method::Post => {
                let rb = with_headers(agent.post(r.url), r);
                let rb = rb
                    .config()
                    .timeout_global(r.timeout)
                    .timeout_recv_response(Some(r.response_timeout))
                    .timeout_recv_body(r.body_timeout)
                    .build();
                rb.send(r.body)
            }
            Method::Get => {
                let rb = with_headers(agent.get(r.url), r);
                rb.config()
                    .timeout_global(r.timeout)
                    .timeout_recv_response(Some(r.response_timeout))
                    .timeout_recv_body(r.body_timeout)
                    .build()
                    .call()
            }
            Method::Head => {
                let rb = with_headers(agent.head(r.url), r);
                rb.config()
                    .timeout_global(r.timeout)
                    .timeout_recv_response(Some(r.response_timeout))
                    .timeout_recv_body(r.body_timeout)
                    .build()
                    .call()
            }
        }
        .map_err(|e| classify(e, &host))?;
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(n, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (n.as_str().to_ascii_lowercase(), v.to_string()))
            })
            .collect();
        Ok(WireResponse {
            status: resp.status().as_u16(),
            headers,
            body: Box::new(resp.into_body().into_reader()),
        })
    }
}

fn classify(e: ureq::Error, host: &str) -> NetError {
    let host = host.to_string();
    match &e {
        ureq::Error::Timeout(_) => NetError::Timeout(host),
        ureq::Error::Io(io) => {
            if let Some(why) = io
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<Refusal>())
            {
                NetError::Refused { host, why: *why }
            } else if io.kind() == std::io::ErrorKind::TimedOut {
                NetError::Timeout(host)
            } else if io
                .get_ref()
                .is_some_and(|inner| inner.is::<rustls::Error>())
            {
                NetError::Tls(host)
            } else {
                NetError::Connect(host)
            }
        }
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Protocol(_) => {
            NetError::Connect(host)
        }
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => NetError::Tls(host),
        ureq::Error::InvalidProxyUrl | ureq::Error::ConnectProxyFailed(_) => {
            NetError::Proxy(e.to_string())
        }
        _ => NetError::Invalid(format!("The request to {host} cannot be sent.")),
    }
}
