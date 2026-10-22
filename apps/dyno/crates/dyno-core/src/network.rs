//! Open ports and connections per process (observe-only): the raw socket rows an adapter reads, the plain-language view
//! built from them, the filters the page asks for and the tracker that notices new listeners and new remote hosts.
//! Pure: the agent passes rows, the process table and the time in as values.

use crate::process::ProcessSample;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

/// Most rows of each kind in one answer; the rest is reported as truncated.
pub const MAX_ROWS: usize = 2000;
/// Remote hosts and listeners the tracker remembers (oldest forgotten beyond this).
pub const MAX_REMEMBERED: usize = 4096;
/// Longest search text, in characters.
pub const MAX_SEARCH: usize = 80;

/// The transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Proto {
    /// TCP.
    Tcp,
    /// UDP.
    Udp,
}

/// A TCP connection state (UDP has none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SocketState {
    /// Waiting for connections.
    Listen,
    /// A connection is being opened.
    SynSent,
    /// A connection request arrived.
    SynReceived,
    /// Connected.
    Established,
    /// Closing (we closed first).
    FinWait,
    /// The other side closed first.
    CloseWait,
    /// Both sides are closing.
    Closing,
    /// Waiting for the last acknowledgement.
    LastAck,
    /// Closed, kept briefly so late packets are not mistaken for a new connection.
    TimeWait,
    /// Closed or not reported.
    Other,
}

impl SocketState {
    /// The name the page shows (the conventional upper-case TCP name).
    pub fn label(self) -> &'static str {
        match self {
            SocketState::Listen => "LISTEN",
            SocketState::SynSent => "SYN_SENT",
            SocketState::SynReceived => "SYN_RECEIVED",
            SocketState::Established => "ESTABLISHED",
            SocketState::FinWait => "FIN_WAIT",
            SocketState::CloseWait => "CLOSE_WAIT",
            SocketState::Closing => "CLOSING",
            SocketState::LastAck => "LAST_ACK",
            SocketState::TimeWait => "TIME_WAIT",
            SocketState::Other => "CLOSED",
        }
    }
}

/// One socket as the OS reports it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SocketRow {
    /// TCP or UDP.
    pub proto: Proto,
    /// Local address.
    pub local: IpAddr,
    /// Local port.
    pub local_port: u16,
    /// The peer, for connected sockets.
    pub remote: Option<(IpAddr, u16)>,
    /// TCP state ([`SocketState::Listen`] for a UDP socket bound without a peer).
    pub state: SocketState,
    /// The owning process, when the OS told us (other users' processes need elevation on macOS and Linux).
    pub pid: Option<u32>,
}

/// How far a listening socket can be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Exposure {
    /// Only this PC (127.0.0.0/8, ::1).
    Loopback,
    /// Every network interface (0.0.0.0, ::): reachable from the network unless a firewall blocks it. "Exposed".
    AllInterfaces,
    /// One specific interface address.
    Interface,
}

/// Classifies a bound address.
pub fn exposure(addr: &IpAddr) -> Exposure {
    if addr.is_loopback() {
        Exposure::Loopback
    } else if addr.is_unspecified() {
        Exposure::AllInterfaces
    } else {
        Exposure::Interface
    }
}

/// Who owns a socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Owner {
    /// A process Dyno could name.
    Process {
        /// Process id.
        pid: u32,
        /// Executable name.
        name: String,
        /// Full path, when readable.
        path: Option<String>,
    },
    /// The OS did not say, or the process is gone; the reason says why and what would help.
    Unknown {
        /// Why.
        reason: String,
    },
}

const UNKNOWN_REASON: &str =
    "The OS did not say which program owns this. It may belong to another user or the system; Dyno never runs elevated, the helper can look it up.";

fn owner_of(pid: Option<u32>, names: &HashMap<u32, (&str, Option<&str>)>) -> Owner {
    match pid {
        Some(pid) => match names.get(&pid) {
            Some((name, path)) => Owner::Process {
                pid,
                name: (*name).to_string(),
                path: path.map(str::to_string),
            },
            None => Owner::Unknown {
                reason: format!("Process {pid} is not in the process list any more."),
            },
        },
        None => Owner::Unknown {
            reason: UNKNOWN_REASON.into(),
        },
    }
}

/// A listening port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenerRow {
    /// TCP or UDP.
    pub proto: Proto,
    /// `ipv4` or `ipv6` as a boolean for the badge.
    pub ipv6: bool,
    /// The bound address text.
    pub local: String,
    /// The port.
    pub port: u16,
    /// How far it can be reached.
    pub exposure: Exposure,
    /// The program.
    pub owner: Owner,
    /// When Dyno first saw it listening (milliseconds), `None` before the first tracker update.
    pub first_seen: Option<Timestamp>,
}

/// A connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRow {
    /// TCP or UDP.
    pub proto: Proto,
    /// Whether the connection is over IPv6.
    pub ipv6: bool,
    /// Local address text.
    pub local: String,
    /// Local port.
    pub local_port: u16,
    /// Remote address text.
    pub remote: String,
    /// Remote port.
    pub remote_port: u16,
    /// Whether the peer is on this PC.
    pub remote_is_loopback: bool,
    /// Whether the peer is a public address that "Look up IP" can look up (false: private, loopback, link-local, CGNAT).
    pub lookup_ok: bool,
    /// TCP state.
    pub state: SocketState,
    /// The state's name.
    pub state_label: String,
    /// The program.
    pub owner: Owner,
    /// When Dyno first saw this connection.
    pub first_seen: Option<Timestamp>,
}

/// What the page asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkQuery {
    /// Show loopback-only listeners and connections to this PC (hidden by default: they are noise).
    pub show_loopback: bool,
    /// Only this protocol.
    pub proto: Option<Proto>,
    /// Only this state (connections).
    pub state: Option<SocketState>,
    /// Only listeners reachable from the network.
    pub exposed_only: bool,
    /// Matches a port, an address or a program name (case-insensitive).
    pub search: String,
}

impl NetworkQuery {
    /// The query with the search text bounded and trimmed (parse, don't validate: done once at the edge).
    pub fn normalised(mut self) -> NetworkQuery {
        self.search = self.search.trim().chars().take(MAX_SEARCH).collect::<String>().to_lowercase();
        self
    }

    fn matches_text(&self, parts: &[&str]) -> bool {
        self.search.is_empty() || parts.iter().any(|p| p.to_lowercase().contains(&self.search))
    }
}

fn owner_text(o: &Owner) -> (String, String) {
    match o {
        Owner::Process { name, path, .. } => (name.clone(), path.clone().unwrap_or_default()),
        Owner::Unknown { .. } => ("owner unknown".into(), String::new()),
    }
}

/// What the page shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    /// When it was read.
    pub at: Timestamp,
    /// Listening ports.
    pub listeners: Vec<ListenerRow>,
    /// Connections.
    pub connections: Vec<ConnectionRow>,
    /// Rows hidden by the per-kind cap ([`MAX_ROWS`]).
    pub truncated: u32,
    /// Sockets whose owner is unknown (shown with a reason; elevation would resolve them).
    pub unknown_owners: u32,
    /// Why the table may be incomplete, when the OS refused part of the read.
    pub note: Option<String>,
}

/// Builds the view: names owners from `processes`, applies `query`, sorts (listeners by port, connections by program) and
/// caps each list. `seen` supplies first-seen times from the tracker.
pub fn build(at: Timestamp, rows: &[SocketRow], processes: &[ProcessSample], query: &NetworkQuery, seen: &Tracker) -> NetworkSnapshot {
    let names: HashMap<u32, (&str, Option<&str>)> = processes.iter().map(|p| (p.pid, (p.name.as_str(), p.exe.as_deref()))).collect();
    let mut listeners = Vec::new();
    let mut connections = Vec::new();
    let mut unknown = 0_u32;
    for r in rows {
        let owner = owner_of(r.pid, &names);
        if matches!(owner, Owner::Unknown { .. }) {
            unknown += 1;
        }
        let (name, path) = owner_text(&owner);
        if query.proto.is_some_and(|p| p != r.proto) {
            continue;
        }
        match (&r.remote, r.state) {
            (None, _) | (_, SocketState::Listen) => {
                let ex = exposure(&r.local);
                if ex == Exposure::Loopback && !query.show_loopback {
                    continue;
                }
                if query.exposed_only && ex != Exposure::AllInterfaces {
                    continue;
                }
                if query.state.is_some_and(|s| s != SocketState::Listen) {
                    continue;
                }
                if !query.matches_text(&[&r.local_port.to_string(), &r.local.to_string(), &name, &path]) {
                    continue;
                }
                listeners.push(ListenerRow {
                    proto: r.proto,
                    ipv6: r.local.is_ipv6(),
                    local: r.local.to_string(),
                    port: r.local_port,
                    exposure: ex,
                    owner,
                    first_seen: seen.listener_seen(r),
                });
            }
            (Some((ip, port)), state) => {
                if ip.is_loopback() && !query.show_loopback {
                    continue;
                }
                if query.exposed_only || query.state.is_some_and(|s| s != state) {
                    continue;
                }
                if !query.matches_text(&[&port.to_string(), &r.local_port.to_string(), &ip.to_string(), &name, &path]) {
                    continue;
                }
                connections.push(ConnectionRow {
                    proto: r.proto,
                    ipv6: ip.is_ipv6(),
                    local: r.local.to_string(),
                    local_port: r.local_port,
                    remote: ip.to_string(),
                    remote_port: *port,
                    remote_is_loopback: ip.is_loopback(),
                    lookup_ok: crate::ip_lookup::is_public(ip),
                    state,
                    state_label: state.label().into(),
                    owner,
                    first_seen: seen.connection_seen(r),
                });
            }
        }
    }
    listeners.sort_by(|a, b| a.port.cmp(&b.port).then_with(|| a.local.cmp(&b.local)));
    connections.sort_by(|a, b| {
        owner_text(&a.owner)
            .0
            .to_lowercase()
            .cmp(&owner_text(&b.owner).0.to_lowercase())
            .then_with(|| a.remote.cmp(&b.remote))
            .then_with(|| a.remote_port.cmp(&b.remote_port))
    });
    let truncated = listeners.len().saturating_sub(MAX_ROWS) + connections.len().saturating_sub(MAX_ROWS);
    listeners.truncate(MAX_ROWS);
    connections.truncate(MAX_ROWS);
    NetworkSnapshot {
        at,
        listeners,
        connections,
        truncated: u32::try_from(truncated).unwrap_or(u32::MAX),
        unknown_owners: unknown,
        note: None,
    }
}

/// Something worth telling the user about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum NetEvent {
    /// A program started listening.
    NewListener {
        /// Its name (or "owner unknown").
        program: String,
        /// The port.
        port: u16,
        /// Protocol.
        proto: Proto,
        /// How far it can be reached.
        exposure: Exposure,
    },
    /// A program connected to a host it had not connected to before.
    NewRemoteHost {
        /// Its name.
        program: String,
        /// The remote address text.
        host: String,
        /// The remote port.
        port: u16,
    },
}

impl NetEvent {
    /// One plain sentence for the activity feed.
    pub fn describe(&self) -> String {
        match self {
            NetEvent::NewListener {
                program,
                port,
                proto,
                exposure,
            } => {
                let reach = match exposure {
                    Exposure::Loopback => "on this PC only",
                    Exposure::AllInterfaces => "on every network interface (reachable from the network)",
                    Exposure::Interface => "on one network interface",
                };
                let p = if *proto == Proto::Tcp { "TCP" } else { "UDP" };
                format!("{program} started listening on {p} port {port}, {reach}.")
            }
            NetEvent::NewRemoteHost { program, host, port } => format!("{program} connected to a new host, {host}:{port}."),
        }
    }

    /// Whether it is worth an entry on the history lane (a new exposed listener is; loopback listeners are not).
    pub fn notable(&self) -> bool {
        match self {
            NetEvent::NewListener { exposure, .. } => *exposure != Exposure::Loopback,
            NetEvent::NewRemoteHost { .. } => true,
        }
    }
}

type ListenerKey = (Proto, IpAddr, u16, Option<u32>);
type ConnKey = (Proto, IpAddr, u16, IpAddr, u16, Option<u32>);

/// Remembers what was seen, so new listeners and new remote hosts are noticed once and rows can show how long they
/// have existed. The first update only learns (nothing is "new" at start-up). Bounded.
#[derive(Debug, Default)]
pub struct Tracker {
    primed: bool,
    listeners: HashMap<ListenerKey, Timestamp>,
    connections: HashMap<ConnKey, Timestamp>,
    hosts: HashSet<(u32, IpAddr)>,
}

impl Tracker {
    /// A tracker that has seen nothing.
    pub fn new() -> Tracker {
        Tracker::default()
    }

    /// When this listener was first seen.
    pub fn listener_seen(&self, r: &SocketRow) -> Option<Timestamp> {
        self.listeners.get(&(r.proto, r.local, r.local_port, r.pid)).copied()
    }

    /// When this connection was first seen.
    pub fn connection_seen(&self, r: &SocketRow) -> Option<Timestamp> {
        let (ip, port) = r.remote?;
        self.connections.get(&(r.proto, r.local, r.local_port, ip, port, r.pid)).copied()
    }

    /// Learns the current rows and returns the events they imply. `names` maps pid to program name.
    pub fn update(&mut self, now: Timestamp, rows: &[SocketRow], processes: &[ProcessSample]) -> Vec<NetEvent> {
        let names: HashMap<u32, &str> = processes.iter().map(|p| (p.pid, p.name.as_str())).collect();
        let program = |pid: Option<u32>| pid.and_then(|p| names.get(&p).copied()).unwrap_or("A program").to_string();
        let mut events = Vec::new();
        let mut live_l: HashMap<ListenerKey, Timestamp> = HashMap::new();
        let mut live_c: HashMap<ConnKey, Timestamp> = HashMap::new();
        for r in rows {
            match (&r.remote, r.state) {
                (None, _) | (_, SocketState::Listen) => {
                    let key = (r.proto, r.local, r.local_port, r.pid);
                    let first = self.listeners.get(&key).copied();
                    live_l.insert(key, first.unwrap_or(now));
                    if first.is_none() && self.primed {
                        events.push(NetEvent::NewListener {
                            program: program(r.pid),
                            port: r.local_port,
                            proto: r.proto,
                            exposure: exposure(&r.local),
                        });
                    }
                }
                (Some((ip, port)), state) => {
                    let key = (r.proto, r.local, r.local_port, *ip, *port, r.pid);
                    let first = self.connections.get(&key).copied();
                    live_c.insert(key, first.unwrap_or(now));
                    if let Some(pid) = r.pid {
                        let fresh = self.hosts.insert((pid, *ip));
                        if fresh && self.primed && !ip.is_loopback() && state == SocketState::Established {
                            events.push(NetEvent::NewRemoteHost {
                                program: program(r.pid),
                                host: ip.to_string(),
                                port: *port,
                            });
                        }
                    }
                }
            }
        }
        self.listeners = live_l;
        self.connections = live_c;
        if self.hosts.len() > MAX_REMEMBERED {
            // Forget the hosts of programs that are no longer connected; the set only has to be bounded.
            let live: HashSet<u32> = rows.iter().filter_map(|r| r.pid).collect();
            self.hosts.retain(|(pid, _)| live.contains(pid));
            if self.hosts.len() > MAX_REMEMBERED {
                self.hosts.clear();
            }
        }
        self.primed = true;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::RunState;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn proc(pid: u32, name: &str) -> ProcessSample {
        ProcessSample {
            pid,
            parent: None,
            name: name.into(),
            exe: Some(format!("/apps/{name}")),
            user: None,
            cpu: 0.0,
            memory: 0,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            started: Timestamp(1),
            state: RunState::Running,
        }
    }

    fn listen(ip: IpAddr, port: u16, pid: Option<u32>) -> SocketRow {
        SocketRow {
            proto: Proto::Tcp,
            local: ip,
            local_port: port,
            remote: None,
            state: SocketState::Listen,
            pid,
        }
    }

    fn conn(remote: IpAddr, port: u16, pid: Option<u32>, state: SocketState) -> SocketRow {
        SocketRow {
            proto: Proto::Tcp,
            local: v4(192, 168, 1, 5),
            local_port: 50_000,
            remote: Some((remote, port)),
            state,
            pid,
        }
    }

    fn rows() -> Vec<SocketRow> {
        vec![
            listen(v4(127, 0, 0, 1), 5432, Some(1)),
            listen(v4(0, 0, 0, 0), 8080, Some(2)),
            listen(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 8080, Some(2)),
            listen(v4(192, 168, 1, 5), 22, None),
            conn(v4(93, 184, 216, 34), 443, Some(2), SocketState::Established),
            conn(v4(127, 0, 0, 1), 5432, Some(1), SocketState::Established),
            conn(v4(93, 184, 216, 34), 443, Some(999), SocketState::TimeWait),
        ]
    }

    fn procs() -> Vec<ProcessSample> {
        vec![proc(1, "postgres"), proc(2, "Server")]
    }

    #[test]
    fn exposure_follows_the_bound_address() {
        assert_eq!(exposure(&v4(127, 0, 0, 1)), Exposure::Loopback);
        assert_eq!(exposure(&IpAddr::V6(Ipv6Addr::LOCALHOST)), Exposure::Loopback);
        assert_eq!(exposure(&v4(0, 0, 0, 0)), Exposure::AllInterfaces);
        assert_eq!(exposure(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)), Exposure::AllInterfaces);
        assert_eq!(exposure(&v4(10, 0, 0, 2)), Exposure::Interface);
    }

    #[test]
    fn loopback_is_hidden_by_default_and_owners_are_named_or_explained() {
        let snap = build(Timestamp(5), &rows(), &procs(), &NetworkQuery::default(), &Tracker::new());
        assert_eq!(snap.listeners.iter().map(|l| l.port).collect::<Vec<_>>(), vec![22, 8080, 8080]);
        assert_eq!(snap.connections.len(), 2, "the loopback connection is hidden");
        assert!(snap.connections.iter().all(|c| c.lookup_ok), "both remaining peers are public");
        assert!(matches!(&snap.listeners[0].owner, Owner::Unknown { reason } if reason.contains("another user")));
        assert!(matches!(&snap.listeners[1].owner, Owner::Process { name, .. } if name == "Server"));
        assert!(matches!(&snap.connections[0].owner, Owner::Unknown { reason } if reason.contains("999")));
        assert_eq!(snap.unknown_owners, 2, "the unnamed listener and the gone pid are counted");
    }

    #[test]
    fn filters_apply_together() {
        let all = NetworkQuery {
            show_loopback: true,
            ..Default::default()
        };
        let s = build(Timestamp(0), &rows(), &procs(), &all, &Tracker::new());
        assert_eq!((s.listeners.len(), s.connections.len()), (4, 3));
        let exposed = NetworkQuery {
            exposed_only: true,
            ..Default::default()
        };
        let s = build(Timestamp(0), &rows(), &procs(), &exposed, &Tracker::new());
        assert_eq!((s.listeners.len(), s.connections.len()), (2, 0));
        assert!(s.listeners.iter().all(|l| l.exposure == Exposure::AllInterfaces));
        let est = NetworkQuery {
            state: Some(SocketState::Established),
            ..Default::default()
        };
        let s = build(Timestamp(0), &rows(), &procs(), &est, &Tracker::new());
        assert_eq!((s.listeners.len(), s.connections.len()), (0, 1));
        let udp = NetworkQuery {
            proto: Some(Proto::Udp),
            ..Default::default()
        };
        assert!(build(Timestamp(0), &rows(), &procs(), &udp, &Tracker::new()).listeners.is_empty());
    }

    #[test]
    fn search_matches_ports_addresses_and_names_and_is_bounded() {
        let q = |s: &str| {
            NetworkQuery {
                search: s.into(),
                show_loopback: true,
                ..Default::default()
            }
            .normalised()
        };
        let s = build(Timestamp(0), &rows(), &procs(), &q("  POSTGRES "), &Tracker::new());
        assert_eq!((s.listeners.len(), s.connections.len()), (1, 1));
        assert_eq!(
            build(Timestamp(0), &rows(), &procs(), &q("93.184"), &Tracker::new())
                .connections
                .len(),
            2
        );
        assert_eq!(
            build(Timestamp(0), &rows(), &procs(), &q("8080"), &Tracker::new()).listeners.len(),
            2
        );
        assert_eq!(q(&"x".repeat(500)).search.chars().count(), MAX_SEARCH);
    }

    #[test]
    fn rows_are_capped_and_the_rest_counted() {
        let many: Vec<SocketRow> = (0..(MAX_ROWS as u16 + 10)).map(|p| listen(v4(0, 0, 0, 0), p, Some(2))).collect();
        let s = build(Timestamp(0), &many, &procs(), &NetworkQuery::default(), &Tracker::new());
        assert_eq!(s.listeners.len(), MAX_ROWS);
        assert_eq!(s.truncated, 10);
    }

    #[test]
    fn the_tracker_learns_quietly_then_reports_new_listeners_and_hosts_once() {
        let mut t = Tracker::new();
        let base = rows();
        assert!(t.update(Timestamp(1), &base, &procs()).is_empty(), "start-up is not news");
        assert!(t.update(Timestamp(2), &base, &procs()).is_empty());
        let mut next = base.clone();
        next.push(listen(v4(0, 0, 0, 0), 9000, Some(2)));
        next.push(listen(v4(127, 0, 0, 1), 9001, Some(1)));
        next.push(conn(v4(1, 1, 1, 1), 443, Some(2), SocketState::Established));
        next.push(conn(v4(1, 1, 1, 1), 80, Some(2), SocketState::Established));
        let ev = t.update(Timestamp(3), &next, &procs());
        assert_eq!(ev.len(), 3, "{ev:?}");
        assert!(ev.iter().any(|e| matches!(
            e,
            NetEvent::NewListener {
                port: 9000,
                exposure: Exposure::AllInterfaces,
                ..
            }
        )));
        assert!(ev
            .iter()
            .any(|e| matches!(e, NetEvent::NewRemoteHost { host, .. } if host == "1.1.1.1")));
        assert!(t.update(Timestamp(4), &next, &procs()).is_empty(), "each is reported once");
        let seen = build(Timestamp(5), &next, &procs(), &NetworkQuery::default(), &t);
        let l = seen.listeners.iter().find(|l| l.port == 9000).unwrap();
        assert_eq!(l.first_seen, Some(Timestamp(3)));
        let old = seen.listeners.iter().find(|l| l.port == 22).unwrap();
        assert_eq!(old.first_seen, Some(Timestamp(1)));
    }

    #[test]
    fn event_wording_and_notability() {
        let exposed = NetEvent::NewListener {
            program: "Server".into(),
            port: 80,
            proto: Proto::Tcp,
            exposure: Exposure::AllInterfaces,
        };
        assert!(exposed.describe().contains("reachable from the network") && exposed.notable());
        let local = NetEvent::NewListener {
            program: "db".into(),
            port: 5432,
            proto: Proto::Udp,
            exposure: Exposure::Loopback,
        };
        assert!(local.describe().contains("this PC only") && local.describe().contains("UDP") && !local.notable());
        let iface = NetEvent::NewListener {
            program: "x".into(),
            port: 1,
            proto: Proto::Tcp,
            exposure: Exposure::Interface,
        };
        assert!(iface.describe().contains("one network interface"));
        assert!(NetEvent::NewRemoteHost {
            program: "p".into(),
            host: "h".into(),
            port: 1
        }
        .describe()
        .contains("new host"));
    }

    #[test]
    fn the_tracker_stays_bounded() {
        let mut t = Tracker::new();
        t.update(Timestamp(0), &[], &[]);
        let rows: Vec<SocketRow> = (0..(MAX_REMEMBERED as u32 + 50))
            .map(|i| conn(IpAddr::V4(Ipv4Addr::from(0x0100_0000 + i)), 443, Some(2), SocketState::Established))
            .collect();
        t.update(Timestamp(1), &rows, &procs());
        assert!(t.hosts.len() <= MAX_REMEMBERED);
    }

    #[test]
    fn state_labels_are_the_conventional_names() {
        assert_eq!(SocketState::Established.label(), "ESTABLISHED");
        assert_eq!(SocketState::TimeWait.label(), "TIME_WAIT");
        assert_eq!(SocketState::Other.label(), "CLOSED");
        for s in [
            SocketState::Listen,
            SocketState::SynSent,
            SocketState::SynReceived,
            SocketState::FinWait,
            SocketState::CloseWait,
            SocketState::Closing,
            SocketState::LastAck,
        ] {
            assert!(!s.label().is_empty());
        }
    }
}
