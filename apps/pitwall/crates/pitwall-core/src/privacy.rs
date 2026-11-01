//! Local only (ADR 0001). Pitwall's only outbound traffic is downloading engines and models the user asked for, from
//! the hosts listed here, over HTTPS, and the check-only model update queries the user turned on (ADR 0017: the file
//! lists of repositories already downloaded from). Requests to the API never cause outbound traffic:
//! images must be inline, tools run locally, and the request path has no HTTP client (enforced in the server).
//!
//! This module decides which URLs a download may touch (every redirect hop is checked again) and which `Host` and
//! `Origin` headers the API accepts, so a web page cannot use Pitwall through DNS rebinding or cross-site requests.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The hosts downloads may reach, and why.
pub const DOWNLOAD_HOSTS: &[(&str, &str)] = &[
    ("huggingface.co", "Hugging Face model search, file lists and downloads"),
    ("hf.co", "Hugging Face (short domain)"),
    ("cdn-lfs.huggingface.co", "Hugging Face file storage"),
    ("cdn-lfs.hf.co", "Hugging Face file storage"),
    ("cdn-lfs-us-1.hf.co", "Hugging Face file storage"),
    ("cdn-lfs-eu-1.hf.co", "Hugging Face file storage"),
    ("cas-bridge.xethub.hf.co", "Hugging Face Xet storage"),
    ("transfer.xethub.hf.co", "Hugging Face Xet storage"),
    ("github.com", "llama.cpp engine releases"),
    ("objects.githubusercontent.com", "GitHub release downloads"),
    ("release-assets.githubusercontent.com", "GitHub release downloads"),
];

/// Hosts whose every subdomain is allowed (Hugging Face's CDN picks a regional host such as `us.aws.cdn.hf.co` per
/// redirect). The match is on whole labels, so `evilcdn.hf.co` does not match `cdn.hf.co`.
pub const DOWNLOAD_HOST_SUFFIXES: &[(&str, &str)] = &[
    ("cdn.hf.co", "Hugging Face CDN (regional hosts)"),
    ("xethub.hf.co", "Hugging Face Xet storage (regional hosts)"),
];

/// Whether `host` is allowed: listed exactly, or a subdomain of a listed suffix.
pub fn host_is_allowed(host: &str) -> bool {
    DOWNLOAD_HOSTS.iter().any(|(h, _)| *h == host)
        || DOWNLOAD_HOST_SUFFIXES
            .iter()
            .any(|(s, _)| host.strip_suffix(s).is_some_and(|p| p.ends_with('.') && p.len() > 1))
}

/// Why a URL may not be fetched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EgressError {
    /// The URL is not one Pitwall downloads from.
    #[error("Pitwall only downloads from Hugging Face and GitHub over HTTPS; {0} is not allowed")]
    NotAllowed(String),
}

/// A URL a download may fetch: `https`, default port, no credentials, an allowed host. Parsed once; holding one
/// means it was checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DownloadUrl {
    url: String,
    host: String,
}

impl DownloadUrl {
    /// Checks a URL against the allow-list.
    pub fn parse(url: &str) -> Result<DownloadUrl, EgressError> {
        let refuse = || EgressError::NotAllowed(shorten(url));
        let rest = url.strip_prefix("https://").ok_or_else(refuse)?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.contains('@') || authority.is_empty() || url.chars().any(|c| c.is_control() || c == ' ' || c == '\\') {
            return Err(refuse());
        }
        let host = match authority.rsplit_once(':') {
            Some((h, "443")) => h,
            Some(_) => return Err(refuse()),
            None => authority,
        }
        .to_ascii_lowercase();
        if !host_is_allowed(&host) {
            return Err(refuse());
        }
        Ok(DownloadUrl {
            url: url.to_string(),
            host,
        })
    }

    /// The URL.
    pub fn as_str(&self) -> &str {
        &self.url
    }

    /// Its host (shown in the UI next to every download).
    pub fn host(&self) -> &str {
        &self.host
    }
}

impl TryFrom<String> for DownloadUrl {
    type Error = EgressError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        DownloadUrl::parse(&s)
    }
}

impl From<DownloadUrl> for String {
    fn from(u: DownloadUrl) -> String {
        u.url
    }
}

impl fmt::Display for DownloadUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url)
    }
}

fn shorten(s: &str) -> String {
    let t: String = s.chars().take(120).collect();
    if t.len() < s.len() {
        format!("{t}…")
    } else {
        t
    }
}

/// The host part of a `Host` header or an origin's authority (`[::1]:8080` → `::1`, `localhost:11434` → `localhost`).
fn host_only(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or_default();
    }
    match authority.rsplit_once(':') {
        Some((h, p)) if p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => authority,
    }
}

/// Whether a host name means this machine.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost") || h == "::1" || h.starts_with("127.")
}

/// Whether the API accepts a request's `Host` header: loopback names always; with LAN exposure also the names and
/// addresses the user's certificate covers. A missing header is accepted (HTTP/1.0 tools).
pub fn host_allowed(host: Option<&str>, lan_names: &[String]) -> bool {
    match host {
        None => true,
        Some(h) => {
            let h = host_only(h.trim());
            is_loopback_host(h) || lan_names.iter().any(|n| n.eq_ignore_ascii_case(h))
        }
    }
}

/// Whether a browser `Origin` may use the API without a key: none (not a browser), `null` is refused, loopback pages
/// and Pitwall's own window are allowed. Other web pages need a key, which they cannot have.
pub fn origin_allowed_without_key(origin: Option<&str>) -> bool {
    let Some(o) = origin.map(str::trim) else { return true };
    if o == "tauri://localhost" {
        return true;
    }
    let Some(rest) = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")) else {
        return false;
    };
    is_loopback_host(host_only(rest.split('/').next().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hugging_face_regional_cdn_hosts_are_allowed_by_whole_label() {
        assert!(DownloadUrl::parse("https://us.aws.cdn.hf.co/xet-bridge-us/abc?x=1").is_ok());
        assert!(DownloadUrl::parse("https://cas-bridge.xethub.hf.co/x").is_ok());
        for bad in [
            "https://evilcdn.hf.co/x",
            "https://cdn.hf.co.evil.com/x",
            "https://.cdn.hf.co/x",
            "https://cdn.hf.co/x",
        ] {
            assert!(DownloadUrl::parse(bad).is_err(), "{bad}");
        }
        assert!(DOWNLOAD_HOST_SUFFIXES.iter().all(|(_, why)| !why.is_empty()));
    }

    #[test]
    fn only_listed_https_hosts() {
        let u = DownloadUrl::parse("https://huggingface.co/Qwen/Qwen3-8B-GGUF/resolve/main/x.gguf").unwrap();
        assert_eq!(u.host(), "huggingface.co");
        assert_eq!(u.to_string(), u.as_str());
        assert!(DownloadUrl::parse("https://GitHub.com:443/ggml-org/llama.cpp/releases").is_ok());
        for bad in [
            "http://huggingface.co/x",
            "https://huggingface.co.evil.com/x",
            "https://evil.com/huggingface.co",
            "https://user@huggingface.co/x",
            "https://huggingface.co:8443/x",
            "https://huggingface.co/x y",
            "https:///x",
            "ftp://github.com",
            "https://example.com/?q=github.com",
        ] {
            let e = DownloadUrl::parse(bad).unwrap_err();
            assert!(e.to_string().contains("only downloads"), "{bad}");
        }
        let long = format!("https://evil.com/{}", "a".repeat(300));
        assert!(DownloadUrl::parse(&long).unwrap_err().to_string().contains('…'));
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(serde_json::from_str::<DownloadUrl>(&json).unwrap(), u);
        assert!(serde_json::from_str::<DownloadUrl>("\"https://evil.com\"").is_err());
        assert!(DOWNLOAD_HOSTS.iter().all(|(_, why)| !why.is_empty()));
    }

    #[test]
    fn hosts_and_origins() {
        assert!(host_allowed(None, &[]));
        for ok in ["localhost:11434", "127.0.0.1:8080", "[::1]:80", "LOCALHOST", "pitwall.localhost"] {
            assert!(host_allowed(Some(ok), &[]), "{ok}");
        }
        assert!(!host_allowed(Some("evil.com"), &[]), "DNS rebinding");
        assert!(!host_allowed(Some("192.168.1.5:8443"), &[]));
        assert!(host_allowed(Some("192.168.1.5:8443"), &["192.168.1.5".into()]));
        assert_eq!(host_only("[::1"), "::1");
        assert_eq!(host_only("a:b"), "a:b");
        assert!(origin_allowed_without_key(None));
        assert!(origin_allowed_without_key(Some("tauri://localhost")));
        assert!(origin_allowed_without_key(Some("http://localhost:3000")));
        assert!(origin_allowed_without_key(Some("https://127.0.0.1")));
        assert!(!origin_allowed_without_key(Some("https://evil.com")));
        assert!(!origin_allowed_without_key(Some("null")));
    }
}
