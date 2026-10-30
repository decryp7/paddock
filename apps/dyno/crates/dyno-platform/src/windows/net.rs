//! TCP and UDP endpoints (IPv4 and IPv6) with their owning process, from the IP Helper owner-pid tables. Whether a TCP
//! endpoint listens comes from the table it was read from (`TCP_TABLE_OWNER_PID_LISTENER` or `_CONNECTIONS`), never from
//! its `dwState` alone: newer Windows 11 builds report a listener's state as 0 ([`crate::parse::win_tcp`]).

use crate::parse::win_tcp::{row_name, row_state, TcpTable};
use dyno_core::network::{Proto, SocketRow, SocketState};
use dyno_core::process::{Connection, Pid};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID,
    TCP_TABLE_CLASS, TCP_TABLE_OWNER_PID_ALL, TCP_TABLE_OWNER_PID_CONNECTIONS, TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

/// The largest table read (bytes).
const MAX_TABLE: u32 = 64 << 20;

pub use crate::parse::win_tcp::mib_name as tcp_state;

/// A port as stored in the tables (network byte order in the low 16 bits).
pub fn port(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

/// An IPv4 address as stored in the tables (network byte order in memory).
pub fn ipv4(raw: u32) -> Ipv4Addr {
    Ipv4Addr::from(raw.to_ne_bytes())
}

/// Rows of a `{ dwNumEntries; ROW table[] }` table in `buf`.
pub(super) fn rows<T: Copy>(buf: &[u8]) -> Vec<T> {
    let Some(head) = buf.get(..4) else { return Vec::new() };
    let n = u32::from_ne_bytes([head[0], head[1], head[2], head[3]]) as usize;
    // Rows start after the count, at the row type's alignment (4 for every table here).
    let start = 4usize.next_multiple_of(align_of::<T>());
    let size = size_of::<T>();
    (0..n)
        .map_while(|i| {
            let at = start + i * size;
            let bytes = buf.get(at..at + size)?;
            // SAFETY: `bytes` holds exactly one T (plain-old-data from the OS); read_unaligned has no alignment needs.
            Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const T) })
        })
        .collect()
}

/// Calls a size-then-fill table API.
fn table(fetch: impl Fn(Option<*mut core::ffi::c_void>, &mut u32) -> u32) -> Option<Vec<u8>> {
    let mut size = 0u32;
    for _ in 0..4 {
        let mut buf = vec![0u8; size as usize];
        let ptr = (!buf.is_empty()).then_some(buf.as_mut_ptr() as *mut core::ffi::c_void);
        match fetch(ptr, &mut size) {
            0 if ptr.is_some() => return Some(buf),
            0 => return Some(Vec::new()),
            e if e == ERROR_INSUFFICIENT_BUFFER.0 && size <= MAX_TABLE => continue,
            _ => return None,
        }
    }
    None
}

pub(super) fn tcp(family: u32) -> Option<Vec<u8>> {
    tcp_class(family, TCP_TABLE_OWNER_PID_ALL)
}

fn tcp_class(family: u32, class: TCP_TABLE_CLASS) -> Option<Vec<u8>> {
    table(|p, size| {
        // SAFETY: `p` is None (size query) or a buffer of `*size` bytes.
        unsafe { GetExtendedTcpTable(p, size, false, family, class, 0) }
    })
}

fn udp(family: u32) -> Option<Vec<u8>> {
    table(|p, size| {
        // SAFETY: as above.
        unsafe { GetExtendedUdpTable(p, size, false, family, UDP_TABLE_OWNER_PID, 0) }
    })
}

/// One endpoint as read from the tables.
#[derive(Debug, Clone)]
struct Endpoint {
    proto: Proto,
    v6: bool,
    local: SocketAddr,
    /// The peer of a TCP connection (`None` for a listener and for UDP).
    remote: Option<SocketAddr>,
    state: SocketState,
    /// What the page shows as the state (`LISTEN`, `ESTABLISHED`, `UNKNOWN (0)`, empty for UDP).
    state_name: String,
    pid: u32,
}

fn sa4(addr: u32, p: u32) -> SocketAddr {
    SocketAddr::from((ipv4(addr), port(p)))
}

fn sa6(addr: [u8; 16], p: u32) -> SocketAddr {
    SocketAddr::from((Ipv6Addr::from(addr), port(p)))
}

/// Every endpoint of the machine; `None` when no table could be read at all. A listener is read from the listener table
/// and a connection from the connections table, so a row's `dwState` never decides whether it listens.
fn endpoints() -> Option<Vec<Endpoint>> {
    let mut out = Vec::new();
    let mut any = false;
    for (table, class) in [
        (TcpTable::Listener, TCP_TABLE_OWNER_PID_LISTENER),
        (TcpTable::Connections, TCP_TABLE_OWNER_PID_CONNECTIONS),
    ] {
        let peer = |remote: SocketAddr| (table == TcpTable::Connections).then_some(remote);
        if let Some(b) = tcp_class(u32::from(AF_INET.0), class) {
            any = true;
            out.extend(rows::<MIB_TCPROW_OWNER_PID>(&b).into_iter().map(|r| Endpoint {
                proto: Proto::Tcp,
                v6: false,
                local: sa4(r.dwLocalAddr, r.dwLocalPort),
                remote: peer(sa4(r.dwRemoteAddr, r.dwRemotePort)),
                state: row_state(table, r.dwState),
                state_name: row_name(table, r.dwState),
                pid: r.dwOwningPid,
            }));
        }
        if let Some(b) = tcp_class(u32::from(AF_INET6.0), class) {
            any = true;
            out.extend(rows::<MIB_TCP6ROW_OWNER_PID>(&b).into_iter().map(|r| Endpoint {
                proto: Proto::Tcp,
                v6: true,
                local: sa6(r.ucLocalAddr, r.dwLocalPort),
                remote: peer(sa6(r.ucRemoteAddr, r.dwRemotePort)),
                state: row_state(table, r.dwState),
                state_name: row_name(table, r.dwState),
                pid: r.dwOwningPid,
            }));
        }
    }
    if let Some(b) = udp(u32::from(AF_INET.0)) {
        any = true;
        out.extend(rows::<MIB_UDPROW_OWNER_PID>(&b).into_iter().map(|r| Endpoint {
            proto: Proto::Udp,
            v6: false,
            local: sa4(r.dwLocalAddr, r.dwLocalPort),
            remote: None,
            state: SocketState::Listen,
            state_name: String::new(),
            pid: r.dwOwningPid,
        }));
    }
    if let Some(b) = udp(u32::from(AF_INET6.0)) {
        any = true;
        out.extend(rows::<MIB_UDP6ROW_OWNER_PID>(&b).into_iter().map(|r| Endpoint {
            proto: Proto::Udp,
            v6: true,
            local: sa6(r.ucLocalAddr, r.dwLocalPort),
            remote: None,
            state: SocketState::Listen,
            state_name: String::new(),
            pid: r.dwOwningPid,
        }));
    }
    any.then_some(out)
}

/// Every endpoint owned by `pid`; `None` when the tables can't be read at all.
pub fn connections(pid: Pid) -> Option<Vec<Connection>> {
    Some(
        endpoints()?
            .into_iter()
            .filter(|e| e.pid == pid)
            .map(|e| Connection {
                protocol: match (e.proto, e.v6) {
                    (Proto::Tcp, false) => "TCP",
                    (Proto::Tcp, true) => "TCP6",
                    (Proto::Udp, false) => "UDP",
                    (Proto::Udp, true) => "UDP6",
                }
                .into(),
                local: e.local.to_string(),
                remote: match (e.proto, e.remote) {
                    (Proto::Udp, _) => String::new(),
                    (Proto::Tcp, Some(r)) => r.to_string(),
                    (Proto::Tcp, None) => String::new(),
                },
                state: e.state_name,
            })
            .collect(),
    )
}

/// Every socket of the machine with its owner, for the Network page; `None` when the tables can't be read at all.
pub fn sockets() -> Option<Vec<SocketRow>> {
    Some(
        endpoints()?
            .into_iter()
            .map(|e| SocketRow {
                proto: e.proto,
                local: e.local.ip(),
                local_port: e.local.port(),
                remote: e.remote.map(|r| (r.ip(), r.port())),
                state: e.state,
                pid: Some(e.pid),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, UdpSocket};

    #[test]
    fn decodes_table_values() {
        assert_eq!(port(u32::from(8080u16.to_be())), 8080);
        assert_eq!(ipv4(u32::from_ne_bytes([127, 0, 0, 1])), Ipv4Addr::LOCALHOST);
        assert_eq!(tcp_state(5), "ESTABLISHED");
        assert_eq!(tcp_state(2), "LISTEN");
        assert_eq!(tcp_state(100), "BOUND");
        assert_eq!(tcp_state(99), "UNKNOWN");
        assert_eq!(sa6([0; 16], u32::from(443u16.to_be())).to_string(), "[::]:443");
        let mut buf = 2u32.to_ne_bytes().to_vec();
        buf.extend(7u32.to_ne_bytes());
        // Truncated: claims two rows, holds one.
        assert_eq!(rows::<u32>(&buf), vec![7]);
        assert!(rows::<u32>(&[1]).is_empty());
    }

    #[test]
    fn finds_this_processs_sockets() {
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let udp = UdpSocket::bind("[::1]:0").unwrap();
        let tport = tcp.local_addr().unwrap().port();
        let uport = udp.local_addr().unwrap().port();
        let client = std::net::TcpStream::connect(("127.0.0.1", tport)).unwrap();
        let (_server_side, _) = tcp.accept().unwrap();
        let cport = client.local_addr().unwrap().port();
        // The listener is listed as listening on every build: by the table it is in, whatever its `dwState` says (2 on
        // Windows 11 26300, 0 on the CI runner since 2026-10-29, job 22533).
        let conns = connections(std::process::id()).unwrap();
        let mine: Vec<&Connection> = conns.iter().filter(|c| c.protocol == "TCP").collect();
        assert!(
            mine.iter()
                .any(|c| c.local == format!("127.0.0.1:{tport}") && c.state == "LISTEN" && c.remote.is_empty()),
            "{conns:?}"
        );
        assert!(
            mine.iter()
                .any(|c| c.local == format!("127.0.0.1:{cport}") && c.remote == format!("127.0.0.1:{tport}") && c.state == "ESTABLISHED"),
            "{conns:?}"
        );
        assert!(
            conns
                .iter()
                .any(|c| c.protocol == "UDP6" && c.local == format!("[::1]:{uport}") && c.state.is_empty()),
            "{conns:?}"
        );
    }

    #[test]
    fn the_machine_list_has_this_processs_listener_with_its_owner() {
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let tport = tcp.local_addr().unwrap().port();
        let rows = sockets().unwrap();
        let mine: Vec<&SocketRow> = rows.iter().filter(|r| r.local_port == tport).collect();
        assert!(
            mine.iter().any(|r| r.proto == Proto::Tcp
                && r.state == SocketState::Listen
                && r.remote.is_none()
                && r.pid == Some(std::process::id())),
            "{mine:?}"
        );
    }
}
