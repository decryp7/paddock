//! What goes out and what comes back.

use crate::error::NetError;
use crate::reach::LanAccess;
use paddock_download::{Header, parse_retry_after};
use std::fmt;
use std::time::{Duration, SystemTime};

/// Headers the transport owns; a caller cannot set them.
const OWNED: [&str; 8] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "te",
    "upgrade",
    "proxy-authorization",
    "accept-encoding",
];
const MAX_HEADERS: usize = 32;
const MAX_HEADER_VALUE: usize = 8 * 1024;

/// The request methods the policy allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Read.
    Get,
    /// Read the headers only.
    Head,
    /// Send a body.
    Post,
}

impl Method {
    /// The method name.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
        }
    }
}

/// One request. `Debug` shows header names only, because values are often secrets.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The address.
    pub url: String,
    /// The headers added by the caller.
    pub headers: Vec<Header>,
    /// The body of a POST.
    pub body: Vec<u8>,
    /// How private-network addresses are treated (see [`Request::without_lan`], [`Request::plain_http_on_lan`]).
    pub lan: LanAccess,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|h| h.name.as_str()).collect();
        write!(
            f,
            "Request({} {}, headers {names:?}, {} body bytes)",
            self.method.as_str(),
            crate::egress::shown(&self.url),
            self.body.len()
        )
    }
}

impl Request {
    /// A GET.
    pub fn get(url: &str) -> Request {
        Request {
            method: Method::Get,
            url: url.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
            lan: LanAccess::Policy,
        }
    }

    /// Refuses a destination on a private network even when the policy's reach is [`Reach::Lan`](crate::Reach::Lan) or farther,
    /// so one egress with a wide reach can still serve requests that must stay public. This computer is not affected.
    pub fn without_lan(mut self) -> Request {
        self.lan = LanAccess::Deny;
        self
    }

    /// Allows plain `http://` to a host that is spelled like a private-network host ([`host_may_be_lan`](crate::host_may_be_lan)) and
    /// **whose addresses are all private**, checked before the hop and again on the addresses the socket connects to. Needs a
    /// policy reach of [`Reach::Lan`](crate::Reach::Lan) or farther.
    pub fn plain_http_on_lan(mut self) -> Request {
        self.lan = LanAccess::PlainHttp;
        self
    }

    /// A HEAD.
    pub fn head(url: &str) -> Request {
        Request {
            method: Method::Head,
            ..Request::get(url)
        }
    }

    /// A POST with a body.
    pub fn post(url: &str, body: Vec<u8>) -> Request {
        Request {
            method: Method::Post,
            body,
            ..Request::get(url)
        }
    }

    /// Adds a header. The name must be a token, the value one line of at most 8 KiB, and the header not one the transport owns
    /// (`Host`, `Content-Length`, `Accept-Encoding`, framing and proxy headers); at most 32 headers.
    pub fn header(mut self, name: &str, value: &str) -> Result<Request, NetError> {
        let bad = |why: &str| {
            NetError::Invalid(format!(
                "header '{}' {why}",
                name.chars().take(40).collect::<String>()
            ))
        };
        let token = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c);
        if name.is_empty() || name.len() > 128 || !name.chars().all(token) {
            return Err(bad("is not a valid header name"));
        }
        if OWNED.iter().any(|h| h.eq_ignore_ascii_case(name)) {
            return Err(bad("is set by the transport and cannot be added"));
        }
        if value.len() > MAX_HEADER_VALUE
            || value
                .chars()
                .any(|c| (c.is_control() && c != '\t') || c == '\u{7f}')
        {
            return Err(bad(
                "has a value that is too long or has control characters",
            ));
        }
        if self.headers.len() >= MAX_HEADERS {
            return Err(NetError::Invalid(format!(
                "at most {MAX_HEADERS} headers are allowed"
            )));
        }
        self.headers.push(Header {
            name: name.to_string(),
            value: value.to_string(),
        });
        Ok(self)
    }

    /// Makes the request conditional: the server answers 304 when `etag` is still current.
    pub fn if_none_match(self, etag: &str) -> Result<Request, NetError> {
        self.header("If-None-Match", etag)
    }
}

/// The answer to a request, body read and capped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The HTTP status.
    pub status: u16,
    /// The address that answered (after redirects), without query or login.
    pub url: String,
    /// Headers, names lower-cased.
    pub headers: Vec<(String, String)>,
    /// The body (empty for HEAD and 304).
    pub body: Vec<u8>,
    /// Redirects followed.
    pub redirects: u32,
}

impl Response {
    /// A header value.
    pub fn header(&self, name: &str) -> Option<&str> {
        header_of(&self.headers, name)
    }

    /// The `ETag`, to send back as `If-None-Match` next time.
    pub fn etag(&self) -> Option<&str> {
        self.header("etag")
    }

    /// Whether the conditional request found nothing new (304).
    pub fn not_modified(&self) -> bool {
        self.status == 304
    }

    /// Whether the status is 2xx.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// What `Retry-After` asks for, at `now`.
    pub fn retry_after(&self, now: SystemTime) -> Option<Duration> {
        self.header("retry-after")
            .and_then(|v| parse_retry_after(v, now))
    }

    /// The body as text (invalid bytes replaced).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub(crate) fn header_of<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
