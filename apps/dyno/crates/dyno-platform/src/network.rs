//! The machine's sockets with their owning process, through `netstat2` (Windows `GetExtendedTcpTable` /
//! `GetExtendedUdpTable`, macOS libproc, Linux `/proc/net` with the inode-to-pid map). Read-only. Another user's
//! processes cannot be resolved without elevation on macOS and Linux: those sockets come back with no pid and the page
//! says so; Dyno never asks for root for this.

use dyno_core::network::SocketRow;
#[cfg(not(windows))]
use dyno_core::network::{Proto, SocketState};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{NetworkSockets, PlatformError, ProcessTraffic, ReverseDns};
#[cfg(not(windows))]
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use std::net::IpAddr;
use std::time::Duration;

/// The real sockets of this machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsSockets;

#[cfg(not(windows))]
fn state(s: TcpState) -> SocketState {
    match s {
        TcpState::Listen => SocketState::Listen,
        TcpState::SynSent => SocketState::SynSent,
        TcpState::SynReceived => SocketState::SynReceived,
        TcpState::Established => SocketState::Established,
        TcpState::FinWait1 | TcpState::FinWait2 => SocketState::FinWait,
        TcpState::CloseWait => SocketState::CloseWait,
        TcpState::Closing => SocketState::Closing,
        TcpState::LastAck => SocketState::LastAck,
        TcpState::TimeWait => SocketState::TimeWait,
        TcpState::Closed | TcpState::DeleteTcb | TcpState::Unknown => SocketState::Other,
    }
}

impl NetworkSockets for OsSockets {
    /// Windows: Dyno's own reader of the IP Helper tables, which knows a listener by the table it is in (netstat2 reads
    /// only the table of all endpoints and takes a listener whose `dwState` is 0, as newer Windows 11 builds report it,
    /// for an unknown state).
    #[cfg(windows)]
    fn sockets(&self) -> Result<Vec<SocketRow>, PlatformError> {
        crate::windows::net::sockets().map_err(PlatformError::Failed)
    }

    #[cfg(not(windows))]
    fn sockets(&self) -> Result<Vec<SocketRow>, PlatformError> {
        let infos = get_sockets_info(
            AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
            ProtocolFlags::TCP | ProtocolFlags::UDP,
        )
        .map_err(|e| PlatformError::Failed(format!("The OS would not list the open sockets ({e}).")))?;
        // One socket can come back once per holder (macOS) or once with all its holders (Linux); a child holds its
        // parent's sockets between fork and exec and keeps one it inherited. Rows are merged per socket and listed once
        // per owner, never once per holder or for whichever holder the OS listed first. Rows with the same tuple
        // (protocol, local address and port, peer, state) and no visible pid (another user's sockets without elevation)
        // collapse into one row: the OS gives nothing that would tell them apart.
        let mut holders: Vec<(SocketRow, Vec<u32>)> = Vec::new();
        let mut seen: std::collections::HashMap<SocketRow, usize> = std::collections::HashMap::new();
        for i in infos {
            let row = match i.protocol_socket_info {
                ProtocolSocketInfo::Tcp(t) => {
                    let st = state(t.state);
                    SocketRow {
                        proto: Proto::Tcp,
                        local: t.local_addr,
                        local_port: t.local_port,
                        remote: (st != SocketState::Listen).then_some((t.remote_addr, t.remote_port)),
                        state: st,
                        pid: None,
                    }
                }
                ProtocolSocketInfo::Udp(u) => SocketRow {
                    proto: Proto::Udp,
                    local: u.local_addr,
                    local_port: u.local_port,
                    remote: None,
                    state: SocketState::Listen,
                    pid: None,
                },
            };
            match seen.get(&row) {
                Some(&at) => holders[at].1.extend(i.associated_pids),
                None => {
                    seen.insert(row.clone(), holders.len());
                    holders.push((row, i.associated_pids));
                }
            }
        }
        let parents = parents_of(holders.iter().filter(|(_, p)| p.len() > 1).flat_map(|(_, p)| p.iter().copied()));
        Ok(holders
            .into_iter()
            .flat_map(|(row, pids)| {
                let owners: Vec<Option<u32>> = match crate::parse::socket_owner::owners(&pids, |p| parents.get(&p).copied()) {
                    o if o.is_empty() => vec![None],
                    o => o.into_iter().map(Some).collect(),
                };
                owners.into_iter().map(move |pid| SocketRow { pid, ..row.clone() })
            })
            .collect())
    }
}

/// The parents of `pids` (the holders of shared sockets: usually none, so usually nothing is read).
#[cfg(not(windows))]
fn parents_of(pids: impl Iterator<Item = u32>) -> std::collections::HashMap<u32, u32> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pids: Vec<Pid> = pids.map(Pid::from_u32).collect();
    if pids.is_empty() {
        return std::collections::HashMap::new();
    }
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), false, ProcessRefreshKind::nothing());
    pids.iter()
        .filter_map(|p| Some((p.as_u32(), sys.process(*p)?.parent()?.as_u32())))
        .collect()
}

/// Reverse DNS through the OS resolver (`getnameinfo`): the user's resolver, search domains and hosts file apply.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsReverseDns;

impl ReverseDns for OsReverseDns {
    fn lookup(&self, ip: IpAddr) -> Result<Option<String>, PlatformError> {
        match dns_lookup::lookup_addr(&ip) {
            // The resolver answers the address itself when there is no name.
            Ok(name) if name == ip.to_string() => Ok(None),
            Ok(name) => Ok(dyno_core::network_names::clean_name(&name)),
            Err(e) => Err(PlatformError::Failed(format!("The resolver could not look up {ip} ({e})."))),
        }
    }
}

/// How long `nettop` may take for one read (it takes about five seconds even on an idle Mac).
const NETTOP_TIMEOUT: Duration = Duration::from_secs(20);

/// Per-process traffic counters, each OS from its cheapest unprivileged source: macOS `nettop -P -L 1`, Linux the
/// kernel's `sock_diag` byte counters matched to programs through `/proc/<pid>/fd`, Windows the per-connection TCP
/// statistics (which need administrator rights, so without them it says why).
#[derive(Debug, Default, Clone, Copy)]
pub struct OsTraffic;

impl ProcessTraffic for OsTraffic {
    #[cfg(target_os = "macos")]
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        let mut cmd = crate::process::command("/usr/bin/nettop");
        cmd.args(["-P", "-L", "1", "-x", "-J", "bytes_in,bytes_out"]);
        let text = crate::process::output_text(cmd, NETTOP_TIMEOUT)?;
        Ok(dyno_core::network_traffic::parse_nettop(&text))
    }

    #[cfg(target_os = "linux")]
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        let _ = NETTOP_TIMEOUT;
        crate::linux::traffic::LinuxTraffic::new(crate::linux::paths::HostPaths::host()).counters()
    }

    #[cfg(windows)]
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        let _ = NETTOP_TIMEOUT;
        crate::windows::traffic::WinTraffic.counters()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        let _ = NETTOP_TIMEOUT;
        Err(PlatformError::Unsupported(
            "Traffic per program is not available on this system.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::network::{Proto, SocketState};
    use std::net::{TcpListener, TcpStream, UdpSocket};

    #[test]
    fn it_sees_a_listener_a_connection_and_a_udp_socket_this_test_created() {
        let me = std::process::id();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (_server_side, _) = listener.accept().unwrap();
        let client_port = client.local_addr().unwrap().port();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_port = udp.local_addr().unwrap().port();

        // A listener is listed as listening on every OS and Windows build (on Windows by the table it is in: newer
        // builds report its `dwState` as 0, CI job 22533).
        let is_listener = |r: &SocketRow| r.proto == Proto::Tcp && r.state == SocketState::Listen && r.local_port == port;
        let rows = OsSockets.sockets().unwrap();
        let listening = rows.iter().find(|r| is_listener(r)).unwrap_or_else(|| {
            let mine: Vec<&SocketRow> = rows.iter().filter(|r| r.local_port == port).collect();
            panic!("the test's own listener is listed: its port has {mine:?}")
        });
        assert_eq!(listening.pid, Some(me), "its owner is this process");
        assert!(listening.local.is_loopback() && listening.remote.is_none());
        let connected = rows
            .iter()
            .find(|r| r.proto == Proto::Tcp && r.local_port == client_port && r.remote.is_some_and(|(_, p)| p == port))
            .expect("the test's own connection is listed");
        assert_eq!(connected.state, SocketState::Established);
        assert_eq!(connected.pid, Some(me));
        assert!(rows
            .iter()
            .any(|r| r.proto == Proto::Udp && r.local_port == udp_port && r.pid == Some(me)));
    }

    #[cfg(not(windows))]
    #[test]
    fn every_state_maps_to_something() {
        for s in [
            TcpState::Closed,
            TcpState::Listen,
            TcpState::SynSent,
            TcpState::SynReceived,
            TcpState::Established,
            TcpState::FinWait1,
            TcpState::FinWait2,
            TcpState::CloseWait,
            TcpState::Closing,
            TcpState::LastAck,
            TcpState::TimeWait,
            TcpState::DeleteTcb,
            TcpState::Unknown,
        ] {
            let _ = state(s);
        }
    }

    /// Review P2 (2026-10-30): a listener a child holds too (here inherited on purpose and kept across exec, as a child
    /// briefly holds every socket between fork and exec) is still listed as this process's, not the child's.
    #[cfg(unix)]
    #[test]
    #[allow(unsafe_code)]
    fn a_listener_a_child_inherited_is_still_listed_as_its_parents() {
        use std::os::fd::AsRawFd;
        /// Ends the child however the test ends.
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let me = std::process::id();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // SAFETY: `dup` of an open descriptor; the copy has no close-on-exec flag, so the child keeps it after exec.
        let inherited = unsafe { libc::dup(listener.as_raw_fd()) };
        assert!(inherited >= 0);
        let child = Child(
            crate::process::command("/bin/sleep")
                .arg("60")
                .stdin(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        // SAFETY: our own copy, closed once; the child has its own.
        unsafe { libc::close(inherited) };
        let kid = child.0.id();
        // The child holds the socket once it runs: wait for the OS to list both holders (a hang guard bounds it).
        // macOS lists the socket once per holder, Linux once with every holder: either way both must be there.
        let both = || {
            let holders: Vec<u32> = get_sockets_info(AddressFamilyFlags::IPV4, ProtocolFlags::TCP)
                .unwrap()
                .into_iter()
                .filter(|i| matches!(&i.protocol_socket_info, ProtocolSocketInfo::Tcp(t) if t.local_port == port && t.state == TcpState::Listen))
                .flat_map(|i| i.associated_pids)
                .collect();
            holders.contains(&me) && holders.contains(&kid)
        };
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !both() {
            assert!(std::time::Instant::now() < end, "the OS never listed the child as a holder");
            // Waits on the condition above; the pause only keeps each poll of the machine's sockets from spinning (the
            // deadline is a hang guard).
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let rows = OsSockets.sockets().unwrap();
        let listening: Vec<&SocketRow> = rows
            .iter()
            .filter(|r| r.proto == Proto::Tcp && r.state == SocketState::Listen && r.local_port == port)
            .collect();
        assert_eq!(listening.len(), 1, "{listening:?}");
        assert_eq!(listening[0].pid, Some(me), "the parent owns it, not child {kid}");
    }

    #[test]
    fn the_resolver_answers_for_localhost_without_failing() {
        // Whatever the machine's hosts file says, a lookup of the loopback address must not error out or hang.
        let r = OsReverseDns.lookup(IpAddr::from([127, 0, 0, 1]));
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn traffic_reads_this_machine_or_says_why() {
        let r = OsTraffic.counters();
        match r {
            // nettop is slow on a loaded machine; a timeout is an honest answer, an empty list is not (macOS).
            Ok(c) if cfg!(target_os = "macos") => assert!(!c.is_empty()),
            Ok(_) => {}
            Err(PlatformError::Unsupported(m)) => assert!(!m.is_empty()),
            Err(_) => {}
        }
    }
}
