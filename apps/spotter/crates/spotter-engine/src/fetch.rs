//! Polite fetching: one request at a time per host (a minimum gap between requests), `ETag` / `Last-Modified`
//! validators, a body cap, timeouts, a redirect cap with loop detection, and the SSRF guard on every hop. Failures
//! are [`SourceError`]s so the core's backoff and statuses apply unchanged.
use crate::guard;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION, WWW_AUTHENTICATE,
};
use reqwest::redirect::Policy;
use reqwest::StatusCode;
use spotter_core::limits::MAX_BODY_BYTES;
use spotter_core::source::http::{parse_token, status_error, BearerChallenge};
use spotter_core::source::{HttpRequest, SourceError};
use spotter_sources::http::{read_bounded, transport};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;
use url::Url;

/// How the fetcher behaves.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    /// The `User-Agent`.
    pub user_agent: String,
    /// How long one request may take.
    pub request_timeout: Duration,
    /// How long connecting may take.
    pub connect_timeout: Duration,
    /// Redirects followed per fetch.
    pub max_redirects: usize,
    /// Largest body read.
    pub max_body: usize,
    /// The least time between two requests to the same host.
    pub min_host_interval: Duration,
}

impl FetchConfig {
    /// Spotter's defaults for a build with this `version`.
    pub fn new(version: &str) -> FetchConfig {
        FetchConfig {
            user_agent: format!("Spotter/{version} (+https://dev.decryptology.net/decryp7/spotter)"),
            request_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(15),
            max_redirects: 5,
            max_body: MAX_BODY_BYTES,
            min_host_interval: Duration::from_millis(1000),
        }
    }
}

/// What the server said about the version of a resource.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Validators {
    /// `ETag`.
    pub etag: Option<String>,
    /// `Last-Modified`.
    pub last_modified: Option<String>,
}

impl Validators {
    /// Whether there is anything to send back.
    pub fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

/// Per-fetch options.
#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    /// The watch may reach local addresses.
    pub allow_local: bool,
    /// Validators from the previous successful fetch; a `304` then means "unchanged".
    pub conditional: Option<Validators>,
    /// Registry credentials (user, token) for a Bearer challenge.
    pub credentials: Option<(String, String)>,
}

/// The answer of a fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// A new body.
    Body {
        /// The bytes (at most the cap).
        bytes: Vec<u8>,
        /// The address after redirects.
        final_url: String,
        /// The validators to remember.
        validators: Validators,
    },
    /// The server answered `304 Not Modified` to the validators sent.
    NotModified,
}

/// The shared HTTP client with its manners.
pub struct PoliteFetcher {
    client: reqwest::Client,
    cfg: FetchConfig,
    hosts: Mutex<HashMap<String, Instant>>,
}

impl PoliteFetcher {
    /// Builds the client (system proxy and certificates, no automatic redirects: they are followed here so each hop is
    /// guarded).
    pub fn new(cfg: FetchConfig) -> Result<PoliteFetcher, String> {
        let client = reqwest::Client::builder()
            .user_agent(cfg.user_agent.clone())
            .connect_timeout(cfg.connect_timeout)
            .redirect(Policy::none())
            .build()
            .map_err(|e| format!("The HTTP client could not be created: {e}"))?;
        Ok(PoliteFetcher {
            client,
            cfg,
            hosts: Mutex::new(HashMap::new()),
        })
    }

    /// The settings in use.
    pub fn config(&self) -> &FetchConfig {
        &self.cfg
    }

    /// The underlying client (for POSTs to notification channels).
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Waits for this host's turn: requests to one host are at least `min_host_interval` apart, also when many are
    /// started at once (each reserves the next free slot).
    async fn wait_turn(&self, host: &str) {
        let slot = {
            let mut hosts = self.hosts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let now = Instant::now();
            let slot = hosts.get(host).map_or(now, |last| (*last + self.cfg.min_host_interval).max(now));
            hosts.insert(host.to_string(), slot);
            slot
        };
        tokio::time::sleep_until(slot).await;
    }

    /// GETs `req`, following up to `max_redirects` redirects.
    pub async fn get(&self, req: &HttpRequest, opts: &FetchOptions) -> Result<Fetched, SourceError> {
        let shown = req.display_url().to_string();
        let mut headers = header_map(req)?;
        if let Some(c) = &opts.conditional {
            add_validators(&mut headers, c);
        }
        let mut url = Url::parse(&req.url).map_err(|_| SourceError::InvalidConfig(format!("{shown} is not a valid address.")))?;
        let mut visited = vec![url.to_string()];
        let mut challenged = false;
        for _ in 0..=self.cfg.max_redirects {
            guard::check_url(&url, opts.allow_local).await?;
            self.wait_turn(url.host_str().unwrap_or("")).await;
            let response = self
                .client
                .get(url.clone())
                .headers(headers.clone())
                .timeout(self.cfg.request_timeout)
                .send()
                .await
                .map_err(|e| transport(e, &shown))?;
            let status = response.status();
            if status == StatusCode::NOT_MODIFIED && opts.conditional.is_some() {
                return Ok(Fetched::NotModified);
            }
            if status == StatusCode::UNAUTHORIZED && !challenged && !headers.contains_key(AUTHORIZATION) {
                challenged = true;
                let challenge = response
                    .headers()
                    .get(WWW_AUTHENTICATE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(BearerChallenge::parse);
                if let Some(c) = challenge {
                    let token = self.token(&c, opts).await?;
                    let value = HeaderValue::from_str(&format!("Bearer {token}"))
                        .map_err(|_| SourceError::InvalidResponse("The registry sent an unusable token.".into()))?;
                    headers.insert(AUTHORIZATION, value);
                    continue;
                }
            }
            if status.is_redirection() {
                let Some(target) = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|l| url.join(l).ok())
                else {
                    return Err(SourceError::InvalidResponse(format!("{shown} redirects without saying where to.")));
                };
                if visited.iter().any(|v| v == target.as_str()) {
                    return Err(SourceError::Failed(format!("{shown} redirects in a circle.")));
                }
                if target.host_str() != url.host_str() {
                    headers.remove(AUTHORIZATION);
                    headers.remove(HeaderName::from_static("private-token"));
                }
                visited.push(target.to_string());
                url = target;
                continue;
            }
            if let Some(e) = status_error(status.as_u16(), &shown) {
                return Err(e);
            }
            let validators = Validators {
                etag: header_text(response.headers(), &ETAG),
                last_modified: header_text(response.headers(), &LAST_MODIFIED),
            };
            let final_url = url.to_string();
            let bytes = read_bounded(response, self.cfg.max_body, &shown).await?;
            return Ok(Fetched::Body {
                bytes,
                final_url,
                validators,
            });
        }
        Err(SourceError::Failed(format!("{shown} redirects too often.")))
    }

    async fn token(&self, c: &BearerChallenge, opts: &FetchOptions) -> Result<String, SourceError> {
        let url =
            Url::parse(&c.token_url()).map_err(|_| SourceError::InvalidResponse("The registry's token address is not valid.".into()))?;
        guard::check_url(&url, opts.allow_local).await?;
        self.wait_turn(url.host_str().unwrap_or("")).await;
        let mut req = self.client.get(url).timeout(self.cfg.request_timeout);
        if let Some((user, token)) = &opts.credentials {
            req = req.basic_auth(user, Some(token));
        }
        let response = req.send().await.map_err(|e| transport(e, &c.realm))?;
        if let Some(e) = status_error(response.status().as_u16(), &c.realm) {
            return Err(e);
        }
        let body = read_bounded(response, 64 << 10, &c.realm).await?;
        parse_token(&body).ok_or_else(|| SourceError::InvalidResponse("The registry's token answer could not be read.".into()))
    }
}

fn header_text(h: &HeaderMap, name: &HeaderName) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

fn add_validators(headers: &mut HeaderMap, v: &Validators) {
    if let Some(e) = v.etag.as_deref().and_then(|e| HeaderValue::from_str(e).ok()) {
        headers.insert(IF_NONE_MATCH, e);
    }
    if let Some(m) = v.last_modified.as_deref().and_then(|m| HeaderValue::from_str(m).ok()) {
        headers.insert(IF_MODIFIED_SINCE, m);
    }
}

fn header_map(req: &HttpRequest) -> Result<HeaderMap, SourceError> {
    let mut map = HeaderMap::new();
    for (k, v) in &req.headers {
        let name =
            HeaderName::from_bytes(k.as_bytes()).map_err(|_| SourceError::InvalidConfig(format!("\"{k}\" is not a header name.")))?;
        let mut value = HeaderValue::from_str(v)
            .map_err(|_| SourceError::InvalidConfig("A header value (perhaps the token) contains invalid characters.".into()))?;
        value.set_sensitive(name == AUTHORIZATION || k.eq_ignore_ascii_case("private-token"));
        map.insert(name, value);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_identify_spotter() {
        let c = FetchConfig::new("1.2.3");
        assert!(c.user_agent.starts_with("Spotter/1.2.3"));
        assert_eq!((c.max_redirects, c.max_body), (5, MAX_BODY_BYTES));
        assert!(Validators::default().is_empty());
        assert!(!Validators {
            etag: Some("x".into()),
            last_modified: None
        }
        .is_empty());
    }

    #[test]
    fn bad_headers_are_config_errors() {
        let bad_name = HttpRequest::get("https://a.b/").header("bad name", "v");
        assert!(matches!(header_map(&bad_name), Err(SourceError::InvalidConfig(_))));
        let bad_value = HttpRequest::get("https://a.b/").header("X", "line\nbreak");
        assert!(matches!(header_map(&bad_value), Err(SourceError::InvalidConfig(_))));
        let ok = header_map(&HttpRequest::get("https://a.b/").header("Authorization", "Bearer t")).unwrap();
        assert!(ok.get(AUTHORIZATION).unwrap().is_sensitive());
    }
}
