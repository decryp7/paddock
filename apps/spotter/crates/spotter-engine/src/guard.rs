//! The SSRF-ish guard: a watch may not reach this computer or the link-local range (cloud metadata services,
//! router admin pages) unless it says it is local. Applied to the first request and to every redirect.
use spotter_core::source::SourceError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
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

/// Checks that `url` may be requested: http or https, and not local unless `allow_local`.
pub async fn check_url(url: &Url, allow_local: bool) -> Result<(), SourceError> {
    check_url_with(url, allow_local, system_resolver).await
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
    match url.host() {
        None => Err(SourceError::InvalidConfig(format!("{} has no host name.", shown(url)))),
        Some(Host::Ipv4(ip)) => (!is_local_ip(IpAddr::V4(ip))).then_some(()).ok_or_else(refuse),
        Some(Host::Ipv6(ip)) => (!is_local_ip(IpAddr::V6(ip))).then_some(()).ok_or_else(refuse),
        Some(Host::Domain(name)) => {
            let addrs = resolve(name.to_string(), port)
                .await
                .map_err(|_| SourceError::Unreachable(format!("{name} could not be found. Spotter will try again.")))?;
            let mut any = false;
            for a in addrs {
                any = true;
                if is_local_ip(a) {
                    return Err(refuse());
                }
            }
            if any {
                Ok(())
            } else {
                Err(SourceError::Unreachable(format!("{name} has no address. Spotter will try again.")))
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
