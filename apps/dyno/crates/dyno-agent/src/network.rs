//! Open ports and connections: reads the OS socket table (cached for a moment, so a polling page costs nothing extra),
//! names the owners from the process table and keeps a tracker that turns new listeners and new remote hosts into
//! activity entries. Observe-only: nothing here closes a connection or touches a firewall.

use crate::agent::{lock, now, Shared};
use dyno_core::activity::{ActivityKind, UndoRecord};
use dyno_core::network::{build, NetEvent, NetworkQuery, NetworkSnapshot, SocketRow, Tracker};
use dyno_core::ports::NetworkSockets;
use dyno_core::protocol::{AgentError, ErrorCode};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A read newer than this is reused by the next request.
const CACHE: Duration = Duration::from_millis(1500);
/// How often the background loop looks while nobody asks, so new listeners are noticed anyway.
pub const BACKGROUND_EVERY: Duration = Duration::from_secs(30);
/// Most events written to the activity log per look (a port scan from somewhere must not flood it).
const MAX_EVENTS_PER_LOOK: usize = 5;

struct State {
    tracker: Tracker,
    cache: Option<(Instant, Vec<SocketRow>)>,
}

/// The network view.
pub struct NetCentre {
    sockets: Box<dyn NetworkSockets>,
    state: Mutex<State>,
}

impl NetCentre {
    /// A centre reading through `sockets`.
    pub fn new(sockets: Box<dyn NetworkSockets>) -> NetCentre {
        NetCentre {
            sockets,
            state: Mutex::new(State {
                tracker: Tracker::new(),
                cache: None,
            }),
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

    /// The page's snapshot for `query`.
    pub fn snapshot(&self, shared: &Shared, query: NetworkQuery) -> Result<NetworkSnapshot, AgentError> {
        let processes = lock(&shared.core).latest.processes.clone();
        let (rows, events) = self.look(&processes)?;
        record(shared, &events);
        let s = lock(&self.state);
        Ok(build(now(), &rows, &processes, &query.normalised(), &s.tracker))
    }

    /// One background look (for the slow loop).
    pub fn tick(&self, shared: &Shared) {
        let processes = lock(&shared.core).latest.processes.clone();
        if let Ok((_, events)) = self.look(&processes) {
            record(shared, &events);
        }
    }
}

/// Writes the notable events to the activity log, a few at a time.
fn record(shared: &Shared, events: &[NetEvent]) {
    let t = now();
    let notable: Vec<&NetEvent> = events.iter().filter(|e| e.notable()).collect();
    if notable.is_empty() {
        return;
    }
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

    #[test]
    fn a_read_is_cached_briefly_and_a_refusal_is_an_error() {
        let src = Arc::new(StdMutex::new(Ok(vec![listener(80)])));
        let c = NetCentre::new(Box::new(Fake(src.clone())));
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
