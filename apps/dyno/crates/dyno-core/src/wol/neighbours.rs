//! The neighbour tables (ARP for IPv4, NDP for IPv6): who the OS has talked to on the local network, with their hardware
//! addresses. Dyno reads them to let the user pick a computer to add, and to see a woken computer answer. The parsers here
//! take the text of `arp -an`, `ndp -an`, `/proc/net/arp` and `ip neigh`; they never panic, cap what they read, and skip lines
//! they do not understand. Windows reads the table through the OS API and builds [`Neighbour`] directly.

use super::mac::Mac;
use serde::{Deserialize, Serialize};

/// The most entries a read keeps.
pub const MAX_NEIGHBOURS: usize = 4096;
/// The most text a parser looks at (bytes).
pub const MAX_TABLE_TEXT: usize = 1 << 20;
/// The longest line a parser looks at; a longer one is skipped.
pub const MAX_LINE: usize = 512;

/// How fresh an entry is, in the words of the kernel's neighbour state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NeighbourState {
    /// Confirmed reachable recently.
    Reachable,
    /// Known but not confirmed lately.
    Stale,
    /// Being probed now (including the delay before a probe).
    Probe,
    /// Added by hand or by the OS; never ages.
    Permanent,
    /// The OS cannot tell the freshness (an old table format).
    Unknown,
}

/// One neighbour table entry that has a hardware address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neighbour {
    /// The hardware address.
    pub mac: Mac,
    /// The IP address, as text.
    pub ip: String,
    /// The interface name the entry belongs to.
    pub interface: String,
    /// How fresh it is.
    pub state: NeighbourState,
    /// Its host name, when the agent's reverse-DNS cache or resolver knows one (filled for "Add from network" only).
    #[serde(default)]
    pub name: Option<String>,
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    // Cut on a char boundary at the cap, so a huge input is bounded without panicking.
    let mut end = text.len().min(MAX_TABLE_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].lines().filter(|l| l.len() <= MAX_LINE)
}

fn push(out: &mut Vec<Neighbour>, n: Neighbour) {
    if out.len() < MAX_NEIGHBOURS {
        out.push(n);
    }
}

fn clean_ip(token: &str) -> Option<String> {
    let t = token.trim_matches(|c| c == '(' || c == ')');
    // Link-local IPv6 may carry a `%zone`; the zone is the interface, kept separately.
    let bare = t.split('%').next().unwrap_or("");
    bare.parse::<std::net::IpAddr>().ok().map(|ip| ip.to_string())
}

/// Parses BSD/macOS `arp -an`: `? (192.0.2.7) at 2:0:5e:10:0:1 on en0 ifscope [ethernet]`; `(incomplete)` lines are skipped.
/// The state is [`NeighbourState::Unknown`] (`arp` does not say) except `permanent`.
pub fn parse_arp_an(text: &str) -> Vec<Neighbour> {
    let mut out = Vec::new();
    for line in lines(text) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // ? (ip) at mac on ifname ...
        let (Some(ip), Some(at), Some(mac)) = (parts.get(1), parts.get(2), parts.get(3)) else {
            continue;
        };
        if *at != "at" || !ip.starts_with('(') {
            continue;
        }
        let (Some(ip), Some(mac)) = (clean_ip(ip), Mac::parse(mac).ok()) else {
            continue;
        };
        let interface = parts
            .iter()
            .position(|p| *p == "on")
            .and_then(|i| parts.get(i + 1))
            .map_or(String::new(), |s| (*s).to_string());
        let state = if parts.contains(&"permanent") {
            NeighbourState::Permanent
        } else {
            NeighbourState::Unknown
        };
        push(
            &mut out,
            Neighbour {
                name: None,
                mac,
                ip,
                interface,
                state,
            },
        );
    }
    out
}

/// Parses BSD/macOS `ndp -an`: `fe80::1%en0  2:0:5e:10:0:1  en0  23h59m  S  R` (columns: address, link-layer address, interface,
/// expire, state, flags). The state letters are R reachable, S stale, D delay, P probe, N none, I incomplete (skipped).
pub fn parse_ndp_an(text: &str) -> Vec<Neighbour> {
    let mut out = Vec::new();
    for line in lines(text) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 || parts[0] == "Neighbor" {
            continue;
        }
        let (Some(ip), Some(mac)) = (clean_ip(parts[0]), Mac::parse(parts[1]).ok()) else {
            continue;
        };
        let state = match parts.iter().skip(3).find(|p| p.len() == 1 && "RSDPNI".contains(**p)) {
            Some(&"R") => NeighbourState::Reachable,
            Some(&"S") => NeighbourState::Stale,
            Some(&"D") | Some(&"P") => NeighbourState::Probe,
            Some(&"I") => continue,
            _ => NeighbourState::Unknown,
        };
        push(
            &mut out,
            Neighbour {
                name: None,
                mac,
                ip,
                interface: parts[2].to_string(),
                state,
            },
        );
    }
    out
}

/// Parses Linux `/proc/net/arp`: header `IP address HW type Flags HW address Mask Device`, then rows. Flag bit 0x2 means complete
/// and 0x4 permanent; the file does not say fresh, so the state is [`NeighbourState::Unknown`].
pub fn parse_proc_net_arp(text: &str) -> Vec<Neighbour> {
    let mut out = Vec::new();
    for line in lines(text).skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 6 {
            continue;
        }
        let flags = u32::from_str_radix(p[2].trim_start_matches("0x"), 16).unwrap_or(0);
        if flags & 0x2 == 0 {
            continue;
        }
        let (Some(ip), Some(mac)) = (clean_ip(p[0]), Mac::parse(p[3]).ok()) else {
            continue;
        };
        let state = if flags & 0x4 != 0 {
            NeighbourState::Permanent
        } else {
            NeighbourState::Unknown
        };
        push(
            &mut out,
            Neighbour {
                name: None,
                mac,
                ip,
                interface: p[5].to_string(),
                state,
            },
        );
    }
    out
}

/// Parses Linux `ip neigh show` (or `ip -o neigh`): `192.0.2.7 dev eth0 lladdr 02:00:5e:10:00:01 REACHABLE`. FAILED, INCOMPLETE
/// and NOARP entries have no usable address and are skipped.
pub fn parse_ip_neigh(text: &str) -> Vec<Neighbour> {
    let mut out = Vec::new();
    for line in lines(text) {
        let p: Vec<&str> = line.split_whitespace().collect();
        let (Some(ip), Some(dev), Some(ll)) = (
            p.first().and_then(|t| clean_ip(t)),
            p.iter().position(|t| *t == "dev").and_then(|i| p.get(i + 1)),
            p.iter().position(|t| *t == "lladdr").and_then(|i| p.get(i + 1)),
        ) else {
            continue;
        };
        let Some(mac) = Mac::parse(ll).ok() else { continue };
        let state = match p.last().copied().unwrap_or("") {
            "REACHABLE" => NeighbourState::Reachable,
            "STALE" => NeighbourState::Stale,
            "DELAY" | "PROBE" => NeighbourState::Probe,
            "PERMANENT" => NeighbourState::Permanent,
            _ => NeighbourState::Unknown,
        };
        push(
            &mut out,
            Neighbour {
                name: None,
                mac,
                ip,
                interface: (*dev).to_string(),
                state,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARP_AN: &str = "\
? (192.0.2.1) at 2:0:5e:10:0:1 on en0 ifscope [ethernet]
? (192.0.2.7) at (incomplete) on en0 ifscope [ethernet]
? (192.0.2.255) at ff:ff:ff:ff:ff:ff on en0 ifscope [ethernet]
? (224.0.0.251) at 1:0:5e:0:0:fb on en0 ifscope permanent [ethernet]
? (192.0.2.9) at 2:0:5e:10:0:9 on en1 permanent [ethernet]
garbage line
";

    #[test]
    fn arp_an_keeps_complete_unicast_entries() {
        let n = parse_arp_an(ARP_AN);
        assert_eq!(n.len(), 2);
        assert_eq!(
            n[0],
            Neighbour {
                name: None,
                mac: Mac([2, 0, 0x5e, 0x10, 0, 1]),
                ip: "192.0.2.1".into(),
                interface: "en0".into(),
                state: NeighbourState::Unknown
            }
        );
        assert_eq!(n[1].state, NeighbourState::Permanent);
        assert_eq!(n[1].interface, "en1");
    }

    #[test]
    fn ndp_an_reads_states_and_drops_incomplete() {
        let text = "\
Neighbor                        Linklayer Address  Netif Expire    St Flgs Prbs
2001:db8::1                     2:0:5e:10:0:1      en0   23h59m50s S  R
fe80::1%en0                     2:0:5e:10:0:2      en0   permanent R
fe80::2%en0                     2:0:5e:10:0:3      en0   expired   S
fe80::3%en0                     (incomplete)       en0   expired   N
fe80::4%en0                     2:0:5e:10:0:4      en0   1s        D
fe80::5%en0                     2:0:5e:10:0:5      en0   1s        I
";
        let n = parse_ndp_an(text);
        let states: Vec<_> = n.iter().map(|x| x.state).collect();
        assert_eq!(
            states,
            [
                NeighbourState::Stale,
                NeighbourState::Reachable,
                NeighbourState::Stale,
                NeighbourState::Probe
            ]
        );
        assert_eq!(n[1].ip, "fe80::1");
    }

    #[test]
    fn proc_net_arp_keeps_complete_rows() {
        let text = "\
IP address       HW type     Flags       HW address            Mask     Device
192.0.2.1        0x1         0x2         02:00:5e:10:00:01     *        eth0
192.0.2.2        0x1         0x0         00:00:00:00:00:00     *        eth0
192.0.2.3        0x1         0x6         02:00:5e:10:00:03     *        eth0
short row
";
        let n = parse_proc_net_arp(text);
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].interface, "eth0");
        assert_eq!(n[1].state, NeighbourState::Permanent);
    }

    #[test]
    fn ip_neigh_reads_states() {
        let text = "\
192.0.2.1 dev eth0 lladdr 02:00:5e:10:00:01 REACHABLE
192.0.2.2 dev eth0 lladdr 02:00:5e:10:00:02 STALE
192.0.2.3 dev eth0  FAILED
2001:db8::5 dev wlan0 lladdr 02:00:5e:10:00:05 router DELAY
192.0.2.4 dev eth0 lladdr 02:00:5e:10:00:04 PERMANENT
";
        let n = parse_ip_neigh(text);
        let states: Vec<_> = n.iter().map(|x| x.state).collect();
        assert_eq!(
            states,
            [
                NeighbourState::Reachable,
                NeighbourState::Stale,
                NeighbourState::Probe,
                NeighbourState::Permanent
            ]
        );
        assert_eq!(n[2].interface, "wlan0");
    }

    #[test]
    fn parsers_survive_any_text_and_cap_what_they_keep() {
        for f in [parse_arp_an, parse_ndp_an, parse_proc_net_arp, parse_ip_neigh] {
            assert!(f("").is_empty());
            assert!(f("\u{0}\u{ffff}é\n\n  \t").is_empty());
            assert!(f(&"x".repeat(MAX_LINE + 10)).is_empty());
            assert!(f(&"é".repeat(MAX_TABLE_TEXT)).is_empty());
        }
        let many = "192.0.2.1 dev eth0 lladdr 02:00:5e:10:00:01 REACHABLE\n".repeat(MAX_NEIGHBOURS + 50);
        assert_eq!(parse_ip_neigh(&many).len(), MAX_NEIGHBOURS);
    }

    proptest::proptest! {
        #[test]
        fn never_panics(s in "\\PC{0,400}") {
            let _ = (parse_arp_an(&s), parse_ndp_an(&s), parse_proc_net_arp(&s), parse_ip_neigh(&s));
        }
    }
}
