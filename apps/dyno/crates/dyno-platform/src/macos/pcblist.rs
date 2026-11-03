//! The kernel's socket lists through `sysctlbyname("net.inet.{tcp,udp}.pcblist_n")` (Dyno ADR 0026): one call per protocol,
//! no per-process and no per-descriptor access, so nothing is held on another program's sockets. Parsing is in
//! [`crate::parse::mac_pcblist`].

use crate::parse::mac_pcblist::{self, PcbError, PcbList, PcbSocket, MAX_BYTES};
use dyno_core::network::{Proto, SocketRow};
use dyno_core::ports::PlatformError;
use dyno_core::process::Connection;
use std::ffi::CString;
use std::sync::Mutex;

/// The buffer both lists are read into, kept between samples. The kernel copies the list out while it holds the
/// protocol's pcb lock exclusively, so the pages must already be mapped: a fault in the copy would stall every
/// socket operation of the machine. `resize` writes the zeros, which touches them.
static BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Fills `buf` with one raw list and returns its length. The list can grow between the size query and the read: a short
/// buffer (`ENOMEM`) is retried a few times with the size asked again.
#[allow(unsafe_code)]
fn raw(name: &str, buf: &mut Vec<u8>) -> Result<usize, String> {
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
        if buf.len() < cap {
            buf.clear();
            buf.resize(cap, 0);
        }
        let mut len: libc::size_t = buf.len();
        // SAFETY: `buf` is `len` writable bytes; the kernel writes at most `len` and sets `len` to what it wrote.
        let rc = unsafe { libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
        if rc == 0 {
            return Ok(len.min(buf.len()));
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ENOMEM) {
            return Err(e.to_string());
        }
        // Ask again with more room than the list had.
        let grow = buf.len().saturating_add(buf.len() / 2);
        buf.clear();
        buf.resize(grow.min(MAX_BYTES), 0);
    }
    Err("the list kept growing while it was read".into())
}

fn name_of(proto: Proto) -> &'static str {
    match proto {
        Proto::Tcp => "net.inet.tcp.pcblist_n",
        Proto::Udp => "net.inet.udp.pcblist_n",
    }
}

fn read(proto: Proto, buf: &mut Vec<u8>) -> Result<PcbList, PcbError> {
    let n = raw(name_of(proto), buf).map_err(|e| PcbError::Malformed(format!("The OS would not give the socket list ({e}).")))?;
    mac_pcblist::parse(&buf[..n], proto)
}

/// What a failed parse of one protocol's list means for the page.
fn failure(e: PcbError) -> PlatformError {
    match e {
        PcbError::UnknownLayout(_) => {
            PlatformError::Unsupported("This version of macOS changed the format of its socket list, so Dyno cannot read it here.".into())
        }
        PcbError::Malformed(m) => PlatformError::Failed(m),
    }
}

/// The two protocols' parsed lists as one (counts added). Each is judged on its own first: a protocol whose layout
/// drifted fails the whole read, so its missing rows are never taken for a machine that has none.
fn merge(tcp: Result<PcbList, PcbError>, udp: Result<PcbList, PcbError>) -> Result<PcbList, PlatformError> {
    let mut all = PcbList::default();
    for l in [tcp.map_err(failure)?, udp.map_err(failure)?] {
        all.sockets.extend(l.sockets);
        all.skipped += l.skipped;
        all.unreadable += l.unreadable;
        all.announced += l.announced;
    }
    Ok(all)
}

/// Both lists, merged into one.
fn merged() -> Result<PcbList, PlatformError> {
    let mut buf = BUF.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let tcp = read(Proto::Tcp, &mut buf);
    let udp = read(Proto::Udp, &mut buf);
    merge(tcp, udp)
}

/// The merged lists, refused when the kernel cut the view down to this process's own sockets (Dyno ADR 0026). Unit tests
/// run as linker-signed binaries whose identifier has no dot, so they always get that cut-down view: there the check
/// is off and the tests look at their own sockets, which are always in it; `a_cut_down_view_is_refused_with_the_fix`
/// covers the check itself.
fn both() -> Result<PcbList, PlatformError> {
    let all = merged()?;
    if !cfg!(test) {
        refuse_cut_down(&all, std::process::id())?;
    }
    Ok(all)
}

fn refuse_cut_down(all: &PcbList, own_pid: u32) -> Result<(), PlatformError> {
    if all.restricted(own_pid) {
        return Err(PlatformError::AccessDenied(cut_down_message(signing_identifier().as_deref())));
    }
    Ok(())
}

/// What the page says about a cut-down list. It names the identifier this program really has: with a dotted one the dot is not
/// what is missing (macOS also cut the view down for a dotted agent that the app started itself as its child, Dyno ADR 0026),
/// so the text must not blame the identifier then.
pub(crate) fn cut_down_message(identifier: Option<&str>) -> String {
    match identifier {
        Some(id) if id.contains('.') => format!(
            "macOS showed this program only its own sockets, although its code-signing identifier ({id}) has a dot. This happens when the app started the agent itself instead of the login service: turn on the Dyno login service in Settings and restart Dyno."
        ),
        Some(id) => format!(
            "macOS showed this program only its own sockets. The agent needs a code-signing identifier with a dot (net.decryptology.dyno.agent); this build has {id}."
        ),
        None => "macOS showed this program only its own sockets, and its code-signing identifier could not be read.".into(),
    }
}

/// This process's code-signing identifier (`csops(CS_OPS_IDENTITY)`), or `None` when it is unsigned or cannot be read.
#[allow(unsafe_code)]
pub fn signing_identifier() -> Option<String> {
    const CS_OPS_IDENTITY: libc::c_uint = 11;
    extern "C" {
        fn csops(pid: libc::pid_t, ops: libc::c_uint, useraddr: *mut libc::c_void, usersize: libc::size_t) -> libc::c_int;
    }
    let mut buf = [0u8; 1024];
    // SAFETY: `buf` is 1024 writable bytes and its size is passed; the kernel writes at most that many.
    let rc = unsafe { csops(libc::getpid(), CS_OPS_IDENTITY, buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    // A blob: magic u32, length u32 (big endian), then the NUL-terminated identifier.
    let text = buf.get(8..)?;
    let end = text.iter().position(|b| *b == 0)?;
    String::from_utf8(text[..end].to_vec()).ok().filter(|s| !s.is_empty())
}

/// What this process is shown of the kernel's socket lists, for the start-up log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketView {
    /// The code-signing identifier, when readable.
    pub identifier: Option<String>,
    /// Sockets the lists announced.
    pub announced: usize,
    /// Sockets actually sent.
    pub sent: usize,
    /// Whether the view is the cut-down one (Dyno ADR 0026).
    pub cut_down: bool,
}

/// Reads both lists once and says what this process is shown (never refuses: the caller logs it).
pub fn socket_view() -> Result<SocketView, PlatformError> {
    let all = merged()?;
    Ok(SocketView {
        identifier: signing_identifier(),
        announced: all.announced,
        sent: all.sockets.len(),
        cut_down: all.restricted(std::process::id()),
    })
}

/// Every TCP and UDP socket of the machine with its owner.
pub fn sockets() -> Result<Vec<SocketRow>, PlatformError> {
    Ok(both()?.sockets.into_iter().map(|s: PcbSocket| s.row).collect())
}

/// The connections of one process (sockets it owns or works through), for the details of a process.
pub fn connections_of(pid: u32) -> Result<Vec<Connection>, PlatformError> {
    Ok(both()?
        .sockets
        .into_iter()
        .filter(|s| s.row.pid == Some(pid) || s.via_pid == Some(pid))
        .map(|s| connection(&s.row))
        .collect())
}

fn endpoint(addr: std::net::IpAddr, port: u16) -> String {
    let a = if addr.is_unspecified() {
        "*".to_string()
    } else if addr.is_ipv6() {
        format!("[{addr}]")
    } else {
        addr.to_string()
    };
    if port == 0 {
        format!("{a}:*")
    } else {
        format!("{a}:{port}")
    }
}

fn connection(r: &SocketRow) -> Connection {
    let six = if r.local.is_ipv6() { "6" } else { "" };
    Connection {
        protocol: format!("{}{six}", if r.proto == Proto::Tcp { "TCP" } else { "UDP" }),
        local: endpoint(r.local, r.local_port),
        remote: r.remote.map(|(a, p)| endpoint(a, p)).unwrap_or_default(),
        state: if r.proto == Proto::Tcp {
            r.state.label().to_string()
        } else {
            String::new()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::network::SocketState;
    use std::collections::BTreeMap;
    use std::net::{TcpListener, TcpStream, UdpSocket};

    #[test]
    fn a_processs_connections_are_its_sockets_in_the_shape_the_details_page_shows() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mine = connections_of(std::process::id()).unwrap();
        let c = mine
            .iter()
            .find(|c| c.local == format!("127.0.0.1:{port}"))
            .expect("the listener is among the connections");
        assert_eq!((c.protocol.as_str(), c.remote.as_str(), c.state.as_str()), ("TCP", "", "LISTEN"));
        assert!(connections_of(0x7fff_fff0).unwrap().is_empty());
    }

    #[test]
    fn the_cut_down_message_does_not_blame_a_dot_that_is_there() {
        // The released 1.0.0 agent had the dotted identifier and still got the cut-down view, yet the page said it lacked a dot.
        let m = cut_down_message(Some("net.decryptology.dyno.agent"));
        assert!(!m.contains("needs a code-signing identifier"), "{m}");
        assert!(m.contains("net.decryptology.dyno.agent") && m.contains("login service"), "{m}");
        assert!(cut_down_message(Some("dyno-agent-55554944")).contains("this build has dyno-agent-55554944"));
        assert!(cut_down_message(None).contains("could not be read"));
    }

    #[test]
    fn a_test_binary_reads_its_own_signing_identifier() {
        let id = signing_identifier().expect("a cargo test binary is linker-signed, so it has an identifier");
        assert!(!id.is_empty());
    }

    #[test]
    fn endpoints_read_like_lsofs_wildcards_ipv6_in_brackets() {
        assert_eq!(endpoint("0.0.0.0".parse().unwrap(), 80), "*:80");
        assert_eq!(endpoint("::1".parse().unwrap(), 80), "[::1]:80");
        assert_eq!(endpoint("192.0.2.1".parse().unwrap(), 0), "192.0.2.1:*");
    }

    #[test]
    fn one_protocol_whose_layout_drifted_fails_the_whole_read_not_just_its_rows() {
        let ok = PcbList {
            announced: 3,
            ..PcbList::default()
        };
        let drifted = || Err(PcbError::UnknownLayout("x".into()));
        for (tcp, udp) in [(drifted(), Ok(ok.clone())), (Ok(ok.clone()), drifted())] {
            assert!(matches!(merge(tcp, udp), Err(PlatformError::Unsupported(_))));
        }
        let both = merge(Ok(ok.clone()), Ok(ok)).unwrap();
        assert_eq!(both.announced, 6);
    }

    #[test]
    fn a_cut_down_view_is_refused_with_the_fix() {
        let cut = PcbList {
            announced: 50,
            ..PcbList::default()
        };
        let e = refuse_cut_down(&cut, 1).unwrap_err();
        assert!(
            matches!(e, PlatformError::AccessDenied(ref m) if m.contains("net.decryptology.dyno.agent")),
            "{e:?}"
        );
        assert!(refuse_cut_down(&PcbList::default(), 1).is_ok());
    }

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

        let tcp = read(Proto::Tcp, &mut Vec::new()).unwrap();
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
        let u = find(&read(Proto::Udp, &mut Vec::new()).unwrap(), uport, None).expect("the UDP socket is listed");
        assert_eq!((u.row.pid, u.row.remote), (Some(me), None));
    }

    /// One netstat row reduced to what both sources know.
    type Key = (bool, u16, u16, String, u32);

    fn port_of(addr_port: &str) -> u16 {
        addr_port.rsplit('.').next().and_then(|p| p.parse().ok()).unwrap_or(0)
    }

    fn from_netstat(proto: &str) -> Vec<Key> {
        // `netstat` started from a test process printed nothing on macOS 27.2 (Dyno ADR 0026), so the caller captures it from a
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
        let l = read(proto, &mut Vec::new()).unwrap();
        println!("{proto:?}: read and parsed in {:?}", t0.elapsed());
        println!(
            "{proto:?}: parsed {} skipped {} announced {} restricted {}",
            l.sockets.len(),
            l.skipped,
            l.announced,
            l.restricted(std::process::id())
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
                    // netstat prints `so_last_pid`; for a delegated socket that is `via_pid`, our owner is `so_e_pid`.
                    s.via_pid.or(s.row.pid).unwrap_or(0),
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
