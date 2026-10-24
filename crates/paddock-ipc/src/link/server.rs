//! The companion's side of the link: a private endpoint with a fresh token, the discovery file that names them, and an
//! accept thread that authenticates every client and serves it with one handler. Stopping (or dropping) the server
//! closes every connection and removes the discovery file if it still names this process.

use super::discovery::Discovery;
use crate::endpoint::Access;
use crate::error::IpcError;
use crate::frame::{FrameLimit, ProtocolVersion};
use crate::handshake::Host;
use crate::peer::{Handler, Peer, PeerOptions};
use crate::token::Token;
use paddock_jobs::time::{Clock, SystemClock};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// How often the accept thread looks at the stop flag.
const ACCEPT_SLICE: Duration = Duration::from_millis(100);

/// How a companion serves its link.
#[derive(Debug, Clone)]
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
    /// How each client's connection runs (its label names the clients).
    pub peer: PeerOptions,
}

impl ServerConfig {
    /// A server writing `discovery` for `build`, unversioned protocol, default limit.
    pub fn new(discovery: impl Into<PathBuf>, build: impl Into<String>) -> ServerConfig {
        ServerConfig {
            discovery: discovery.into(),
            socket_dir: None,
            fixed: None,
            build: build.into(),
            protocol: ProtocolVersion::UNVERSIONED,
            limit: FrameLimit::default(),
            peer: PeerOptions::new("the app"),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Shared {
    stop: AtomicBool,
    peers: Mutex<Vec<Peer>>,
    refused: Mutex<Vec<IpcError>>,
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
        Self::start_with_clock(config, handler, &SystemClock)
    }

    /// [`LinkServer::start`] with the clock that stamps the discovery file.
    pub fn start_with_clock(
        config: ServerConfig,
        handler: Arc<dyn Handler>,
        clock: &dyn Clock,
    ) -> Result<LinkServer, IpcError> {
        let mut host = match (&config.fixed, &config.socket_dir) {
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
        let discovery = Discovery {
            endpoint: host.endpoint().to_string(),
            token: host.token().to_hex(),
            pid: std::process::id(),
            build: config.build.clone(),
            protocol: config.protocol,
            started_ms: clock.wall_ms(),
        };
        discovery
            .write(&config.discovery)
            .map_err(|e| IpcError::io(e.kind(), format!("cannot write the discovery file: {e}")))?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            refused: Mutex::new(Vec::new()),
        });
        let endpoint = host.endpoint().to_string();
        let s = shared.clone();
        let opts = config.peer.clone();
        let accept = std::thread::Builder::new()
            .name("paddock-ipc-link-accept".into())
            .spawn(move || {
                while !s.stop.load(Ordering::SeqCst) {
                    match host.accept(ACCEPT_SLICE) {
                        Ok(conn) => match Peer::start(conn, handler.clone(), opts.clone()) {
                            Ok(peer) => {
                                let mut peers = lock(&s.peers);
                                peers.retain(|p| !p.is_closed());
                                peers.push(peer);
                            }
                            Err(e) => lock(&s.refused).push(e),
                        },
                        Err(IpcError::Timeout) => {}
                        // A client that failed the handshake is refused; the server keeps listening.
                        Err(e) => {
                            let mut refused = lock(&s.refused);
                            if refused.len() < 64 {
                                refused.push(e);
                            }
                        }
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

    /// The open connections (to push to every client).
    pub fn peers(&self) -> Vec<Peer> {
        let mut peers = lock(&self.shared.peers);
        peers.retain(|p| !p.is_closed());
        peers.clone()
    }

    /// Clients that were refused (wrong token, another protocol), the most recent 64.
    pub fn refused(&self) -> Vec<IpcError> {
        lock(&self.shared.refused).clone()
    }

    /// Stops accepting, closes every connection and removes the discovery file if it still names this process.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.accept.take() {
            let _ = t.join();
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
