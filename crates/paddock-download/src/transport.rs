//! The port to the network: one GET with an optional byte range. `HttpTransport` (feature `http`) is the
//! real one; tests and apps with their own HTTP stack implement [`Transport`] themselves. Header values arrive raw and are parsed
//! here, once, by pure functions.

use crate::error::TransportError;
use std::io::Read;
use std::time::{Duration, SystemTime};

/// A byte range of a GET: `start..=end`, or `start..` to the end of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    /// Inclusive, as in the header; `None` for the rest of the file.
    pub end: Option<u64>,
}

impl ByteRange {
    /// The `Range` header value.
    pub fn header(&self) -> String {
        match self.end {
            Some(e) => format!("bytes={}-{e}", self.start),
            None => format!("bytes={}-", self.start),
        }
    }
}

/// One GET. Contract for implementations: send `Accept-Encoding: identity` (never decompress), send `Range` and `If-Range` when
/// given, follow redirects, return any status as a [`Response`] (only a failure without a status line is an error), and stop
/// reading the body after `body_timeout` when one is given. Called from several threads at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Get<'a> {
    pub url: &'a str,
    pub range: Option<ByteRange>,
    /// The validator a range must still match (`If-Range`): a changed file is answered 200 instead of 206.
    pub if_range: Option<&'a str>,
    pub body_timeout: Option<Duration>,
    /// Headers the caller added to the request ([`Request::header`](crate::Request::header)): send them with every request,
    /// including range requests. Implementations must not forward them to another host on a redirect.
    pub headers: &'a [Header],
}

/// A request header the caller added (an `Authorization` token, an `Accept`). Names and values were checked when it was built:
/// no control characters, and none of the headers the downloader owns. `Debug` shows the name only, because values are often
/// secrets.
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Header({}: <hidden>)", self.name)
    }
}

/// Headers the downloader sets itself (or that would break framing); a caller cannot add them.
const OWNED_HEADERS: [&str; 10] = [
    "host",
    "range",
    "if-range",
    "accept-encoding",
    "content-length",
    "transfer-encoding",
    "connection",
    "te",
    "upgrade",
    "proxy-authorization",
];

impl Header {
    /// Checks a header: a token as name, a value without control characters other than tab, and not one of the headers the
    /// downloader owns (`Range`, `If-Range`, `Accept-Encoding`, `Host`, framing headers, `Proxy-Authorization`).
    pub fn new(name: &str, value: &str) -> Result<Header, crate::DownloadError> {
        let bad = |why: &str| {
            crate::DownloadError::InvalidRequest(format!(
                "header '{}' {why}",
                name.chars().take(40).collect::<String>()
            ))
        };
        let token = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c);
        if name.is_empty() || name.len() > 128 || !name.chars().all(token) {
            return Err(bad("is not a valid header name"));
        }
        if OWNED_HEADERS.iter().any(|h| h.eq_ignore_ascii_case(name)) {
            return Err(bad("is set by the downloader and cannot be added"));
        }
        if value.len() > 8 * 1024
            || value
                .chars()
                .any(|c| (c.is_control() && c != '\t') || c == '\u{7f}')
        {
            return Err(bad(
                "has a value that is too long or has control characters",
            ));
        }
        Ok(Header {
            name: name.to_string(),
            value: value.to_string(),
        })
    }
}

/// The headers the downloader reads, as raw values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    pub content_length: Option<String>,
    pub content_range: Option<String>,
    pub content_encoding: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub retry_after: Option<String>,
}

/// A status, the headers and the body. Dropping the body closes the connection.
pub struct Response {
    pub status: u16,
    pub headers: Headers,
    pub body: Box<dyn Read + Send>,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

/// The network port. See [`Get`] for the contract.
pub trait Transport: Send + Sync {
    fn get(&self, request: &Get<'_>) -> Result<Response, TransportError>;
}

/// A parsed `Content-Range: bytes a-b/n` (`n` is `None` for `*`), or the unsatisfied form `bytes */n` of a 416.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRange {
    Bytes {
        start: u64,
        end: u64,
        total: Option<u64>,
    },
    Unsatisfied {
        total: u64,
    },
}

/// Parses a `Content-Range` value; `None` for anything malformed (end before start, end past the total).
pub fn parse_content_range(v: &str) -> Option<ContentRange> {
    let rest = v.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse::<u64>().ok()?),
    };
    if range.trim() == "*" {
        return total.map(|total| ContentRange::Unsatisfied { total });
    }
    let (a, b) = range.trim().split_once('-')?;
    let (start, end) = (a.trim().parse::<u64>().ok()?, b.trim().parse::<u64>().ok()?);
    if end < start || total.is_some_and(|t| end >= t) {
        return None;
    }
    Some(ContentRange::Bytes { start, end, total })
}

/// Parses `Content-Length`.
pub fn parse_length(v: &str) -> Option<u64> {
    v.trim().parse().ok()
}

/// How long a `Retry-After` asks to wait at `now`: delta-seconds or an HTTP date (a date in the past is zero).
pub fn parse_retry_after(v: &str, now: SystemTime) -> Option<Duration> {
    let v = v.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = httpdate::parse_http_date(v).ok()?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

/// The validator a resume and `If-Range` may rely on: a strong ETag, else `Last-Modified`.
pub(crate) fn validator(h: &Headers) -> Option<String> {
    match &h.etag {
        Some(e) if !e.trim_start().starts_with("W/") && !e.trim().is_empty() => {
            Some(e.trim().to_string())
        }
        _ => h
            .last_modified
            .as_ref()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty()),
    }
}

/// True when the body is sent as is (no `Content-Encoding` other than `identity`).
pub(crate) fn is_identity(h: &Headers) -> bool {
    h.content_encoding
        .as_deref()
        .is_none_or(|e| e.trim().eq_ignore_ascii_case("identity") || e.trim().is_empty())
}
