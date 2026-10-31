//! The kernel's socket lists through `sysctlbyname("net.inet.{tcp,udp}.pcblist_n")` (ADR 0026): one call per protocol,
//! no per-process and no per-descriptor access, so nothing is held on another program's sockets. Parsing is in
//! [`crate::parse::mac_pcblist`].

use crate::parse::mac_pcblist::{self, PcbError, PcbList, MAX_BYTES};
use dyno_core::network::Proto;
use std::ffi::CString;

/// Reads one raw list. The list can grow between the size query and the read: the buffer gets slack and a short read
/// (`ENOMEM`) is retried a few times with the size asked again.
#[allow(unsafe_code)]
fn raw(name: &str) -> Result<Vec<u8>, String> {
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    for _ in 0..8 {
        let mut size: libc::size_t = 0;
        // SAFETY: a null buffer with a valid size pointer asks for the size only; the name is a NUL-terminated string.
        let rc = unsafe { libc::sysctlbyname(cname.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let cap = size.saturating_add(size / 4).saturating_add(4096);
        if cap > MAX_BYTES {
            return Err(format!("the list is {size} bytes, over the {MAX_BYTES} limit"));
        }
        let mut buf = vec![0u8; cap];
        let mut len: libc::size_t = cap;
        // SAFETY: `buf` is `len` writable bytes; the kernel writes at most `len` and sets `len` to what it wrote.
        let rc = unsafe { libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
        if rc == 0 {
            buf.truncate(len.min(cap));
            return Ok(buf);
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ENOMEM) {
            return Err(e.to_string());
        }
    }
    Err("the list kept growing while it was read".into())
}

/// The kernel's list for `proto`, parsed.
pub fn list(proto: Proto) -> Result<PcbList, PcbError> {
    let name = match proto {
        Proto::Tcp => "net.inet.tcp.pcblist_n",
        Proto::Udp => "net.inet.udp.pcblist_n",
    };
    let bytes = raw(name).map_err(|e| PcbError::Malformed(format!("The OS would not give the socket list ({e}).")))?;
    mac_pcblist::parse(&bytes, proto)
}

/// The raw bytes of a list, for capturing test fixtures (tests only).
#[cfg(test)]
pub fn raw_for_tests(proto: Proto) -> Vec<u8> {
    raw(match proto {
        Proto::Tcp => "net.inet.tcp.pcblist_n",
        Proto::Udp => "net.inet.udp.pcblist_n",
    })
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::network::SocketState;
    use std::collections::BTreeMap;
    use std::net::{TcpListener, TcpStream, UdpSocket};

    #[test]
    fn the_kernel_lists_this_tests_own_sockets_with_this_process_as_owner() {
        let me = std::process::id();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (_server_side, _) = listener.accept().unwrap();
        let cport = client.local_addr().unwrap().port();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let uport = udp.local_addr().unwrap().port();
        let v6 = TcpListener::bind("[::1]:0").unwrap();
        let v6port = v6.local_addr().unwrap().port();

        let tcp = list(Proto::Tcp).unwrap();
        let find = |l: &PcbList, p: u16, st: Option<SocketState>| {
            l.sockets
                .iter()
                .find(|s| s.row.local_port == p && st.is_none_or(|x| x == s.row.state))
                .cloned()
        };
        let l = find(&tcp, port, Some(SocketState::Listen)).expect("the listener is listed");
        assert_eq!(l.row.pid, Some(me));
        assert!(l.row.local.is_loopback() && l.row.remote.is_none());
        let c = find(&tcp, cport, None).expect("the client end is listed");
        assert_eq!((c.row.state, c.row.pid), (SocketState::Established, Some(me)));
        assert_eq!(c.row.remote.map(|r| r.1), Some(port));
        let v = find(&tcp, v6port, Some(SocketState::Listen)).expect("the IPv6 listener is listed");
        assert!(v.row.local.is_ipv6() && v.row.local.is_loopback());
        let u = find(&list(Proto::Udp).unwrap(), uport, None).expect("the UDP socket is listed");
        assert_eq!((u.row.pid, u.row.remote), (Some(me), None));
    }

    /// One netstat row reduced to what both sources know.
    type Key = (bool, u16, u16, String, u32);

    fn port_of(addr_port: &str) -> u16 {
        addr_port.rsplit('.').next().and_then(|p| p.parse().ok()).unwrap_or(0)
    }

    fn from_netstat(proto: &str) -> Vec<Key> {
        // `netstat` started from a test process printed nothing on macOS 27.2 (ADR 0026), so the caller captures it from a
        // shell just before: `netstat -anv -p tcp > $DIR/tcp.txt`, likewise udp, and sets PCB_NETSTAT_DIR.
        let dir = std::env::var("PCB_NETSTAT_DIR").expect("PCB_NETSTAT_DIR holds tcp.txt and udp.txt captured with netstat -anv");
        let text = std::fs::read_to_string(format!("{dir}/{proto}.txt")).unwrap();
        let mut rows = Vec::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 8 || !(f[0].starts_with("tcp") || f[0].starts_with("udp")) {
                continue;
            }
            let tcp = f[0].starts_with("tcp");
            let skip = if tcp { 6 } else { 5 };
            let pid = f
                .iter()
                .skip(skip)
                .find_map(|t| t.rsplit_once(':').and_then(|(_, p)| p.parse::<u32>().ok()))
                .unwrap_or(0);
            let state = if tcp {
                f[5].replace("SYN_RCVD", "SYN_RECEIVED")
            } else {
                "LISTEN".into()
            };
            let state = if state.starts_with("FIN_WAIT") { "FIN_WAIT".into() } else { state };
            let (p, remote) = (f[3], f[4]);
            let rp = if tcp && state != "LISTEN" || !tcp && port_of(remote) != 0 {
                port_of(remote)
            } else {
                0
            };
            rows.push((tcp, port_of(p), rp, state, pid));
        }
        rows
    }

    fn from_kernel(proto: Proto) -> Vec<Key> {
        let t0 = std::time::Instant::now();
        let l = list(proto).unwrap();
        println!("{proto:?}: read and parsed in {:?}", t0.elapsed());
        println!(
            "{proto:?}: parsed {} skipped {} announced {} restricted {}",
            l.sockets.len(),
            l.skipped,
            l.announced,
            l.restricted()
        );
        l.sockets
            .into_iter()
            .map(|s| {
                let st = if proto == Proto::Tcp {
                    s.row.state.label().to_string()
                } else {
                    "LISTEN".into()
                };
                (
                    proto == Proto::Tcp,
                    s.row.local_port,
                    s.row.remote.map_or(0, |r| r.1),
                    st,
                    s.row.pid.unwrap_or(0),
                )
            })
            .collect()
    }

    /// Manual comparison with `netstat -anv` on a real Mac (ignored: it reads the whole machine's sockets, which
    /// ER-14 keeps out of the default run). Run with a test binary whose code-signing identifier contains a dot (ADR
    /// 0026), after capturing netstat as described in `from_netstat`.
    #[test]
    #[ignore = "reads every socket of the machine; run by hand"]
    fn the_list_agrees_with_netstat_anv() {
        for (name, proto) in [("tcp", Proto::Tcp), ("udp", Proto::Udp)] {
            let count = |v: Vec<Key>| {
                let mut m: BTreeMap<Key, i64> = BTreeMap::new();
                for k in v {
                    *m.entry(k).or_default() += 1;
                }
                m
            };
            let (a, b) = (count(from_kernel(proto)), count(from_netstat(name)));
            let mut only_ours = 0;
            let mut only_netstat = 0;
            for (k, n) in &a {
                only_ours += (n - b.get(k).copied().unwrap_or(0)).max(0);
            }
            for (k, n) in &b {
                only_netstat += (n - a.get(k).copied().unwrap_or(0)).max(0);
            }
            let total: i64 = a.values().sum();
            println!("{name}: kernel list {total}, only ours {only_ours}, only netstat {only_netstat}");
            for (k, n) in &a {
                if b.get(k).copied().unwrap_or(0) < *n {
                    println!("  ours only: {k:?}");
                }
            }
            for (k, n) in &b {
                if a.get(k).copied().unwrap_or(0) < *n {
                    println!("  netstat only: {k:?}");
                }
            }
            assert!(only_ours.max(only_netstat) * 20 <= total.max(20), "{name}: more than 5% differ");
        }
    }
}
