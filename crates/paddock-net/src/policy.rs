//! The limits of one [`Egress`](crate::Egress).

use crate::reach::Reach;
use std::time::Duration;

/// Redirects a default policy follows.
pub const DEFAULT_REDIRECTS: u32 = 5;
/// Most redirects any policy may allow.
pub const MAX_REDIRECTS: u32 = 10;
/// Most entries of the allow-list.
pub const MAX_PATTERNS: usize = 256;

/// A host the app may contact: `api.example.com` (exactly) or `*.example.com` (any subdomain, not the apex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPattern(String);

impl HostPattern {
    /// Parses a pattern (lower-cased); `None` for an empty one, one with a scheme, port, path or space, or a wildcard anywhere
    /// but the front.
    pub fn parse(text: &str) -> Option<HostPattern> {
        let t = text.trim().to_ascii_lowercase();
        let rest = t.strip_prefix("*.").unwrap_or(&t);
        let ok = !rest.is_empty()
            && !rest.contains(['*', '/', ':', ' ', '@', '?', '#'])
            && !rest.starts_with('.')
            && !rest.ends_with('.');
        ok.then_some(HostPattern(t))
    }

    /// Whether `host` matches.
    pub fn matches(&self, host: &str) -> bool {
        let h = host.trim_end_matches('.').to_ascii_lowercase();
        match self.0.strip_prefix("*.") {
            Some(suffix) => {
                h.len() > suffix.len() + 1
                    && h.ends_with(suffix)
                    && h.as_bytes()[h.len() - suffix.len() - 1] == b'.'
            }
            None => h == self.0,
        }
    }
}

/// What an app lets itself do on the network. The defaults are the strict ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// The farthest the app may reach ([`Reach::Public`] unless it talks to local services).
    pub reach: Reach,
    /// Redirects followed (each one is checked like a first request); 0 follows none.
    pub max_redirects: u32,
    /// Largest answer body read, in bytes.
    pub max_body: u64,
    /// Largest request body sent, in bytes.
    pub max_request_body: u64,
    /// Longest to wait for a connection.
    pub connect_timeout: Duration,
    /// Longest for a whole request including its body (not applied to streaming answers).
    pub timeout: Duration,
    /// The only hosts that may be contacted, or `None` for any.
    pub hosts: Option<Vec<HostPattern>>,
    /// Least time between two requests to one host (politeness); zero for none.
    pub min_gap: Duration,
    /// The `User-Agent` sent.
    pub user_agent: String,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            reach: Reach::Public,
            max_redirects: DEFAULT_REDIRECTS,
            max_body: 8 << 20,
            max_request_body: 1 << 20,
            connect_timeout: Duration::from_secs(10),
            timeout: Duration::from_secs(30),
            hosts: None,
            min_gap: Duration::ZERO,
            user_agent: concat!("paddock-net/", env!("CARGO_PKG_VERSION")).to_string(),
        }
    }
}

impl Policy {
    /// Sets how far the app may reach.
    pub fn reach(mut self, reach: Reach) -> Policy {
        self.reach = reach;
        self
    }

    /// Sets the redirect limit (at most [`MAX_REDIRECTS`]).
    pub fn max_redirects(mut self, n: u32) -> Policy {
        self.max_redirects = n.min(MAX_REDIRECTS);
        self
    }

    /// Sets the answer size limit.
    pub fn max_body(mut self, bytes: u64) -> Policy {
        self.max_body = bytes;
        self
    }

    /// Restricts the app to these hosts (patterns that do not parse are dropped; at most [`MAX_PATTERNS`]). An empty list
    /// allows no host.
    pub fn allow_hosts(mut self, patterns: &[&str]) -> Policy {
        self.hosts = Some(
            patterns
                .iter()
                .filter_map(|p| HostPattern::parse(p))
                .take(MAX_PATTERNS)
                .collect(),
        );
        self
    }

    /// Sets the least time between two requests to one host.
    pub fn min_gap(mut self, gap: Duration) -> Policy {
        self.min_gap = gap;
        self
    }

    /// Sets the whole-request time limit.
    pub fn timeout(mut self, t: Duration) -> Policy {
        self.timeout = t;
        self
    }

    /// Whether `host` is on the allow-list (always true without one).
    pub fn host_allowed(&self, host: &str) -> bool {
        self.hosts
            .as_ref()
            .is_none_or(|l| l.iter().any(|p| p.matches(host)))
    }
}
