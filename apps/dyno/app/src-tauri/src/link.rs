//! The link to the agent (ADR 0002): finds the running agent through `agent.json`, starts one when there is none,
//! keeps one dyno-ipc connection, forwards the webview's requests and fans the agent's pushes out to a [`Sink`].
//! It reconnects with backoff when the connection drops and replaces an agent of another build.
//!
//! One agent per data folder: the agent itself refuses to start while another holds the folder's lock
//! ([`dyno_agent::instance`]), and the link never starts a second one while the one it started is still running. It waits
//! for a starting agent on its run state, not on a clock (ADR 0031): as long as the process lives and has not written its
//! discovery file, it is starting (a cold, slow disk can take a minute); when it exits first, the exit says why. Quitting
//! ends exactly the process this app started, through the handle it holds (rule 53).
//!
//! The login service: while launchd or systemd supervises the agent (it starts it and restarts it), the app starts no
//! agent of its own and waits for the service's. When the service is turned on while the app's own agent runs, the app
//! hands over: it asks its agent to exit and ends that process, and the service's agent, refused until then, takes the
//! data folder. When it is turned off, the app asks the former service's agent to exit over its connection (never a
//! kill: it did not start it) and starts its own again. A Windows `Run` entry starts nothing until the next logon, so
//! there the app keeps its own agent.
//!
//! The decisions (what to do about the discovery file, how long to wait, which window wants which detail, whether to
//! stop the agent on quit) are pure functions with unit tests; the thread around them is thin.

use crate::dto::{AgentState, AgentStatus, CommandError, ErrorCode};
use crate::logging;
use dyno_agent::discovery::Discovery;
use dyno_agent::paths::Dirs;
use dyno_core::protocol::{LiveDetail, LiveUpdate, Message, Reply, Request, PROTOCOL_VERSION};
use dyno_core::{activity::ActivityEntry, alerts::Alert};
use dyno_ipc::{Client, IpcError, Peer, PeerOptions};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Every request fails after this long.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the one remaining slow request may take: saving the inventory report (it reads the software list, which runs the
/// package tools). Everything else that can run long (finding files, reading the system logs, checking the backups, hashing a
/// file, the sleep, boot, app-usage and software reads) is a job: starting it answers at once and it reports progress and
/// accepts Cancel, so no request waits for it.
pub const EXTRAS_TIMEOUT: Duration = Duration::from_secs(120);

/// How long to wait for the agent's answer to `request`.
pub fn timeout_for(request: &Request) -> Duration {
    match request {
        Request::SaveInventoryReport { .. } => EXTRAS_TIMEOUT,
        _ => REQUEST_TIMEOUT,
    }
}
// pace: how often a starting agent's run state and discovery file are looked at; it bounds nothing.
const START_POLL: Duration = Duration::from_millis(100);
/// After this many looks at a still-starting agent (about 5 s) the status says it is still starting. A hint, not a limit.
const STILL_STARTING_AFTER: u32 = 50;
/// While waiting for the service's agent, the service is asked again every this many looks (about 5 s).
const SERVICE_RECHECK: u32 = 50;
/// The login service is rechecked this many times in a row with no process holding the agent lock before the app takes
/// it that the service starts nothing: four rechecks (about 20 s) outlast launchd's `ThrottleInterval` of 10 s and
/// systemd's `RestartSec` of 5 s, so a service that is merely restarting a refused agent is never mistaken for one that
/// does not run. A count of observations, not a limit on the wait.
const SERVICE_IDLE_RECHECKS: u32 = 4;
/// A request made before the first connection waits this long for it (the window asks for data the moment it opens,
/// usually before the agent has answered; after a first connection, requests fail fast instead).
pub const CONNECT_GRACE: Duration = Duration::from_secs(8);
/// Failed attempts in a row before the status says `failed` (it keeps trying).
pub const FAILED_AFTER: u32 = 3;

/// What the link tells the rest of the app.
pub trait Sink: Send + Sync {
    /// A live update arrived.
    fn live(&self, update: LiveUpdate);
    /// An alert arrived.
    fn alert(&self, alert: Alert);
    /// An activity entry arrived.
    fn activity(&self, entry: ActivityEntry);
    /// An alarm changed (raised, acknowledged, cleared...).
    fn alarm(&self, _change: dyno_core::alarms::AlarmChange) {}
    /// A disk scan's status changed (at most four per second, a final one at the end).
    fn disk_scan(&self, _status: dyno_core::disk::scan::DiskScanStatus) {}
    /// The volumes mounted now changed.
    fn disk_volumes(&self, _volumes: Vec<dyno_core::disk::volume::Volume>) {}
    /// A job's status changed (the newest of each job; a final state always arrives). The shell relays it as `paddock://jobs`.
    fn job(&self, _status: paddock_jobs::wire::WireSnapshot) {}
    /// The connection state changed.
    fn agent(&self, status: AgentStatus);
    /// A wait for the agent is taking long (`Some`, with its cause), or is over (`None`). The chip shows it with Retry.
    fn stalled(&self, _stall: Option<Stall>) {}
    /// A connection was made: the shell reads what it needs from the agent (settings, for the tray).
    fn connected(&self, _link: &Arc<AgentLink>) {}
}

/// Why a wait for the agent has gone on longer than usual. A wait is never given up on (ADR 0031): this is information
/// for the status chip, with a Retry that acts on the cause ([`AgentLink::retry`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stall {
    /// The login service is meant to start the agent, and has not.
    WaitingForService,
    /// The agent this app started is running but has not written its discovery file.
    AgentNotAnswering,
    /// Another agent holds the data folder and has not answered.
    OtherAgentHolds,
}

/// The agent process a [`Launcher`] started, as far as it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child {
    /// It started none (or cannot tell).
    None,
    /// The process it started is running. Its pid names it reliably: the launcher holds the process (unreaped), so the
    /// pid cannot have been handed on (rule 53).
    Running {
        /// The process id.
        pid: u32,
    },
    /// The process it started has exited, with this code (`None`: ended by a signal).
    Exited {
        /// The exit code.
        code: Option<i32>,
    },
}

/// Starts the agent process.
pub trait Launcher: Send + Sync {
    /// Starts `dyno-agent run` detached, unless the process it started before is still running: there is never more than
    /// one. Returns at once; the link watches [`Launcher::child`] and the discovery file. Fails once [`Launcher::close`]d.
    fn launch(&self) -> Result<(), String>;
    /// The process this launcher started last.
    fn child(&self) -> Child;
    /// No more launches: the app is quitting.
    fn close(&self) {}
    /// Waits briefly for the process this launcher started to exit (after `Shutdown`), ending it when it does not.
    fn finish(&self, wait: Duration);
}

/// What the OS service manager says about the agent's login service.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServiceState {
    /// A service is registered (Quit then leaves the agent running).
    pub installed: bool,
    /// The service manager starts the agent now and restarts it when it exits (launchd `KeepAlive`, systemd
    /// `Restart=always`), so the app must not run an agent of its own beside it. A Windows `Run` entry only starts it
    /// at the next logon: installed, not supervising.
    pub supervises: bool,
}

/// Asks the OS about the service (it may be slow: only the link's own thread and background threads call it).
pub type ServiceProbe = Arc<dyn Fn() -> ServiceState + Send + Sync>;

/// What to do about the discovery file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Nothing is registered: start an agent.
    Spawn,
    /// An agent of this build: connect to it.
    Connect,
    /// An agent of another build (or protocol): ask it to exit, then start ours.
    Replace,
}

/// The decision for the discovery file's content.
pub fn plan(found: Option<&Discovery>, app_version: &str, protocol: u32) -> Plan {
    match found {
        None => Plan::Spawn,
        Some(d) if d.version == app_version && d.protocol == protocol => Plan::Connect,
        Some(_) => Plan::Replace,
    }
}

/// How long to wait after `failures` failed attempts in a row: 1 s, 2 s, 4 s … at most 15 s.
pub fn backoff(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.min(4)).min(15))
}

/// The state to report after `failures` failed attempts.
pub fn state_after_failures(failures: u32) -> AgentState {
    if failures >= FAILED_AFTER {
        AgentState::Failed
    } else {
        AgentState::Restarting
    }
}

/// Whether quitting the app stops the agent: only one this app started and no service keeps alive.
pub fn stop_agent_on_quit(started_by_this_app: bool, service_installed: bool) -> bool {
    started_by_this_app && !service_installed
}

/// What each window wants from the live stream. The agent keeps one subscription per connection, so the link asks for
/// the most any window wants (`processes` while a task manager is open), never less than `summary` (the tray icon
/// needs the numbers even when no window is open).
#[derive(Debug, Default)]
pub struct DetailVotes {
    votes: HashMap<String, LiveDetail>,
}

impl DetailVotes {
    /// A window's wish.
    pub fn set(&mut self, window: &str, detail: LiveDetail) {
        self.votes.insert(window.to_string(), detail);
    }

    /// The window is gone.
    pub fn remove(&mut self, window: &str) {
        self.votes.remove(window);
    }

    /// What the connection should ask the agent for.
    pub fn effective(&self) -> LiveDetail {
        if self.votes.values().any(|d| *d == LiveDetail::Processes) {
            LiveDetail::Processes
        } else {
            LiveDetail::Summary
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every guarded value stays consistent if a holder panicked.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The connection manager.
pub struct AgentLink {
    dirs: Dirs,
    app_version: String,
    sink: Arc<dyn Sink>,
    launcher: Box<dyn Launcher>,
    peer: Mutex<Option<Arc<Peer<Message>>>>,
    votes: Mutex<DetailVotes>,
    /// The detail last sent over the current connection.
    sent: Mutex<Option<LiveDetail>>,
    started_by_us: AtomicBool,
    /// The connection loop is running.
    running: AtomicBool,
    /// A connection has been made at least once.
    ever_connected: AtomicBool,
    quit_done: AtomicBool,
    service_installed: AtomicBool,
    /// The login service starts and restarts the agent (the last answer of the probe).
    supervised: AtomicBool,
    probe: Mutex<Option<ServiceProbe>>,
    stopping: Mutex<bool>,
    wake: Condvar,
    /// How often a wait looks again, in milliseconds (pace only; tests shorten it).
    poll_ms: AtomicU64,
    /// Counts Retry requests ([`AgentLink::retry`]); a wait acts on a count it has not seen.
    retries: AtomicU32,
    /// The login service did not start its agent, so the app runs its own beside the (broken) service.
    service_fallback: AtomicBool,
    /// A stall was reported and not yet cleared.
    stall_shown: AtomicBool,
    /// How many times a wait has looked again ([`AgentLink::looks`]).
    looks: AtomicU64,
}

impl AgentLink {
    /// A link for the agent in `dirs`.
    pub fn new(dirs: Dirs, app_version: &str, sink: Arc<dyn Sink>, launcher: Box<dyn Launcher>) -> Arc<AgentLink> {
        Arc::new(AgentLink {
            dirs,
            app_version: app_version.to_string(),
            sink,
            launcher,
            peer: Mutex::new(None),
            votes: Mutex::new(DetailVotes::default()),
            sent: Mutex::new(None),
            started_by_us: AtomicBool::new(false),
            running: AtomicBool::new(false),
            ever_connected: AtomicBool::new(false),
            quit_done: AtomicBool::new(false),
            service_installed: AtomicBool::new(false),
            supervised: AtomicBool::new(false),
            probe: Mutex::new(None),
            stopping: Mutex::new(false),
            wake: Condvar::new(),
            poll_ms: AtomicU64::new(START_POLL.as_millis() as u64),
            retries: AtomicU32::new(0),
            service_fallback: AtomicBool::new(false),
            stall_shown: AtomicBool::new(false),
            looks: AtomicU64::new(0),
        })
    }

    /// Runs the connection loop on its own thread.
    pub fn spawn(self: &Arc<Self>) -> std::io::Result<std::thread::JoinHandle<()>> {
        let me = self.clone();
        self.running.store(true, Ordering::SeqCst);
        std::thread::Builder::new().name("dyno-link".into()).spawn(move || me.run())
    }

    /// How often a wait for the agent looks again (default [`START_POLL`]). The thresholds are counted in looks, so this
    /// only changes the pace.
    pub fn set_poll(&self, every: Duration) {
        self.poll_ms
            .store(u64::try_from(every.as_millis()).unwrap_or(u64::MAX).max(1), Ordering::SeqCst);
    }

    /// How many times a wait for the agent has looked again so far: its progress, counted rather than timed.
    pub fn looks(&self) -> u64 {
        self.looks.load(Ordering::SeqCst)
    }

    /// The Retry of the status chip: acts on the wait in progress (ends and relaunches the agent this app started and
    /// that does not answer, starts the app's own agent when the login service starts none, looks for the agent again
    /// when another one holds the data folder). Without a wait in progress it does nothing.
    pub fn retry(&self) {
        self.retries.fetch_add(1, Ordering::SeqCst);
        self.wake.notify_all();
    }

    /// How to ask the OS about the login service. Without one, no service supervises the agent.
    pub fn set_service_probe(&self, probe: ServiceProbe) {
        *lock(&self.probe) = Some(probe);
    }

    /// Asks the OS about the login service and records the answer.
    fn probe_service(&self) -> Option<ServiceState> {
        let probe = lock(&self.probe).clone()?;
        let state = probe();
        self.service_installed.store(state.installed, Ordering::SeqCst);
        self.supervised.store(state.supervises, Ordering::SeqCst);
        if !state.supervises {
            self.service_fallback.store(false, Ordering::SeqCst);
        }
        Some(state)
    }

    /// The login service may have changed (the switch was turned, or a connection was made): asks the OS, then hands
    /// the agent over to the service, or takes it back from a service that was removed. Blocking (it may wait a few
    /// seconds for the app's own agent to exit): call it off the UI thread.
    pub fn refresh_service(&self) {
        let was_supervised = self.supervised.load(Ordering::SeqCst);
        let Some(state) = self.probe_service() else { return };
        if state.supervises {
            self.hand_over();
        } else if was_supervised && !state.installed {
            self.take_back();
        }
    }

    /// The service supervises the agent: ends the app's own agent (only the process this app started, rule 53), so the
    /// service's, refused while ours held the data folder, takes over. The loop then waits for it.
    fn hand_over(&self) {
        // The service has shown it starts nothing (see `wait_for_service`): ending the app's agent would leave none.
        if self.service_fallback.load(Ordering::SeqCst) || !matches!(self.launcher.child(), Child::Running { .. }) {
            return;
        }
        logging::info("the login service runs the agent now; stopping the app's own agent");
        let peer = lock(&self.peer).clone().filter(|p| !p.is_closed());
        if let Some(p) = peer.filter(|_| self.started_by_this_app()) {
            let _ = p.call(Message::Request(Box::new(Request::Shutdown)), REQUEST_TIMEOUT);
        }
        // shutdown: the process this app started gets a few seconds to exit, then it is ended through its handle.
        self.launcher.finish(Duration::from_secs(3));
        self.started_by_us.store(false, Ordering::SeqCst);
    }

    /// The service was removed but its agent still answers: asks it to exit over its connection (it was not started by
    /// this app, so it is never killed); the loop then starts the app's own.
    fn take_back(&self) {
        if self.started_by_this_app() {
            return;
        }
        let Some(p) = lock(&self.peer).clone().filter(|p| !p.is_closed()) else {
            return;
        };
        logging::info("the login service was removed; asking its agent to exit and starting the app's own");
        let _ = p.call(Message::Request(Box::new(Request::Shutdown)), REQUEST_TIMEOUT);
    }

    /// Records whether a service keeps the agent alive (decides what quitting does).
    pub fn set_service_installed(&self, installed: bool) {
        self.service_installed.store(installed, Ordering::SeqCst);
    }

    /// Whether this app started the agent it talks to.
    pub fn started_by_this_app(&self) -> bool {
        self.started_by_us.load(Ordering::SeqCst)
    }

    /// Whether the link is connected.
    pub fn connected(&self) -> bool {
        lock(&self.peer).as_ref().is_some_and(|p| !p.is_closed())
    }

    fn is_stopping(&self) -> bool {
        *lock(&self.stopping)
    }

    /// Waits up to `d`, or until the link is stopped. Returns whether it should go on.
    fn pause(&self, d: Duration) -> bool {
        let guard = lock(&self.stopping);
        let (guard, _) = self
            .wake
            .wait_timeout_while(guard, d, |stop| !*stop)
            .unwrap_or_else(|e| e.into_inner());
        !*guard
    }

    // ------------------------------------------------------------------------------------------------ requests

    /// Sends one request and returns the reply. A `Subscribe` is not forwarded as is: it is a window's wish
    /// ([`AgentLink::set_detail`]).
    pub fn call(&self, request: Request) -> Result<Reply, CommandError> {
        let peer = self.peer_or_wait().ok_or_else(CommandError::unavailable)?;
        let timeout = timeout_for(&request);
        match peer.call(Message::Request(Box::new(request)), timeout) {
            Ok(Message::Reply(r)) => (*r).map_err(CommandError::from),
            Ok(_) => Err(CommandError::internal("The agent sent something unexpected.")),
            Err(IpcError::Timeout(_)) => Err(CommandError::new(ErrorCode::Timeout, "The agent did not answer in time.")),
            Err(IpcError::Closed) => Err(CommandError::unavailable()),
            Err(e) => Err(CommandError::new(ErrorCode::Internal, format!("The agent link failed: {e}"))),
        }
    }

    /// The connection; before the very first one has been made, waits for it up to [`CONNECT_GRACE`].
    fn peer_or_wait(&self) -> Option<Arc<Peer<Message>>> {
        let current = || lock(&self.peer).clone().filter(|p| !p.is_closed());
        let end = Instant::now() + CONNECT_GRACE;
        loop {
            if let Some(p) = current() {
                return Some(p);
            }
            let waiting = self.running.load(Ordering::SeqCst) && !self.ever_connected.load(Ordering::SeqCst) && !self.is_stopping();
            if !waiting || Instant::now() >= end {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A window's wish for the live detail; the connection asks for the most any window wants.
    pub fn set_detail(&self, window: &str, detail: LiveDetail) -> Result<(), CommandError> {
        lock(&self.votes).set(window, detail);
        self.sync_detail()
    }

    /// A window is gone: it no longer wants anything.
    pub fn drop_window(&self, window: &str) {
        lock(&self.votes).remove(window);
        let _ = self.sync_detail();
    }

    fn sync_detail(&self) -> Result<(), CommandError> {
        let wanted = lock(&self.votes).effective();
        if *lock(&self.sent) == Some(wanted) {
            return Ok(());
        }
        self.call(Request::Subscribe { detail: wanted })?;
        *lock(&self.sent) = Some(wanted);
        Ok(())
    }

    // --------------------------------------------------------------------------------------------- connecting

    fn run(self: Arc<Self>) {
        let mut failures = 0u32;
        while !self.is_stopping() {
            self.sink.agent(AgentStatus::new(
                if failures == 0 {
                    AgentState::Starting
                } else {
                    state_after_failures(failures)
                },
                None,
            ));
            match self.establish() {
                Ok(peer) => {
                    failures = 0;
                    *lock(&self.sent) = None;
                    *lock(&self.peer) = Some(peer.clone());
                    self.ever_connected.store(true, Ordering::SeqCst);
                    if let Err(e) = self.sync_detail() {
                        logging::info(&format!("could not subscribe: {}", e.message));
                    }
                    logging::info("connected to the agent");
                    self.sink.connected(&self);
                    self.sink.agent(AgentStatus::new(AgentState::Running, None));
                    while !self.is_stopping() && !peer.is_closed() {
                        if !self.pause(Duration::from_millis(250)) {
                            break;
                        }
                    }
                    *lock(&self.peer) = None;
                    peer.close();
                    if !self.is_stopping() {
                        logging::info("the agent link dropped; reconnecting");
                        self.sink.agent(AgentStatus::new(
                            AgentState::Restarting,
                            Some("The connection to the Dyno agent dropped; reconnecting.".into()),
                        ));
                        self.pause(Duration::from_millis(500));
                    }
                }
                Err(message) => {
                    failures += 1;
                    logging::info(&format!("no agent yet ({failures}): {message}"));
                    self.sink.agent(AgentStatus::new(state_after_failures(failures), Some(message)));
                    self.pause(backoff(failures));
                }
            }
        }
    }

    /// Stops the loop (it starts nothing from now on) and closes the connection. With `stop_agent`, the agent this app
    /// started is asked to exit when it is the one connected, and its process is then waited for and ended when it does
    /// not exit, also while it is still starting. Returns whether an agent of this app was stopped (see
    /// [`stop_agent_on_quit`]).
    pub fn shutdown(&self, stop_agent: bool) -> bool {
        // First: nothing may start an agent after this point (the loop sees `stopping`, the launcher is closed).
        self.launcher.close();
        *lock(&self.stopping) = true;
        self.wake.notify_all();
        let peer = lock(&self.peer).take();
        let mut stopped = false;
        if stop_agent {
            if let Some(p) = peer.as_ref().filter(|_| self.started_by_this_app()) {
                stopped = p.call(Message::Request(Box::new(Request::Shutdown)), REQUEST_TIMEOUT).is_ok();
            }
            stopped |= matches!(self.launcher.child(), Child::Running { .. });
            // shutdown: the process this app started gets a few seconds to exit, then it is ended through its handle.
            self.launcher.finish(Duration::from_secs(3));
        }
        if let Some(p) = peer {
            p.close();
        }
        stopped
    }

    /// The quit decision for this link.
    pub fn quit(&self) -> bool {
        if self.quit_done.swap(true, Ordering::SeqCst) {
            return false;
        }
        let ours = self.started_by_this_app() || matches!(self.launcher.child(), Child::Running { .. });
        let stop = stop_agent_on_quit(ours, self.service_installed.load(Ordering::SeqCst));
        logging::info(&format!(
            "quitting; the agent {}",
            if stop {
                "is stopped (this app started it)"
            } else {
                "keeps running"
            }
        ));
        self.shutdown(stop)
    }

    fn try_connect(&self, d: &Discovery) -> Result<Arc<Peer<Message>>, String> {
        let token = d.token().ok_or("The discovery file is damaged.")?;
        let conn = Client::connect(&d.endpoint, &token).map_err(|e| format!("The agent did not answer ({e})."))?;
        let sink = self.sink.clone();
        let opts = PeerOptions::new().on_push(move |m| match m {
            Message::Live(l) => sink.live(*l),
            Message::Alert(a) => sink.alert(a),
            Message::Activity(a) => sink.activity(a),
            Message::Alarm(c) => sink.alarm(c),
            Message::DiskScan(s) => sink.disk_scan(*s),
            Message::Job(j) => sink.job(*j),
            Message::DiskVolumes(v) => sink.disk_volumes(v),
            _ => {}
        });
        Peer::start(conn, opts).map(Arc::new).map_err(|e| e.to_string())
    }

    /// Connects to the agent, starting or replacing it when needed.
    fn establish(&self) -> Result<Arc<Peer<Message>>, String> {
        for _ in 0..3 {
            let found = Discovery::read(&self.dirs.discovery());
            match plan(found.as_ref(), &self.app_version, PROTOCOL_VERSION) {
                Plan::Connect => {
                    if let Some(d) = &found {
                        match self.try_connect(d) {
                            Ok(p) => {
                                // Ours only when it is the very process this app started (held, so its pid is its own).
                                let ours = self.launcher.child() == Child::Running { pid: d.pid };
                                self.started_by_us.store(ours, Ordering::SeqCst);
                                if !ours {
                                    // The service's agent is up after all.
                                    self.service_fallback.store(false, Ordering::SeqCst);
                                }
                                return Ok(p);
                            }
                            // Unreachable: its process is gone (the file is stale) or not answering yet. The file is
                            // left alone: a new agent replaces it, or exits at once when the old one still holds the
                            // data folder.
                            Err(e) => logging::info(&format!("agent.json names an agent that did not answer: {e}")),
                        }
                    }
                    self.start_agent()?;
                }
                Plan::Replace => {
                    let d = found.as_ref().map(|d| (d.version.clone(), d.protocol)).unwrap_or_default();
                    logging::info(&format!(
                        "the running agent is another build ({} / protocol {}); replacing it",
                        d.0, d.1
                    ));
                    if let Some(found) = &found {
                        if let Ok(p) = self.try_connect(found) {
                            let _ = p.call(Message::Request(Box::new(Request::Shutdown)), Duration::from_secs(5));
                            p.close();
                            self.wait_gone(Duration::from_secs(5));
                        }
                    }
                    if let Some(found) = &found {
                        // Its process is gone but the file stayed: removed only if it is still that very file (a newer
                        // agent may have written its own meanwhile).
                        Discovery::remove_if_same(&self.dirs.discovery(), found);
                    }
                }
                Plan::Spawn => self.start_agent()?,
            }
            if self.is_stopping() {
                return Err("Dyno is closing.".into());
            }
        }
        Err("The Dyno agent could not be started. Its log is in the Dyno data folder.".into())
    }

    fn wait_gone(&self, wait: Duration) {
        let end = Instant::now() + wait;
        while Instant::now() < end && Discovery::read(&self.dirs.discovery()).is_some() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// One pace of a wait: counts the look, then waits [`AgentLink::set_poll`] (or until the link stops).
    fn look_again(&self) {
        self.looks.fetch_add(1, Ordering::SeqCst);
        self.pause(Duration::from_millis(self.poll_ms.load(Ordering::SeqCst)));
    }

    /// Tells the chip what a long wait is waiting for (Retry acts on it), and the window as a "still starting" note.
    fn show_stall(&self, stall: Stall, note: &str) {
        logging::info(&format!("still waiting: {note}"));
        self.sink.agent(AgentStatus::new(AgentState::Starting, Some(note.to_string())));
        self.stall_shown.store(true, Ordering::SeqCst);
        self.sink.stalled(Some(stall));
    }

    /// The wait is over (or starts anew): the chip drops the stall it showed.
    fn clear_stall(&self) {
        if self.stall_shown.swap(false, Ordering::SeqCst) {
            self.sink.stalled(None);
        }
    }

    /// Whether Retry was chosen since `seen` (the count when the wait began); `seen` is brought up to date.
    fn retry_chosen(&self, seen: &mut u32) -> bool {
        let now = self.retries.load(Ordering::SeqCst);
        std::mem::replace(seen, now) != now
    }

    /// Whether a discovery file found while waiting since `since_ms` names an agent that is up: written after the wait
    /// began, or answering. A file from before the wait that nothing answers is an old agent's, not the one waited for.
    fn names_live_agent(&self, d: &Discovery, since_ms: i64) -> bool {
        written_since(d, since_ms) || self.try_connect(d).map(|p| p.close()).is_ok()
    }

    /// Starts the agent unless the one this app started is still running, and waits for it to write its discovery file
    /// (a connection is made by the next pass of [`establish`]). The wait is judged on the process, not on a clock
    /// (ADR 0031): while it runs, it is starting. After [`STILL_STARTING_AFTER`] looks the chip says so with Retry,
    /// which ends the process this app started (through the handle it holds) and launches again; the wait itself never
    /// ends on time.
    fn start_agent(&self) -> Result<(), String> {
        if self.is_stopping() {
            return Ok(());
        }
        let mut seen_retry = self.retries.load(Ordering::SeqCst);
        if matches!(self.launcher.child(), Child::Running { .. }) {
            logging::info("the agent this app started is still starting; waiting for it");
        } else if !self.service_fallback.load(Ordering::SeqCst) && self.probe_service().is_some_and(|s| s.supervises) {
            return self.wait_for_service(seen_retry);
        } else {
            logging::info("starting the agent");
            self.launcher.launch()?;
        }
        let mut looks = 0u32;
        loop {
            if self.is_stopping() {
                return Ok(());
            }
            if self.retry_chosen(&mut seen_retry) {
                logging::info("Retry: ending the agent this app started and starting it again");
                self.launcher.finish(Duration::from_secs(3));
                self.started_by_us.store(false, Ordering::SeqCst);
                self.clear_stall();
                return Ok(());
            }
            match start_progress(self.launcher.child(), Discovery::read(&self.dirs.discovery()).as_ref()) {
                StartProgress::Starting => {}
                StartProgress::Ready => {
                    self.clear_stall();
                    return Ok(());
                }
                StartProgress::OtherAgent => {
                    self.clear_stall();
                    return self.wait_for_other_agent(seen_retry);
                }
                StartProgress::Failed(message) => {
                    self.clear_stall();
                    return Err(message);
                }
            }
            looks += 1;
            if looks == STILL_STARTING_AFTER {
                self.show_stall(
                    Stall::AgentNotAnswering,
                    "The Dyno agent is still starting. A slow disk or a busy computer can take a while.",
                );
            }
            self.look_again();
        }
    }

    /// The login service supervises the agent: waits for its discovery file, however long its start takes, and looks at
    /// the service again now and then (turned off meanwhile: the next pass starts the app's own). A service that
    /// supervises on paper but starts nothing (switched off in Login Items, a job launchd dropped, a unit that fails) is
    /// told by the agent lock: when no process has held it across [`SERVICE_IDLE_RECHECKS`] rechecks in a row, longer
    /// than the service manager's restart interval, the app starts its own agent (safe: the lock makes the loser of two
    /// exit with code 3) and says so. Retry does that at once.
    fn wait_for_service(&self, mut seen_retry: u32) -> Result<(), String> {
        logging::info("the login service runs the agent; waiting for it");
        let since = now_ms();
        let mut looks = 0u32;
        let mut idle = 0u32;
        while !self.is_stopping() {
            if Discovery::read(&self.dirs.discovery()).is_some_and(|d| self.names_live_agent(&d, since)) {
                self.clear_stall();
                return Ok(());
            }
            if self.retry_chosen(&mut seen_retry) {
                self.fall_back_to_own_agent("Retry was chosen");
                return Ok(());
            }
            looks += 1;
            if looks == STILL_STARTING_AFTER {
                self.show_stall(
                    Stall::WaitingForService,
                    "Waiting for the Dyno agent that your system starts at login.",
                );
            }
            if looks.is_multiple_of(SERVICE_RECHECK) {
                if !self.probe_service().is_some_and(|s| s.supervises) {
                    self.clear_stall();
                    return Ok(());
                }
                if dyno_agent::instance::is_held(&self.dirs) {
                    idle = 0;
                } else {
                    idle += 1;
                    if idle >= SERVICE_IDLE_RECHECKS {
                        self.fall_back_to_own_agent("no agent has run for the login service");
                        return Ok(());
                    }
                }
            }
            self.look_again();
        }
        Ok(())
    }

    /// The login service starts no agent: the app runs its own from now on (until the service shows it works, or is
    /// turned off), and says so.
    fn fall_back_to_own_agent(&self, why: &str) {
        logging::info(&format!("the login service started no agent ({why}); starting the app's own"));
        self.service_fallback.store(true, Ordering::SeqCst);
        self.clear_stall();
        self.sink.agent(AgentStatus::new(
            AgentState::Starting,
            Some(
                "The login service did not start the Dyno agent, so Dyno started its own. Check Dyno in System Settings, Login Items."
                    .into(),
            ),
        ));
    }

    /// The agent this app started exited because another one holds the data folder (the login service's, or one that
    /// was slow to write its discovery file): waits for that one's file while it holds the folder. A file from before
    /// the wait that nothing answers is an old agent's and does not count. After [`STILL_STARTING_AFTER`] looks the chip
    /// says who holds the folder, with a Retry that runs the connection steps again.
    fn wait_for_other_agent(&self, mut seen_retry: u32) -> Result<(), String> {
        logging::info("another agent holds the data folder; waiting for it");
        let since = now_ms();
        let mut looks = 0u32;
        while !self.is_stopping() {
            let live = Discovery::read(&self.dirs.discovery()).is_some_and(|d| self.names_live_agent(&d, since));
            if live || !dyno_agent::instance::is_held(&self.dirs) || self.retry_chosen(&mut seen_retry) {
                self.clear_stall();
                return Ok(());
            }
            looks += 1;
            if looks == STILL_STARTING_AFTER {
                self.show_stall(
                    Stall::OtherAgentHolds,
                    "Another Dyno agent holds the data folder and has not answered. It may be starting slowly, or running on another computer that shares the folder.",
                );
            }
            self.look_again();
        }
        Ok(())
    }
}

/// Now, in milliseconds since the epoch (what an agent writes as `started`).
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Where a start stands, from the launched process and the discovery file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartProgress {
    /// Still starting: wait.
    Starting,
    /// The discovery file names the process (or, for a launcher without a process, any agent wrote one).
    Ready,
    /// It exited because another agent holds the data folder.
    OtherAgent,
    /// It exited for another reason.
    Failed(String),
}

/// The decision for a start in progress (pure).
pub fn start_progress(child: Child, found: Option<&Discovery>) -> StartProgress {
    match child {
        Child::Running { pid } if found.is_some_and(|d| d.pid == pid) => StartProgress::Ready,
        Child::Running { .. } => StartProgress::Starting,
        Child::Exited {
            code: Some(dyno_agent::cli::EXIT_ALREADY_RUNNING),
        } => StartProgress::OtherAgent,
        Child::Exited { code } => StartProgress::Failed(match code {
            Some(c) => format!("The Dyno agent stopped while starting (exit code {c}). Its log is in the Dyno data folder."),
            None => "The Dyno agent was ended while starting.".into(),
        }),
        Child::None if found.is_some() => StartProgress::Ready,
        Child::None => StartProgress::Starting,
    }
}

/// Whether a discovery file found while waiting since `since_ms` (ms since the epoch) was written after the wait began.
pub fn written_since(d: &Discovery, since_ms: i64) -> bool {
    d.started >= since_ms
}

// ---------------------------------------------------------------------------------------------------------------

/// Starts the real agent program detached (no console, its own process group, not tied to the app's lifetime).
pub struct ProcessLauncher {
    exe: Box<dyn Fn() -> Result<std::path::PathBuf, String> + Send + Sync>,
    slot: Mutex<Slot>,
}

/// The process a [`ProcessLauncher`] started, held until it has exited and been reaped.
#[derive(Default)]
struct Slot {
    child: Option<std::process::Child>,
    /// The exit code of the last one, once reaped (`Some(None)`: ended by a signal).
    exited: Option<Option<i32>>,
    closed: bool,
}

impl Slot {
    /// Reaps the child when it has exited, and says what it is.
    fn look(&mut self) -> Child {
        if let Some(c) = self.child.as_mut() {
            match c.try_wait() {
                Ok(None) => return Child::Running { pid: c.id() },
                Ok(Some(status)) => {
                    self.exited = Some(status.code());
                    self.child = None;
                }
                // It cannot be asked: treat it as still running (it is still held; the quit ends it).
                Err(_) => return Child::Running { pid: c.id() },
            }
        }
        match self.exited {
            Some(code) => Child::Exited { code },
            None => Child::None,
        }
    }
}

impl ProcessLauncher {
    /// A launcher whose program is found by `exe` each time (so a reinstalled app is picked up).
    pub fn new(exe: impl Fn() -> Result<std::path::PathBuf, String> + Send + Sync + 'static) -> ProcessLauncher {
        ProcessLauncher {
            exe: Box::new(exe),
            slot: Mutex::new(Slot::default()),
        }
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&self) -> Result<(), String> {
        // Held across the start: a quit (`close`, `finish`) waits for it and then sees the new process.
        let mut slot = lock(&self.slot);
        if slot.closed {
            return Err("Dyno is closing.".into());
        }
        if matches!(slot.look(), Child::Running { .. }) {
            return Ok(());
        }
        let exe = (self.exe)()?;
        // The one place the app starts a program: our own agent, which reports problems through its log and exits; no
        // console window (see the creation flags below) and it is never waited on.
        #[allow(clippy::disallowed_methods)]
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("run")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Its own process group: a Ctrl+C in the terminal that started the app does not reach it.
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
            cmd.creation_flags(0x0000_0008 | 0x0800_0000 | 0x0000_0200);
        }
        let child = cmd
            .spawn()
            .map_err(|e| format!("The Dyno agent ({}) could not be started: {e}", exe.display()))?;
        slot.child = Some(child);
        slot.exited = None;
        Ok(())
    }

    fn child(&self) -> Child {
        lock(&self.slot).look()
    }

    fn close(&self) {
        lock(&self.slot).closed = true;
    }

    fn finish(&self, wait: Duration) {
        // Held throughout: nothing is launched while the process this launcher started is being ended.
        let mut slot = lock(&self.slot);
        let Some(child) = slot.child.as_mut() else { return };
        let end = Instant::now() + wait;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                _ if Instant::now() >= end => {
                    let _ = child.kill();
                    break child.wait().ok();
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        slot.child = None;
        slot.exited = Some(status.and_then(|s| s.code()));
    }
}

/// The agent program to start: `DYNO_AGENT_BIN` when set and valid (development), else `dyno-agent` next to the app
/// (where Tauri puts the sidecar). An empty file is the build's placeholder and counts as missing.
pub fn agent_exe(app_exe: &std::path::Path, env_override: Option<&std::ffi::OsStr>) -> Result<std::path::PathBuf, String> {
    let usable = |p: &std::path::Path| p.metadata().is_ok_and(|m| m.is_file() && m.len() > 0);
    if let Some(p) = env_override.filter(|p| !p.is_empty()).map(std::path::PathBuf::from) {
        return if usable(&p) {
            Ok(p)
        } else {
            Err(format!("DYNO_AGENT_BIN ({}) is not a program.", p.display()))
        };
    }
    let name = if cfg!(windows) { "dyno-agent.exe" } else { "dyno-agent" };
    let p = app_exe.parent().map(|d| d.join(name)).unwrap_or_else(|| name.into());
    if usable(&p) {
        Ok(p)
    } else {
        Err(format!(
            "{} is missing; reinstall Dyno (development builds: build dyno-agent and copy it there).",
            p.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_agent::Running;
    use dyno_platform::fake::{process, Fake};
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{channel, Receiver, Sender};

    #[test]
    fn only_saving_the_inventory_waits_minutes_and_every_job_request_is_quick() {
        use dyno_core::issues::IssuesQuery;
        use dyno_core::jobs::JobSpec;
        for job in [
            Request::StartJob {
                spec: JobSpec::RescanIssues {
                    query: IssuesQuery::default(),
                },
            },
            Request::StartJob {
                spec: JobSpec::RefreshBackup,
            },
            Request::Jobs,
            Request::CancelJob { id: 1 },
            Request::JobResult { id: 1 },
        ] {
            assert_eq!(timeout_for(&job), REQUEST_TIMEOUT, "{job:?}");
        }
        assert_eq!(timeout_for(&Request::Settings), REQUEST_TIMEOUT);
        assert_eq!(
            timeout_for(&Request::SaveInventoryReport {
                path: "/x.md".into(),
                format: dyno_core::extras::software::InventoryFormat::Markdown
            }),
            EXTRAS_TIMEOUT
        );
    }

    fn disc(version: &str, protocol: u32) -> Discovery {
        Discovery {
            endpoint: "e".into(),
            token: "00".into(),
            pid: 1,
            started: 0,
            version: version.into(),
            protocol,
        }
    }

    #[test]
    fn the_discovery_file_decides_what_to_do() {
        assert_eq!(plan(None, "0.1.0", 1), Plan::Spawn);
        assert_eq!(plan(Some(&disc("0.1.0", 1)), "0.1.0", 1), Plan::Connect);
        assert_eq!(
            plan(Some(&disc("0.0.9", 1)), "0.1.0", 1),
            Plan::Replace,
            "an older agent is replaced"
        );
        assert_eq!(plan(Some(&disc("0.1.0", 2)), "0.1.0", 1), Plan::Replace, "another protocol too");
    }

    #[test]
    fn backoff_grows_then_caps() {
        let secs: Vec<u64> = (0..7).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 15, 15, 15]);
        assert_eq!(state_after_failures(1), AgentState::Restarting);
        assert_eq!(state_after_failures(FAILED_AFTER), AgentState::Failed);
    }

    #[test]
    fn a_start_is_judged_on_the_process_and_its_discovery_file_not_on_a_clock() {
        let ours = |pid| Discovery { pid, ..disc("0.1.0", 1) };
        let running = Child::Running { pid: 42 };
        assert_eq!(
            start_progress(running, None),
            StartProgress::Starting,
            "alive without a file: still starting"
        );
        assert_eq!(
            start_progress(running, Some(&ours(7))),
            StartProgress::Starting,
            "a file of another process (stale) is not our agent's"
        );
        assert_eq!(start_progress(running, Some(&ours(42))), StartProgress::Ready);
        assert_eq!(
            start_progress(
                Child::Exited {
                    code: Some(dyno_agent::cli::EXIT_ALREADY_RUNNING)
                },
                None
            ),
            StartProgress::OtherAgent
        );
        match start_progress(Child::Exited { code: Some(1) }, Some(&ours(42))) {
            StartProgress::Failed(m) => assert!(m.contains("exit code 1"), "{m}"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            start_progress(Child::Exited { code: None }, None),
            StartProgress::Failed(_)
        ));
        assert_eq!(start_progress(Child::None, Some(&ours(7))), StartProgress::Ready);
        assert_eq!(start_progress(Child::None, None), StartProgress::Starting);
    }

    /// A stand-in agent program: a shell script in a temp folder that ignores `run` and does what the test says.
    #[cfg(unix)]
    fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[cfg(unix)]
    #[test]
    fn the_process_launcher_holds_one_agent_at_a_time_and_ends_exactly_it() {
        let dir = tempfile::tempdir().unwrap();
        // Never writes a discovery file: an agent stuck in a slow start.
        let exe = script(dir.path(), "slow-agent", "exec sleep 600");
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = starts.clone();
        let launcher = ProcessLauncher::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(exe.clone())
        });
        assert_eq!(launcher.child(), Child::None);
        launcher.launch().unwrap();
        let Child::Running { pid } = launcher.child() else {
            panic!("not running")
        };
        launcher.launch().unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1, "a second agent was started while the first runs");
        assert_eq!(launcher.child(), Child::Running { pid });
        // Quit: it does not exit by itself, so it is ended through the handle, and nothing starts afterwards.
        launcher.close();
        launcher.finish(Duration::from_millis(200));
        assert!(launcher.launch().is_err(), "a closed launcher starts nothing");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[test]
    fn the_process_launcher_reports_how_its_agent_exited() {
        let dir = tempfile::tempdir().unwrap();
        let code = dyno_agent::cli::EXIT_ALREADY_RUNNING;
        let exe = script(dir.path(), "refused-agent", &format!("exit {code}"));
        let launcher = ProcessLauncher::new(move || Ok(exe.clone()));
        launcher.launch().unwrap();
        let mut state = launcher.child();
        // The script exits at once; its exit is what is waited on (a condition, not a time).
        while matches!(state, Child::Running { .. }) {
            std::thread::sleep(Duration::from_millis(10));
            state = launcher.child();
        }
        assert_eq!(state, Child::Exited { code: Some(code) });
        assert_eq!(
            launcher.child(),
            Child::Exited { code: Some(code) },
            "the exit is remembered once reaped"
        );
    }

    #[test]
    fn quitting_stops_only_an_agent_this_app_started_without_a_service() {
        assert!(stop_agent_on_quit(true, false));
        assert!(!stop_agent_on_quit(true, true), "a service keeps its agent");
        assert!(!stop_agent_on_quit(false, false), "an agent somebody else started stays");
        assert!(!stop_agent_on_quit(false, true));
    }

    #[test]
    fn the_connection_asks_for_the_most_any_window_wants() {
        let mut v = DetailVotes::default();
        assert_eq!(v.effective(), LiveDetail::Summary, "the tray needs numbers with no window open");
        v.set("main", LiveDetail::Processes);
        v.set("tray", LiveDetail::Summary);
        assert_eq!(v.effective(), LiveDetail::Processes);
        v.remove("main");
        assert_eq!(v.effective(), LiveDetail::Summary);
    }

    #[test]
    fn the_agent_program_is_next_to_the_app_or_overridden() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("dyno-app");
        let name = if cfg!(windows) { "dyno-agent.exe" } else { "dyno-agent" };
        assert!(agent_exe(&app, None).unwrap_err().contains("missing"));
        std::fs::write(dir.path().join(name), b"").unwrap();
        assert!(agent_exe(&app, None).is_err(), "the build's empty placeholder is not a program");
        std::fs::write(dir.path().join(name), b"x").unwrap();
        assert_eq!(agent_exe(&app, None).unwrap(), dir.path().join(name));
        let other = dir.path().join("other");
        std::fs::write(&other, b"x").unwrap();
        assert_eq!(agent_exe(&app, Some(other.as_os_str())).unwrap(), other);
        assert!(agent_exe(&app, Some(dir.path().join("nope").as_os_str()))
            .unwrap_err()
            .contains("DYNO_AGENT_BIN"));
    }

    struct Recorder {
        live: AtomicUsize,
        statuses: Sender<AgentStatus>,
        stalls: Mutex<Vec<Option<Stall>>>,
    }

    impl Recorder {
        fn new(statuses: Sender<AgentStatus>) -> Arc<Recorder> {
            Arc::new(Recorder {
                live: AtomicUsize::new(0),
                statuses,
                stalls: Mutex::new(Vec::new()),
            })
        }

        fn stalled_with(&self, stall: Stall) -> bool {
            lock(&self.stalls).contains(&Some(stall))
        }
    }

    impl Sink for Recorder {
        fn live(&self, _: LiveUpdate) {
            self.live.fetch_add(1, Ordering::SeqCst);
        }
        fn alert(&self, _: Alert) {}
        fn activity(&self, _: ActivityEntry) {}
        fn agent(&self, s: AgentStatus) {
            let _ = self.statuses.send(s);
        }
        fn stalled(&self, stall: Option<Stall>) {
            lock(&self.stalls).push(stall);
        }
    }

    /// Starts an in-process agent over the fake platform when asked (what the real launcher does with a process).
    /// Its "process" is this test process: the agent's discovery file names `std::process::id()`.
    struct InProcessLauncher {
        dirs: Dirs,
        started: Mutex<Option<Running>>,
        launches: AtomicUsize,
        exited: AtomicBool,
        fail: bool,
    }

    impl Launcher for InProcessLauncher {
        fn launch(&self) -> Result<(), String> {
            if matches!(self.child(), Child::Running { .. }) {
                return Ok(());
            }
            self.launches.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("no agent program".into());
            }
            let fake = Fake::new();
            fake.add_process(process(100, "busy", 60.0));
            let running = Running::start(self.dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(())))?;
            *lock(&self.started) = Some(running);
            self.exited.store(false, Ordering::SeqCst);
            Ok(())
        }
        fn child(&self) -> Child {
            match lock(&self.started).as_ref() {
                Some(r) if !r.stopped() => Child::Running { pid: std::process::id() },
                _ if self.exited.load(Ordering::SeqCst) => Child::Exited { code: Some(0) },
                _ => Child::None,
            }
        }
        fn finish(&self, _: Duration) {
            if let Some(r) = lock(&self.started).take() {
                r.stop();
                self.exited.store(true, Ordering::SeqCst);
            }
        }
    }

    struct Rig {
        link: Arc<AgentLink>,
        sink: Arc<Recorder>,
        statuses: Receiver<AgentStatus>,
        launcher: Arc<InProcessLauncher>,
        _tmp: tempfile::TempDir,
    }

    struct Shared(Arc<InProcessLauncher>);
    impl Launcher for Shared {
        fn launch(&self) -> Result<(), String> {
            self.0.launch()
        }
        fn child(&self) -> Child {
            self.0.child()
        }
        fn finish(&self, w: Duration) {
            self.0.finish(w)
        }
    }

    fn rig(fail: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let (tx, rx) = channel();
        let sink = Recorder::new(tx);
        let launcher = Arc::new(InProcessLauncher {
            dirs: dirs.clone(),
            started: Mutex::new(None),
            launches: AtomicUsize::new(0),
            exited: AtomicBool::new(false),
            fail,
        });
        let link = AgentLink::new(dirs, env!("CARGO_PKG_VERSION"), sink.clone(), Box::new(Shared(launcher.clone())));
        Rig {
            link,
            sink,
            statuses: rx,
            launcher,
            _tmp: tmp,
        }
    }

    fn wait_for(rx: &Receiver<AgentStatus>, state: AgentState) -> AgentStatus {
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            if let Ok(s) = rx.recv_timeout(Duration::from_millis(200)) {
                if s.state == state {
                    return s;
                }
            }
        }
        panic!("never reached {state:?}");
    }

    #[test]
    fn it_starts_an_agent_when_none_runs_forwards_requests_and_stops_what_it_started() {
        let r = rig(false);
        let handle = r.link.spawn().unwrap();
        wait_for(&r.statuses, AgentState::Running);
        assert!(r.link.started_by_this_app());
        assert_eq!(r.launcher.launches.load(Ordering::SeqCst), 1);
        // A request round-trips.
        match r.link.call(Request::Settings).unwrap() {
            Reply::Settings(_) => {}
            other => panic!("{other:?}"),
        }
        // Live updates arrive as pushes without any window asking (the tray needs them).
        let end = Instant::now() + Duration::from_secs(15);
        while r.sink.live.load(Ordering::SeqCst) == 0 && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(r.sink.live.load(Ordering::SeqCst) > 0, "no live update arrived");
        // The agent's own errors keep their code.
        let err = r
            .link
            .call(Request::DiskChildren {
                scan: 99,
                node: None,
                offset: 0,
                limit: 1,
                min_fraction: 0.0,
            })
            .unwrap_err();
        assert_ne!(err.code, ErrorCode::AgentUnavailable);
        // Windows vote for a detail.
        r.link.set_detail("main", LiveDetail::Processes).unwrap();
        r.link.drop_window("main");
        // Quit: the agent this app started is asked to exit.
        assert!(r.link.quit());
        handle.join().unwrap();
        assert!(!r.link.dirs.discovery().exists(), "the agent removed its discovery file");
        assert_eq!(r.link.call(Request::Settings).unwrap_err().code, ErrorCode::AgentUnavailable);
    }

    #[test]
    fn a_request_made_before_the_first_connection_waits_for_it() {
        let r = rig(false);
        let handle = r.link.spawn().unwrap();
        // No waiting for the Running status: the window's first requests arrive while the agent is still starting.
        assert!(matches!(r.link.call(Request::Settings), Ok(Reply::Settings(_))));
        r.link.quit();
        handle.join().unwrap();
    }

    #[test]
    fn it_connects_to_a_running_agent_and_leaves_it_running_on_quit() {
        let r = rig(false);
        // Somebody else's agent (the login service's): not started through this app's launcher.
        let fake = Fake::new();
        let other = Running::start(r.link.dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        let handle = r.link.spawn().unwrap();
        wait_for(&r.statuses, AgentState::Running);
        assert_eq!(r.launcher.launches.load(Ordering::SeqCst), 0, "no second agent");
        assert!(!r.link.started_by_this_app());
        assert!(!r.link.quit(), "somebody else's agent stays");
        handle.join().unwrap();
        assert!(r.link.dirs.discovery().exists(), "it is still there");
        assert!(!other.stopped());
        other.stop();
    }

    #[test]
    fn a_service_installed_agent_survives_quit_even_if_this_app_started_it() {
        let r = rig(false);
        r.link.set_service_installed(true);
        let handle = r.link.spawn().unwrap();
        wait_for(&r.statuses, AgentState::Running);
        assert!(r.link.started_by_this_app());
        assert!(!r.link.quit());
        handle.join().unwrap();
        assert!(r.link.dirs.discovery().exists());
        r.launcher.finish(Duration::ZERO);
    }

    #[test]
    fn it_reconnects_after_the_agent_goes_away() {
        let r = rig(false);
        let handle = r.link.spawn().unwrap();
        wait_for(&r.statuses, AgentState::Running);
        r.launcher.finish(Duration::ZERO);
        let s = wait_for(&r.statuses, AgentState::Restarting);
        assert!(s.message.is_some() || s.state == AgentState::Restarting);
        wait_for(&r.statuses, AgentState::Running);
        assert!(r.link.call(Request::Settings).is_ok());
        r.link.quit();
        handle.join().unwrap();
    }

    /// An agent whose start blocks until the test opens the gate: what a cold, slow disk does to the real one (the
    /// Dyno 1.0 test VM took 50 s to read a 32 MB binary). Each launch is one "process" (a thread) that starts its
    /// agent once the gate opens.
    struct SlowLauncher {
        dirs: Dirs,
        gate: Arc<(Mutex<bool>, Condvar)>,
        launches: AtomicUsize,
        /// How often the link looked at the process: the progress of a wait, counted instead of timed.
        looks: AtomicUsize,
        ended: AtomicUsize,
        agents: Arc<Mutex<Vec<Running>>>,
        threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    }

    impl SlowLauncher {
        fn new(dirs: Dirs) -> Arc<SlowLauncher> {
            Arc::new(SlowLauncher {
                dirs,
                gate: Arc::new((Mutex::new(false), Condvar::new())),
                launches: AtomicUsize::new(0),
                looks: AtomicUsize::new(0),
                ended: AtomicUsize::new(0),
                agents: Arc::new(Mutex::new(Vec::new())),
                threads: Mutex::new(Vec::new()),
            })
        }

        fn open(&self) {
            *lock(&self.gate.0) = true;
            self.gate.1.notify_all();
        }

        fn running_agents(&self) -> usize {
            lock(&self.agents).iter().filter(|r| !r.stopped()).count()
        }

        fn stop_all(&self) {
            self.open();
            for t in std::mem::take(&mut *lock(&self.threads)) {
                let _ = t.join();
            }
            for r in std::mem::take(&mut *lock(&self.agents)) {
                r.stop();
            }
        }
    }

    struct SlowShared(Arc<SlowLauncher>);

    impl Launcher for SlowShared {
        fn launch(&self) -> Result<(), String> {
            let me = &self.0;
            me.launches.fetch_add(1, Ordering::SeqCst);
            let (gate, dirs, agents) = (me.gate.clone(), me.dirs.clone(), me.agents.clone());
            let t = std::thread::spawn(move || {
                let open = lock(&gate.0);
                drop(gate.1.wait_while(open, |o| !*o).unwrap_or_else(|e| e.into_inner()));
                let fake = Fake::new();
                if let Ok(r) = Running::start(dirs, fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))) {
                    lock(&agents).push(r);
                }
            });
            lock(&me.threads).push(t);
            Ok(())
        }
        fn child(&self) -> Child {
            self.0.looks.fetch_add(1, Ordering::SeqCst);
            if self.0.ended.load(Ordering::SeqCst) > 0 {
                Child::Exited { code: Some(0) }
            } else if self.0.launches.load(Ordering::SeqCst) > 0 {
                Child::Running { pid: std::process::id() }
            } else {
                Child::None
            }
        }
        fn finish(&self, _: Duration) {
            self.0.ended.fetch_add(1, Ordering::SeqCst);
            for r in lock(&self.0.agents).iter() {
                r.shared.request_stop();
            }
        }
    }

    fn slow_rig() -> (Arc<AgentLink>, Arc<SlowLauncher>, Receiver<AgentStatus>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let (tx, rx) = channel();
        let sink = Recorder::new(tx);
        let launcher = SlowLauncher::new(dirs.clone());
        let link = AgentLink::new(dirs, env!("CARGO_PKG_VERSION"), sink, Box::new(SlowShared(launcher.clone())));
        link.set_poll(Duration::from_millis(1));
        (link, launcher, rx, tmp)
    }

    /// Waits until `cond` holds or `bound` has passed; returns whether it held.
    fn until(bound: Duration, cond: impl Fn() -> bool) -> bool {
        let end = Instant::now() + bound;
        while !cond() {
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    #[test]
    fn a_slow_agent_start_is_waited_for_and_never_doubled() {
        let (link, launcher, statuses, _tmp) = slow_rig();
        let handle = link.spawn().unwrap();
        // The wait is measured in looks at the process, not in seconds (ER-12): the old link gave up after 50 looks and
        // started a second agent, so twice that many looks with the gate shut must still show exactly one launch. The
        // bound only keeps a broken link from hanging the test.
        assert!(
            until(Duration::from_secs(60), || launcher.looks.load(Ordering::SeqCst) >= 100),
            "the link never looked at the starting agent"
        );
        assert_eq!(
            launcher.launches.load(Ordering::SeqCst),
            1,
            "a second agent was started while the first was still starting"
        );
        launcher.open();
        wait_for(&statuses, AgentState::Running);
        assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
        assert_eq!(launcher.running_agents(), 1, "exactly one agent runs");
        assert!(link.started_by_this_app());
        assert!(link.quit(), "the agent this app started is stopped on quit");
        handle.join().unwrap();
        assert!(
            until(Duration::from_secs(10), || launcher.running_agents() == 0),
            "an agent outlived the quit"
        );
        launcher.stop_all();
    }

    #[test]
    fn quitting_while_the_agent_is_still_starting_ends_it() {
        let (link, launcher, _statuses, _tmp) = slow_rig();
        let handle = link.spawn().unwrap();
        assert!(until(Duration::from_secs(10), || launcher.launches.load(Ordering::SeqCst) == 1));
        link.quit();
        handle.join().unwrap();
        assert_eq!(
            launcher.ended.load(Ordering::SeqCst),
            1,
            "the starting agent was left behind as an orphan"
        );
        launcher.stop_all();
    }

    /// The OS service manager (launchd, systemd) over the fake platform: what it reports, and its agent, which it starts
    /// itself (never through the app's launcher) and starts again while another agent holds the data folder, as
    /// `KeepAlive` and `Restart=always` do with an agent that exits with code 3.
    struct FakeServiceManager {
        dirs: Dirs,
        state: Mutex<ServiceState>,
        agent: Mutex<Option<Running>>,
        refused: AtomicUsize,
        done: AtomicBool,
        thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    }

    impl FakeServiceManager {
        fn new(dirs: Dirs, installed: bool, supervises: bool) -> Arc<FakeServiceManager> {
            Arc::new(FakeServiceManager {
                dirs,
                state: Mutex::new(ServiceState { installed, supervises }),
                agent: Mutex::new(None),
                refused: AtomicUsize::new(0),
                done: AtomicBool::new(false),
                thread: Mutex::new(None),
            })
        }

        fn probe(self: &Arc<Self>) -> ServiceProbe {
            let me = self.clone();
            Arc::new(move || *lock(&me.state))
        }

        fn set(&self, installed: bool, supervises: bool) {
            *lock(&self.state) = ServiceState { installed, supervises };
        }

        /// Runs its job: starts the agent, again and again while it is refused; when the agent exits (asked to), its
        /// "process" ends (the lock is freed) and, while the service still supervises, it is started again.
        fn start_job(self: &Arc<Self>) {
            let me = self.clone();
            *lock(&self.thread) = Some(std::thread::spawn(move || {
                while !me.done.load(Ordering::SeqCst) {
                    let fake = Fake::new();
                    match Running::start(me.dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))) {
                        Ok(r) => {
                            *lock(&me.agent) = Some(r);
                            while !me.done.load(Ordering::SeqCst) && me.agent_running() {
                                // pace: how often the fake looks at its agent.
                                std::thread::sleep(Duration::from_millis(20));
                            }
                            if let Some(r) = lock(&me.agent).take() {
                                r.stop();
                            }
                            if !lock(&me.state).supervises {
                                return;
                            }
                        }
                        Err(_) => {
                            me.refused.fetch_add(1, Ordering::SeqCst);
                            // pace: the service manager's restart interval, shortened for the test.
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                }
            }));
        }

        fn agent_running(&self) -> bool {
            lock(&self.agent).as_ref().is_some_and(|r| !r.stopped())
        }

        fn stop(&self) {
            self.done.store(true, Ordering::SeqCst);
            if let Some(t) = lock(&self.thread).take() {
                let _ = t.join();
            }
            if let Some(r) = lock(&self.agent).take() {
                r.stop();
            }
        }
    }

    #[test]
    fn with_the_service_supervising_the_app_never_starts_an_agent_of_its_own() {
        let r = rig(false);
        let manager = FakeServiceManager::new(r.link.dirs.clone(), true, true);
        r.link.set_service_probe(manager.probe());
        let handle = r.link.spawn().unwrap();
        // The service's agent comes up after the app (a login with a slow disk): the app waits for it.
        manager.start_job();
        wait_for(&r.statuses, AgentState::Running);
        assert_eq!(
            r.launcher.launches.load(Ordering::SeqCst),
            0,
            "the app started an agent beside the service's"
        );
        assert!(!r.link.started_by_this_app());
        // Rule 53: a refresh with the service supervising ends nothing this app did not start.
        r.link.refresh_service();
        assert!(manager.agent_running() && r.link.connected());
        assert!(!r.link.quit(), "the service's agent stays");
        handle.join().unwrap();
        assert!(manager.agent_running());
        manager.stop();
    }

    #[test]
    fn turning_launch_at_login_on_hands_the_agent_over_to_the_service() {
        let r = rig(false);
        let manager = FakeServiceManager::new(r.link.dirs.clone(), false, false);
        r.link.set_service_probe(manager.probe());
        let handle = r.link.spawn().unwrap();
        wait_for(&r.statuses, AgentState::Running);
        assert!(r.link.started_by_this_app());
        // The switch: the service is registered and its manager starts its agent at once (refused while ours runs).
        manager.set(true, true);
        manager.start_job();
        r.link.refresh_service();
        assert!(
            until(Duration::from_secs(20), || manager.agent_running()
                && r.link.connected()
                && !r.link.started_by_this_app()),
            "the service's agent did not take over (refused {} times)",
            manager.refused.load(Ordering::SeqCst)
        );
        assert_eq!(r.launcher.child(), Child::Exited { code: Some(0) }, "the app's own agent was ended");
        assert_eq!(r.launcher.launches.load(Ordering::SeqCst), 1, "no agent of the app's own again");
        assert!(!r.link.quit(), "the service's agent stays");
        handle.join().unwrap();
        assert!(manager.agent_running());
        manager.stop();
    }

    #[test]
    fn turning_launch_at_login_off_brings_back_an_agent_of_the_apps_own() {
        let r = rig(false);
        let manager = FakeServiceManager::new(r.link.dirs.clone(), true, true);
        r.link.set_service_probe(manager.probe());
        manager.start_job();
        let handle = r.link.spawn().unwrap();
        wait_for(&r.statuses, AgentState::Running);
        assert!(!r.link.started_by_this_app());
        // The switch: the service is removed. This manager leaves its agent running (as an unregister that does not
        // stop the job would): the app asks it to exit over its connection and starts its own.
        manager.set(false, false);
        r.link.refresh_service();
        assert!(
            until(Duration::from_secs(20), || r.link.connected() && r.link.started_by_this_app()),
            "no agent of the app's own after the service was removed"
        );
        assert!(!manager.agent_running(), "the former service's agent was asked to exit");
        assert_eq!(r.launcher.launches.load(Ordering::SeqCst), 1);
        assert!(r.link.quit(), "the app's own agent is stopped on quit");
        handle.join().unwrap();
        manager.stop();
    }

    /// A launcher that does what the test says, counting what the link asks of it: after a launch the process is in
    /// `after_launch` (running and silent, or exited with a code), until `finish` ends it.
    struct StubLauncher {
        launches: AtomicUsize,
        looks: AtomicUsize,
        finished: AtomicUsize,
        after_launch: Child,
        state: Mutex<Child>,
    }

    impl StubLauncher {
        fn new(after_launch: Child) -> Arc<StubLauncher> {
            Arc::new(StubLauncher {
                launches: AtomicUsize::new(0),
                looks: AtomicUsize::new(0),
                finished: AtomicUsize::new(0),
                after_launch,
                state: Mutex::new(Child::None),
            })
        }

        fn launches(&self) -> usize {
            self.launches.load(Ordering::SeqCst)
        }
    }

    struct StubShared(Arc<StubLauncher>);

    impl Launcher for StubShared {
        fn launch(&self) -> Result<(), String> {
            let me = &self.0;
            if matches!(*lock(&me.state), Child::Running { .. }) {
                return Ok(());
            }
            me.launches.fetch_add(1, Ordering::SeqCst);
            *lock(&me.state) = me.after_launch;
            Ok(())
        }
        fn child(&self) -> Child {
            self.0.looks.fetch_add(1, Ordering::SeqCst);
            *lock(&self.0.state)
        }
        fn finish(&self, _: Duration) {
            self.0.finished.fetch_add(1, Ordering::SeqCst);
            *lock(&self.0.state) = Child::Exited { code: None };
        }
    }

    struct StubRig {
        link: Arc<AgentLink>,
        launcher: Arc<StubLauncher>,
        sink: Arc<Recorder>,
        statuses: Receiver<AgentStatus>,
        _tmp: tempfile::TempDir,
    }

    fn stub_rig(after_launch: Child) -> StubRig {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let (tx, rx) = channel();
        let sink = Recorder::new(tx);
        let launcher = StubLauncher::new(after_launch);
        let link = AgentLink::new(
            dirs,
            env!("CARGO_PKG_VERSION"),
            sink.clone(),
            Box::new(StubShared(launcher.clone())),
        );
        link.set_poll(Duration::from_millis(1));
        StubRig {
            link,
            launcher,
            sink,
            statuses: rx,
            _tmp: tmp,
        }
    }

    /// A discovery file of this build that names a dead agent: nothing answers at its endpoint.
    fn stale_file() -> Discovery {
        Discovery {
            endpoint: "dyno-test-nothing-listens-here".into(),
            token: "00".into(),
            pid: 1,
            started: 0,
            version: env!("CARGO_PKG_VERSION").into(),
            protocol: PROTOCOL_VERSION,
        }
    }

    /// An agent that holds the data folder (and its lock) and has been started by somebody else.
    fn other_agent(dirs: &Dirs) -> Running {
        let fake = Fake::new();
        Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap()
    }

    /// Every status up to and including the first with `state`.
    fn collect_until(rx: &Receiver<AgentStatus>, state: AgentState) -> Vec<AgentStatus> {
        let mut seen = Vec::new();
        let end = Instant::now() + Duration::from_secs(60);
        while Instant::now() < end {
            if let Ok(s) = rx.recv_timeout(Duration::from_millis(200)) {
                let done = s.state == state;
                seen.push(s);
                if done {
                    return seen;
                }
            }
        }
        panic!("never reached {state:?}: {seen:?}");
    }

    fn drained(rx: &Receiver<AgentStatus>) -> Vec<AgentStatus> {
        rx.try_iter().collect()
    }

    #[test]
    fn a_discovery_file_counts_as_ready_only_if_written_since_the_wait_began() {
        let d = Discovery {
            started: 1_000,
            ..stale_file()
        };
        assert!(written_since(&d, 1_000));
        assert!(written_since(&d, 999));
        assert!(
            !written_since(&d, 1_001),
            "a file from before the wait is not the agent we wait for"
        );
    }

    #[test]
    fn a_stale_discovery_file_beside_a_slow_lock_holder_is_not_ready_and_launches_nothing_more() {
        let r = stub_rig(Child::Exited {
            code: Some(dyno_agent::cli::EXIT_ALREADY_RUNNING),
        });
        // Another agent holds the data folder but is slow to write its file; the file there is an old agent's.
        let holder = other_agent(&r.link.dirs);
        let fresh = holder.discovery.clone();
        stale_file().write(&r.link.dirs.discovery()).unwrap();
        let handle = r.link.spawn().unwrap();
        assert!(until(Duration::from_secs(60), || r.link.looks() >= 100), "the link never waited");
        assert_eq!(
            r.launcher.launches(),
            1,
            "the old file made the link launch agents that exit at once"
        );
        let seen = drained(&r.statuses);
        assert!(
            seen.iter().all(|s| !matches!(s.state, AgentState::Failed | AgentState::Restarting)),
            "a false outage was reported: {seen:?}"
        );
        // The holder writes its file: the link connects, still without another launch.
        fresh.write(&r.link.dirs.discovery()).unwrap();
        wait_for(&r.statuses, AgentState::Running);
        assert_eq!(r.launcher.launches(), 1);
        r.link.shutdown(false);
        handle.join().unwrap();
        holder.stop();
    }

    #[test]
    fn an_agent_that_never_answers_is_reported_with_retry_and_retry_ends_that_process_and_launches_again() {
        let r = stub_rig(Child::Running { pid: 4242 });
        let handle = r.link.spawn().unwrap();
        assert!(
            until(Duration::from_secs(60), || r.sink.stalled_with(Stall::AgentNotAnswering)),
            "no stall was reported for an agent that never wrote its discovery file"
        );
        assert_eq!(r.launcher.launches(), 1, "it keeps waiting, it does not start another");
        assert_eq!(r.launcher.finished.load(Ordering::SeqCst), 0);
        // Retry: the held process is ended through its handle, then launched again.
        r.link.retry();
        assert!(
            until(Duration::from_secs(60), || r.launcher.launches() == 2),
            "Retry did not launch the agent again"
        );
        assert_eq!(
            r.launcher.finished.load(Ordering::SeqCst),
            1,
            "the old process was not ended through the launcher"
        );
        r.link.shutdown(true);
        handle.join().unwrap();
        assert_eq!(r.launcher.launches(), 2);
    }

    #[test]
    fn another_agent_that_holds_the_folder_without_answering_is_reported_with_retry() {
        let r = stub_rig(Child::Exited {
            code: Some(dyno_agent::cli::EXIT_ALREADY_RUNNING),
        });
        // As after a replace: the old agent's file is gone, but it still holds the folder.
        let holder = other_agent(&r.link.dirs);
        let fresh = holder.discovery.clone();
        let _ = std::fs::remove_file(r.link.dirs.discovery());
        let handle = r.link.spawn().unwrap();
        assert!(
            until(Duration::from_secs(60), || r.sink.stalled_with(Stall::OtherAgentHolds)),
            "no stall was reported while another agent holds the data folder"
        );
        assert_eq!(r.launcher.launches(), 1);
        // Retry runs the connection steps again (and finds the holder still there).
        r.link.retry();
        assert!(
            until(Duration::from_secs(60), || r.launcher.launches() == 2),
            "Retry did not run establish again"
        );
        fresh.write(&r.link.dirs.discovery()).unwrap();
        wait_for(&r.statuses, AgentState::Running);
        r.link.shutdown(false);
        handle.join().unwrap();
        holder.stop();
    }

    #[test]
    fn a_login_service_that_starts_no_agent_is_waited_for_then_replaced_by_the_apps_own_with_a_note() {
        let r = rig(false);
        r.link.set_poll(Duration::from_millis(1));
        // It says it supervises; nothing ever runs (switched off in Login Items, a job launchd dropped, a unit that fails).
        let manager = FakeServiceManager::new(r.link.dirs.clone(), true, true);
        r.link.set_service_probe(manager.probe());
        let handle = r.link.spawn().unwrap();
        let seen = collect_until(&r.statuses, AgentState::Running);
        assert_eq!(
            r.launcher.launches.load(Ordering::SeqCst),
            1,
            "the app never started an agent of its own"
        );
        assert!(r.link.started_by_this_app());
        assert!(
            r.sink.stalled_with(Stall::WaitingForService),
            "no stall was reported while waiting for the service"
        );
        // The note says what happened (it is also in the log).
        assert!(
            seen.iter()
                .filter_map(|s| s.message.as_deref())
                .any(|m| m.contains("did not start")),
            "{seen:?}"
        );
        // The hand-over does not take the app's agent away again: the service has shown it starts nothing.
        r.link.refresh_service();
        assert!(r.link.connected() && matches!(r.launcher.child(), Child::Running { .. }));
        assert_eq!(r.launcher.launches.load(Ordering::SeqCst), 1);
        // A registered service keeps its agent on quit, so the app's own is ended through the launcher.
        r.link.quit();
        handle.join().unwrap();
        r.launcher.finish(Duration::ZERO);
    }

    #[test]
    fn requests_fail_fast_with_agent_unavailable_while_not_connected() {
        let r = rig(true);
        assert_eq!(r.link.call(Request::Settings).unwrap_err().code, ErrorCode::AgentUnavailable);
        assert!(!r.link.connected());
    }

    #[test]
    fn an_agent_that_cannot_be_started_is_reported_with_a_reason() {
        let r = rig(true);
        let handle = r.link.spawn().unwrap();
        let s = wait_for(&r.statuses, AgentState::Restarting);
        assert!(s.message.unwrap().contains("no agent program"));
        r.link.shutdown(false);
        handle.join().unwrap();
    }
}
