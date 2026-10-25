//! HTTP requests as values, and what HTTP answers mean for a check. The adapter performs the request; the core decides
//! what it asks and how it reads the answer.

use super::SourceError;

/// One GET request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The address.
    pub url: String,
    /// Extra headers (authorization, accept). Values may hold secrets: never log them.
    pub headers: Vec<(String, String)>,
}

impl HttpRequest {
    /// A request without extra headers.
    pub fn get(url: impl Into<String>) -> HttpRequest {
        HttpRequest {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    /// The same request with one more header.
    pub fn header(mut self, name: &str, value: impl Into<String>) -> HttpRequest {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The same request with a header when `value` is given.
    pub fn header_if(self, name: &str, value: Option<String>) -> HttpRequest {
        match value {
            Some(v) => self.header(name, v),
            None => self,
        }
    }

    /// The URL without its query, for messages (queries may hold tokens).
    pub fn display_url(&self) -> &str {
        self.url.split(['?', '#']).next().unwrap_or(&self.url)
    }
}

/// What a non-success status means. `None` for 2xx.
pub fn status_error(status: u16, url: &str) -> Option<SourceError> {
    match status {
        200..=299 => None,
        401 | 403 => Some(SourceError::Unauthorized(format!(
            "{url} refused access (HTTP {status}). Check the token, or add one if the source is private."
        ))),
        404 | 410 => Some(SourceError::NotFound(format!(
            "{url} was not found (HTTP {status}). Check the address and the name."
        ))),
        429 => Some(SourceError::Unreachable(format!(
            "{url} is rate limiting requests (HTTP 429). Spotter will retry later; a token raises the limit."
        ))),
        500..=599 => Some(SourceError::Unreachable(format!(
            "{url} has a server problem (HTTP {status}). Spotter will retry."
        ))),
        _ => Some(SourceError::Failed(format!("{url} answered HTTP {status}."))),
    }
}

/// The parts of a `WWW-Authenticate: Bearer realm="…",service="…",scope="…"` challenge (OCI registries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BearerChallenge {
    /// Where to ask for a token.
    pub realm: String,
    /// The registry's service name.
    pub service: Option<String>,
    /// The scope to ask for.
    pub scope: Option<String>,
}

impl BearerChallenge {
    /// Parses a challenge header; `None` unless it is a Bearer challenge with an `https` (or local) realm.
    pub fn parse(header: &str) -> Option<BearerChallenge> {
        let rest = header.trim();
        let (scheme, params) = rest.split_once(' ')?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return None;
        }
        let mut realm = None;
        let mut service = None;
        let mut scope = None;
        for (key, value) in params_of(params) {
            match key.to_ascii_lowercase().as_str() {
                "realm" => realm = Some(value),
                "service" => service = Some(value),
                "scope" => scope = Some(value),
                _ => {}
            }
        }
        // Tokens only over HTTPS, except from a registry on this machine.
        let realm =
            realm.filter(|r| r.starts_with("https://") || r.starts_with("http://127.0.0.1") || r.starts_with("http://localhost"))?;
        Some(BearerChallenge { realm, service, scope })
    }

    /// The token request URL.
    pub fn token_url(&self) -> String {
        let mut url = self.realm.clone();
        let mut sep = if url.contains('?') { '&' } else { '?' };
        for (k, v) in [("service", &self.service), ("scope", &self.scope)] {
            if let Some(v) = v {
                url.push(sep);
                url.push_str(k);
                url.push('=');
                url.push_str(&encode_query(v));
                sep = '&';
            }
        }
        url
    }
}

fn params_of(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else { break };
        let key = key.trim().trim_start_matches(',').trim().to_string();
        let after = after.trim_start();
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            match quoted.split_once('"') {
                Some((v, n)) => (v.to_string(), n),
                None => (quoted.to_string(), ""),
            }
        } else {
            match after.split_once(',') {
                Some((v, n)) => (v.trim().to_string(), n),
                None => (after.trim().to_string(), ""),
            }
        };
        out.push((key, value));
        rest = next.trim_start().trim_start_matches(',').trim_start();
    }
    out
}

/// Percent-encodes a query value.
pub fn encode_query(v: &str) -> String {
    url::form_urlencoded::byte_serialize(v.as_bytes()).collect()
}

/// Percent-encodes one path segment (`/` included).
pub fn encode_segment(v: &str) -> String {
    v.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The token in an OCI token response (`token` or `access_token`).
pub fn parse_token(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("token")
        .or_else(|| v.get("access_token"))
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_build_and_hide_queries() {
        let r = HttpRequest::get("https://a.b/x?token=s#f")
            .header("A", "1")
            .header_if("B", None)
            .header_if("C", Some("3".into()));
        assert_eq!(r.headers, vec![("A".into(), "1".into()), ("C".into(), "3".into())]);
        assert_eq!(r.display_url(), "https://a.b/x");
    }

    #[test]
    fn statuses_map_to_errors() {
        assert_eq!(status_error(204, "u"), None);
        assert!(matches!(status_error(401, "u"), Some(SourceError::Unauthorized(_))));
        assert!(matches!(status_error(403, "u"), Some(SourceError::Unauthorized(_))));
        assert!(matches!(status_error(404, "u"), Some(SourceError::NotFound(_))));
        assert!(matches!(status_error(429, "u"), Some(SourceError::Unreachable(m)) if m.contains("rate")));
        assert!(matches!(status_error(503, "u"), Some(SourceError::Unreachable(_))));
        assert!(matches!(status_error(418, "u"), Some(SourceError::Failed(_))));
    }

    #[test]
    fn bearer_challenges_parse() {
        let c = BearerChallenge::parse(r#"Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:o/a:pull""#).unwrap();
        assert_eq!(c.service.as_deref(), Some("ghcr.io"));
        assert_eq!(
            c.token_url(),
            "https://ghcr.io/token?service=ghcr.io&scope=repository%3Ao%2Fa%3Apull"
        );
        let unquoted = BearerChallenge::parse("bearer realm=\"https://t.io/x?a=1\", scope=s, other=1").unwrap();
        assert_eq!(unquoted.token_url(), "https://t.io/x?a=1&scope=s");
        assert_eq!(
            BearerChallenge::parse("Bearer realm=\"https://r\",scope=last")
                .unwrap()
                .scope
                .as_deref(),
            Some("last")
        );
        assert_eq!(
            BearerChallenge::parse("Bearer realm=\"https://unterminated").unwrap().realm,
            "https://unterminated"
        );
        assert_eq!(BearerChallenge::parse("Basic realm=\"https://x\""), None);
        assert_eq!(BearerChallenge::parse("Bearer realm=\"http://insecure\""), None);
        assert!(BearerChallenge::parse("Bearer realm=\"http://127.0.0.1:5000/token\"").is_some());
        assert_eq!(BearerChallenge::parse("Bearer"), None);
        assert_eq!(BearerChallenge::parse("Bearer noequals"), None);
    }

    #[test]
    fn tokens_and_encoding() {
        assert_eq!(parse_token(br#"{"token":"t"}"#).as_deref(), Some("t"));
        assert_eq!(parse_token(br#"{"access_token":"a"}"#).as_deref(), Some("a"));
        assert_eq!(parse_token(br#"{"token":""}"#), None);
        assert_eq!(parse_token(b"nope"), None);
        assert_eq!(encode_segment("@scope/na me"), "%40scope%2Fna%20me");
        assert_eq!(encode_query("a b&c"), "a+b%26c");
    }
}
