//! The one door to the network: policy checks before every hop, redirects followed by hand, answers capped.

use crate::error::NetError;
use crate::policy::Policy;
use crate::reach::{self, Reach, Refusal};
use crate::request::{Method, Request, Response, header_of};
use crate::wire::{Lookup, Wire, WireRequest};
use paddock_download::{Get, Header, Headers, TransportError, TransportErrorKind};
use paddock_jobs::CancelToken;
use paddock_jobs::time::Clock;
use std::collections::HashMap;
use std::io::Read;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use url::{Host, Url};

/// Headers dropped when a redirect goes to another host.
const CROSS_HOST_DROP: [&str; 3] = ["authorization", "cookie", "proxy-authorization"];

/// An answer whose body has not been read.
pub struct Streaming {
    /// The HTTP status.
    pub status: u16,
    /// The address that answered (after redirects), without query or login.
    pub url: String,
    /// Headers, names lower-cased.
    pub headers: Vec<(String, String)>,
    /// The body; the caller applies its own limit.
    pub body: Box<dyn Read + Send>,
    /// Redirects followed.
    pub redirects: u32,
}

/// The outbound door of an app. One per app (it holds the offline switch, the per-host gaps and, in the real wire, the
/// connection pools); share it by `Arc`.
pub struct Egress {
    policy: Policy,
    wire: Arc<dyn Wire>,
    lookup: Arc<dyn Lookup>,
    clock: Arc<dyn Clock>,
    offline: AtomicBool,
    last: Mutex<HashMap<String, Duration>>,
}

impl Egress {
    /// An egress over the given wire, resolver and clock.
    pub fn new(
        policy: Policy,
        wire: Arc<dyn Wire>,
        lookup: Arc<dyn Lookup>,
        clock: Arc<dyn Clock>,
    ) -> Egress {
        Egress {
            policy,
            wire,
            lookup,
            clock,
            offline: AtomicBool::new(false),
            last: Mutex::new(HashMap::new()),
        }
    }

    /// The policy in force.
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Switches offline mode: every request fails with [`NetError::Offline`] before any name is resolved or socket opened.
    pub fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::SeqCst);
    }

    /// Whether offline mode is on.
    pub fn is_offline(&self) -> bool {
        self.offline.load(Ordering::SeqCst)
    }

    /// Sends a request and reads the answer (at most `max_body` bytes). Blocks: call it from a job.
    pub fn send(&self, request: &Request, cancel: &CancelToken) -> Result<Response, NetError> {
        let s = self.run_with(request, cancel, false, self.policy.timeout, None)?;
        let host = host_label(&s.url);
        let body = if s.status == 304 || request.method == Method::Head {
            Vec::new()
        } else {
            read_capped(
                s.body,
                header_of(&s.headers, "content-length"),
                self.policy.max_body,
                &host,
            )?
        };
        Ok(Response {
            status: s.status,
            url: s.url,
            headers: s.headers,
            body,
            redirects: s.redirects,
        })
    }

    /// Sends a request and hands back the answer with its body unread, for large downloads. No whole-request time limit applies;
    /// `body_timeout` limits the wait between chunks.
    pub fn stream(
        &self,
        request: &Request,
        cancel: &CancelToken,
        body_timeout: Option<Duration>,
    ) -> Result<Streaming, NetError> {
        self.run_with(request, cancel, true, self.policy.timeout, body_timeout)
    }

    /// [`stream`](Self::stream) with its own limit for the wait for the status line and headers.
    pub fn stream_within(
        &self,
        request: &Request,
        cancel: &CancelToken,
        response_timeout: Duration,
        body_timeout: Option<Duration>,
    ) -> Result<Streaming, NetError> {
        self.run_with(request, cancel, true, response_timeout, body_timeout)
    }

    fn run_with(
        &self,
        request: &Request,
        cancel: &CancelToken,
        streaming: bool,
        response_timeout: Duration,
        body_timeout: Option<Duration>,
    ) -> Result<Streaming, NetError> {
        if request.body.len() as u64 > self.policy.max_request_body {
            return Err(NetError::Invalid(format!(
                "The request body is larger than the {} byte limit.",
                self.policy.max_request_body
            )));
        }
        let mut url = Url::parse(&request.url).map_err(|_| {
            NetError::Invalid(format!("{} is not a valid address.", shown(&request.url)))
        })?;
        let mut method = request.method;
        let mut body = request.body.clone();
        let mut headers = request.headers.clone();
        let mut limit = self.policy.reach;
        let mut hop = 0;
        loop {
            if self.is_offline() {
                return Err(NetError::Offline);
            }
            if cancel.is_cancelled() {
                return Err(NetError::Cancelled);
            }
            let class = self.check(&url, limit, request)?;
            if hop == 0 {
                // A redirect may never go farther than where the request started.
                limit = limit.min(class);
            }
            self.pace(&url, cancel)?;
            let wire_request = WireRequest {
                method,
                url: url.as_str(),
                reach: limit,
                headers: &headers,
                body: &body,
                user_agent: &self.policy.user_agent,
                connect_timeout: self.policy.connect_timeout,
                timeout: (!streaming).then_some(self.policy.timeout),
                response_timeout,
                body_timeout,
            };
            let reply = self.wire.send(&wire_request)?;
            let Some(location) = redirect_location(reply.status, &reply.headers)
                .filter(|_| self.policy.max_redirects > 0)
            else {
                return Ok(Streaming {
                    status: reply.status,
                    url: shown(url.as_str()),
                    headers: reply.headers,
                    body: reply.body,
                    redirects: hop,
                });
            };
            if hop >= self.policy.max_redirects {
                return Err(NetError::TooManyRedirects);
            }
            drop(reply.body);
            let next = url
                .join(&location)
                .map_err(|_| NetError::BadRedirect("the Location is not an address".into()))?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err(NetError::BadRedirect(
                    "it goes from https to plain http".into(),
                ));
            }
            if next.host_str() != url.host_str() {
                headers.retain(|h| {
                    !CROSS_HOST_DROP
                        .iter()
                        .any(|d| d.eq_ignore_ascii_case(&h.name))
                });
            }
            if matches!(reply.status, 301..=303) && method == Method::Post {
                method = Method::Get;
                body.clear();
                headers.retain(|h| !h.name.eq_ignore_ascii_case("content-type"));
            }
            url = next;
            hop += 1;
        }
    }

    /// The checks before a hop: scheme, login, allow-list, and the reach of every address. Returns the reach the farthest address needs.
    fn check(&self, url: &Url, limit: Reach, request: &Request) -> Result<Reach, NetError> {
        let shown_url = shown(url.as_str());
        let host = url
            .host()
            .ok_or_else(|| NetError::Invalid(format!("{shown_url} has no host name.")))?;
        let name = url.host_str().unwrap_or_default().to_string();
        if !url.username().is_empty() || url.password().is_some() {
            return Err(NetError::Invalid(format!(
                "{shown_url} must not contain a login; send credentials in a header."
            )));
        }
        match url.scheme() {
            "https" => {}
            "http" if reach::host_is_loopback(&name) && limit >= Reach::Loopback => {}
            "http"
                if request.plain_http_lan
                    && limit >= Reach::Lan
                    && reach::host_may_be_lan(&name) => {}
            _ => return Err(NetError::Scheme(name)),
        }
        let plain_lan = url.scheme() == "http" && !reach::host_is_loopback(&name);
        if !self.policy.host_allowed(&name) {
            return Err(NetError::NotAllowed(name));
        }
        let refuse = |why: Refusal| NetError::Refused {
            host: name.clone(),
            why,
        };
        let addrs: Vec<IpAddr> = match host {
            Host::Ipv4(ip) => vec![ip.into()],
            Host::Ipv6(ip) => vec![ip.into()],
            Host::Domain(d) => {
                let port = url.port_or_known_default().unwrap_or(443);
                self.lookup
                    .lookup(d, port)
                    .map_err(|_| NetError::Connect(name.clone()))?
            }
        };
        let needed = reach::vet(&addrs, limit).map_err(refuse)?;
        if request.deny_lan
            && addrs
                .iter()
                .any(|a| reach::classify(*a) == reach::Class::Lan)
        {
            return Err(refuse(Refusal::TooFar(reach::Class::Lan)));
        }
        // Plain http only ever goes to a private network: a name spelled like one that points at a public address is refused.
        if plain_lan && needed < Reach::Lan {
            return Err(NetError::Scheme(name));
        }
        Ok(needed)
    }

    /// Keeps at least `min_gap` between two requests to one host, waiting on the clock (cancellable).
    fn pace(&self, url: &Url, cancel: &CancelToken) -> Result<(), NetError> {
        let gap = self.policy.min_gap;
        if gap.is_zero() {
            return Ok(());
        }
        let key = url.host_str().unwrap_or_default().to_ascii_lowercase();
        let wait = {
            let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
            let now = self.clock.monotonic();
            let at = last.get(&key).map_or(now, |l| (*l + gap).max(now));
            last.insert(key, at);
            at.saturating_sub(now)
        };
        if wait.is_zero() {
            Ok(())
        } else {
            self.clock
                .wait(wait, cancel)
                .map_err(|_| NetError::Cancelled)
        }
    }

    /// This egress as the transport of a `paddock_download::Downloader`, so downloads obey the same policy.
    pub fn download_transport(self: &Arc<Self>) -> DownloadTransport {
        DownloadTransport(self.clone())
    }
}

/// `paddock_download::Transport` over an [`Egress`]: GETs with `Range` and `If-Range`, every hop checked, no redirect left to
/// the wire.
pub struct DownloadTransport(Arc<Egress>);

impl paddock_download::Transport for DownloadTransport {
    fn get(&self, request: &Get<'_>) -> Result<paddock_download::Response, TransportError> {
        let mut req = Request::get(request.url);
        req.headers = request.headers.to_vec();
        if let Some(r) = request.range {
            req.headers.push(Header {
                name: "Range".into(),
                value: r.header(),
            });
        }
        if let Some(v) = request.if_range {
            req.headers.push(Header {
                name: "If-Range".into(),
                value: v.to_string(),
            });
        }
        let s = self
            .0
            .stream(&req, &CancelToken::new(), request.body_timeout)
            .map_err(to_transport)?;
        let get = |n: &str| header_of(&s.headers, n).map(str::to_string);
        let headers = Headers {
            content_length: get("content-length"),
            content_range: get("content-range"),
            content_encoding: get("content-encoding"),
            etag: get("etag"),
            last_modified: get("last-modified"),
            retry_after: get("retry-after"),
        };
        Ok(paddock_download::Response {
            status: s.status,
            headers,
            body: s.body,
        })
    }
}

fn to_transport(e: NetError) -> TransportError {
    use TransportErrorKind as K;
    let kind = match &e {
        NetError::Connect(_) => K::Connect,
        NetError::Timeout(_) => K::Timeout,
        NetError::Tls(_) => K::Tls,
        NetError::Proxy(_) => K::Proxy,
        _ => K::Invalid,
    };
    TransportError::new(kind, e.to_string())
}

fn redirect_location(status: u16, headers: &[(String, String)]) -> Option<String> {
    matches!(status, 301 | 302 | 303 | 307 | 308)
        .then(|| header_of(headers, "location").map(str::to_string))
        .flatten()
}

fn read_capped(
    body: Box<dyn Read + Send>,
    length: Option<&str>,
    limit: u64,
    host: &str,
) -> Result<Vec<u8>, NetError> {
    if length
        .and_then(|l| l.trim().parse::<u64>().ok())
        .is_some_and(|n| n > limit)
    {
        return Err(NetError::TooLarge(limit));
    }
    let mut out = Vec::new();
    body.take(limit.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                NetError::Timeout(host.to_string())
            } else {
                NetError::Connect(host.to_string())
            }
        })?;
    if out.len() as u64 > limit {
        return Err(NetError::TooLarge(limit));
    }
    Ok(out)
}

/// The address without query, fragment or login, for messages.
pub(crate) fn shown(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut u) => {
            u.set_query(None);
            u.set_fragment(None);
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.to_string()
        }
        Err(_) => url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect(),
    }
}

fn host_label(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}
