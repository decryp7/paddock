//! `/proc/net/{tcp,tcp6,udp,udp6}` tables: the sockets of a network namespace with their socket inode, which is how a
//! process's sockets (`socket:[inode]` descriptors) are matched to connections.

use std::net::{Ipv4Addr, Ipv6Addr};

/// A table of `/proc/net`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    /// `tcp`.
    Tcp,
    /// `tcp6`.
    Tcp6,
    /// `udp`.
    Udp,
    /// `udp6`.
    Udp6,
}

impl Proto {
    /// The file name under `/proc/net`.
    pub fn file(self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Tcp6 => "tcp6",
            Proto::Udp => "udp",
            Proto::Udp6 => "udp6",
        }
    }

    /// The protocol name shown to the user (`TCP`, `TCP6`, `UDP`, `UDP6`).
    pub fn label(self) -> &'static str {
        match self {
            Proto::Tcp => "TCP",
            Proto::Tcp6 => "TCP6",
            Proto::Udp => "UDP",
            Proto::Udp6 => "UDP6",
        }
    }

    fn is_tcp(self) -> bool {
        matches!(self, Proto::Tcp | Proto::Tcp6)
    }

    fn is_v6(self) -> bool {
        matches!(self, Proto::Tcp6 | Proto::Udp6)
    }

    /// All four tables.
    pub const ALL: [Proto; 4] = [Proto::Tcp, Proto::Tcp6, Proto::Udp, Proto::Udp6];
}

/// One socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetRow {
    /// Local `address:port` (`[address]:port` for IPv6).
    pub local: String,
    /// Remote `address:port`; empty when the socket has no peer (listening or unconnected).
    pub remote: String,
    /// TCP state (`ESTABLISHED`, `LISTEN`…); empty for UDP.
    pub state: &'static str,
    /// The socket's inode.
    pub inode: u64,
}

/// The name of a TCP state code.
pub fn tcp_state(code: u8) -> &'static str {
    match code {
        0x01 => "ESTABLISHED",
        0x02 => "SYN_SENT",
        0x03 => "SYN_RECV",
        0x04 => "FIN_WAIT1",
        0x05 => "FIN_WAIT2",
        0x06 => "TIME_WAIT",
        0x07 => "CLOSE",
        0x08 => "CLOSE_WAIT",
        0x09 => "LAST_ACK",
        0x0A => "LISTEN",
        0x0B => "CLOSING",
        _ => "UNKNOWN",
    }
}

/// `0100007F:0277` (IPv4, little-endian words as the kernel prints them on little-endian machines) as `127.0.0.1:631`.
fn address(field: &str, v6: bool) -> Option<(String, u16, bool)> {
    let (host, port) = field.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    if v6 {
        if host.len() != 32 || !host.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, chunk) in bytes.chunks_mut(4).enumerate() {
            let word = u32::from_str_radix(&host[i * 8..i * 8 + 8], 16).ok()?;
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        let ip = Ipv6Addr::from(bytes);
        Some((format!("[{ip}]:{port}"), port, ip.is_unspecified()))
    } else {
        if host.len() != 8 {
            return None;
        }
        let ip = Ipv4Addr::from(u32::from_str_radix(host, 16).ok()?.to_le_bytes());
        Some((format!("{ip}:{port}"), port, ip.is_unspecified()))
    }
}

/// Parses one `/proc/net` table. Rows it cannot read are skipped.
pub fn parse_table(text: &str, proto: Proto) -> Vec<NetRow> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (local, _, _) = address(f.get(1)?, proto.is_v6())?;
            let (remote, rport, runspec) = address(f.get(2)?, proto.is_v6())?;
            let state = if proto.is_tcp() {
                tcp_state(u8::from_str_radix(f.get(3)?, 16).ok()?)
            } else {
                ""
            };
            Some(NetRow {
                local,
                remote: if runspec && rport == 0 { String::new() } else { remote },
                state,
                inode: f.get(9)?.parse().ok()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0277 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 11111 1 0000000000000000 100 0 0 10 0
   1: 0F01A8C0:C350 22D8B85D:01BB 01 00000000:00000000 02:000A7C4E 00000000  1000        0 22222 2 0000000000000000 20 4 30 10 -1
   2: garbage
";
    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:0016 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 33333 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn tcp_rows() {
        let rows = parse_table(TCP, Proto::Tcp);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            NetRow {
                local: "127.0.0.1:631".into(),
                remote: String::new(),
                state: "LISTEN",
                inode: 11111
            }
        );
        assert_eq!(rows[1].local, "192.168.1.15:50000");
        assert_eq!(rows[1].remote, "93.184.216.34:443");
        assert_eq!((rows[1].state, rows[1].inode), ("ESTABLISHED", 22222));
    }

    #[test]
    fn tcp6_and_udp_rows() {
        let rows = parse_table(TCP6, Proto::Tcp6);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].local, "[::1]:22");
        assert_eq!(rows[0].state, "LISTEN");
        let udp = parse_table(TCP, Proto::Udp);
        assert!(udp.iter().all(|r| r.state.is_empty()));
        assert!(parse_table(TCP6, Proto::Tcp).is_empty(), "an IPv6 row is not an IPv4 address");
    }

    #[test]
    fn labels_and_states() {
        assert_eq!(Proto::Udp6.label(), "UDP6");
        assert_eq!(Proto::Tcp.file(), "tcp");
        assert_eq!(tcp_state(0x06), "TIME_WAIT");
        assert_eq!(tcp_state(0xEE), "UNKNOWN");
    }

    #[test]
    fn junk_never_panics() {
        for s in [
            "",
            "x\n\n",
            "h\n 0: é:é é:é 0A",
            "h\n 0: 0100007F:zz 00000000:0000 0A",
            "h\n 0: éééééééééééééééééééééééééééééééé:0001 a:b",
        ] {
            for p in Proto::ALL {
                let _ = parse_table(s, p);
            }
        }
    }
}
