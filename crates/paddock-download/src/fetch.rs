//! Small GETs through the downloader's transport: release information, catalogues, any JSON an app asks a host for. They share
//! the downloader's proxy, certificate store, per-host caps and back-off, honour a cancel token, and are bounded in size and
//! time. For files use [`Downloader::download`](crate::Downloader::download).

use crate::engine::{Cancels, Ctl, Downloader, Retry};
use crate::error::DownloadError;
use crate::request::{MAX_HEADERS, host_of};
use crate::transport::{Get, Header, Response, is_identity, parse_length};
use paddock_jobs::CancelToken;
use serde::de::DeserializeOwned;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

/// The most body bytes a small GET reads unless [`Fetch::max_bytes`] says otherwise (16 MiB).
pub const DEFAULT_FETCH_BYTES: u64 = 16 * 1024 * 1024;

/// How one small GET runs.
#[derive(Debug, Clone)]
pub struct Fetch {
    headers: Vec<Header>,
    pub(crate) max_bytes: u64,
    timeout: Duration,
    cancel: Option<CancelToken>,
}

impl Default for Fetch {
    fn default() -> Self {
        Fetch {
            headers: Vec::new(),
            max_bytes: DEFAULT_FETCH_BYTES,
            timeout: Duration::from_secs(60),
            cancel: None,
        }
    }
}

impl Fetch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a request header (same rules as [`Request::header`](crate::Request::header)).
    pub fn header(mut self, name: &str, value: &str) -> Result<Self, DownloadError> {
        if self.headers.len() >= MAX_HEADERS {
            return Err(DownloadError::InvalidRequest(format!(
                "a request takes at most {MAX_HEADERS} added headers"
            )));
        }
        self.headers.push(Header::new(name, value)?);
        Ok(self)
    }

    /// The most body bytes read (default 16 MiB); a longer answer is [`DownloadError::ReplyTooLarge`].
    pub fn max_bytes(mut self, bytes: u64) -> Self {
        self.max_bytes = bytes;
        self
    }

    /// The longest the body may take to arrive (default 60 s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// A token that stops the request (checked before each attempt, while waiting and while reading).
    pub fn cancel(mut self, token: &CancelToken) -> Self {
        self.cancel = Some(token.clone());
        self
    }
}

impl Downloader {
    /// GETs `url` and returns the body (status 200), retrying transient failures, 5xx and 429/503 like a download does
    /// ([`Config::attempts`](crate::Config::attempts), the host's `Retry-After`). Blocks the calling thread: call it from a job
    /// worker or a blocking task, never from a UI or IPC thread.
    pub fn get_bytes(&self, url: &str, fetch: &Fetch) -> Result<Vec<u8>, DownloadError> {
        let host = host_of(url)?;
        let ctl = Arc::new(Ctl::new(
            Cancels {
                job: fetch.cancel.clone().unwrap_or_default(),
                external: None,
            },
            self.net.hosts.clock().clone(),
        ));
        let (net, mut retry) = (&self.net, Retry::default());
        loop {
            ctl.cancel.check()?;
            let permit = net
                .hosts
                .acquire(&host, &|| ctl.halted(), &|| ctl.touch())
                .ok_or_else(|| ctl.halt_error())?;
            let get = Get {
                url,
                range: None,
                if_range: None,
                body_timeout: Some(fetch.timeout),
                headers: &fetch.headers,
            };
            let resp = match net.transport.get(&get) {
                Ok(r) => r,
                Err(e) if e.is_transient() => {
                    drop(permit);
                    retry.failed(&net.config, &ctl, &host, &e.to_string())?;
                    continue;
                }
                Err(e) => {
                    return Err(DownloadError::Network {
                        host,
                        detail: e.to_string(),
                    });
                }
            };
            match resp.status {
                200 => {
                    if !is_identity(&resp.headers) {
                        return Err(DownloadError::Protocol {
                            host,
                            detail: "the host compresses the answer".into(),
                        });
                    }
                    match read_body(resp, fetch.max_bytes, &ctl.cancel) {
                        Ok(body) => return Ok(body),
                        Err(BodyError::Failed(e)) => return Err(e),
                        // The connection dropped mid-body: another attempt.
                        Err(BodyError::Io(detail)) => {
                            drop(permit);
                            retry.failed(&net.config, &ctl, &host, &detail)?;
                        }
                    }
                }
                429 | 503 => {
                    drop(permit);
                    retry.throttled(&net.config, &net.hosts, &host, &resp)?;
                }
                408 | 500..=599 => {
                    drop(permit);
                    retry.failed(&net.config, &ctl, &host, &format!("HTTP {}", resp.status))?;
                }
                status => return Err(DownloadError::Http { host, status }),
            }
        }
    }

    /// As [`get_bytes`](Self::get_bytes), as UTF-8 text.
    pub fn get_text(&self, url: &str, fetch: &Fetch) -> Result<String, DownloadError> {
        let bytes = self.get_bytes(url, fetch)?;
        String::from_utf8(bytes).map_err(|_| DownloadError::Protocol {
            host: host_of(url).unwrap_or_default(),
            detail: "the answer is not UTF-8 text".into(),
        })
    }

    /// GETs `url` (with `Accept: application/json` unless `fetch` sets an `Accept`) and parses the body. A body that is not the
    /// expected JSON is a [`DownloadError::Protocol`] naming the line and column, never the content.
    pub fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        fetch: &Fetch,
    ) -> Result<T, DownloadError> {
        let mut fetch = fetch.clone();
        if !fetch
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("accept"))
        {
            fetch = fetch.header("Accept", "application/json")?;
        }
        let bytes = self.get_bytes(url, &fetch)?;
        serde_json::from_slice(&bytes).map_err(|e| DownloadError::Protocol {
            host: host_of(url).unwrap_or_default(),
            detail: format!(
                "the answer is not the expected JSON ({:?} at line {}, column {})",
                e.classify(),
                e.line(),
                e.column()
            ),
        })
    }
}

enum BodyError {
    /// Ends the request (too large, cancelled).
    Failed(DownloadError),
    /// The connection failed while reading: worth another attempt.
    Io(String),
}

/// Reads at most `limit` bytes, noticing a cancel between chunks and refusing an announced length over the limit up front.
fn read_body(mut resp: Response, limit: u64, cancel: &Cancels) -> Result<Vec<u8>, BodyError> {
    if let Some(size) = resp
        .headers
        .content_length
        .as_deref()
        .and_then(parse_length)
        && size > limit
    {
        return Err(BodyError::Failed(DownloadError::ReplyTooLarge {
            size,
            limit,
        }));
    }
    let mut body = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        cancel.check().map_err(BodyError::Failed)?;
        let n = resp
            .body
            .read(&mut buf)
            .map_err(|e| BodyError::Io(e.to_string()))?;
        if n == 0 {
            return Ok(body);
        }
        let size = body.len() as u64 + n as u64;
        if size > limit {
            return Err(BodyError::Failed(DownloadError::ReplyTooLarge {
                size,
                limit,
            }));
        }
        body.extend_from_slice(&buf[..n]);
    }
}
