//! The neighbour tables per OS: macOS `arp -an` and `ndp -an`; Linux `ip neigh` (both families, with freshness) and
//! `/proc/net/arp` when `ip` is missing; Windows the IP Helper table (`GetIpNetTable2`, in `win.rs`). Bounded and parsed by
//! `dyno_core::wol::neighbours`, which never panics.

use crate::runner::{tool, ToolRunner};
use dyno_core::ports::PlatformError;
use dyno_core::wol::neighbours::{parse_arp_an, parse_ip_neigh, parse_ndp_an, parse_proc_net_arp, MAX_NEIGHBOURS};
use dyno_core::wol::wake::NeighbourSource;
use dyno_core::wol::{Neighbour, NeighbourState};
use std::sync::Mutex;

#[cfg(windows)]
#[allow(unsafe_code)]
mod win;

/// Reads the tables the way this OS offers them.
pub struct OsNeighbours {
    runner: Mutex<Box<dyn ToolRunner>>,
    proc_net_arp: std::path::PathBuf,
}

impl OsNeighbours {
    /// Over `runner`, reading `/proc/net/arp` where Linux falls back to it.
    pub fn new(runner: Box<dyn ToolRunner>) -> OsNeighbours {
        OsNeighbours {
            runner: Mutex::new(runner),
            proc_net_arp: "/proc/net/arp".into(),
        }
    }

    #[cfg(test)]
    fn with_proc_file(mut self, path: std::path::PathBuf) -> OsNeighbours {
        self.proc_net_arp = path;
        self
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<String, PlatformError> {
        let r = self.runner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let out = r.run_tool(&tool(program, args), crate::runner::TOOL_TIMEOUT)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            Err(PlatformError::Failed(format!("{program} failed: {}", out.error)))
        }
    }

    /// BSD/macOS: ARP and NDP. Either table may fail alone (no IPv6 on the machine).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn bsd(&self) -> Result<Vec<Neighbour>, PlatformError> {
        let v4 = self.run("/usr/sbin/arp", &["-an"]);
        let v6 = self.run("/usr/sbin/ndp", &["-an"]);
        match (v4, v6) {
            (Err(e), Err(_)) => Err(e),
            (a, b) => {
                let mut out = a.map(|t| parse_arp_an(&t)).unwrap_or_default();
                out.extend(b.map(|t| parse_ndp_an(&t)).unwrap_or_default());
                Ok(out)
            }
        }
    }

    /// Linux: `ip neigh`, else `/proc/net/arp`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn linux(&self) -> Result<Vec<Neighbour>, PlatformError> {
        match self.run("ip", &["-o", "neigh", "show"]) {
            Ok(text) => Ok(parse_ip_neigh(&text)),
            Err(why) => {
                let text = std::fs::read_to_string(&self.proc_net_arp)
                    .map_err(|e| PlatformError::Unsupported(format!("the neighbour table is not readable ({why}; {e})")))?;
                Ok(parse_proc_net_arp(&text))
            }
        }
    }
}

impl NeighbourSource for OsNeighbours {
    fn neighbours(&self) -> Result<Vec<Neighbour>, PlatformError> {
        #[cfg(windows)]
        let mut all = win::table()?;
        #[cfg(target_os = "macos")]
        let mut all = self.bsd()?;
        #[cfg(not(any(windows, target_os = "macos")))]
        let mut all = self.linux()?;
        // A multicast-looking IPv6 entry or a loopback is never a computer to wake.
        all.retain(|n| {
            n.ip.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| !ip.is_multicast() && !ip.is_loopback())
        });
        all.truncate(MAX_NEIGHBOURS);
        Ok(all)
    }
}

/// The OS's neighbour state numbers (`NL_NEIGHBOR_STATE`) as a freshness; unreachable and incomplete entries have no address.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn state_of_nl(state: i32) -> Option<NeighbourState> {
    match state {
        2 | 3 => Some(NeighbourState::Probe),
        4 => Some(NeighbourState::Stale),
        5 => Some(NeighbourState::Reachable),
        6 => Some(NeighbourState::Permanent),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::script::{fail, out, Script};
    use super::*;

    fn over(answers: Vec<(&'static str, Result<crate::runner::ToolOutput, PlatformError>)>) -> OsNeighbours {
        OsNeighbours::new(Box::new(Script {
            answers,
            calls: Default::default(),
        }))
    }

    #[test]
    fn bsd_joins_arp_and_ndp_and_survives_one_failing() {
        let n = over(vec![
            ("arp", out("? (192.0.2.1) at 2:0:5e:10:0:1 on en0 ifscope [ethernet]\n")),
            (
                "ndp",
                out("Neighbor Linklayer Address Netif Expire St Flgs Prbs\n2001:db8::1 2:0:5e:10:0:1 en0 5s R\n"),
            ),
        ]);
        assert_eq!(n.bsd().unwrap().len(), 2);
        let n = over(vec![
            ("arp", out("? (192.0.2.1) at 2:0:5e:10:0:1 on en0 ifscope [ethernet]\n")),
            ("ndp", fail("no ipv6")),
        ]);
        assert_eq!(n.bsd().unwrap().len(), 1);
        let n = over(vec![("arp", fail("denied")), ("ndp", fail("denied"))]);
        assert!(n.bsd().unwrap_err().to_string().contains("failed"));
    }

    #[test]
    fn linux_prefers_ip_and_falls_back_to_the_proc_file() {
        let n = over(vec![("ip", out("192.0.2.1 dev eth0 lladdr 02:00:5e:10:00:01 REACHABLE\n"))]);
        assert_eq!(n.linux().unwrap()[0].state, NeighbourState::Reachable);
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("arp");
        std::fs::write(
            &f,
            "IP address HW type Flags HW address Mask Device\n192.0.2.2 0x1 0x2 02:00:5e:10:00:02 * eth0\n",
        )
        .unwrap();
        let n = over(vec![]).with_proc_file(f);
        assert_eq!(n.linux().unwrap().len(), 1);
        let n = over(vec![]).with_proc_file(dir.path().join("missing"));
        assert!(n.linux().unwrap_err().to_string().contains("not readable"));
    }

    #[test]
    fn os_states_map_and_unusable_ones_have_none() {
        assert_eq!(state_of_nl(5), Some(NeighbourState::Reachable));
        assert_eq!(state_of_nl(4), Some(NeighbourState::Stale));
        assert_eq!(state_of_nl(2), Some(NeighbourState::Probe));
        assert_eq!(state_of_nl(6), Some(NeighbourState::Permanent));
        assert_eq!(state_of_nl(0), None);
        assert_eq!(state_of_nl(1), None);
        assert_eq!(state_of_nl(99), None);
    }
}
