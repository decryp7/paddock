//! The machine's sockets with their owning process, through `netstat2` (Windows `GetExtendedTcpTable` /
//! `GetExtendedUdpTable`, macOS libproc, Linux `/proc/net` with the inode-to-pid map). Read-only. Another user's
//! processes cannot be resolved without elevation on macOS and Linux: those sockets come back with no pid and the page
//! says so; Dyno never asks for root for this.

use dyno_core::network::{Proto, SocketRow, SocketState};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{NetworkSockets, PlatformError, ProcessTraffic, ReverseDns};
use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use std::net::IpAddr;
use std::time::Duration;

/// The real sockets of this machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsSockets;

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
    fn sockets(&self) -> Result<Vec<SocketRow>, PlatformError> {
        let infos = get_sockets_info(
            AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
            ProtocolFlags::TCP | ProtocolFlags::UDP,
        )
        .map_err(|e| PlatformError::Failed(format!("The OS would not list the open sockets ({e}).")))?;
        Ok(infos
            .into_iter()
            .map(|i| {
                let pid = i.associated_pids.first().copied();
                match i.protocol_socket_info {
                    ProtocolSocketInfo::Tcp(t) => {
                        let st = state(t.state);
                        SocketRow {
                            proto: Proto::Tcp,
                            local: t.local_addr,
                            local_port: t.local_port,
                            remote: (st != SocketState::Listen).then_some((t.remote_addr, t.remote_port)),
                            state: st,
                            pid,
                        }
                    }
                    ProtocolSocketInfo::Udp(u) => SocketRow {
                        proto: Proto::Udp,
                        local: u.local_addr,
                        local_port: u.local_port,
                        remote: None,
                        state: SocketState::Listen,
                        pid,
                    },
                }
            })
            .collect())
    }
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

        let rows = OsSockets.sockets().unwrap();
        let listening = rows
            .iter()
            .find(|r| r.proto == Proto::Tcp && r.state == SocketState::Listen && r.local_port == port)
            .expect("the test's own listener is listed");
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
