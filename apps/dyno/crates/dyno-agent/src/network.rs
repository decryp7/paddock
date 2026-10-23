//! Open ports and connections: reads the OS socket table (cached for a moment, so a polling page costs nothing extra),
//! names the owners from the process table and keeps a tracker that turns new listeners and new remote hosts into
//! activity entries. Observe-only: nothing here closes a connection or touches a firewall.

use crate::agent::{lock, now, Shared};
use dyno_core::activity::{ActivityKind, UndoRecord};
use dyno_core::network::{build, NetEvent, NetworkQuery, NetworkSnapshot, SocketRow, Tracker};
use dyno_core::network_names::{clean_name, parse_addresses, HostName};
use dyno_core::network_traffic::{TrafficRow, TrafficTracker};
use dyno_core::ports::{NetworkSockets, PlatformError, ProcessTraffic, ReverseDns};
use dyno_core::protocol::{AgentError, ErrorCode};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A read newer than this is reused by the next request.
const CACHE: Duration = Duration::from_millis(1500);
/// How often the background loop looks while nobody asks, so new listeners are noticed anyway.
pub const BACKGROUND_EVERY: Duration = Duration::from_secs(30);
/// Most events written to the activity log per look (a port scan from somewhere must not flood it).
const MAX_EVENTS_PER_LOOK: usize = 5;
/// How old the traffic rates may get before a background refresh (`nettop` itself takes about five seconds).
const TRAFFIC_EVERY: Duration = Duration::from_secs(8);
/// How long a name stays cached, and how long "no name" does (a failed lookup is retried sooner).
const NAME_TTL: Duration = Duration::from_secs(10 * 60);
const NO_NAME_TTL: Duration = Duration::from_secs(2 * 60);
const FAILED_TTL: Duration = Duration::from_secs(30);
/// Most names remembered.
const MAX_NAMES: usize = 2048;
/// Lookups in flight at once, and how long one request waits for them (slow ones finish into the cache for next time).
const RESOLVE_WORKERS: usize = 8;
const RESOLVE_DEADLINE: Duration = Duration::from_secs(3);

struct State {
    tracker: Tracker,
    cache: Option<(Instant, Vec<SocketRow>)>,
}

struct Traffic {
    tracker: TrafficTracker,
    /// The last read: when, rows, and why there are none.
    cache: Option<(Instant, Vec<TrafficRow>, Option<String>)>,
    refreshing: bool,
}

type Names = Arc<Mutex<HashMap<IpAddr, (Instant, Option<String>, Duration)>>>;

/// The network view.
pub struct NetCentre {
    sockets: Box<dyn NetworkSockets>,
    dns: Arc<dyn ReverseDns>,
    traffic_source: Arc<dyn ProcessTraffic>,
    state: Mutex<State>,
    traffic: Arc<Mutex<Traffic>>,
    names: Names,
}

impl NetCentre {
    /// A centre reading through `sockets`, resolving names through `dns` and reading traffic through `traffic`.
    pub fn new(sockets: Box<dyn NetworkSockets>, dns: Arc<dyn ReverseDns>, traffic: Box<dyn ProcessTraffic>) -> NetCentre {
        NetCentre {
            sockets,
            dns,
            traffic_source: Arc::from(traffic),
            state: Mutex::new(State {
                tracker: Tracker::new(),
                cache: None,
            }),
            traffic: Arc::new(Mutex::new(Traffic {
                tracker: TrafficTracker::new(),
                cache: None,
                refreshing: false,
            })),
            names: Arc::default(),
        }
    }

    /// Reads the rows (from the cache when fresh), learns them and returns them with the events they imply.
    fn look(&self, processes: &[dyno_core::process::ProcessSample]) -> Result<(Vec<SocketRow>, Vec<NetEvent>), AgentError> {
        let mut s = lock(&self.state);
        if let Some((at, rows)) = &s.cache {
            if at.elapsed() < CACHE {
                return Ok((rows.clone(), Vec::new()));
            }
        }
        let rows = self
            .sockets
            .sockets()
            .map_err(|e| AgentError::new(ErrorCode::Internal, e.to_string()))?;
        let events = s.tracker.update(now(), &rows, processes);
        s.cache = Some((Instant::now(), rows.clone()));
        Ok((rows, events))
    }

    /// Established connections per process id, for the program baseline. A refused read is "none": the baseline then simply
    /// has no connection counts this time.
    pub fn connection_counts(&self) -> HashMap<u32, u32> {
        let rows = match self.sockets.sockets() {
            Ok(r) => r,
            Err(_) => return HashMap::new(),
        };
        let mut out: HashMap<u32, u32> = HashMap::new();
        for r in rows
            .iter()
            .filter(|r| r.remote.is_some() && r.state == dyno_core::network::SocketState::Established)
        {
            if let Some(pid) = r.pid {
                *out.entry(pid).or_default() += 1;
            }
        }
        out
    }

    /// Bytes per second per program, or why there are none. The OS tool can take seconds (macOS `nettop`), so this
    /// never waits for it: it returns the last read and starts a refresh in the background when that is stale.
    fn traffic(&self) -> (Vec<TrafficRow>, Option<String>) {
        let mut t = lock(&self.traffic);
        let stale = t.cache.as_ref().is_none_or(|(at, ..)| at.elapsed() >= TRAFFIC_EVERY);
        if stale && !t.refreshing {
            t.refreshing = true;
            let (source, state) = (self.traffic_source.clone(), self.traffic.clone());
            std::thread::spawn(move || refresh_traffic(source.as_ref(), &state));
        }
        match &t.cache {
            Some((_, rows, note)) => (rows.clone(), note.clone()),
            None => (Vec::new(), Some("Reading traffic per program…".into())),
        }
    }

    /// The page's snapshot for `query`.
    pub fn snapshot(&self, shared: &Shared, query: NetworkQuery) -> Result<NetworkSnapshot, AgentError> {
        let processes = lock(&shared.core).latest.processes.clone();
        let (rows, events) = self.look(&processes)?;
        record(shared, &events);
        let mut snapshot = {
            let s = lock(&self.state);
            build(now(), &rows, &processes, &query.normalised(), &s.tracker)
        };
        (snapshot.traffic, snapshot.traffic_note) = self.traffic();
        Ok(snapshot)
    }

    /// One background look (for the slow loop).
    pub fn tick(&self, shared: &Shared) {
        let processes = lock(&shared.core).latest.processes.clone();
        if let Ok((_, events)) = self.look(&processes) {
            record(shared, &events);
        }
    }

    /// Reverse DNS names for `addresses`, from the cache or the OS resolver. Waits at most a few seconds: addresses
    /// that did not answer in time come back without a name and finish into the cache for the next request.
    pub fn resolve(&self, addresses: &[String]) -> Result<Vec<HostName>, AgentError> {
        let ips = parse_addresses(addresses).map_err(|m| AgentError::new(ErrorCode::BadRequest, m))?;
        let mut found: HashMap<IpAddr, Option<String>> = HashMap::new();
        let mut pending: VecDeque<IpAddr> = VecDeque::new();
        {
            let cache = lock(&self.names);
            for ip in &ips {
                match cache.get(ip) {
                    Some((at, name, ttl)) if at.elapsed() < *ttl => {
                        found.insert(*ip, name.clone());
                    }
                    _ => pending.push_back(*ip),
                }
            }
        }
        if !pending.is_empty() {
            let total = pending.len();
            let queue = Arc::new(Mutex::new(pending));
            let (tx, rx) = mpsc::channel::<(IpAddr, Option<String>)>();
            for _ in 0..RESOLVE_WORKERS.min(total) {
                let (queue, tx, dns, names) = (queue.clone(), tx.clone(), self.dns.clone(), self.names.clone());
                std::thread::spawn(move || loop {
                    let Some(ip) = lock(&queue).pop_front() else { break };
                    let (name, ttl) = match dns.lookup(ip) {
                        Ok(Some(n)) => (clean_name(&n), NAME_TTL),
                        Ok(None) => (None, NO_NAME_TTL),
                        Err(_) => (None, FAILED_TTL),
                    };
                    {
                        let mut cache = lock(&names);
                        if cache.len() >= MAX_NAMES {
                            cache.clear();
                        }
                        cache.insert(ip, (Instant::now(), name.clone(), ttl));
                    }
                    let _ = tx.send((ip, name));
                });
            }
            drop(tx);
            let deadline = Instant::now() + RESOLVE_DEADLINE;
            while found.len() < ips.len() {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok((ip, name)) => {
                        found.insert(ip, name);
                    }
                    Err(_) => break,
                }
            }
        }
        Ok(addresses
            .iter()
            .map(|a| HostName {
                address: a.clone(),
                name: a.trim().parse::<IpAddr>().ok().and_then(|ip| found.get(&ip).cloned().flatten()),
            })
            .collect())
    }
}

/// Reads the counters once and stores the rates (or the reason there are none).
fn refresh_traffic(source: &dyn ProcessTraffic, state: &Mutex<Traffic>) {
    let read = source.counters();
    let mut t = lock(state);
    let (rows, note) = match read {
        Ok(counters) => (t.tracker.update(now(), &counters), None),
        Err(PlatformError::Unsupported(why)) => (Vec::new(), Some(why)),
        Err(e) => (Vec::new(), Some(format!("Traffic per program could not be read ({e})."))),
    };
    t.cache = Some((Instant::now(), rows, note));
    t.refreshing = false;
}

/// Writes the notable events to the activity log, a few at a time, and raises an alarm for the ones the user asked to
/// be told about.
fn record(shared: &Shared, events: &[NetEvent]) {
    let t = now();
    let notable: Vec<&NetEvent> = events.iter().filter(|e| e.notable()).collect();
    if notable.is_empty() {
        return;
    }
    let mut changes = Vec::new();
    {
        let mut core = lock(&shared.core);
        for e in notable.iter().take(MAX_EVENTS_PER_LOOK) {
            let title = match e {
                NetEvent::NewListener { .. } => "New listener",
                NetEvent::NewRemoteHost { .. } => "New remote host",
            };
            core.log(t, ActivityKind::System, title.into(), e.describe(), UndoRecord::None, None);
        }
        if notable.len() > MAX_EVENTS_PER_LOOK {
            core.log(
                t,
                ActivityKind::System,
                "More network changes".into(),
                format!(
                    "{} more new listeners or hosts appeared at the same time.",
                    notable.len() - MAX_EVENTS_PER_LOOK
                ),
                UndoRecord::None,
                None,
            );
        }
        let rules = core.settings.network_alerts;
        let core = &mut *core;
        for e in notable.iter().take(MAX_EVENTS_PER_LOOK) {
            if let Some(o) = dyno_core::network_alerts::observation(e, rules) {
                changes.extend(core.alarms.apply(&core.store, o.into_event(t)));
            }
        }
    }
    crate::sampler::push_alarm_changes(shared, changes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::network::{Proto, SocketState};
    use dyno_core::ports::PlatformError;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::{Arc, Mutex as StdMutex};

    struct Fake(Arc<StdMutex<Result<Vec<SocketRow>, String>>>);

    impl NetworkSockets for Fake {
        fn sockets(&self) -> Result<Vec<SocketRow>, PlatformError> {
            self.0.lock().unwrap().clone().map_err(PlatformError::Failed)
        }
    }

    fn listener(port: u16) -> SocketRow {
        SocketRow {
            proto: Proto::Tcp,
            local: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            local_port: port,
            remote: None,
            state: SocketState::Listen,
            pid: Some(1),
        }
    }

    struct Dns(Arc<StdMutex<Vec<IpAddr>>>);

    impl ReverseDns for Dns {
        fn lookup(&self, ip: IpAddr) -> Result<Option<String>, PlatformError> {
            self.0.lock().unwrap().push(ip);
            match ip.to_string().as_str() {
                "10.0.0.1" => Ok(Some("router.lan.".into())),
                "10.0.0.2" => Ok(None),
                "10.0.0.3" => Err(PlatformError::Failed("resolver down".into())),
                "10.0.0.4" => Ok(Some("bad name".into())),
                _ => Ok(None),
            }
        }
    }

    struct NoTraffic(Result<Vec<dyno_core::network_traffic::TrafficCounter>, PlatformError>);

    impl ProcessTraffic for NoTraffic {
        fn counters(&self) -> Result<Vec<dyno_core::network_traffic::TrafficCounter>, PlatformError> {
            match &self.0 {
                Ok(v) => Ok(v.clone()),
                Err(PlatformError::Unsupported(m)) => Err(PlatformError::Unsupported(m.clone())),
                Err(e) => Err(PlatformError::Failed(e.to_string())),
            }
        }
    }

    fn centre(
        calls: Arc<StdMutex<Vec<IpAddr>>>,
        traffic: Result<Vec<dyno_core::network_traffic::TrafficCounter>, PlatformError>,
    ) -> NetCentre {
        NetCentre::new(
            Box::new(Fake(Arc::new(StdMutex::new(Ok(vec![]))))),
            Arc::new(Dns(calls)),
            Box::new(NoTraffic(traffic)),
        )
    }

    #[test]
    fn names_are_resolved_cleaned_cached_and_failures_do_not_poison_the_rest() {
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let c = centre(calls.clone(), Ok(vec![]));
        let asked: Vec<String> = ["10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.1"]
            .map(String::from)
            .to_vec();
        let out = c.resolve(&asked).unwrap();
        let names: Vec<Option<&str>> = out.iter().map(|h| h.name.as_deref()).collect();
        assert_eq!(names, vec![Some("router.lan"), None, None, None, Some("router.lan")]);
        assert_eq!(calls.lock().unwrap().len(), 4, "a repeated address is looked up once");
        c.resolve(&asked).unwrap();
        assert_eq!(calls.lock().unwrap().len(), 4, "the second request is answered from the cache");
        assert_eq!(c.resolve(&[]).unwrap(), vec![]);
    }

    #[test]
    fn a_bad_address_or_too_many_are_refused() {
        let c = centre(Arc::default(), Ok(vec![]));
        assert_eq!(c.resolve(&["nope".into()]).unwrap_err().code, ErrorCode::BadRequest);
        let many: Vec<String> = (0..65).map(|i| format!("10.1.0.{i}")).collect();
        assert_eq!(c.resolve(&many).unwrap_err().code, ErrorCode::BadRequest);
    }

    fn settled(c: &NetCentre) -> (Vec<TrafficRow>, Option<String>) {
        let first = c.traffic();
        assert_eq!(
            first.1.as_deref(),
            Some("Reading traffic per program…"),
            "the first call never waits for the OS"
        );
        for _ in 0..200 {
            if !lock(&c.traffic).refreshing {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        c.traffic()
    }

    #[test]
    fn traffic_is_read_in_the_background_and_is_a_note_when_the_os_cannot_say() {
        let c = centre(Arc::default(), Err(PlatformError::Unsupported("only on macOS".into())));
        let (rows, note) = settled(&c);
        assert!(rows.is_empty());
        assert_eq!(note.as_deref(), Some("only on macOS"));
        let c = centre(Arc::default(), Err(PlatformError::Failed("boom".into())));
        assert!(settled(&c).1.unwrap().contains("boom"));
        let c = centre(Arc::default(), Ok(vec![]));
        assert_eq!(settled(&c).1, None);
    }

    #[test]
    fn a_read_is_cached_briefly_and_a_refusal_is_an_error() {
        let src = Arc::new(StdMutex::new(Ok(vec![listener(80)])));
        let c = NetCentre::new(
            Box::new(Fake(src.clone())),
            Arc::new(Dns(Arc::default())),
            Box::new(NoTraffic(Ok(vec![]))),
        );
        let (rows, ev) = c.look(&[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(ev.is_empty(), "the first look only learns");
        *src.lock().unwrap() = Ok(vec![listener(80), listener(81)]);
        assert_eq!(c.look(&[]).unwrap().0.len(), 1, "served from the cache");
        lock(&c.state).cache = None;
        let (rows, ev) = c.look(&[]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(ev.len(), 1, "port 81 is new");
        *src.lock().unwrap() = Err("denied".into());
        lock(&c.state).cache = None;
        let e = c.look(&[]).unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal);
        assert!(e.to_string().contains("denied"));
    }
}
