//! How far from the public internet an address is, and which addresses may never be reached.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The farthest an app may reach. Ordered: `Public < Lan < Loopback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Reach {
    /// Public addresses only (the default).
    #[default]
    Public,
    /// Public addresses and private networks (10/8, 172.16/12, 192.168/16, 100.64/10, fc00::/7).
    Lan,
    /// Everything above and this computer (127/8, ::1): for an app's own local services.
    Loopback,
}

/// How a request treats private-network addresses, on top of the policy's [`Reach`]. One value instead of two flags, so the two
/// cannot disagree; the wire applies it to the addresses it connects to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum LanAccess {
    /// Whatever the policy's reach allows.
    #[default]
    Policy,
    /// Never a private-network address, even when the policy's reach allows them (this computer is not affected).
    Deny,
    /// Plain `http://` is allowed, but only to private-network addresses: every address the host resolves to must be one
    /// (this computer excepted). `https://` is as with [`LanAccess::Policy`].
    PlainHttp,
}

/// What an address is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// A public address.
    Public,
    /// A private network address.
    Lan,
    /// This computer.
    Loopback,
    /// Never reachable: link-local (cloud metadata, router pages), unspecified, broadcast, multicast.
    Never,
}

impl Reach {
    /// Whether an address of this class may be reached.
    pub fn allows(self, class: Class) -> bool {
        match class {
            Class::Public => true,
            Class::Lan => self >= Reach::Lan,
            Class::Loopback => self >= Reach::Loopback,
            Class::Never => false,
        }
    }

    /// The reach an address needs.
    pub fn needed_for(class: Class) -> Option<Reach> {
        match class {
            Class::Public => Some(Reach::Public),
            Class::Lan => Some(Reach::Lan),
            Class::Loopback => Some(Reach::Loopback),
            Class::Never => None,
        }
    }
}

/// Classifies an address (an IPv4-mapped IPv6 address by its IPv4 part).
pub fn classify(ip: IpAddr) -> Class {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => classify_v4(v4),
            None => classify_v6(v6),
        },
    }
}

fn classify_v4(ip: Ipv4Addr) -> Class {
    let o = ip.octets();
    if ip.is_loopback() {
        Class::Loopback
    } else if ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || o[0] == 0
    {
        Class::Never
    } else if ip.is_private() || (o[0] == 100 && (o[1] & 0xc0) == 64) {
        Class::Lan
    } else {
        Class::Public
    }
}

/// The IPv4 address an IPv6 address carries, if it is one of the forms that tunnel or translate IPv4: NAT64 (`64:ff9b::/96`,
/// `64:ff9b:1::/48` with RFC 6052's /48 layout), IPv4-compatible (`::a.b.c.d`) and 6to4 (`2002::/16`). Such an address is as far
/// away as the IPv4 address inside it.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let o = ip.octets();
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0]
        || (s[..6] == [0, 0, 0, 0, 0, 0] && s[6..] != [0, 0] && s[6..] != [0, 1])
    {
        return Some(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
    }
    if s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1 {
        return Some(Ipv4Addr::new(o[6], o[7], o[9], o[10]));
    }
    if s[0] == 0x2002 {
        return Some(Ipv4Addr::new(o[2], o[3], o[4], o[5]));
    }
    None
}

fn classify_v6(ip: Ipv6Addr) -> Class {
    let s0 = ip.segments()[0];
    if let Some(v4) = embedded_v4(ip) {
        classify_v4(v4)
    } else if ip.is_loopback() {
        Class::Loopback
    } else if ip.is_unspecified() || ip.is_multicast() || (s0 & 0xffc0) == 0xfe80
        // Site-local (fec0::/10) is deprecated and was never a private-network range worth reaching.
        || (s0 & 0xffc0) == 0xfec0
    {
        Class::Never
    } else if (s0 & 0xfe00) == 0xfc00 {
        Class::Lan
    } else {
        Class::Public
    }
}

/// Whether a host name or literal is this computer by its spelling: `localhost`, `*.localhost`, `127.x.y.z`, `::1`.
pub fn host_is_loopback(host: &str) -> bool {
    let h = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    h.parse::<IpAddr>()
        .is_ok_and(|ip| classify(ip) == Class::Loopback)
}

/// Whether a host is spelled like one on a private network: an IP literal that [`classify`] calls [`Class::Lan`], or a name that
/// is a single label (`nas`) or ends in `.local`, `.lan`, `.home.arpa`, `.internal` or `.localdomain`. A name cannot be
/// classified without resolving it, so this is the spelling check for forms and plain-http decisions; [`vet`] still checks the
/// addresses the name resolves to.
pub fn host_may_be_lan(host: &str) -> bool {
    let h = host
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if h.is_empty() {
        return false;
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        return classify(ip) == Class::Lan;
    }
    if !h.contains('.') {
        return h != "localhost";
    }
    [".local", ".lan", ".home.arpa", ".internal", ".localdomain"]
        .iter()
        .any(|suffix| h.ends_with(suffix))
}

/// Why a name's addresses were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The name has no address.
    NoAddress,
    /// An address is never reachable (link-local, unspecified, multicast).
    Never,
    /// An address is on a network the app may not reach (private or this computer).
    TooFar(Class),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refusal::NoAddress => "the name has no address",
            Refusal::Never => "the name points at a link-local, unspecified or multicast address",
            Refusal::TooFar(Class::Loopback) => "the name points at this computer",
            Refusal::TooFar(_) => "the name points at a private network",
        })
    }
}

impl std::error::Error for Refusal {}

/// The policy for the addresses of one name: at least one, and **all** of them within `reach` (a name that points at a
/// forbidden address even once is refused, as a DNS-rebinding answer would be). Returns the reach the farthest of them needs.
pub fn vet(addrs: &[IpAddr], reach: Reach) -> Result<Reach, Refusal> {
    if addrs.is_empty() {
        return Err(Refusal::NoAddress);
    }
    let mut farthest = Reach::Public;
    for a in addrs {
        let c = classify(*a);
        if c == Class::Never {
            return Err(Refusal::Never);
        }
        if !reach.allows(c) {
            return Err(Refusal::TooFar(c));
        }
        farthest = farthest.max(Reach::needed_for(c).unwrap_or(Reach::Public));
    }
    Ok(farthest)
}
