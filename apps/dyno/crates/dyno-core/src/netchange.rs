//! Network change events (WP-35): Wi-Fi joins and leaves, VPN up and down and IPv4 address changes, found by comparing
//! two looks at the network interfaces. Pure: the agent reads the interfaces through [`crate::ports::NetworkLinks`]
//! at a slow pace and records what [`NetWatcher::observe`] returns on the history timeline.
//!
//! Deliberately quiet: virtual and bridge interfaces (Docker, VMs, AWDL) are ignored, link-local and temporary IPv6
//! addresses never count, and the first look only sets the baseline.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// What an interface is, as far as the timeline cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    /// A Wi-Fi adapter.
    Wifi,
    /// A VPN tunnel.
    Vpn,
    /// Wired or any other physical adapter.
    Other,
    /// Loopback, containers, VMs, bridges and the like: never reported.
    Ignored,
}

/// One interface with the addresses that count (global; no loopback, link-local or unspecified).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The OS's name for it (`en0`, `wg0`, `Wi-Fi`).
    pub name: String,
    /// What it is.
    pub kind: LinkKind,
    /// Its usable addresses.
    pub addresses: Vec<IpAddr>,
}

impl Link {
    /// Whether the interface has an address it can talk with.
    pub fn is_up(&self) -> bool {
        !self.addresses.is_empty()
    }

    fn v4(&self) -> Vec<IpAddr> {
        let mut v: Vec<IpAddr> = self.addresses.iter().copied().filter(IpAddr::is_ipv4).collect();
        v.sort();
        v
    }
}

/// One look at the network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetState {
    /// The interfaces.
    pub links: Vec<Link>,
    /// The Wi-Fi network joined, when the OS tells (macOS may hide it without Location Services).
    pub ssid: Option<String>,
}

impl NetState {
    fn wifi_up(&self) -> bool {
        self.links.iter().any(|l| l.kind == LinkKind::Wifi && l.is_up())
    }
}

/// Something that changed between two looks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetChange {
    /// Wi-Fi connected (to `ssid` when known).
    WifiJoined {
        /// The network name.
        ssid: Option<String>,
    },
    /// Wi-Fi disconnected.
    WifiLeft {
        /// The network it was on.
        ssid: Option<String>,
    },
    /// A VPN tunnel came up.
    VpnUp {
        /// The interface.
        name: String,
    },
    /// A VPN tunnel went down.
    VpnDown {
        /// The interface.
        name: String,
    },
    /// The IPv4 address of an interface changed.
    IpChanged {
        /// The interface.
        interface: String,
        /// Before.
        from: IpAddr,
        /// After.
        to: IpAddr,
    },
}

impl NetChange {
    /// The timeline text.
    pub fn label(&self) -> String {
        let named = |verb: &str, ssid: &Option<String>| match ssid {
            Some(s) => format!("{verb} Wi-Fi “{s}”"),
            None => format!("{verb} Wi-Fi"),
        };
        match self {
            NetChange::WifiJoined { ssid } => named("Joined", ssid),
            NetChange::WifiLeft { ssid } => named("Left", ssid),
            NetChange::VpnUp { name } => format!("VPN up ({name})"),
            NetChange::VpnDown { name } => format!("VPN down ({name})"),
            NetChange::IpChanged { interface, from, to } => format!("IP address on {interface} changed: {from} → {to}"),
        }
    }
}

/// Names of virtual interfaces that are never reported (by prefix).
const IGNORED_PREFIX: [&str; 13] = [
    "docker", "veth", "br-", "virbr", "vmnet", "vboxnet", "awdl", "llw", "anpi", "bridge", "gif", "stf", "vethernet",
];
/// Loopback names (whole name).
const IGNORED_EXACT: [&str; 3] = ["lo", "lo0", "loopback pseudo-interface 1"];
/// Names (or words of names) of VPN tunnels.
const VPN: [&str; 15] = [
    "utun", "tun", "tap", "wg", "ppp", "ipsec", "tailscale", "zt", "vpn", "nordlynx", "wireguard", "wintun", "proton", "mullvad", "cisco",
];

/// Classifies an interface by its name. `wifi_device` says the OS lists it as the Wi-Fi adapter.
pub fn classify(name: &str, wifi_device: bool) -> LinkKind {
    let n = name.to_lowercase();
    if wifi_device {
        return LinkKind::Wifi;
    }
    // A VPN adapter on Windows has a descriptive name ("OpenVPN TAP-Windows Adapter"): any word that starts like a VPN does counts.
    let words = || n.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty());
    if VPN.iter().any(|v| n.starts_with(v)) || words().any(|w| VPN.iter().any(|v| w.starts_with(v) && (v.len() > 3 || w == *v))) {
        return LinkKind::Vpn;
    }
    if IGNORED_EXACT.contains(&n.as_str()) || IGNORED_PREFIX.iter().any(|v| n.starts_with(v)) {
        return LinkKind::Ignored;
    }
    LinkKind::Other
}

/// Whether an address counts as usable: not loopback, unspecified, link-local, multicast or an IPv6 unique-local temporary.
pub fn usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => !(a.is_loopback() || a.is_unspecified() || a.is_link_local() || a.is_multicast() || a.is_broadcast()),
        IpAddr::V6(a) => {
            let seg = a.segments();
            !(a.is_loopback() || a.is_unspecified() || a.is_multicast() || (seg[0] & 0xffc0) == 0xfe80)
        }
    }
}

/// What changed from `prev` to `next`, oldest first.
pub fn diff(prev: &NetState, next: &NetState) -> Vec<NetChange> {
    let mut out = Vec::new();
    let (was, is) = (prev.wifi_up(), next.wifi_up());
    match (was, is) {
        (true, false) => out.push(NetChange::WifiLeft { ssid: prev.ssid.clone() }),
        (false, true) => out.push(NetChange::WifiJoined { ssid: next.ssid.clone() }),
        (true, true) if prev.ssid.is_some() && next.ssid.is_some() && prev.ssid != next.ssid => {
            out.push(NetChange::WifiLeft { ssid: prev.ssid.clone() });
            out.push(NetChange::WifiJoined { ssid: next.ssid.clone() });
        }
        _ => {}
    }
    let find = |s: &'_ NetState, name: &str| s.links.iter().find(|l| l.name == name).cloned();
    for l in next.links.iter().filter(|l| l.kind == LinkKind::Vpn) {
        let before = find(prev, &l.name).is_some_and(|p| p.is_up());
        if l.is_up() && !before {
            out.push(NetChange::VpnUp { name: l.name.clone() });
        } else if !l.is_up() && before {
            out.push(NetChange::VpnDown { name: l.name.clone() });
        }
    }
    for l in prev
        .links
        .iter()
        .filter(|l| l.kind == LinkKind::Vpn && l.is_up() && !next.links.iter().any(|n| n.name == l.name))
    {
        out.push(NetChange::VpnDown { name: l.name.clone() });
    }
    for l in next.links.iter().filter(|l| matches!(l.kind, LinkKind::Wifi | LinkKind::Other)) {
        let (Some(p), now) = (find(prev, &l.name), l.v4()) else { continue };
        let before = p.v4();
        if let (Some(from), Some(to)) = (before.first(), now.first()) {
            if before != now && !now.contains(from) {
                out.push(NetChange::IpChanged {
                    interface: l.name.clone(),
                    from: *from,
                    to: *to,
                });
            }
        }
    }
    out
}

/// Remembers the last look and turns each new one into changes.
#[derive(Debug, Default)]
pub struct NetWatcher {
    last: Option<NetState>,
}

impl NetWatcher {
    /// A watcher with no baseline.
    pub fn new() -> NetWatcher {
        NetWatcher::default()
    }

    /// Records `state` and returns what changed since the previous look (nothing on the first).
    pub fn observe(&mut self, state: NetState) -> Vec<NetChange> {
        let changes = self.last.as_ref().map(|p| diff(p, &state)).unwrap_or_default();
        self.last = Some(state);
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn link(name: &str, kind: LinkKind, addrs: &[&str]) -> Link {
        Link {
            name: name.into(),
            kind,
            addresses: addrs.iter().map(|a| ip(a)).collect(),
        }
    }

    fn state(links: Vec<Link>, ssid: Option<&str>) -> NetState {
        NetState {
            links,
            ssid: ssid.map(str::to_string),
        }
    }

    #[test]
    fn interfaces_are_classified_by_name() {
        assert_eq!(classify("en0", true), LinkKind::Wifi);
        assert_eq!(classify("en1", false), LinkKind::Other);
        assert_eq!(classify("utun4", false), LinkKind::Vpn);
        assert_eq!(classify("wg0", false), LinkKind::Vpn);
        assert_eq!(classify("tailscale0", false), LinkKind::Vpn);
        assert_eq!(classify("OpenVPN TAP-Windows Adapter V9", false), LinkKind::Vpn);
        for n in ["lo0", "lo", "docker0", "veth12ab", "br-1a2b", "awdl0", "llw0", "bridge100", "vmnet8", "virbr0"] {
            assert_eq!(classify(n, false), LinkKind::Ignored, "{n}");
        }
        // "lo" is a whole name or a prefix of a loopback name, but a real adapter such as "lowpan" is not special.
        assert_eq!(classify("eth0", false), LinkKind::Other);
    }

    #[test]
    fn only_global_addresses_count() {
        for a in ["127.0.0.1", "169.254.1.1", "0.0.0.0", "224.0.0.1", "::1", "fe80::1", "ff02::1", "::"] {
            assert!(!usable(&ip(a)), "{a}");
        }
        for a in ["192.168.1.5", "10.0.0.2", "2001:db8::1", "fd00::1"] {
            assert!(usable(&ip(a)), "{a}");
        }
    }

    #[test]
    fn the_first_look_is_only_a_baseline() {
        let mut w = NetWatcher::new();
        assert!(w.observe(state(vec![link("en0", LinkKind::Wifi, &["192.168.1.5"])], Some("Home"))).is_empty());
        assert!(w.observe(state(vec![link("en0", LinkKind::Wifi, &["192.168.1.5"])], Some("Home"))).is_empty());
    }

    #[test]
    fn wifi_joins_leaves_and_moves() {
        let off = state(vec![link("en0", LinkKind::Wifi, &[])], None);
        let home = state(vec![link("en0", LinkKind::Wifi, &["192.168.1.5"])], Some("Home"));
        let cafe = state(vec![link("en0", LinkKind::Wifi, &["10.1.1.9"])], Some("Cafe"));
        assert_eq!(diff(&off, &home), vec![NetChange::WifiJoined { ssid: Some("Home".into()) }]);
        assert_eq!(diff(&home, &off), vec![NetChange::WifiLeft { ssid: Some("Home".into()) }]);
        let moved = diff(&home, &cafe);
        assert_eq!(
            moved[..2],
            [
                NetChange::WifiLeft { ssid: Some("Home".into()) },
                NetChange::WifiJoined { ssid: Some("Cafe".into()) }
            ]
        );
        // The address changed as well, but Wi-Fi moves are told once with the two names; the IP line follows.
        assert!(matches!(moved[2], NetChange::IpChanged { .. }));
        // A hidden name never invents a move.
        let hidden = state(vec![link("en0", LinkKind::Wifi, &["192.168.1.5"])], None);
        assert!(diff(&home, &hidden).is_empty());
        assert_eq!(diff(&off, &hidden), vec![NetChange::WifiJoined { ssid: None }]);
    }

    #[test]
    fn vpn_tunnels_come_and_go() {
        let none = state(vec![link("en0", LinkKind::Other, &["192.168.1.5"])], None);
        let up = state(
            vec![link("en0", LinkKind::Other, &["192.168.1.5"]), link("utun4", LinkKind::Vpn, &["10.8.0.2"])],
            None,
        );
        let only_ll = state(
            vec![link("en0", LinkKind::Other, &["192.168.1.5"]), link("utun4", LinkKind::Vpn, &[])],
            None,
        );
        assert_eq!(diff(&none, &up), vec![NetChange::VpnUp { name: "utun4".into() }]);
        assert_eq!(diff(&up, &none), vec![NetChange::VpnDown { name: "utun4".into() }]);
        assert_eq!(diff(&up, &only_ll), vec![NetChange::VpnDown { name: "utun4".into() }]);
        assert!(diff(&none, &only_ll).is_empty());
        assert!(diff(&up, &up).is_empty());
    }

    #[test]
    fn ip_changes_ignore_ipv6_churn_ignored_links_and_new_interfaces() {
        let a = state(vec![link("en0", LinkKind::Other, &["192.168.1.5", "2001:db8::1"])], None);
        let b = state(vec![link("en0", LinkKind::Other, &["192.168.1.5", "2001:db8::2"])], None);
        assert!(diff(&a, &b).is_empty());
        let c = state(vec![link("en0", LinkKind::Other, &["10.0.0.7"])], None);
        let changes = diff(&a, &c);
        assert_eq!(
            changes,
            vec![NetChange::IpChanged {
                interface: "en0".into(),
                from: ip("192.168.1.5"),
                to: ip("10.0.0.7")
            }]
        );
        assert_eq!(changes[0].label(), "IP address on en0 changed: 192.168.1.5 → 10.0.0.7");
        // A secondary address added is not a change of the address; an interface that was down is a link event, not an IP one.
        let d = state(vec![link("en0", LinkKind::Other, &["192.168.1.5", "192.168.1.6"])], None);
        assert!(diff(&a, &d).is_empty());
        let down = state(vec![link("en0", LinkKind::Other, &[])], None);
        assert!(diff(&down, &c).is_empty());
        let docker = state(vec![link("docker0", LinkKind::Ignored, &["172.17.0.1"])], None);
        let docker2 = state(vec![link("docker0", LinkKind::Ignored, &["172.18.0.1"])], None);
        assert!(diff(&docker, &docker2).is_empty());
    }

    #[test]
    fn labels_read_well() {
        assert_eq!(NetChange::WifiJoined { ssid: Some("Home".into()) }.label(), "Joined Wi-Fi “Home”");
        assert_eq!(NetChange::WifiLeft { ssid: None }.label(), "Left Wi-Fi");
        assert_eq!(NetChange::VpnUp { name: "wg0".into() }.label(), "VPN up (wg0)");
        assert_eq!(NetChange::VpnDown { name: "wg0".into() }.label(), "VPN down (wg0)");
    }
}
