//! A process's TCP and UDP endpoints (IPv4 and IPv6) from the IP Helper owner-pid tables.

use dyno_core::process::{Connection, Pid};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID,
    TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

/// The largest table read (bytes).
const MAX_TABLE: u32 = 64 << 20;

/// The MIB_TCP_STATE name.
pub fn tcp_state(state: u32) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RECEIVED",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT_1",
        7 => "FIN_WAIT_2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        _ => "UNKNOWN",
    }
}

/// A port as stored in the tables (network byte order in the low 16 bits).
pub fn port(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

/// An IPv4 address as stored in the tables (network byte order in memory).
pub fn ipv4(raw: u32) -> Ipv4Addr {
    Ipv4Addr::from(raw.to_ne_bytes())
}

fn v4(addr: u32, p: u32) -> String {
    SocketAddr::from((ipv4(addr), port(p))).to_string()
}

fn v6(addr: [u8; 16], p: u32) -> String {
    SocketAddr::from((Ipv6Addr::from(addr), port(p))).to_string()
}

/// Rows of a `{ dwNumEntries; ROW table[] }` table in `buf`.
fn rows<T: Copy>(buf: &[u8]) -> Vec<T> {
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

fn tcp(family: u32) -> Option<Vec<u8>> {
    table(|p, size| {
        // SAFETY: `p` is None (size query) or a buffer of `*size` bytes.
        unsafe { GetExtendedTcpTable(p, size, false, family, TCP_TABLE_OWNER_PID_ALL, 0) }
    })
}

fn udp(family: u32) -> Option<Vec<u8>> {
    table(|p, size| {
        // SAFETY: as above.
        unsafe { GetExtendedUdpTable(p, size, false, family, UDP_TABLE_OWNER_PID, 0) }
    })
}

/// Every endpoint owned by `pid`; `None` when the tables can't be read at all.
pub fn connections(pid: Pid) -> Option<Vec<Connection>> {
    let mut out = Vec::new();
    let mut any = false;
    if let Some(b) = tcp(u32::from(AF_INET.0)) {
        any = true;
        out.extend(
            rows::<MIB_TCPROW_OWNER_PID>(&b)
                .into_iter()
                .filter(|r| r.dwOwningPid == pid)
                .map(|r| Connection {
                    protocol: "TCP".into(),
                    local: v4(r.dwLocalAddr, r.dwLocalPort),
                    remote: if r.dwState == 2 {
                        String::new()
                    } else {
                        v4(r.dwRemoteAddr, r.dwRemotePort)
                    },
                    state: tcp_state(r.dwState).into(),
                }),
        );
    }
    if let Some(b) = tcp(u32::from(AF_INET6.0)) {
        any = true;
        out.extend(
            rows::<MIB_TCP6ROW_OWNER_PID>(&b)
                .into_iter()
                .filter(|r| r.dwOwningPid == pid)
                .map(|r| Connection {
                    protocol: "TCP6".into(),
                    local: v6(r.ucLocalAddr, r.dwLocalPort),
                    remote: if r.dwState == 2 {
                        String::new()
                    } else {
                        v6(r.ucRemoteAddr, r.dwRemotePort)
                    },
                    state: tcp_state(r.dwState).into(),
                }),
        );
    }
    if let Some(b) = udp(u32::from(AF_INET.0)) {
        any = true;
        out.extend(
            rows::<MIB_UDPROW_OWNER_PID>(&b)
                .into_iter()
                .filter(|r| r.dwOwningPid == pid)
                .map(|r| Connection {
                    protocol: "UDP".into(),
                    local: v4(r.dwLocalAddr, r.dwLocalPort),
                    remote: String::new(),
                    state: String::new(),
                }),
        );
    }
    if let Some(b) = udp(u32::from(AF_INET6.0)) {
        any = true;
        out.extend(
            rows::<MIB_UDP6ROW_OWNER_PID>(&b)
                .into_iter()
                .filter(|r| r.dwOwningPid == pid)
                .map(|r| Connection {
                    protocol: "UDP6".into(),
                    local: v6(r.ucLocalAddr, r.dwLocalPort),
                    remote: String::new(),
                    state: String::new(),
                }),
        );
    }
    any.then_some(out)
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
        assert_eq!(tcp_state(99), "UNKNOWN");
        assert_eq!(v6([0; 16], u32::from(443u16.to_be())), "[::]:443");
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
        let conns = connections(std::process::id()).unwrap();
        assert!(
            conns
                .iter()
                .any(|c| c.protocol == "TCP" && c.local == format!("127.0.0.1:{tport}") && c.state == "LISTEN"),
            "{conns:?}"
        );
        assert!(
            conns.iter().any(|c| c.protocol == "UDP6" && c.local == format!("[::1]:{uport}")),
            "{conns:?}"
        );
    }
}
