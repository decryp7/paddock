//! The companion's side of the link: a private (or fixed service) endpoint with a fresh token, the discovery file that
//! names them, and an accept thread that hands each client to a handshake thread of its own (a silent client delays
//! nobody; each handshake is bounded on the clock), caps the connections (Pitcrew's per-connection cap), and serves every
//! client with one handler. [`Subscriptions`] carries Pitcrew's pushed events to the connections that asked for a
//! topic. Stopping (or dropping) the server closes every connection and removes the discovery file if it still names
//! this process.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use super::discovery::Discovery;
use crate::endpoint::{Access, Stream};
use crate::error::{IpcError, RemoteError, codes};
use crate::frame::{FrameLimit, ProtocolVersion};
use crate::handshake::{Authenticator, HANDSHAKE_TIMEOUT, Host};
use crate::peer::{Handler, Peer, PeerOptions};
use crate::token::Token;
use paddock_jobs::time::{Clock, SystemClock};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// How long one OS accept waits before the accept thread looks at its stop flag (not a limit of anything).
const ACCEPT_SLICE: Duration = Duration::from_millis(100);
/// The pause after an accept that failed (no descriptor left, for example) before the next one, so a failing accept does not spin.
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(50);
/// Refused clients remembered for [`LinkServer::refused`].
const MAX_REFUSED: usize = 64;

/// How a companion serves its link.
#[derive(Clone)]
pub struct ServerConfig {
    /// The discovery file (e.g. `AppDirs::runtime().join("agent.json")`).
    pub discovery: PathBuf,
    /// Where the private socket folder goes (`AppDirs::runtime()`); the temporary folder when `None` or too long.
    pub socket_dir: Option<PathBuf>,
    /// A fixed endpoint name and its access list instead of a fresh private one (a Windows service serving the user's
    /// app: `Access::service_for_interactive_users()`). The token is still fresh per start.
    pub fixed: Option<(String, Access)>,
    /// This build (written to the discovery file; a client of another build replaces this companion).
    pub build: String,
    /// The protocol served.
    pub protocol: ProtocolVersion,
    /// The frame limit.
    pub limit: FrameLimit,
    /// Open connections plus handshakes at most; one more is refused with `busy` (Pitcrew's cap).
    pub max_connections: usize,
    /// Handshakes in progress at most; a client beyond them is refused with `busy` at once.
    pub max_handshakes: usize,
    /// How long one client has for its handshake, on `clock`.
    pub handshake_timeout: Duration,
    /// The clock of the handshakes and the discovery file's start time.
    pub clock: Arc<dyn Clock>,
    /// How each client's connection runs (its label names the clients).
    pub peer: PeerOptions,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("discovery", &self.discovery)
            .field("build", &self.build)
            .field("protocol", &self.protocol)
            .field("max_connections", &self.max_connections)
            .finish_non_exhaustive()
    }
}

impl ServerConfig {
    /// A server writing `discovery` for `build`: unversioned protocol, default limit, 64 connections, 16 handshakes of
    /// at most [`HANDSHAKE_TIMEOUT`], the system clock.
    pub fn new(discovery: impl Into<PathBuf>, build: impl Into<String>) -> ServerConfig {
        ServerConfig {
            discovery: discovery.into(),
            socket_dir: None,
            fixed: None,
            build: build.into(),
            protocol: ProtocolVersion::UNVERSIONED,
            limit: FrameLimit::default(),
            max_connections: 64,
            max_handshakes: 16,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            clock: Arc::new(SystemClock),
            peer: PeerOptions::new("the app"),
        }
    }
}

/// Runs `work` with `stream` on a thread from `spawn` and returns whether it started; when no thread can be started, the
/// failure goes to `record` and then the client is told `busy` with the cause instead of seeing its connection end (CI run 7763; a stopgap of ADR 0072 for I7).
fn hand_over(
    auth: &Authenticator,
    stream: Stream,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
    work: impl FnOnce(Stream) + Send + 'static,
    record: impl FnOnce(IpcError),
) -> bool {
    // The stream waits in a slot the thread empties; a thread that never started leaves it there to be answered.
    let slot = Arc::new(Mutex::new(Some(stream)));
    let theirs = slot.clone();
    let started = spawn(Box::new(move || {
        let taken = lock(&theirs).take();
        if let Some(stream) = taken {
            work(stream);
        }
    }));
    let Err(e) = started else { return true };
    let why = format!("cannot start a handshake thread: {e}");
    // Recorded before the client is answered, so the server's record never trails what the client was told.
    record(IpcError::Spawn(why.clone()));
    let left = lock(&slot).take();
    if let Some(stream) = left {
        auth.refuse_busy(
            stream,
            &format!("the host cannot take a connection now ({why}); try again later"),
        );
    }
    false
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Shared {
    stop: AtomicBool,
    peers: Mutex<Vec<Peer>>,
    handshakes: AtomicUsize,
    refused: Mutex<Vec<IpcError>>,
}

impl Shared {
    fn refuse(&self, e: IpcError) {
        let mut refused = lock(&self.refused);
        if refused.len() == MAX_REFUSED {
            refused.remove(0);
        }
        refused.push(e);
    }

    fn open_peers(&self) -> usize {
        let mut peers = lock(&self.peers);
        peers.retain(|p| !p.is_closed());
        peers.len()
    }
}

/// A running link server. Dropping it stops it.
pub struct LinkServer {
    shared: Arc<Shared>,
    discovery: PathBuf,
    endpoint: String,
    accept: Option<JoinHandle<()>>,
}

impl LinkServer {
    /// Listens, writes the discovery file and starts accepting clients, each served by `handler`.
    pub fn start(config: ServerConfig, handler: Arc<dyn Handler>) -> Result<LinkServer, IpcError> {
        let host = match (&config.fixed, &config.socket_dir) {
            (Some((name, access)), _) => Host::listen_at(
                name,
                access,
                Token::random()?,
                config.limit,
                config.protocol,
            )?,
            (None, Some(dir)) => Host::listen_in(dir, config.limit, config.protocol)?,
            (None, None) => Host::listen(config.limit, config.protocol)?,
        };
        let mut host = host.with_clock(config.clock.clone(), config.handshake_timeout);
        let discovery = Discovery {
            endpoint: host.endpoint().to_string(),
            token: host.token().to_hex(),
            pid: std::process::id(),
            build: config.build.clone(),
            protocol: config.protocol,
            started_ms: config.clock.wall_ms(),
        };
        discovery
            .write(&config.discovery)
            .map_err(|e| IpcError::io(e.kind(), format!("cannot write the discovery file: {e}")))?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            handshakes: AtomicUsize::new(0),
            refused: Mutex::new(Vec::new()),
        });
        let endpoint = host.endpoint().to_string();
        let s = shared.clone();
        let auth = host.authenticator();
        let (opts, max_connections, max_handshakes) = (
            config.peer.clone(),
            config.max_connections,
            config.max_handshakes,
        );
        let accept = std::thread::Builder::new()
            .name("paddock-ipc-link-accept".into())
            .spawn(move || {
                while !s.stop.load(Ordering::SeqCst) {
                    // A timeout is the stop-flag slice; any other error is recorded and the server keeps listening, after
                    // telling a client it could not take why (no descriptor left) and a short pause, so an accept that fails
                    // at once does not spin.
                    let (stream, os_pid) = match host.accept_raw(ACCEPT_SLICE) {
                        Ok(accepted) => accepted,
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                        Err(e) => {
                            // Recorded before the client is answered, so `refused()` never trails what a client was told.
                            s.refuse(IpcError::from(std::io::Error::new(e.kind(), e.to_string())));
                            host.turn_away(&e);
                            std::thread::sleep(ACCEPT_ERROR_PAUSE);
                            continue;
                        }
                    };
                    let in_progress = s.handshakes.load(Ordering::SeqCst);
                    if in_progress >= max_handshakes
                        || s.open_peers() + in_progress >= max_connections
                    {
                        s.refuse(IpcError::Remote(RemoteError::new(
                            codes::BUSY,
                            "too many connections",
                        )));
                        auth.refuse_busy(stream, "too many connections; try again");
                        continue;
                    }
                    s.handshakes.fetch_add(1, Ordering::SeqCst);
                    let (s2, auth, handler, opts) =
                        (s.clone(), auth.clone(), handler.clone(), opts.clone());
                    let spawned = hand_over(
                        &auth.clone(),
                        stream,
                        |work| {
                            std::thread::Builder::new()
                                .name("paddock-ipc-link-handshake".into())
                                .spawn(work)
                                .map(drop)
                        },
                        move |stream| {
                            match auth
                                .authenticate(stream, os_pid, None)
                                .and_then(|c| Peer::start(c, handler, opts))
                            {
                                Ok(peer) if s2.stop.load(Ordering::SeqCst) => peer.close(),
                                Ok(peer) => lock(&s2.peers).push(peer),
                                Err(e) => s2.refuse(e),
                            }
                            s2.handshakes.fetch_sub(1, Ordering::SeqCst);
                        },
                        |e| s.refuse(e),
                    );
                    if !spawned {
                        s.handshakes.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            })
            .map_err(|e| IpcError::Spawn(format!("cannot start the link's accept thread: {e}")))?;
        Ok(LinkServer {
            shared,
            discovery: config.discovery,
            endpoint,
            accept: Some(accept),
        })
    }

    /// The endpoint clients connect to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The open connections.
    pub fn peers(&self) -> Vec<Peer> {
        let mut peers = lock(&self.shared.peers);
        peers.retain(|p| !p.is_closed());
        peers.clone()
    }

    /// Clients that were refused (wrong token, another protocol, too many, a handshake that ran out), the most recent 64.
    pub fn refused(&self) -> Vec<IpcError> {
        lock(&self.shared.refused).clone()
    }

    /// Stops accepting, closes every connection and removes the discovery file if it still names this process.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.accept.take() {
            paddock_proc::cleanup::join("the link server's accept thread", t);
        }
        for p in lock(&self.shared.peers).drain(..) {
            p.close();
        }
        Discovery::remove_if_owned_by(&self.discovery, std::process::id());
    }
}

impl Drop for LinkServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Which connections asked for which topics (Pitcrew's subscribed connections): the app's handler subscribes the
/// calling peer (`ctx.peer()`), and [`Subscriptions::publish`] pushes an event to every open subscriber. Closed
/// connections are forgotten. Cheap to clone; clones share the table.
#[derive(Clone, Default)]
pub struct Subscriptions(Arc<Mutex<BTreeMap<String, BTreeMap<u64, Peer>>>>);

impl Subscriptions {
    /// An empty table.
    pub fn new() -> Subscriptions {
        Subscriptions::default()
    }

    /// `peer` receives the events of `topic` until it unsubscribes or closes.
    pub fn subscribe(&self, peer: &Peer, topic: &str) {
        lock(&self.0)
            .entry(topic.to_string())
            .or_default()
            .insert(peer.id(), peer.clone());
    }

    /// `peer` no longer receives `topic`.
    pub fn unsubscribe(&self, peer: &Peer, topic: &str) {
        if let Some(t) = lock(&self.0).get_mut(topic) {
            t.remove(&peer.id());
        }
    }

    /// The topics `peer` is subscribed to.
    pub fn topics_of(&self, peer: &Peer) -> BTreeSet<String> {
        lock(&self.0)
            .iter()
            .filter(|(_, peers)| peers.contains_key(&peer.id()))
            .map(|(t, _)| t.clone())
            .collect()
    }

    /// Pushes `body` to every open subscriber of `topic`; returns how many received it. A push that fails (the connection
    /// closed, the event over the frame limit) skips that subscriber.
    pub fn publish(&self, topic: &str, body: &[u8]) -> usize {
        let peers: Vec<Peer> = {
            let mut all = lock(&self.0);
            let Some(t) = all.get_mut(topic) else {
                return 0;
            };
            t.retain(|_, p| !p.is_closed());
            t.values().cloned().collect()
        };
        peers
            .iter()
            .filter(|p| p.push(body.to_vec()).is_ok())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::Listener;
    use crate::frame::{self, Frame};
    use std::time::Duration;

    /// A handshake thread that cannot start leaves the client told `busy` with the cause, not a connection that just ends.
    #[test]
    fn a_client_whose_handshake_thread_cannot_start_is_told_busy_with_the_cause() {
        let host = Host::listen(FrameLimit::default(), ProtocolVersion::UNVERSIONED).unwrap();
        let mut listener = Listener::bind().unwrap();
        let mut client = Stream::connect(listener.name(), Duration::from_secs(10)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (stream, _) = listener.accept(Duration::from_secs(10)).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let r = ran.clone();
        let recorded = Arc::new(Mutex::new(None));
        let rec = recorded.clone();
        let started = hand_over(
            &host.authenticator(),
            stream,
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "no thread left",
                ))
            },
            move |_| r.store(true, Ordering::SeqCst),
            move |e| *lock(&rec) = Some(e),
        );
        assert!(!started);
        let e = lock(&recorded).take();
        assert!(
            matches!(&e, Some(IpcError::Spawn(m)) if m.contains("no thread left")),
            "{e:?}"
        );
        assert!(!ran.load(Ordering::SeqCst));
        let told = frame::read_frame(&mut client, FrameLimit::default());
        assert!(
            matches!(&told, Ok(Frame::Fail { id: 0, error }) if error.is(codes::BUSY) && error.message.contains("no thread left")),
            "{told:?}"
        );
    }

    /// A thread that starts gets the stream.
    #[test]
    fn a_started_handshake_thread_gets_the_stream() {
        let host = Host::listen(FrameLimit::default(), ProtocolVersion::UNVERSIONED).unwrap();
        let mut listener = Listener::bind().unwrap();
        let _client = Stream::connect(listener.name(), Duration::from_secs(10)).unwrap();
        let (stream, _) = listener.accept(Duration::from_secs(10)).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let r = ran.clone();
        let started = hand_over(
            &host.authenticator(),
            stream,
            |work| {
                work();
                Ok(())
            },
            move |_| r.store(true, Ordering::SeqCst),
            |e| panic!("nothing to record: {e:?}"),
        );
        assert!(started);
        assert!(ran.load(Ordering::SeqCst));
    }
}
