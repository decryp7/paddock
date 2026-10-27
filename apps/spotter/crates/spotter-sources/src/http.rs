//! HTTP for sources: bounded bodies, deadlines, the OCI anonymous/credentialed token dance, and failures mapped to
//! statuses ([`spotter_core::source::http::status_error`]).

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, WWW_AUTHENTICATE};
use spotter_core::limits::MAX_BODY_BYTES;
use spotter_core::source::http::{parse_token, status_error, BearerChallenge};
use spotter_core::source::{HttpRequest, SourceError};
use std::time::Duration;

/// The HTTP client sources share.
#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
}

/// How long one request may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl Http {
    /// A client that identifies as `Spotter/<version>` and uses the system's proxy settings and certificate store.
    pub fn new(version: &str) -> Result<Http, String> {
        reqwest::Client::builder()
            .user_agent(format!("Spotter/{version} (+https://dev.decryptology.net/decryp7/spotter)"))
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map(|client| Http { client })
            .map_err(|e| format!("The HTTP client could not be created: {e}"))
    }

    /// The underlying client (downloads use it without the request timeout).
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// GETs `req` and returns its body (at most `max` bytes). Answers a registry's Bearer challenge once, with
    /// `credentials` (user, token) when given.
    pub async fn get(&self, req: &HttpRequest, credentials: Option<&(String, String)>, max: usize) -> Result<Vec<u8>, SourceError> {
        let url = req.display_url().to_string();
        let mut headers = headers_of(req)?;
        let mut response = self.send(&req.url, &headers, &url).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && !headers.contains_key(AUTHORIZATION) {
            let challenge = response
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .and_then(BearerChallenge::parse);
            if let Some(c) = challenge {
                let token = self.token(&c, credentials).await?;
                let value = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| SourceError::InvalidResponse("The registry sent an unusable token.".into()))?;
                headers.insert(AUTHORIZATION, value);
                response = self.send(&req.url, &headers, &url).await?;
            }
        }
        if let Some(e) = status_error(response.status().as_u16(), &url) {
            return Err(e);
        }
        read_bounded(response, max, &url).await
    }

    async fn token(&self, c: &BearerChallenge, credentials: Option<&(String, String)>) -> Result<String, SourceError> {
        let mut req = self.client.get(c.token_url()).timeout(REQUEST_TIMEOUT);
        if let Some((user, token)) = credentials {
            req = req.basic_auth(user, Some(token));
        }
        let response = req.send().await.map_err(|e| transport(e, &c.realm))?;
        if let Some(e) = status_error(response.status().as_u16(), &c.realm) {
            return Err(e);
        }
        let body = read_bounded(response, 64 << 10, &c.realm).await?;
        parse_token(&body).ok_or_else(|| SourceError::InvalidResponse("The registry's token answer could not be read.".into()))
    }

    async fn send(&self, url: &str, headers: &HeaderMap, shown: &str) -> Result<reqwest::Response, SourceError> {
        self.client
            .get(url)
            .headers(headers.clone())
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| transport(e, shown))
    }
}

fn headers_of(req: &HttpRequest) -> Result<HeaderMap, SourceError> {
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

/// Reads at most `max` bytes of a body; a larger body is an error rather than a truncated parse.
pub async fn read_bounded(mut response: reqwest::Response, max: usize, url: &str) -> Result<Vec<u8>, SourceError> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(too_large(url, max));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| transport(e, url))? {
        if body.len() + chunk.len() > max {
            return Err(too_large(url, max));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn too_large(url: &str, max: usize) -> SourceError {
    SourceError::InvalidResponse(format!(
        "{url} answered with more than {} MiB; Spotter reads at most that.",
        max >> 20
    ))
}

/// What a transport failure means.
pub fn transport(e: reqwest::Error, url: &str) -> SourceError {
    if e.is_timeout() {
        SourceError::Unreachable(format!("{url} did not answer in time. Spotter will try again."))
    } else if e.is_connect() {
        SourceError::Unreachable(format!("{url} could not be reached (no connection). Spotter will try again."))
    } else if e.is_redirect() {
        SourceError::Failed(format!("{url} redirects too often."))
    } else {
        SourceError::Unreachable(format!("{url} could not be read ({e})."))
    }
}

/// The default body limit.
pub const MAX_BODY: usize = MAX_BODY_BYTES;
