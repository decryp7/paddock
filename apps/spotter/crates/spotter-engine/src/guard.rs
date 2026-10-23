//! The SSRF-ish guard: a watch may not reach this computer or the link-local range (cloud metadata services,
//! router admin pages) unless it says it is local. Applied to the first request and to every redirect.
use spotter_core::source::SourceError;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use url::{Host, Url};

/// Whether the address is loopback, link-local or unspecified (also as an IPv4-mapped IPv6 address).
pub fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => local_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return local_v4(mapped);
            }
            v6.is_loopback() || v6.is_unspecified() || is_link_local_v6(v6)
        }
    }
}

fn local_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast()
}

fn is_link_local_v6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Why a name's addresses were refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VetError {
    /// At least one address is this computer or its local link.
    Local,
    /// The name has no address at all.
    NoAddress,
}

impl std::fmt::Display for VetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            VetError::Local => "the name points at this computer or its local link",
            VetError::NoAddress => "the name has no address",
        })
    }
}

impl std::error::Error for VetError {}

/// The policy for the addresses of one name: all of them must be public (a name that points at this computer even
/// once is refused, as a rebinding answer would), and there must be at least one.
pub fn vet_addresses(addrs: Vec<IpAddr>) -> Result<Vec<IpAddr>, VetError> {
    if addrs.is_empty() {
        return Err(VetError::NoAddress);
    }
    if addrs.iter().any(|a| is_local_ip(*a)) {
        return Err(VetError::Local);
    }
    Ok(addrs)
}

/// Whether a failed request failed because [`GuardedResolver`] refused a local address.
pub fn refused_as_local(e: &(dyn std::error::Error + 'static)) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = current {
        if err.downcast_ref::<VetError>() == Some(&VetError::Local) {
            return true;
        }
        current = err.source();
    }
    false
}

/// Looks a name up; injected so tests can answer without a network.
pub type Lookup = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = std::io::Result<Vec<IpAddr>>> + Send>> + Send + Sync>;

/// The HTTP client's resolver for watches that are not local: it applies the guard to the very addresses the client
/// connects to. The check before a request cannot do that on its own: a name that answers with a public address for the
/// check and with `127.0.0.1` for the connection (DNS rebinding) would slip through. Resolving here pins the answer.
#[derive(Clone)]
pub struct GuardedResolver {
    lookup: Lookup,
}

impl GuardedResolver {
    /// The operating system's resolver.
    pub fn system() -> GuardedResolver {
        GuardedResolver::with_lookup(Arc::new(|name| Box::pin(async move { system_resolver(name, 0).await })))
    }

    /// A resolver that asks `lookup`.
    pub fn with_lookup(lookup: Lookup) -> GuardedResolver {
        GuardedResolver { lookup }
    }

    /// Every address of `name`, unvetted.
    async fn addresses(&self, name: String) -> std::io::Result<Vec<IpAddr>> {
        (self.lookup)(name).await
    }
}

impl reqwest::dns::Resolve for GuardedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let (host, lookup) = (name.as_str().to_string(), self.lookup.clone());
        Box::pin(async move {
            let addrs = lookup(host).await?;
            let vetted = vet_addresses(addrs)?;
            Ok(Box::new(vetted.into_iter().map(|ip| SocketAddr::new(ip, 0))) as reqwest::dns::Addrs)
        })
    }
}

/// Checks that `url` may be requested: http or https, and not local unless `allow_local`.
pub async fn check_url(url: &Url, allow_local: bool) -> Result<(), SourceError> {
    check_url_with(url, allow_local, system_resolver).await
}

/// [`check_url`] asking `resolver`, the one the HTTP client connects with.
pub async fn check_url_via(resolver: &GuardedResolver, url: &Url, allow_local: bool) -> Result<(), SourceError> {
    check_url_with(url, allow_local, |name, _port| resolver.addresses(name)).await
}

/// The operating system's resolver: every address of `name`.
async fn system_resolver(name: String, port: u16) -> std::io::Result<Vec<IpAddr>> {
    Ok(tokio::net::lookup_host((name, port)).await?.map(|a| a.ip()).collect())
}

/// The policy behind [`check_url`] with the name resolution injected, so it can be tested without a network.
async fn check_url_with<R, F>(url: &Url, allow_local: bool, resolve: R) -> Result<(), SourceError>
where
    R: FnOnce(String, u16) -> F,
    F: std::future::Future<Output = std::io::Result<Vec<IpAddr>>>,
{
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SourceError::InvalidConfig(format!(
            "{} is not an http:// or https:// address.",
            shown(url)
        )));
    }
    if allow_local {
        return Ok(());
    }
    let refuse = || {
        SourceError::InvalidConfig(format!(
            "{} is on this computer or its local link, which Spotter only reads when the watch is marked as a local service.",
            shown(url)
        ))
    };
    let port = url.port_or_known_default().unwrap_or(80);
    // http(s) addresses always have a host; the message is for a future scheme.
    let host = url
        .host()
        .ok_or_else(|| SourceError::InvalidConfig(format!("{} has no host name.", shown(url))))?;
    match host {
        Host::Ipv4(ip) => (!is_local_ip(IpAddr::V4(ip))).then_some(()).ok_or_else(refuse),
        Host::Ipv6(ip) => (!is_local_ip(IpAddr::V6(ip))).then_some(()).ok_or_else(refuse),
        Host::Domain(name) => {
            let addrs = resolve(name.to_string(), port)
                .await
                .map_err(|_| SourceError::Unreachable(format!("{name} could not be found. Spotter will try again.")))?;
            match vet_addresses(addrs) {
                Ok(_) => Ok(()),
                Err(VetError::Local) => Err(refuse()),
                Err(VetError::NoAddress) => Err(SourceError::Unreachable(format!("{name} has no address. Spotter will try again."))),
            }
        }
    }
}

/// The address without query or credentials, for messages.
fn shown(url: &Url) -> String {
    let mut u = url.clone();
    u.set_query(None);
    u.set_fragment(None);
    let _ = u.set_username("");
    let _ = u.set_password(None);
    u.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn local_ranges_are_recognised() {
        for local in [
            "127.0.0.1",
            "127.9.9.9",
            "169.254.169.254",
            "0.0.0.0",
            "255.255.255.255",
            "::1",
            "::",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.1.1",
        ] {
            assert!(is_local_ip(ip(local)), "{local}");
        }
        for public in ["8.8.8.8", "192.168.1.5", "10.0.0.2", "2001:db8::1", "fec0::1", "::ffff:8.8.8.8"] {
            assert!(!is_local_ip(ip(public)), "{public}");
        }
    }

    #[tokio::test]
    async fn urls_are_checked() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(check_url(&u("https://8.8.8.8/x"), false).await.is_ok());
        assert!(
            check_url(&u("http://192.168.1.10/x"), false).await.is_ok(),
            "private LAN is allowed"
        );
        for bad in [
            "http://127.0.0.1:8080/",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data?token=secret",
        ] {
            let e = check_url(&u(bad), false).await.unwrap_err();
            assert!(matches!(e, SourceError::InvalidConfig(_)), "{bad}: {e:?}");
            assert!(!e.message().contains("secret"), "queries are not echoed");
        }
        assert!(check_url(&u("http://127.0.0.1/"), true).await.is_ok());
        assert!(check_url(&u("ftp://example.org/"), true).await.is_err());
        assert!(check_url(&u("file:///etc/passwd"), false).await.is_err());
    }

    #[tokio::test]
    async fn names_are_resolved_through_the_injected_resolver() {
        let u = Url::parse("http://some.example:8080/").unwrap();
        let fixed = |ips: Vec<IpAddr>| {
            move |_: String, port: u16| async move {
                assert_eq!(port, 8080);
                Ok(ips)
            }
        };
        assert!(check_url_with(&u, false, fixed(vec![ip("8.8.8.8")])).await.is_ok());
        let e = check_url_with(&u, false, fixed(vec![ip("8.8.8.8"), ip("127.0.0.1")]))
            .await
            .unwrap_err();
        assert!(matches!(e, SourceError::InvalidConfig(_)), "{e:?}");
        let e = check_url_with(&u, false, fixed(vec![])).await.unwrap_err();
        assert!(matches!(e, SourceError::Unreachable(_)), "{e:?}");
        let e = check_url_with(&u, false, |_, _| async { Err(std::io::Error::other("no dns")) })
            .await
            .unwrap_err();
        assert!(matches!(e, SourceError::Unreachable(_)), "{e:?}");
        let e = check_url_with(&u, true, |_, _| async { Err(std::io::Error::other("unused")) }).await;
        assert!(e.is_ok(), "allow_local skips resolution");
    }

    #[tokio::test]
    async fn the_system_resolver_answers_for_literals() {
        assert_eq!(system_resolver("127.0.0.1".into(), 80).await.unwrap(), vec![ip("127.0.0.1")]);
        let e = check_url(&Url::parse("http://localhost/").unwrap(), false).await.unwrap_err();
        assert!(matches!(e, SourceError::InvalidConfig(_)), "{e:?}");
    }
}

#[cfg(test)]
mod pin_tests {
    use super::*;
    use reqwest::dns::Resolve;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn resolver(answer: std::io::Result<Vec<IpAddr>>) -> GuardedResolver {
        let answer = Arc::new(std::sync::Mutex::new(Some(answer)));
        GuardedResolver::with_lookup(Arc::new(move |_| {
            let a = answer.lock().unwrap().take().unwrap_or_else(|| Ok(Vec::new()));
            Box::pin(async move { a })
        }))
    }

    async fn resolve(r: &GuardedResolver) -> Result<Vec<SocketAddr>, Box<dyn std::error::Error + Send + Sync>> {
        let name: reqwest::dns::Name = "pinned.test".parse().unwrap();
        Ok(r.resolve(name).await?.collect())
    }

    #[test]
    fn the_policy_needs_an_address_and_refuses_any_local_one() {
        assert_eq!(vet_addresses(vec![ip("8.8.8.8"), ip("2001:db8::1")]).unwrap().len(), 2);
        assert_eq!(vet_addresses(vec![]), Err(VetError::NoAddress));
        assert_eq!(vet_addresses(vec![ip("8.8.8.8"), ip("127.0.0.1")]), Err(VetError::Local));
        assert_eq!(vet_addresses(vec![ip("::ffff:169.254.169.254")]), Err(VetError::Local));
        assert!(VetError::Local.to_string().contains("this computer"));
        assert!(VetError::NoAddress.to_string().contains("no address"));
    }

    #[test]
    fn a_refusal_is_found_anywhere_in_an_error_chain() {
        #[derive(Debug)]
        struct Wrap(VetError);
        impl std::fmt::Display for Wrap {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("wrapped")
            }
        }
        impl std::error::Error for Wrap {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        assert_eq!(Wrap(VetError::Local).to_string(), "wrapped");
        assert!(refused_as_local(&Wrap(VetError::Local)));
        assert!(refused_as_local(&VetError::Local));
        assert!(!refused_as_local(&Wrap(VetError::NoAddress)));
        assert!(!refused_as_local(&std::io::Error::other("x")));
    }

    #[tokio::test]
    async fn the_resolver_hands_out_public_addresses_and_refuses_the_rest() {
        let ok = resolve(&resolver(Ok(vec![ip("93.184.216.34")]))).await.unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].ip(), ip("93.184.216.34"));
        for bad in [Ok(vec![ip("127.0.0.1")]), Ok(vec![]), Err(std::io::Error::other("no dns"))] {
            assert!(resolve(&resolver(bad)).await.is_err());
        }
        let e = resolve(&resolver(Ok(vec![ip("10.0.0.1"), ip("169.254.169.254")])))
            .await
            .unwrap_err();
        assert!(refused_as_local(e.as_ref()), "{e:?}");
    }

    #[tokio::test]
    async fn the_system_resolver_variant_resolves_literals() {
        let r = GuardedResolver::system();
        assert_eq!(r.addresses("127.0.0.1".into()).await.unwrap(), vec![ip("127.0.0.1")]);
        let e = check_url_via(&r, &Url::parse("http://localhost/").unwrap(), false)
            .await
            .unwrap_err();
        assert!(matches!(e, SourceError::InvalidConfig(_)), "{e:?}");
    }
}
