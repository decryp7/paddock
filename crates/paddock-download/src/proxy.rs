//! Proxies, as plain values: where to connect ([`ProxyServer`], with credentials that never reach a log), which hosts skip the
//! proxy ([`NoProxy`]) and the per-scheme choice ([`ProxyRules`]). Everything here is pure (the environment and the operating
//! system's answer are passed in as text), so it is tested without a network; `HttpTransport` (feature `http`) applies it.
//!
//! Where rules come from, in the order [`ProxyRules::detect`] tries them:
//! 1. the environment: `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY`, each in lower case first, then upper case
//!    (an empty value counts as unset); `ALL_PROXY` fills the schemes the others leave open;
//! 2. the operating system, when the environment names no proxy: macOS system configuration (`scutil --proxy`), Windows
//!    Internet Settings (`HKCU\...\Internet Settings`, what browsers use), then the machine-wide WinHTTP proxy. Linux has no
//!    system store of its own (the environment is what its desktops export).
//!
//! Loopback addresses and `localhost` never go through a detected proxy. Automatic configuration scripts (PAC) and SOCKS
//! proxies are not supported; a PAC-only setup downloads directly, and a `socks://` address is refused with a message.

use crate::error::DownloadError;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};

/// Longest proxy address or bypass list read (settings come from a user or the system, so they are bounded).
pub const MAX_PROXY_TEXT: usize = 8 * 1024;
/// Most bypass entries kept.
pub const MAX_NO_PROXY_ENTRIES: usize = 256;

/// The login for an authenticated proxy (HTTP Basic, sent as `Proxy-Authorization` on the tunnel request). Its `Debug` output
/// hides the password, and nothing in this crate formats it into a message.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProxyCredentials {
    username: String,
    password: String,
}

impl ProxyCredentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        ProxyCredentials {
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    pub(crate) fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for ProxyCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyCredentials")
            .field("username", &self.username)
            .field("password", &"<hidden>")
            .finish()
    }
}

/// How the client talks to the proxy itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProxyScheme {
    /// `http://`: a CONNECT proxy over plain TCP (by far the most common, also for `https` targets).
    Http,
    /// `https://`: a CONNECT proxy over TLS.
    Https,
}

impl ProxyScheme {
    fn default_port(self) -> u16 {
        match self {
            ProxyScheme::Http => 80,
            ProxyScheme::Https => 443,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ProxyScheme::Http => "http",
            ProxyScheme::Https => "https",
        }
    }
}

/// One proxy: scheme, host, port and optional login.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProxyServer {
    scheme: ProxyScheme,
    host: String,
    port: u16,
    credentials: Option<ProxyCredentials>,
}

impl ProxyServer {
    pub fn new(scheme: ProxyScheme, host: impl Into<String>, port: u16) -> Self {
        ProxyServer {
            scheme,
            host: host.into(),
            port,
            credentials: None,
        }
    }

    /// Parses `host`, `host:port`, `http://host:port` or `http://user:pass@host:port` (user and password percent-decoded; an
    /// IPv6 host in brackets). Without a scheme it is `http`; without a port, the scheme's. Refuses SOCKS and anything else, with
    /// a message that never repeats the text (it may hold a password).
    pub fn parse(text: &str) -> Result<Self, DownloadError> {
        let bad =
            |why: &str| DownloadError::InvalidRequest(format!("invalid proxy address: {why}"));
        let t = text.trim();
        if t.is_empty() || t.len() > MAX_PROXY_TEXT {
            return Err(bad("it is empty or too long"));
        }
        if t.chars().any(|c| c.is_control() || c == ' ') {
            return Err(bad("it contains spaces or control characters"));
        }
        let (scheme, rest) = match t.split_once("://") {
            Some((s, rest)) => (
                match s.to_ascii_lowercase().as_str() {
                    "http" => ProxyScheme::Http,
                    "https" => ProxyScheme::Https,
                    s if s.starts_with("socks") => {
                        return Err(bad("SOCKS proxies are not supported; use an HTTP proxy"));
                    }
                    _ => return Err(bad("the scheme must be http or https")),
                },
                rest,
            ),
            None => (ProxyScheme::Http, t),
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, authority),
        };
        let (host, port) =
            split_host_port(hostport).ok_or_else(|| bad("the host or port is malformed"))?;
        if host.is_empty() {
            return Err(bad("it has no host"));
        }
        let credentials = match userinfo {
            None => None,
            Some(u) => {
                let (user, pass) = u.split_once(':').unwrap_or((u, ""));
                let (user, pass) = (
                    percent_decode(user).ok_or_else(|| bad("the user name is malformed"))?,
                    percent_decode(pass).ok_or_else(|| bad("the password is malformed"))?,
                );
                Some(ProxyCredentials::new(user, pass))
            }
        };
        Ok(ProxyServer {
            scheme,
            host,
            port: port.unwrap_or(scheme.default_port()),
            credentials,
        })
    }

    /// The same proxy with a login (replaces one given in the address).
    pub fn with_credentials(mut self, username: &str, password: &str) -> Self {
        self.credentials = Some(ProxyCredentials::new(username, password));
        self
    }

    pub fn scheme(&self) -> ProxyScheme {
        self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn credentials(&self) -> Option<&ProxyCredentials> {
        self.credentials.as_ref()
    }
}

/// `scheme://host:port`, never the login.
impl fmt::Display for ProxyServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(f, "{}://[{}]:{}", self.scheme.name(), self.host, self.port)
        } else {
            write!(f, "{}://{}:{}", self.scheme.name(), self.host, self.port)
        }
    }
}

impl fmt::Debug for ProxyServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProxyServer({self}")?;
        if let Some(c) = &self.credentials {
            write!(f, ", user {:?}, password hidden", c.username)?;
        }
        f.write_str(")")
    }
}

/// `host[:port]` or `[v6][:port]`.
fn split_host_port(s: &str) -> Option<(String, Option<u16>)> {
    if let Some(rest) = s.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':')?.parse().ok()?),
        };
        return Some((host.to_ascii_lowercase(), port));
    }
    match s.split_once(':') {
        None => Some((s.to_ascii_lowercase(), None)),
        Some((h, p)) if !p.contains(':') => Some((h.to_ascii_lowercase(), Some(p.parse().ok()?))),
        Some(_) => None,
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Rule {
    /// `*`: every host.
    All,
    /// `<local>`: names without a dot (Windows).
    Local,
    /// `example.com`: the host and its subdomains.
    Domain(String),
    /// `.example.com` and `*.example.com`: subdomains only.
    Subdomains(String),
    /// A pattern with `*` inside (`10.*`).
    Glob(String),
    Ip(IpAddr),
    Cidr(IpAddr, u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    rule: Rule,
    port: Option<u16>,
}

/// The hosts that skip the proxy (`NO_PROXY`, the macOS exceptions list, the Windows bypass list).
///
/// Entries are separated by commas, semicolons or white space and matched case-insensitively. Forms: `*` (everything),
/// `example.com` (that host and its subdomains), `.example.com` or `*.example.com` (subdomains only), `10.*` (a pattern),
/// `192.168.0.0/16`, `169.254/16` (an IPv4 range, short forms allowed), an IP literal, `<local>` (names without a dot) and any
/// of these with `:port`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoProxy(Vec<Entry>);

impl NoProxy {
    /// Reads a list; entries that are not one of the forms are skipped.
    pub fn parse(list: &str) -> Self {
        let mut n = NoProxy::default();
        n.extend(list);
        n
    }

    /// Adds the entries of another list (at most [`MAX_NO_PROXY_ENTRIES`] in all).
    pub fn extend(&mut self, list: &str) {
        let list = &list[..floor_char(list, MAX_PROXY_TEXT)];
        for token in list.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
            if self.0.len() >= MAX_NO_PROXY_ENTRIES {
                return;
            }
            if let Some(e) = parse_entry(token) {
                self.0.push(e);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// True when a request to `host:port` goes direct.
    pub fn matches(&self, host: &str, port: u16) -> bool {
        let host = host
            .trim_matches(['[', ']'])
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let ip = host.parse::<IpAddr>().ok();
        self.0.iter().any(|e| {
            e.port.is_none_or(|p| p == port)
                && match &e.rule {
                    Rule::All => true,
                    Rule::Local => ip.is_none() && !host.contains('.'),
                    Rule::Domain(d) => {
                        host == *d
                            || host
                                .strip_suffix(d.as_str())
                                .is_some_and(|p| p.ends_with('.'))
                    }
                    Rule::Subdomains(d) => host
                        .strip_suffix(d.as_str())
                        .is_some_and(|p| p.ends_with('.')),
                    Rule::Glob(p) => glob(p.as_bytes(), host.as_bytes()),
                    Rule::Ip(a) => ip == Some(*a),
                    Rule::Cidr(net, bits) => ip.is_some_and(|a| in_cidr(a, *net, *bits)),
                }
        })
    }
}

fn floor_char(s: &str, max: usize) -> usize {
    let mut i = max.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn parse_entry(token: &str) -> Option<Entry> {
    let t = token.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    if t == "*" {
        return Some(Entry {
            rule: Rule::All,
            port: None,
        });
    }
    if t == "<local>" {
        return Some(Entry {
            rule: Rule::Local,
            port: None,
        });
    }
    // An optional :port (not the colons of an IPv6 literal).
    let (body, port) = if let Some(rest) = t.strip_prefix('[') {
        let (h, after) = rest.split_once(']')?;
        (
            h.to_string(),
            after.strip_prefix(':').and_then(|p| p.parse().ok()),
        )
    } else if t.matches(':').count() == 1 {
        let (h, p) = t.split_once(':')?;
        (h.to_string(), Some(p.parse().ok()?))
    } else {
        (t, None)
    };
    if body.is_empty() {
        return None;
    }
    let rule = if let Some((net, bits)) = body.split_once('/') {
        let (net, bits) = (parse_net(net)?, bits.parse::<u8>().ok()?);
        if bits > if net.is_ipv4() { 32 } else { 128 } {
            return None;
        }
        Rule::Cidr(net, bits)
    } else if let Ok(ip) = body.parse::<IpAddr>() {
        Rule::Ip(ip)
    } else if let Some(d) = body.strip_prefix("*.").or_else(|| body.strip_prefix('.')) {
        if d.is_empty() || d.contains('*') {
            return None;
        }
        Rule::Subdomains(d.trim_end_matches('.').to_string())
    } else if body.contains('*') {
        Rule::Glob(body)
    } else {
        Rule::Domain(body.trim_end_matches('.').to_string())
    };
    Some(Entry { rule, port })
}

/// An IP, or a short IPv4 form (`169.254` is `169.254.0.0`).
fn parse_net(s: &str) -> Option<IpAddr> {
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Some(ip);
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut o = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        o[i] = p.parse().ok()?;
    }
    Some(IpAddr::V4(Ipv4Addr::from(o)))
}

fn in_cidr(addr: IpAddr, net: IpAddr, bits: u8) -> bool {
    let (a, n, width) = match (addr, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => (u32::from(a) as u128, u32::from(n) as u128, 32u32),
        (IpAddr::V6(a), IpAddr::V6(n)) => (u128::from(a), u128::from(n), 128u32),
        _ => return false,
    };
    let bits = u32::from(bits).min(width);
    if bits == 0 {
        return true;
    }
    let shift = width - bits;
    (a >> shift) == (n >> shift)
}

/// `*` matches any run of bytes (including none).
fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if pi < p.len() && p[pi] == s[si] {
            pi += 1;
            si += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

/// Which proxy, if any, a URL uses: one per scheme of the target, and the hosts that go direct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyRules {
    http: Option<ProxyServer>,
    https: Option<ProxyServer>,
    no_proxy: NoProxy,
}

impl ProxyRules {
    /// No proxy at all.
    pub fn none() -> Self {
        ProxyRules::default()
    }

    /// One proxy for `http` and `https` targets.
    pub fn all(server: ProxyServer) -> Self {
        ProxyRules {
            http: Some(server.clone()),
            https: Some(server),
            no_proxy: NoProxy::default(),
        }
    }

    /// Separate proxies for `http://` and `https://` targets.
    pub fn per_scheme(http: Option<ProxyServer>, https: Option<ProxyServer>) -> Self {
        ProxyRules {
            http,
            https,
            no_proxy: NoProxy::default(),
        }
    }

    /// Adds hosts that go direct (see [`NoProxy`] for the forms).
    pub fn with_no_proxy(mut self, list: &str) -> Self {
        self.no_proxy.extend(list);
        self
    }

    /// True when no scheme has a proxy.
    pub fn is_empty(&self) -> bool {
        self.http.is_none() && self.https.is_none()
    }

    pub fn no_proxy(&self) -> &NoProxy {
        &self.no_proxy
    }

    /// The proxy a request to `url` goes through, or `None` for a direct connection (no proxy for its scheme, or the host is on
    /// the bypass list).
    pub fn for_url(&self, url: &str) -> Option<&ProxyServer> {
        let (https, host, port) = target_of(url)?;
        let server = if https {
            self.https.as_ref()
        } else {
            self.http.as_ref()
        }?;
        (!self.no_proxy.matches(&host, port)).then_some(server)
    }

    /// Reads the proxy variables through `get` (name to value); see the module docs for the names. An unusable value is an
    /// error naming the variable, never its text.
    pub fn from_env_with(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, DownloadError> {
        let var = |lower: &str| -> Result<Option<ProxyServer>, DownloadError> {
            let upper = lower.to_ascii_uppercase();
            let Some(value) = [lower, upper.as_str()]
                .iter()
                .find_map(|n| get(n).filter(|v| !v.trim().is_empty()))
            else {
                return Ok(None);
            };
            ProxyServer::parse(&value).map(Some).map_err(|e| {
                DownloadError::InvalidRequest(format!(
                    "{upper} is not a usable proxy: {}",
                    e.to_string().trim_start_matches("invalid proxy address: ")
                ))
            })
        };
        let all = var("all_proxy")?;
        let mut rules = ProxyRules {
            http: var("http_proxy")?.or_else(|| all.clone()),
            https: var("https_proxy")?.or(all),
            no_proxy: NoProxy::default(),
        };
        if let Some(list) = ["no_proxy", "NO_PROXY"].iter().find_map(|n| get(n)) {
            rules.no_proxy.extend(&list);
        }
        Ok(rules)
    }

    /// [`from_env_with`](Self::from_env_with) over the process environment.
    pub fn from_env() -> Result<Self, DownloadError> {
        Self::from_env_with(&|n| std::env::var(n).ok())
    }

    /// The environment first, then (when it names no proxy) the operating system's settings; loopback always goes direct. This
    /// is what `ProxyMode::System` uses.
    pub fn detect() -> Result<Self, DownloadError> {
        Self::detect_with(&|n| std::env::var(n).ok(), &crate::proxy_os::system_rules)
    }

    /// [`detect`](Self::detect) with the environment and the system's answer injected.
    pub fn detect_with(
        get: &dyn Fn(&str) -> Option<String>,
        system: &dyn Fn() -> ProxyRules,
    ) -> Result<Self, DownloadError> {
        let mut rules = Self::from_env_with(get)?;
        if rules.is_empty() {
            let os = system();
            rules.http = os.http;
            rules.https = os.https;
            rules.no_proxy.0.extend(os.no_proxy.0);
            rules.no_proxy.0.truncate(MAX_NO_PROXY_ENTRIES);
        }
        rules
            .no_proxy
            .extend("localhost,.localhost,127.0.0.0/8,::1");
        Ok(rules)
    }

    pub(crate) fn from_parts(
        http: Option<ProxyServer>,
        https: Option<ProxyServer>,
        no_proxy: NoProxy,
    ) -> Self {
        ProxyRules {
            http,
            https,
            no_proxy,
        }
    }
}

/// `(is https, host, port)` of a URL, without user info; `None` for another scheme.
pub(crate) fn target_of(url: &str) -> Option<(bool, String, u16)> {
    let lower = url.to_ascii_lowercase();
    let (https, rest) = if let Some(r) = lower.strip_prefix("https://") {
        (true, r)
    } else {
        (false, lower.strip_prefix("http://")?)
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let hostport = authority.rsplit('@').next()?;
    let (host, port) = split_host_port(hostport)?;
    Some((https, host, port.unwrap_or(if https { 443 } else { 80 })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |n| m.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone())
    }

    #[test]
    fn addresses_parse_with_defaults_and_decoded_logins() {
        let p = ProxyServer::parse("Proxy.Corp").unwrap();
        assert_eq!(
            (p.scheme(), p.host(), p.port()),
            (ProxyScheme::Http, "proxy.corp", 80)
        );
        let p = ProxyServer::parse("https://[::1]:8443/").unwrap();
        assert_eq!(
            (p.scheme(), p.host(), p.port()),
            (ProxyScheme::Https, "::1", 8443)
        );
        assert_eq!(p.to_string(), "https://[::1]:8443");
        let p = ProxyServer::parse("http://al%40ice:p%3Ass@10.0.0.1:3128").unwrap();
        assert_eq!(p.credentials().unwrap().username(), "al@ice");
        assert_eq!(p.credentials().unwrap().password(), "p:ss");
        assert_eq!(p.port(), 3128);
    }

    #[test]
    fn bad_addresses_are_refused_without_echoing_the_text() {
        for bad in [
            "",
            "socks5://u:secret@h:1",
            "ftp://h",
            "http://",
            "http://u:secret@:80",
            "h:notaport",
            "http://[::1",
            "a b",
            "http://u:%zz@h",
        ] {
            let e = ProxyServer::parse(bad).unwrap_err().to_string();
            assert!(e.starts_with("invalid proxy address"), "{bad}: {e}");
            assert!(!e.contains("secret"), "{e}");
        }
    }

    #[test]
    fn credentials_stay_out_of_debug_and_display() {
        let p = ProxyServer::parse("http://alice:hunter2@h:8").unwrap();
        for s in [
            format!("{p:?}"),
            p.to_string(),
            format!("{:?}", ProxyRules::all(p.clone())),
            format!("{:?}", p.credentials()),
        ] {
            assert!(!s.contains("hunter2"), "{s}");
        }
        assert!(format!("{p:?}").contains("alice"));
        let q = ProxyServer::new(ProxyScheme::Http, "h", 8).with_credentials("u", "pw");
        assert_eq!(q.credentials().unwrap().password(), "pw");
    }

    #[test]
    fn no_proxy_forms_match_like_the_tools_people_know() {
        let n = NoProxy::parse(
            "example.com, .internal.test;*.lan 10.* 192.168.0.0/16,169.254/16 [::1] box:8080 <local>",
        );
        let yes = [
            ("example.com", 80),
            ("www.EXAMPLE.com", 443),
            ("a.internal.test", 80),
            ("host.lan", 80),
            ("10.2.3.4", 80),
            ("192.168.9.9", 80),
            ("169.254.169.254", 80),
            ("::1", 80),
            ("box", 8080),
            ("printer", 80),
            ("example.com.", 80),
        ];
        for (h, p) in yes {
            assert!(n.matches(h, p), "{h}");
        }
        for (h, p) in [
            ("notexample.com", 80),
            ("internal.test", 80),
            ("8.8.8.8", 80),
            ("172.16.0.1", 80),
            ("a.b.example.org", 80),
        ] {
            assert!(!n.matches(h, p), "{h}");
        }
        assert!(NoProxy::parse("*").matches("anything.example", 1));
        assert!(!NoProxy::parse("").matches("x", 1));
        assert!(!NoProxy::parse("host:8080").matches("host", 80));
    }

    #[test]
    fn malformed_no_proxy_entries_are_skipped_and_the_list_is_bounded() {
        assert!(NoProxy::parse("a/99, 1.2.3.4/33, *.*x, :80, host:abc").is_empty());
        let many = (0..1000)
            .map(|i| format!("h{i}.test"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(NoProxy::parse(&many).0.len(), MAX_NO_PROXY_ENTRIES);
    }

    #[test]
    fn environment_names_work_in_both_cases_and_all_proxy_fills_the_gaps() {
        let r = ProxyRules::from_env_with(&env(&[
            ("HTTPS_PROXY", "s:1"),
            ("all_proxy", "a:2"),
            ("NO_PROXY", "x.test"),
        ]))
        .unwrap();
        assert_eq!(r.for_url("https://h.test/f").unwrap().host(), "s");
        assert_eq!(r.for_url("http://h.test/f").unwrap().host(), "a");
        assert!(
            r.for_url("https://x.test/f").is_none() && r.for_url("https://sub.x.test/").is_none()
        );
        // Lower case wins over upper case; empty is unset.
        let r = ProxyRules::from_env_with(&env(&[
            ("http_proxy", "lo:1"),
            ("HTTP_PROXY", "up:1"),
            ("https_proxy", " "),
        ]))
        .unwrap();
        assert_eq!(r.for_url("http://h.test").unwrap().host(), "lo");
        assert!(r.for_url("https://h.test").is_none());
        assert!(ProxyRules::from_env_with(&env(&[])).unwrap().is_empty());
        assert!(r.for_url("ftp://h.test").is_none());
    }

    #[test]
    fn a_bad_variable_is_named_but_not_quoted() {
        let e = ProxyRules::from_env_with(&env(&[("https_proxy", "socks5://u:secret@h:1")]))
            .unwrap_err()
            .to_string();
        assert!(e.contains("HTTPS_PROXY") && !e.contains("secret"), "{e}");
    }

    #[test]
    fn detect_prefers_the_environment_then_the_system_and_always_skips_loopback() {
        let os = || ProxyRules::all(ProxyServer::parse("os:1").unwrap()).with_no_proxy("*.corp");
        let r = ProxyRules::detect_with(&env(&[("HTTPS_PROXY", "env:2")]), &os).unwrap();
        assert_eq!(r.for_url("https://h.test").unwrap().host(), "env");
        assert!(
            r.for_url("http://h.test").is_none(),
            "the environment decides alone"
        );
        let r = ProxyRules::detect_with(&env(&[]), &os).unwrap();
        assert_eq!(r.for_url("https://h.test").unwrap().host(), "os");
        assert!(r.for_url("https://a.corp").is_none());
        for u in [
            "http://localhost:9/",
            "http://127.0.0.1:9/",
            "https://[::1]/",
            "http://x.localhost/",
        ] {
            assert!(r.for_url(u).is_none(), "{u}");
        }
        // A NO_PROXY from the environment still applies when the system supplies the proxy.
        let r = ProxyRules::detect_with(&env(&[("no_proxy", "skip.test")]), &os).unwrap();
        assert!(r.for_url("https://skip.test").is_none() && r.for_url("https://a.corp").is_none());
        assert!(r.for_url("https://h.test").is_some());
    }

    #[test]
    fn targets_are_read_without_user_info_and_with_default_ports() {
        assert_eq!(
            target_of("HTTPS://u:p@Host.Test/x?y"),
            Some((true, "host.test".into(), 443))
        );
        assert_eq!(
            target_of("http://[::1]:81/"),
            Some((false, "::1".into(), 81))
        );
        assert_eq!(target_of("file:///x"), None);
    }
}
