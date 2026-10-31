//! macOS `sysctl net.inet.tcp.pcblist_n` / `net.inet.udp.pcblist_n`: the kernel's list of protocol control blocks,
//! one `xinpcb_n`, `xsocket_n`, two `xsockbuf_n`, one `xsockstat_n` (and, for TCP, one `xtcpcb_n`) per socket. It is what
//! `netstat -anv` prints (`process:pid` is `xsocket_n.so_last_pid`). ADR 0026.
//!
//! The structs are `#ifdef PRIVATE` in XNU (`bsd/netinet/in_pcb.h`, `bsd/sys/socketvar.h`, `bsd/netinet/tcp_var.h`), not
//! in the public SDK and not stable API. So this parser trusts only what the kernel states itself: every record starts
//! with `{ len: u32, kind: u32 }`, records are padded to 8 bytes, and a record is read only when its `len` covers the
//! fields we read. A record that is too short is a [`PcbError::UnknownLayout`] (the caller shows totals and says why);
//! a record of an unknown kind is skipped by its `len`. Never panics on any input; every size is bounded. Pure and
//! compiled on every OS, so it is tested everywhere on captured fixtures.

use dyno_core::network::{Proto, SocketRow, SocketState};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Largest buffer accepted from one sysctl (a bound on kernel output we do not control the size of).
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
/// Most sockets taken from one list.
pub const MAX_SOCKETS: usize = 262_144;

/// `sizeof(struct xinpgen)`: the header and trailer of a list (`xig_len`, `xig_count`, `xig_gen`, `xig_sogen`).
const XINPGEN_LEN: usize = 24;
const XSO_SOCKET: u32 = 0x001;
const XSO_INPCB: u32 = 0x010;
const XSO_TCPCB: u32 = 0x020;
const XSO_STATS: u32 = 0x008;

/// Bytes each record must cover for the fields read from it (offsets below, `#pragma pack(4)`).
const MIN_INPCB: usize = 80; // laddr ends at 80
const MIN_SOCKET: usize = 76; // so_e_pid ends at 76
const MIN_TCPCB: usize = 40; // t_state ends at 40

// xinpcb_n offsets.
const INP_FPORT: usize = 16;
const INP_LPORT: usize = 18;
const INP_GENCNT: usize = 28;
const INP_VFLAG: usize = 44;
const INP_FADDR: usize = 48;
const INP_LADDR: usize = 64;
// xsocket_n offsets.
const SO_PROTOCOL: usize = 36;
const SO_LAST_PID: usize = 68;
const SO_E_PID: usize = 72;
// xtcpcb_n offsets.
const TCP_STATE: usize = 36;

const INP_IPV4: u8 = 0x1;
const INP_IPV6: u8 = 0x2;
const IPPROTO_TCP: i32 = 6;
const IPPROTO_UDP: i32 = 17;
const TCPS_TIME_WAIT: i32 = 10;

/// Why a list could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcbError {
    /// The kernel's record layout is not the one this parser knows (a header or record shorter than the fields read).
    UnknownLayout(String),
    /// The bytes are not a well-formed list (a record that runs past the end, or a size over the bounds).
    Malformed(String),
}

impl std::fmt::Display for PcbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PcbError::UnknownLayout(m) | PcbError::Malformed(m) => f.write_str(m),
        }
    }
}

/// One socket of the kernel's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcbSocket {
    /// The row for the page, with `pid` set to the last user of the socket when it is a live process.
    pub row: SocketRow,
    /// The effective pid when the socket works for another process (a delegated socket); `None` when the same or unset.
    pub effective_pid: Option<u32>,
}

/// The result of parsing one list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PcbList {
    /// The sockets, in the kernel's order.
    pub sockets: Vec<PcbSocket>,
    /// Groups left out: freed while the kernel copied them out, another protocol, or incomplete.
    pub skipped: usize,
    /// How many sockets the kernel announced in the header (`xig_count`) when it started the list.
    pub announced: usize,
}

impl PcbList {
    /// True when the kernel announced many more sockets than it sent. macOS 27.2 sends a process only its own sockets
    /// unless the executable's code-signing identifier contains a dot (measured, ADR 0026); such a list is not the
    /// machine's sockets and must not be shown as if it were. A few sockets come and go while the list is copied, so
    /// a small gap is not a restriction.
    pub fn restricted(&self) -> bool {
        let seen = self.sockets.len() + self.skipped;
        self.announced > seen + (self.announced / 4).max(16)
    }
}

/// A port: network byte order inside the otherwise little-endian struct.
fn be16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

/// The state a `t_state` number means (`TCPS_*` in `tcp_fsm.h`, stable since BSD).
fn tcp_state(t: i32) -> SocketState {
    match t {
        1 => SocketState::Listen,
        2 => SocketState::SynSent,
        3 => SocketState::SynReceived,
        4 => SocketState::Established,
        5 => SocketState::CloseWait,
        6 | 9 => SocketState::FinWait,
        7 => SocketState::Closing,
        8 => SocketState::LastAck,
        10 => SocketState::TimeWait,
        _ => SocketState::Other,
    }
}

/// An `in_addr_4in6` / `in6_addr` union at `at`: an IPv4 address lives in its last four bytes.
fn address(b: &[u8], at: usize, vflag: u8) -> Option<IpAddr> {
    let raw: [u8; 16] = b.get(at..at.checked_add(16)?)?.try_into().ok()?;
    if vflag & INP_IPV6 != 0 {
        Some(IpAddr::V6(Ipv6Addr::from(raw)))
    } else if vflag & INP_IPV4 != 0 {
        Some(IpAddr::V4(Ipv4Addr::new(raw[12], raw[13], raw[14], raw[15])))
    } else {
        None
    }
}

fn pid(p: u32) -> Option<u32> {
    // pid_t is signed; 0 is "no process" (kernel-owned or exited), negatives never occur.
    (p != 0 && p <= i32::MAX as u32).then_some(p)
}

/// The records of one socket, as slices into the list.
#[derive(Default)]
struct Group<'a> {
    inpcb: Option<&'a [u8]>,
    socket: Option<&'a [u8]>,
    tcpcb: Option<&'a [u8]>,
}

fn build(g: &Group<'_>, proto: Proto, gen_limit: u64) -> Result<Option<PcbSocket>, PcbError> {
    let (Some(inp), Some(so)) = (g.inpcb, g.socket) else {
        return Ok(None);
    };
    // Freed while the kernel copied the list out (netstat skips these too).
    if u64_at(inp, INP_GENCNT).is_some_and(|g| g > gen_limit) {
        return Ok(None);
    }
    let vflag = inp.get(INP_VFLAG).copied().unwrap_or(0);
    let (Some(local), Some(faddr)) = (address(inp, INP_LADDR, vflag), address(inp, INP_FADDR, vflag)) else {
        return Ok(None);
    };
    let (Some(fport), Some(lport)) = (be16_at(inp, INP_FPORT), be16_at(inp, INP_LPORT)) else {
        return Ok(None);
    };
    let protocol = i32_at(so, SO_PROTOCOL).unwrap_or(0);
    let state = match (proto, g.tcpcb) {
        (Proto::Tcp, Some(t)) => tcp_state(i32_at(t, TCP_STATE).unwrap_or(0)),
        (Proto::Tcp, None) => return Ok(None),
        (Proto::Udp, _) => SocketState::Listen,
    };
    // A connection in TIME_WAIT has no socket any more: its record is zeroed (protocol 0, no pid).
    let wanted = if proto == Proto::Tcp { IPPROTO_TCP } else { IPPROTO_UDP };
    let time_wait = proto == Proto::Tcp && protocol == 0 && i32_at(g.tcpcb.unwrap_or(&[]), TCP_STATE) == Some(TCPS_TIME_WAIT);
    if protocol != wanted && !time_wait {
        return Ok(None);
    }
    let last = u32_at(so, SO_LAST_PID).and_then(pid);
    let eff = u32_at(so, SO_E_PID).and_then(pid).filter(|e| Some(*e) != last);
    let connected = proto == Proto::Tcp && state != SocketState::Listen || (proto == Proto::Udp && fport != 0);
    Ok(Some(PcbSocket {
        row: SocketRow {
            proto,
            local,
            local_port: lport,
            remote: connected.then_some((faddr, fport)),
            state,
            pid: last,
        },
        effective_pid: eff,
    }))
}

/// Parses the bytes `sysctl net.inet.{tcp,udp}.pcblist_n` returned for `proto`.
pub fn parse(bytes: &[u8], proto: Proto) -> Result<PcbList, PcbError> {
    if bytes.len() > MAX_BYTES {
        return Err(PcbError::Malformed(format!(
            "The kernel's socket list is {} bytes, over the {MAX_BYTES} limit.",
            bytes.len()
        )));
    }
    let head_len = u32_at(bytes, 0).map(|l| l as usize);
    if head_len != Some(XINPGEN_LEN) {
        return Err(PcbError::UnknownLayout(format!(
            "The socket list starts with a header of {head_len:?} bytes, not {XINPGEN_LEN}."
        )));
    }
    let gen_limit = u64_at(bytes, 8).unwrap_or(u64::MAX);
    let mut out = PcbList {
        announced: u32_at(bytes, 4).unwrap_or(0) as usize,
        ..PcbList::default()
    };
    let mut group = Group::default();
    let mut at = XINPGEN_LEN;
    // The last 24 bytes are the trailer (another header with the generation at the end): records stop before it.
    while at.saturating_add(XINPGEN_LEN) < bytes.len() {
        let (Some(len), Some(kind)) = (u32_at(bytes, at), u32_at(bytes, at + 4)) else {
            break;
        };
        let len = len as usize;
        if len < 8 {
            return Err(PcbError::Malformed(format!(
                "A socket record at byte {at} says it is {len} bytes long."
            )));
        }
        let Some(rec) = at.checked_add(len).and_then(|end| bytes.get(at..end)) else {
            return Err(PcbError::Malformed(format!(
                "A socket record at byte {at} runs past the end of the list."
            )));
        };
        let need = match kind {
            XSO_INPCB => Some(MIN_INPCB),
            XSO_SOCKET => Some(MIN_SOCKET),
            XSO_TCPCB => Some(MIN_TCPCB),
            _ => None,
        };
        if need.is_some_and(|n| len < n) {
            return Err(PcbError::UnknownLayout(format!(
                "The kernel's record of kind {kind:#x} is {len} bytes; this reader needs at least {}.",
                need.unwrap_or(0)
            )));
        }
        match kind {
            XSO_INPCB => {
                // A new socket starts; whatever was pending is incomplete.
                if group.inpcb.is_some() {
                    out.skipped += 1;
                }
                group = Group {
                    inpcb: Some(rec),
                    ..Group::default()
                };
            }
            XSO_SOCKET => group.socket = Some(rec),
            XSO_TCPCB => group.tcpcb = Some(rec),
            _ => {}
        }
        // A TCP socket ends with its tcpcb, a UDP socket with its statistics.
        let done = match proto {
            Proto::Tcp => kind == XSO_TCPCB,
            Proto::Udp => kind == XSO_STATS,
        };
        if done {
            match build(&group, proto, gen_limit)? {
                Some(s) if out.sockets.len() < MAX_SOCKETS => out.sockets.push(s),
                _ => out.skipped += 1,
            }
            group = Group::default();
        }
        at = match at.checked_add((len + 7) & !7) {
            Some(n) => n,
            None => break,
        };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &[u8] = include_bytes!("../../tests/fixtures/mac_pcblist/tcp.bin");
    const UDP: &[u8] = include_bytes!("../../tests/fixtures/mac_pcblist/udp.bin");

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn tcp_fixture_gives_every_state_address_family_and_owner() {
        let l = parse(TCP, Proto::Tcp).unwrap();
        assert_eq!((l.sockets.len(), l.skipped, l.announced), (7, 0, 7));
        assert!(!l.restricted());
        let r: Vec<&SocketRow> = l.sockets.iter().map(|s| &s.row).collect();
        assert_eq!(
            r[0],
            &SocketRow {
                proto: Proto::Tcp,
                local: ip("192.0.2.10"),
                local_port: 49152,
                remote: Some((ip("198.51.100.7"), 443)),
                state: SocketState::Established,
                pid: Some(4242)
            }
        );
        assert_eq!((r[1].state, r[1].local, r[1].remote), (SocketState::Listen, ip("127.0.0.1"), None));
        assert_eq!(
            (r[2].state, r[2].local, r[2].local_port, r[2].pid),
            (SocketState::Listen, ip("::"), 5000, Some(4244))
        );
        assert_eq!((r[3].local, r[3].remote), (ip("2001:db8::1"), Some((ip("2001:db8::2"), 443))));
        assert_eq!(
            l.sockets[3].effective_pid,
            Some(4246),
            "a delegated socket names its effective process too"
        );
        assert_eq!(l.sockets[0].effective_pid, None);
        assert_eq!(
            (r[4].state, r[4].pid),
            (SocketState::TimeWait, None),
            "TIME_WAIT has no socket, so no owner"
        );
        assert_eq!(r[5].state, SocketState::CloseWait);
        assert_eq!(r[6].state, SocketState::FinWait);
    }

    #[test]
    fn udp_fixture_gives_bound_and_connected_sockets() {
        let l = parse(UDP, Proto::Udp).unwrap();
        let r: Vec<&SocketRow> = l.sockets.iter().map(|s| &s.row).collect();
        assert_eq!(r.len(), 3);
        assert_eq!(
            (r[0].local, r[0].local_port, r[0].remote, r[0].pid),
            (ip("0.0.0.0"), 5353, None, Some(4250))
        );
        assert_eq!((r[1].local, r[1].pid), (ip("::"), Some(4251)));
        assert_eq!(r[2].remote, Some((ip("198.51.100.9"), 53)));
        assert!(r.iter().all(|x| x.state == SocketState::Listen && x.proto == Proto::Udp));
    }

    #[test]
    fn a_list_the_kernel_cut_down_to_own_sockets_is_restricted() {
        // The header announces 7 sockets; rewrite it to announce 400 as a machine with many would.
        let mut b = TCP.to_vec();
        b[4..8].copy_from_slice(&400u32.to_le_bytes());
        assert!(parse(&b, Proto::Tcp).unwrap().restricted());
        b[4..8].copy_from_slice(&12u32.to_le_bytes());
        assert!(
            !parse(&b, Proto::Tcp).unwrap().restricted(),
            "a few sockets coming and going is not a restriction"
        );
    }

    #[test]
    fn a_sockets_freed_while_copying_is_left_out() {
        // Header generation 3 is below the records' generation (1000): every record is newer than the list.
        let mut b = TCP.to_vec();
        b[8..16].copy_from_slice(&3u64.to_le_bytes());
        let l = parse(&b, Proto::Tcp).unwrap();
        assert_eq!((l.sockets.len(), l.skipped), (0, 7));
    }

    #[test]
    fn another_protocol_in_the_list_is_left_out() {
        let l = parse(UDP, Proto::Tcp).unwrap();
        assert!(l.sockets.is_empty());
    }

    #[test]
    fn every_truncation_and_every_single_byte_change_is_handled_without_panicking() {
        for proto_bytes in [(TCP, Proto::Tcp), (UDP, Proto::Udp)] {
            let (full, proto) = proto_bytes;
            for cut in 0..full.len() {
                let _ = parse(&full[..cut], proto);
            }
            for at in 0..full.len() {
                for v in [0u8, 1, 0x7f, 0xff] {
                    let mut b = full.to_vec();
                    b[at] = v;
                    let _ = parse(&b, proto);
                }
            }
        }
    }

    #[test]
    fn a_header_of_another_size_is_an_unknown_layout() {
        let mut b = TCP.to_vec();
        b[0] = 32;
        assert!(matches!(parse(&b, Proto::Tcp), Err(PcbError::UnknownLayout(_))));
        assert!(matches!(parse(&[], Proto::Tcp), Err(PcbError::UnknownLayout(_))));
    }

    #[test]
    fn a_record_shorter_than_the_fields_read_is_an_unknown_layout() {
        let mut b = TCP.to_vec();
        // The first record (xinpcb_n) now claims to be 48 bytes: the kernel's layout is not the one we know.
        b[24..28].copy_from_slice(&48u32.to_le_bytes());
        assert!(matches!(parse(&b, Proto::Tcp), Err(PcbError::UnknownLayout(_))));
    }

    #[test]
    fn a_record_running_past_the_end_or_of_no_length_is_malformed() {
        let mut b = TCP.to_vec();
        b[24..28].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        assert!(matches!(parse(&b, Proto::Tcp), Err(PcbError::Malformed(_))));
        b[24..28].copy_from_slice(&3u32.to_le_bytes());
        assert!(matches!(parse(&b, Proto::Tcp), Err(PcbError::Malformed(_))));
    }

    #[test]
    fn a_list_over_the_size_bound_is_refused_unread() {
        let big = vec![0u8; MAX_BYTES + 1];
        assert!(matches!(parse(&big, Proto::Tcp), Err(PcbError::Malformed(_))));
    }
}
