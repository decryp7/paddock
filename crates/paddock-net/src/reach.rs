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

fn classify_v6(ip: Ipv6Addr) -> Class {
    let s0 = ip.segments()[0];
    if ip.is_loopback() {
        Class::Loopback
    } else if ip.is_unspecified() || ip.is_multicast() || (s0 & 0xffc0) == 0xfe80 {
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
/// forbidden address even once is refused, as a DNS-rebinding answer would be). Returns the farthest class found.
pub fn vet(addrs: &[IpAddr], reach: Reach) -> Result<Class, Refusal> {
    if addrs.is_empty() {
        return Err(Refusal::NoAddress);
    }
    let mut farthest = Class::Public;
    for a in addrs {
        let c = classify(*a);
        if c == Class::Never {
            return Err(Refusal::Never);
        }
        if !reach.allows(c) {
            return Err(Refusal::TooFar(c));
        }
        if rank(c) > rank(farthest) {
            farthest = c;
        }
    }
    Ok(farthest)
}

fn rank(c: Class) -> u8 {
    match c {
        Class::Public => 0,
        Class::Lan => 1,
        Class::Loopback => 2,
        Class::Never => 3,
    }
}
