//! The app's side of the link (Dyno's `link.rs` and Pitcrew's `service_client.rs` as one): a background thread that
//! reads the discovery file, connects to a live companion of this build, replaces one of another build, ignores a stale
//! file, starts a companion when none runs (with a [`Launcher`]), and reconnects with back-off when the link drops. Every
//! wait is on the injected [`Clock`], so tests drive the back-off with a `FakeClock`.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use super::discovery::{self, Discovery, Found};
use crate::error::IpcError;
use crate::frame::{FrameLimit, ProtocolVersion};
use crate::handshake::Client;
use crate::peer::{Handler, NoCalls, Peer, PeerOptions};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Backoff, BackoffPolicy, Clock, Retry, SystemClock};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// How often the client looks for the discovery file while it waits for a companion it started (or one it is replacing).
const POLL: Duration = Duration::from_millis(50);

/// Starts the companion when none runs.
pub trait Launcher: Send + Sync {
    /// Starts it, detached or supervised; returns at once. The client then waits for its discovery file.
    fn launch(&self) -> Result<(), String>;
}

/// How a companion of another build is made to exit.
#[derive(Clone)]
pub enum Replace {
    /// Ask it to stop (SIGTERM, a close request on Windows), then end it after a grace period (`paddock_proc::terminate`).
    Terminate {
        /// How long it has to stop by itself.
        grace: Duration,
    },
    /// The app asks it through its own protocol (e.g. Dyno's `Shutdown` request); returns whether it was asked. The client
    /// then waits for its process to end, and ends it after `grace` if it did not.
    Ask {
        /// The app's request.
        ask: Arc<dyn Fn(&Discovery) -> bool + Send + Sync>,
        /// How long it has to stop after being asked.
        grace: Duration,
    },
    /// Never replace: report it and retry (a companion shared by several apps).
    Never,
}

impl std::fmt::Debug for Replace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Replace::Terminate { grace } => {
                f.debug_struct("Terminate").field("grace", grace).finish()
            }
            Replace::Ask { grace, .. } => f
                .debug_struct("Ask")
                .field("grace", grace)
                .finish_non_exhaustive(),
            Replace::Never => f.write_str("Never"),
        }
    }
}

/// How the client connects.
#[derive(Clone)]
pub struct ClientConfig {
    /// The discovery file the companion writes.
    pub discovery: PathBuf,
    /// This app's build; a companion of another build (or protocol) is replaced.
    pub build: String,
    /// The protocol this app speaks.
    pub protocol: ProtocolVersion,
    /// The frame limit.
    pub limit: FrameLimit,
    /// Reconnect back-off (default: 250 ms doubling to 15 s, reset after a minute connected, never gives up).
    pub backoff: BackoffPolicy,
    /// How long a started (or replaced) companion has to write its discovery file.
    pub start_wait: Duration,
    /// How a companion of another build is made to exit.
    pub replace: Replace,
    /// How the connection runs (label, pushes).
    pub peer: PeerOptions,
    /// The clock every wait runs on.
    pub clock: Arc<dyn Clock>,
    /// Whether the process a discovery file names still runs ([`discovery::is_running`]: the pid's holder must have started
    /// before the file was written, ADR 0033; tests inject one).
    pub is_alive: Arc<dyn Fn(&Discovery) -> bool + Send + Sync>,
}

impl ClientConfig {
    /// A client of `build` for the companion that writes `discovery`, on the system clock.
    pub fn new(
        discovery: impl Into<PathBuf>,
        build: impl Into<String>,
        label: impl Into<String>,
    ) -> ClientConfig {
        ClientConfig {
            discovery: discovery.into(),
            build: build.into(),
            protocol: ProtocolVersion::UNVERSIONED,
            limit: FrameLimit::default(),
            backoff: BackoffPolicy::retry_forever(
                Duration::from_millis(250),
                Duration::from_secs(15),
                Duration::from_secs(60),
            ),
            start_wait: Duration::from_secs(10),
            replace: Replace::Terminate {
                grace: Duration::from_secs(5),
            },
            peer: PeerOptions::new(label),
            clock: Arc::new(SystemClock),
            is_alive: Arc::new(discovery::is_running),
        }
    }
}

/// The link's state, for a status chip or a health entry.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkState {
    /// Looking for (or starting) the companion.
    Connecting,
    /// Connected to the companion `pid` of `build`.
    Connected {
        /// Its build.
        build: String,
        /// Its process id.
        pid: u32,
    },
    /// The last attempt failed (or the link dropped); the next one is after `after`.
    Retrying {
        /// Failures in a row.
        attempt: u32,
        /// The wait before the next attempt.
        after: Duration,
        /// Why, in plain words.
        reason: String,
    },
    /// The back-off policy gave up; [`LinkClient::retry`] starts over.
    GaveUp {
        /// Why the last attempt failed.
        reason: String,
    },
    /// The client was stopped.
    Stopped,
}

/// A decision about the discovery file (pure, see [`plan`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Nothing runs: start a companion (or wait for one).
    Start,
    /// A live companion of this build and protocol: connect.
    Connect(Discovery),
    /// A live companion of another build or protocol: make it exit, then start ours.
    Replace(Discovery),
    /// The file's process is gone: remove it, then start.
    RemoveStale(Discovery),
    /// The file is not owner-only: do not use it.
    Refuse(String),
}

/// What to do about what the discovery file holds.
pub fn plan(found: Found, build: &str, protocol: ProtocolVersion) -> Plan {
    match found {
        Found::Nothing => Plan::Start,
        Found::Stale(d) => Plan::RemoveStale(d),
        Found::Untrusted(why) => Plan::Refuse(why),
        Found::Live(d) if d.build == build && d.protocol == protocol => Plan::Connect(d),
        Found::Live(d) => Plan::Replace(d),
    }
}

/// Hears every state change of a [`LinkClient`].
pub type StateFn = Arc<dyn Fn(&LinkState) + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Shared {
    state: Mutex<(LinkState, Option<Peer>)>,
    changed: Condvar,
    stop: CancelToken,
    retry: Mutex<CancelToken>,
    on_state: Option<StateFn>,
}

impl Shared {
    fn set(&self, state: LinkState, peer: Option<Peer>) {
        {
            let mut g = lock(&self.state);
            *g = (state.clone(), peer);
        }
        self.changed.notify_all();
        if let Some(f) = &self.on_state {
            f(&state);
        }
    }
}

/// A running link client. Dropping it stops it (the companion keeps running).
pub struct LinkClient {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl LinkClient {
    /// Starts the client thread. `handler` answers the companion's calls; `launcher` starts a companion when none runs
    /// (without one the client waits for one to appear); `on_state` hears every state change.
    pub fn start(
        config: ClientConfig,
        handler: Option<Arc<dyn Handler>>,
        launcher: Option<Arc<dyn Launcher>>,
        on_state: Option<StateFn>,
    ) -> Result<LinkClient, IpcError> {
        let shared = Arc::new(Shared {
            state: Mutex::new((LinkState::Connecting, None)),
            changed: Condvar::new(),
            stop: CancelToken::new(),
            retry: Mutex::new(CancelToken::new()),
            on_state,
        });
        let s = shared.clone();
        let handler = handler.unwrap_or_else(|| Arc::new(NoCalls));
        let thread = std::thread::Builder::new()
            .name("paddock-ipc-link-client".into())
            .spawn(move || {
                Runner {
                    config,
                    handler,
                    launcher,
                    shared: s,
                }
                .run()
            })
            .map_err(|e| IpcError::Spawn(format!("cannot start the link client: {e}")))?;
        Ok(LinkClient {
            shared,
            thread: Some(thread),
        })
    }

    /// The current state.
    pub fn state(&self) -> LinkState {
        lock(&self.shared.state).0.clone()
    }

    /// The connection, while connected.
    pub fn peer(&self) -> Option<Peer> {
        lock(&self.shared.state).1.clone()
    }

    /// Waits until the state satisfies `f` (at most `guard` of real time, a hang guard); returns the state then.
    pub fn wait_for(&self, guard: Duration, f: impl Fn(&LinkState) -> bool) -> Option<LinkState> {
        let g = lock(&self.shared.state);
        let (g, _) = self
            .shared
            .changed
            .wait_timeout_while(g, guard, |(s, _)| !f(s))
            .unwrap_or_else(|p| p.into_inner());
        f(&g.0).then(|| g.0.clone())
    }

    /// Waits until connected (at most `guard`, a hang guard) and returns the connection.
    pub fn wait_connected(&self, guard: Duration) -> Option<Peer> {
        self.wait_for(guard, |s| matches!(s, LinkState::Connected { .. }))?;
        self.peer()
    }

    /// Ends the current wait at once (the user pressed Retry), also after the back-off gave up.
    pub fn retry(&self) {
        lock(&self.shared.retry).cancel();
    }

    /// Stops the client and closes the connection; the companion keeps running.
    pub fn stop(&mut self) {
        self.shared.stop.cancel();
        self.retry();
        if let Some(p) = self.peer() {
            p.close();
        }
        if let Some(t) = self.thread.take() {
            paddock_proc::cleanup::join_warn("the link client thread", t);
        }
        self.shared.set(LinkState::Stopped, None);
    }
}

impl Drop for LinkClient {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Runner {
    config: ClientConfig,
    handler: Arc<dyn Handler>,
    launcher: Option<Arc<dyn Launcher>>,
    shared: Arc<Shared>,
}

impl Runner {
    fn stopping(&self) -> bool {
        self.shared.stop.is_cancelled()
    }

    /// Waits `d` on the clock; false when the client is stopping. A Retry ends the wait early.
    fn pause(&self, d: Duration) -> bool {
        let retry = lock(&self.shared.retry).clone();
        // silent: Err(Cancelled) is the wake-up (stop or Retry); the state is read right after
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = self.config.clock.wait(d, &retry);
        if retry.is_cancelled() {
            *lock(&self.shared.retry) = CancelToken::new();
        }
        !self.stopping()
    }

    fn run(self) {
        let clock = self.config.clock.clone();
        let mut backoff = Backoff::new(self.config.backoff);
        let mut attempt = 0u32;
        while !self.stopping() {
            self.shared.set(LinkState::Connecting, None);
            let reason = match self.establish() {
                Ok((peer, d)) => {
                    attempt = 0;
                    backoff.healthy(clock.monotonic());
                    self.shared.set(
                        LinkState::Connected {
                            build: d.build.clone(),
                            pid: d.pid,
                        },
                        Some(peer.clone()),
                    );
                    // `stop` sets its flag before it closes the published peer, so one of the two sees the other.
                    if !self.stopping() {
                        peer.wait_closed(None);
                    }
                    if self.stopping() {
                        peer.close();
                        break;
                    }
                    format!("the connection to {} dropped", self.config.peer.label)
                }
                Err(reason) => reason,
            };
            if self.stopping() {
                break;
            }
            attempt = attempt.saturating_add(1);
            match backoff.failed(clock.monotonic()) {
                Retry::After(after) => {
                    self.shared.set(
                        LinkState::Retrying {
                            attempt,
                            after,
                            reason,
                        },
                        None,
                    );
                    if !self.pause(after) {
                        break;
                    }
                }
                Retry::GiveUp => {
                    self.shared.set(LinkState::GaveUp { reason }, None);
                    if !self.pause(Duration::MAX) {
                        break;
                    }
                    backoff.reset();
                    attempt = 0;
                }
            }
        }
    }

    fn connect(&self, d: &Discovery) -> Result<Peer, String> {
        let token = d.token().ok_or("the discovery file holds no valid token")?;
        let conn = Client::connect_on(
            &d.endpoint,
            &token,
            self.config.limit,
            self.config.protocol,
            &self.config.clock,
            crate::handshake::HANDSHAKE_TIMEOUT,
        )
        .map_err(|e| format!("{} did not answer ({e})", self.config.peer.label))?;
        Peer::start(conn, self.handler.clone(), self.config.peer.clone()).map_err(|e| e.to_string())
    }

    fn found(&self) -> Found {
        let alive = self.config.is_alive.clone();
        discovery::read(&self.config.discovery, move |d| alive(d))
    }

    /// Connects, starting or replacing the companion when needed (three rounds at most).
    fn establish(&self) -> Result<(Peer, Discovery), String> {
        let label = &self.config.peer.label;
        let mut last = format!("{label} is not running");
        for _ in 0..3 {
            match plan(self.found(), &self.config.build, self.config.protocol) {
                Plan::Connect(d) => return self.connect(&d).map(|p| (p, d)),
                Plan::RemoveStale(d) => {
                    Discovery::remove_if_owned_by(&self.config.discovery, d.pid);
                    last = format!("{label} (pid {}) is not running any more", d.pid);
                    self.start()?;
                }
                Plan::Replace(d) => {
                    last = format!("{label} of build {} was running", d.build);
                    self.replace(&d)?;
                    self.start()?;
                }
                Plan::Refuse(why) => return Err(why),
                Plan::Start => self.start()?,
            }
        }
        Err(last)
    }

    /// Starts a companion when there is a launcher and waits for its discovery file; without one, there is nothing to do.
    fn start(&self) -> Result<(), String> {
        let label = &self.config.peer.label;
        let Some(launcher) = &self.launcher else {
            return Err(format!("{label} is not running"));
        };
        // One may have appeared meanwhile (another app of this build started it): never start a second.
        if !matches!(self.found(), Found::Live(_)) {
            launcher.launch()?;
        }
        let waited = self.poll_until(self.config.start_wait, || {
            matches!(self.found(), Found::Live(_))
        });
        if waited {
            Ok(())
        } else {
            Err(format!("{label} did not start in time"))
        }
    }

    /// Makes a companion of another build exit and waits for its process to end.
    fn replace(&self, d: &Discovery) -> Result<(), String> {
        let label = &self.config.peer.label;
        if d.pid == std::process::id() {
            return Err(format!(
                "{label} of build {} runs in this process; it cannot be replaced",
                d.build
            ));
        }
        let alive = self.config.is_alive.clone();
        let gone = |d: &Discovery| !alive(d);
        // The pid comes from the file and may name another process by now: only the process that started before the file
        // was written is ended (ADR 0033).
        let companion = paddock_proc::ProcessId::of_recorded(d.pid, d.started_ms);
        match &self.config.replace {
            Replace::Never => {
                return Err(format!(
                    "{label} of build {} is running and is not replaced",
                    d.build
                ));
            }
            Replace::Terminate { grace } => {
                if let Some(id) = companion {
                    paddock_proc::terminate(id, *grace);
                }
            }
            Replace::Ask { ask, grace } => {
                if !(ask(d) && self.poll_until(*grace, || gone(d)))
                    && let Some(id) = companion
                {
                    paddock_proc::terminate(id, Duration::ZERO);
                }
            }
        }
        if !self.poll_until(self.config.start_wait, || gone(d)) {
            return Err(format!("{label} of build {} did not exit", d.build));
        }
        Discovery::remove_if_owned_by(&self.config.discovery, d.pid);
        Ok(())
    }

    /// Polls `done` every [`POLL`] on the clock for up to `limit`; false on timeout or stop.
    fn poll_until(&self, limit: Duration, done: impl Fn() -> bool) -> bool {
        let deadline = paddock_jobs::time::Deadline::after(self.config.clock.clone(), limit);
        loop {
            if done() {
                return true;
            }
            if deadline.passed() || self.config.clock.wait(POLL, &self.shared.stop).is_err() {
                return done();
            }
        }
    }
}
