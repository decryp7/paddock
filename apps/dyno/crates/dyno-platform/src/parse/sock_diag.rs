//! Linux `sock_diag` (netlink `INET_DIAG`) messages: the request that dumps every TCP socket with its `tcp_info`, and
//! the parser of the replies. The kernel (4.1 and later) reports `bytes_sent` and `bytes_received` per socket in
//! `tcp_info`, and any user may ask, so per-program traffic needs no elevation. Pure: the adapter does the socket I/O.

use dyno_core::network_traffic::{TrafficCounter, MAX_COUNTERS};
use std::collections::HashMap;

/// Most sockets accepted from one dump (a bound on kernel output we do not control the size of).
pub const MAX_DIAG_SOCKETS: usize = 262_144;

const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const NLM_F_REQUEST_DUMP: u16 = 0x0301;
const NLMSG_HDR: usize = 16;
/// `struct inet_diag_msg` without the attributes that follow it.
const DIAG_MSG: usize = 72;
const INET_DIAG_INFO: u16 = 2;
const IPPROTO_TCP: u8 = 6;
/// Offsets inside `struct tcp_info`.
const OFF_BYTES_ACKED: usize = 120;
const OFF_BYTES_RECEIVED: usize = 128;
const OFF_BYTES_SENT: usize = 200;

/// The netlink request that dumps all TCP sockets of `family` (`AF_INET` = 2, `AF_INET6` = 10) with `tcp_info`.
pub fn tcp_dump_request(family: u8, seq: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(72);
    b.extend_from_slice(&72u32.to_ne_bytes());
    b.extend_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
    b.extend_from_slice(&NLM_F_REQUEST_DUMP.to_ne_bytes());
    b.extend_from_slice(&seq.to_ne_bytes());
    b.extend_from_slice(&0u32.to_ne_bytes()); // port id
                                              // struct inet_diag_req_v2: family, protocol, ext (bit INET_DIAG_INFO - 1), pad, states, then a zero socket id.
    b.extend_from_slice(&[family, IPPROTO_TCP, 1 << (INET_DIAG_INFO - 1), 0]);
    b.extend_from_slice(&0x0fffu32.to_ne_bytes());
    b.resize(72, 0);
    b
}

/// One TCP socket with its owner's user id, inode and cumulative byte counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagSocket {
    /// Socket inode (matches `socket:[inode]` in `/proc/<pid>/fd`).
    pub inode: u64,
    /// Bytes received over the socket's life.
    pub bytes_in: u64,
    /// Bytes sent over the socket's life.
    pub bytes_out: u64,
}

/// What one datagram from the kernel held.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DiagChunk {
    /// Sockets that carried a `tcp_info` with byte counters.
    pub sockets: Vec<DiagSocket>,
    /// The dump is complete (`NLMSG_DONE`).
    pub done: bool,
    /// The kernel refused with this (negative errno) code.
    pub error: Option<i32>,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(b.get(o..o.checked_add(2)?)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(o..o.checked_add(4)?)?.try_into().ok()?))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_ne_bytes(b.get(o..o.checked_add(8)?)?.try_into().ok()?))
}

/// The counters in a `tcp_info` blob: `bytes_received`, and `bytes_sent` (older kernels without it: `bytes_acked`).
fn tcp_info_bytes(info: &[u8]) -> Option<(u64, u64)> {
    let received = u64_at(info, OFF_BYTES_RECEIVED)?;
    let sent = u64_at(info, OFF_BYTES_SENT).or_else(|| u64_at(info, OFF_BYTES_ACKED))?;
    Some((received, sent))
}

/// Parses one datagram of replies. Truncated or malformed messages end the parse instead of panicking.
pub fn parse_diag_chunk(buf: &[u8]) -> DiagChunk {
    let mut chunk = DiagChunk::default();
    let mut pos = 0usize;
    while let (Some(len), Some(kind)) = (u32_at(buf, pos), u16_at(buf, pos + 4)) {
        let len = len as usize;
        let Some(end) = pos.checked_add(len).filter(|e| len >= NLMSG_HDR && *e <= buf.len()) else {
            break;
        };
        let body = &buf[pos + NLMSG_HDR..end];
        match kind {
            NLMSG_DONE => {
                chunk.done = true;
                break;
            }
            NLMSG_ERROR => {
                // A zero code is an acknowledgement; a negative one is the refusal.
                chunk.error = i32::from_ne_bytes(body.get(..4).and_then(|s| s.try_into().ok()).unwrap_or([0; 4])).checked_neg();
                chunk.done = true;
                break;
            }
            _ => {
                if chunk.sockets.len() < MAX_DIAG_SOCKETS {
                    if let Some(s) = parse_socket(body) {
                        chunk.sockets.push(s);
                    }
                }
            }
        }
        pos = pos.saturating_add(len.next_multiple_of(4));
    }
    chunk
}

fn parse_socket(body: &[u8]) -> Option<DiagSocket> {
    let inode = u64::from(u32_at(body, 68)?);
    // Attributes follow the fixed part: { u16 len; u16 type; payload }, each padded to 4 bytes.
    let mut pos = DIAG_MSG;
    while let (Some(len), Some(kind)) = (u16_at(body, pos), u16_at(body, pos + 2)) {
        let len = usize::from(len);
        if len < 4 {
            break;
        }
        let end = pos.checked_add(len)?;
        if kind == INET_DIAG_INFO {
            let (bytes_in, bytes_out) = tcp_info_bytes(body.get(pos + 4..end)?)?;
            return Some(DiagSocket {
                inode,
                bytes_in,
                bytes_out,
            });
        }
        pos = pos.checked_add(len.next_multiple_of(4))?;
    }
    None
}

/// The inode in a `/proc/<pid>/fd` link text such as `socket:[48213]`.
pub fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?.strip_suffix(']')?.parse().ok()
}

/// Sums the sockets of each process: `owners` maps a socket inode to `(pid, program name)`. Sockets nobody visible owns
/// (other users' processes) are left out.
pub fn counters_by_process(sockets: &[DiagSocket], owners: &HashMap<u64, (u32, String)>) -> Vec<TrafficCounter> {
    let mut by_pid: HashMap<u32, TrafficCounter> = HashMap::new();
    for s in sockets {
        let Some((pid, name)) = owners.get(&s.inode) else { continue };
        let c = by_pid.entry(*pid).or_insert_with(|| TrafficCounter {
            pid: *pid,
            name: name.clone(),
            bytes_in: 0,
            bytes_out: 0,
        });
        c.bytes_in = c.bytes_in.saturating_add(s.bytes_in);
        c.bytes_out = c.bytes_out.saturating_add(s.bytes_out);
    }
    let mut v: Vec<_> = by_pid.into_values().collect();
    v.sort_by_key(|c| c.pid);
    v.truncate(MAX_COUNTERS);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply message for a socket: header, fixed part with the inode, and optionally a tcp_info attribute.
    fn reply(inode: u32, info: Option<(u64, u64)>, kind: u16) -> Vec<u8> {
        let mut body = vec![0u8; DIAG_MSG];
        body[68..72].copy_from_slice(&inode.to_ne_bytes());
        // An unrelated attribute first, to prove attributes are walked.
        body.extend_from_slice(&[6, 0, 8, 0, 1, 2, 0, 0]);
        if let Some((rx, tx)) = info {
            let mut ti = vec![0u8; 216];
            ti[OFF_BYTES_RECEIVED..OFF_BYTES_RECEIVED + 8].copy_from_slice(&rx.to_ne_bytes());
            ti[OFF_BYTES_SENT..OFF_BYTES_SENT + 8].copy_from_slice(&tx.to_ne_bytes());
            body.extend_from_slice(&((ti.len() + 4) as u16).to_ne_bytes());
            body.extend_from_slice(&INET_DIAG_INFO.to_ne_bytes());
            body.extend_from_slice(&ti);
        }
        let mut m = Vec::new();
        m.extend_from_slice(&((NLMSG_HDR + body.len()) as u32).to_ne_bytes());
        m.extend_from_slice(&kind.to_ne_bytes());
        m.extend_from_slice(&[0; 10]);
        m.extend_from_slice(&body);
        m
    }

    fn control(kind: u16, payload: &[u8]) -> Vec<u8> {
        let mut m = Vec::new();
        m.extend_from_slice(&((NLMSG_HDR + payload.len()) as u32).to_ne_bytes());
        m.extend_from_slice(&kind.to_ne_bytes());
        m.extend_from_slice(&[0; 10]);
        m.extend_from_slice(payload);
        m
    }

    #[test]
    fn the_request_asks_for_tcp_info_of_all_sockets() {
        let r = tcp_dump_request(2, 7);
        assert_eq!(r.len(), 72);
        assert_eq!(u32::from_ne_bytes(r[0..4].try_into().unwrap()), 72);
        assert_eq!(u16::from_ne_bytes(r[4..6].try_into().unwrap()), SOCK_DIAG_BY_FAMILY);
        assert_eq!(&r[16..20], &[2, 6, 2, 0]);
    }

    #[test]
    fn sockets_with_counters_are_read_and_the_dump_end_is_seen() {
        let mut buf = reply(100, Some((5_000, 700)), 20);
        buf.extend(reply(101, None, 20));
        buf.extend(reply(102, Some((1, 2)), 20));
        buf.extend(control(NLMSG_DONE, &[0; 4]));
        let c = parse_diag_chunk(&buf);
        assert!(c.done && c.error.is_none());
        assert_eq!(
            c.sockets,
            vec![
                DiagSocket {
                    inode: 100,
                    bytes_in: 5_000,
                    bytes_out: 700
                },
                DiagSocket {
                    inode: 102,
                    bytes_in: 1,
                    bytes_out: 2
                },
            ]
        );
    }

    #[test]
    fn a_refusal_carries_its_errno_and_an_ack_carries_none() {
        let c = parse_diag_chunk(&control(NLMSG_ERROR, &(-1i32).to_ne_bytes()));
        assert_eq!((c.done, c.error), (true, Some(1)));
        assert_eq!(parse_diag_chunk(&control(NLMSG_ERROR, &0i32.to_ne_bytes())).error, Some(0));
        assert_eq!(parse_diag_chunk(&control(NLMSG_ERROR, &[])).error, Some(0));
    }

    #[test]
    fn garbage_and_truncation_never_panic() {
        let good = reply(1, Some((9, 9)), 20);
        for n in 0..good.len() {
            let _ = parse_diag_chunk(&good[..n]);
        }
        assert!(parse_diag_chunk(&[]).sockets.is_empty());
        assert!(parse_diag_chunk(&[0xff; 64]).sockets.is_empty());
        // A length smaller than the header must not loop forever.
        assert!(parse_diag_chunk(&[0u8; 16]).sockets.is_empty());
        // A socket without a tcp_info attribute is skipped.
        let short = reply(5, None, 20);
        assert!(parse_diag_chunk(&short).sockets.is_empty());
    }

    #[test]
    fn an_old_kernel_without_bytes_sent_falls_back_to_bytes_acked() {
        let mut ti = vec![0u8; 136];
        ti[OFF_BYTES_ACKED..OFF_BYTES_ACKED + 8].copy_from_slice(&40u64.to_ne_bytes());
        ti[OFF_BYTES_RECEIVED..OFF_BYTES_RECEIVED + 8].copy_from_slice(&30u64.to_ne_bytes());
        assert_eq!(tcp_info_bytes(&ti), Some((30, 40)));
        assert_eq!(tcp_info_bytes(&ti[..100]), None);
    }

    #[test]
    fn socket_links_become_inodes() {
        assert_eq!(socket_inode("socket:[48213]"), Some(48_213));
        assert_eq!(socket_inode("pipe:[1]"), None);
        assert_eq!(socket_inode("socket:[x]"), None);
        assert_eq!(socket_inode(""), None);
    }

    #[test]
    fn sockets_are_summed_per_visible_owner() {
        let sockets = [
            DiagSocket {
                inode: 1,
                bytes_in: 10,
                bytes_out: 1,
            },
            DiagSocket {
                inode: 2,
                bytes_in: 20,
                bytes_out: 2,
            },
            DiagSocket {
                inode: 3,
                bytes_in: 99,
                bytes_out: 99,
            },
        ];
        let owners = HashMap::from([(1, (7, "alpha".to_string())), (2, (7, "alpha".to_string()))]);
        let c = counters_by_process(&sockets, &owners);
        assert_eq!(c.len(), 1, "inode 3 belongs to nobody we can see");
        assert_eq!((c[0].pid, c[0].bytes_in, c[0].bytes_out), (7, 30, 3));
    }
}
