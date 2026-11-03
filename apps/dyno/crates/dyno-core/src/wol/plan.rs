//! Where a magic packet goes: per network card, the limited broadcast (`255.255.255.255`) from a socket bound to the card's own
//! address, and the directed broadcast of its subnet (ADR 0032 section 3, D6). Pure: the interfaces are passed in.

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// One IPv4 address of one network card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetIf {
    /// The card's name (`en0`, `Ethernet`).
    pub name: String,
    /// Its address.
    pub addr: Ipv4Addr,
    /// The subnet's prefix length.
    pub prefix: u8,
}

/// The most sends one wake plans (a machine with many cards still stays bounded).
pub const MAX_SENDS: usize = 32;

/// One datagram to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendPlan {
    /// The card's name, for the report.
    pub interface: String,
    /// The address the socket binds to, so the datagram leaves through that card.
    pub source: Ipv4Addr,
    /// Where it goes.
    pub dest: Ipv4Addr,
}

/// The directed broadcast address of `addr/prefix`, `None` for a prefix with no broadcast address (/31, /32) or an invalid one.
pub fn directed_broadcast(addr: Ipv4Addr, prefix: u8) -> Option<Ipv4Addr> {
    if prefix == 0 || prefix > 30 {
        return None;
    }
    let mask = u32::MAX << (32 - u32::from(prefix));
    Some(Ipv4Addr::from(u32::from(addr) | !mask))
}

/// The datagrams to send: for each non-loopback, non-unspecified card that matches `device_interface` (every card when the
/// device's card is unknown or matches none of them), the limited broadcast and, when `directed`, the subnet broadcast.
pub fn plan_sends(interfaces: &[NetIf], device_interface: Option<&str>, directed: bool) -> Vec<SendPlan> {
    let usable = |i: &&NetIf| !i.addr.is_loopback() && !i.addr.is_unspecified() && !i.addr.is_multicast();
    let all: Vec<&NetIf> = interfaces.iter().filter(usable).collect();
    let matching: Vec<&NetIf> = match device_interface {
        Some(name) => all.iter().copied().filter(|i| i.name == name).collect(),
        None => Vec::new(),
    };
    let chosen = if matching.is_empty() { all } else { matching };
    let mut out: Vec<SendPlan> = Vec::new();
    for i in chosen {
        let mut dests = vec![Ipv4Addr::BROADCAST];
        if directed {
            if let Some(d) = directed_broadcast(i.addr, i.prefix) {
                dests.push(d);
            }
        }
        for dest in dests {
            let p = SendPlan {
                interface: i.name.clone(),
                source: i.addr,
                dest,
            };
            if out.len() < MAX_SENDS && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nif(name: &str, a: [u8; 4], prefix: u8) -> NetIf {
        NetIf {
            name: name.into(),
            addr: Ipv4Addr::from(a),
            prefix,
        }
    }

    #[test]
    fn the_directed_broadcast_fills_the_host_bits() {
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(192, 0, 2, 17), 24),
            Some(Ipv4Addr::new(192, 0, 2, 255))
        );
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(198, 51, 100, 5), 22),
            Some(Ipv4Addr::new(198, 51, 103, 255))
        );
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(10, 0, 0, 1), 8),
            Some(Ipv4Addr::new(10, 255, 255, 255))
        );
        assert_eq!(directed_broadcast(Ipv4Addr::new(10, 0, 0, 1), 31), None);
        assert_eq!(directed_broadcast(Ipv4Addr::new(10, 0, 0, 1), 32), None);
        assert_eq!(directed_broadcast(Ipv4Addr::new(10, 0, 0, 1), 0), None);
        assert_eq!(directed_broadcast(Ipv4Addr::new(10, 0, 0, 1), 200), None);
    }

    #[test]
    fn every_card_gets_both_broadcasts_when_the_device_card_is_unknown() {
        let ifs = [
            nif("lo0", [127, 0, 0, 1], 8),
            nif("en0", [192, 0, 2, 17], 24),
            nif("en1", [198, 51, 100, 5], 24),
        ];
        let p = plan_sends(&ifs, None, true);
        assert_eq!(p.len(), 4);
        assert_eq!(
            p[0],
            SendPlan {
                interface: "en0".into(),
                source: Ipv4Addr::new(192, 0, 2, 17),
                dest: Ipv4Addr::BROADCAST
            }
        );
        assert_eq!(p[1].dest, Ipv4Addr::new(192, 0, 2, 255));
        assert_eq!(p[3].dest, Ipv4Addr::new(198, 51, 100, 255));
    }

    #[test]
    fn a_known_card_limits_the_plan_and_an_unmatched_name_falls_back_to_all() {
        let ifs = [nif("en0", [192, 0, 2, 17], 24), nif("en1", [198, 51, 100, 5], 24)];
        let p = plan_sends(&ifs, Some("en1"), true);
        assert!(p.iter().all(|s| s.interface == "en1") && p.len() == 2);
        assert_eq!(plan_sends(&ifs, Some("gone0"), true).len(), 4);
    }

    #[test]
    fn directed_off_sends_the_limited_broadcast_only_and_odd_prefixes_have_no_directed() {
        let ifs = [nif("en0", [192, 0, 2, 17], 24), nif("ppp0", [203, 0, 113, 2], 32)];
        let p = plan_sends(&ifs, None, false);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|s| s.dest == Ipv4Addr::BROADCAST));
        assert_eq!(plan_sends(&ifs, None, true).len(), 3);
    }

    #[test]
    fn no_usable_card_means_no_sends_and_the_plan_is_bounded() {
        assert!(plan_sends(&[nif("lo0", [127, 0, 0, 1], 8), nif("x", [0, 0, 0, 0], 0)], None, true).is_empty());
        assert!(plan_sends(&[], None, true).is_empty());
        let many: Vec<NetIf> = (0..100u8).map(|i| nif(&format!("e{i}"), [10, i, 0, 1], 24)).collect();
        assert_eq!(plan_sends(&many, None, true).len(), MAX_SENDS);
        let dup = [nif("en0", [192, 0, 2, 17], 24), nif("en0", [192, 0, 2, 17], 24)];
        assert_eq!(plan_sends(&dup, None, true).len(), 2);
    }
}
