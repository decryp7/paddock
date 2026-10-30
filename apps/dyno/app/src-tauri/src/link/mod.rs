//! The link to the agent (ADR 0002, ADR 0025): finds the running agent, starts one when there is none, keeps one dyno-ipc
//! connection, forwards the webview's requests and fans the agent's pushes out to a [`Sink`].
//!
//! The supervision is a pure state machine ([`machine::Supervisor`]) with one owner thread ([`driver`], the generic
//! mechanism in [`crate::supervise`]): the phase says what the link waits for, every answer from the world carries the number
//! of the thing it is about, and every transition is published so the chip, a request and a test wait on a state and not
//! on a flag. [`AgentLink`] is the facade: its methods send events or read the published state.
//!
//! One agent per data folder: the agent itself refuses to start while another holds the folder's lock
//! ([`dyno_agent::instance`]), and the link never starts a second one while the one it started is held. It waits for a
//! starting agent on its run state, not on a clock (ADR 0031). Quitting ends exactly the process this app started, through
//! the handle it holds (rule 53), and decides that once.
//!
//! The login service: while launchd or systemd supervises the agent, the app starts no agent of its own and waits for the
//! service's. When the service is turned on while the app's own agent runs, the app hands over: it asks its agent to exit
//! and ends that process, and the service's agent, refused until then, takes the data folder. When it is turned off, the app
//! asks the former service's agent to exit over its connection (never a kill: it did not start it) and starts its own once
//! the data folder is free. A Windows `Run` entry starts nothing until the next logon, so there the app keeps its own agent.

pub mod driver;
pub mod machine;
pub mod ports;

#[cfg(test)]
mod model;
#[cfg(test)]
mod tests;

use crate::dto::{AgentState, AgentStatus, CommandError, ErrorCode};
use crate::logging;
use crate::supervise::{run_owner, spawn_ticker, Transitions};
use dyno_agent::discovery::Discovery;
use dyno_agent::paths::Dirs;
use dyno_core::protocol::{LiveDetail, LiveUpdate, Message, Reply, Request};
use dyno_core::{activity::ActivityEntry, alerts::Alert};
use dyno_ipc::{IpcError, Peer};
use machine::{Event, Snapshot, Supervisor};
use paddock_proc::StallCause;
pub use ports::{agent_exe, Child, Launcher, ProcessLauncher};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

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

// pace: how often the world is looked at while the link waits; it bounds nothing. A backoff of n seconds is n times
// `machine::TICKS_PER_SECOND` ticks.
const PACE: Duration = Duration::from_millis(100);
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
    /// A look at the system (the login service, the agent's files) or a connection is not answered.
    ServiceNotAnswering,
    /// Starting the agent fails again and again.
    AgentKeepsFailing,
}

impl From<StallCause> for Stall {
    /// Dyno's chip for a shared stall cause: a note or a stuck start is the agent not answering.
    fn from(cause: StallCause) -> Stall {
        match cause {
            StallCause::WaitingForSupervisor => Stall::WaitingForService,
            StallCause::HeldByOther => Stall::OtherAgentHolds,
            _ => Stall::AgentNotAnswering,
        }
    }
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

/// The connection manager: the facade over the supervisor. Its methods send an event to the owner thread or read what it
/// published; none of them decides anything about the agent.
pub struct AgentLink {
    pub(crate) dirs: Dirs,
    pub(crate) app_version: String,
    pub(crate) sink: Arc<dyn Sink>,
    pub(crate) launcher: Arc<dyn Launcher>,
    /// What requests go to (set by the supervisor's `UsePeer`).
    pub(crate) peer: Mutex<Option<Arc<Peer<Message>>>>,
    votes: Mutex<DetailVotes>,
    /// The detail last sent over the current connection.
    sent: Mutex<Option<LiveDetail>>,
    /// Serialises the syncs of the subscription.
    syncing: Mutex<()>,
    probe: Mutex<Option<ServiceProbe>>,
    events: Sender<Event>,
    inbox: Mutex<Option<Receiver<Event>>>,
    transitions: Transitions<Snapshot>,
    pace: Duration,
    tuning: machine::Tuning,
    /// `quit` was called (its answer is for the first call only).
    quit_called: AtomicBool,
}

impl AgentLink {
    /// A link for the agent in `dirs`.
    pub fn new(dirs: Dirs, app_version: &str, sink: Arc<dyn Sink>, launcher: Box<dyn Launcher>) -> Arc<AgentLink> {
        AgentLink::paced(dirs, app_version, sink, launcher, PACE)
    }

    /// A link that looks at the world every `pace` (tests shorten it; the thresholds are counted in ticks, so the pace
    /// changes how fast, never what).
    pub fn paced(dirs: Dirs, app_version: &str, sink: Arc<dyn Sink>, launcher: Box<dyn Launcher>, pace: Duration) -> Arc<AgentLink> {
        AgentLink::tuned(dirs, app_version, sink, launcher, pace, machine::Tuning::default())
    }

    /// [`AgentLink::paced`] with other counts (see [`machine::Tuning`]): a test that must not meet a stall, however loaded the
    /// machine is, counts a wait as long.
    pub fn tuned(
        dirs: Dirs,
        app_version: &str,
        sink: Arc<dyn Sink>,
        launcher: Box<dyn Launcher>,
        pace: Duration,
        tuning: machine::Tuning,
    ) -> Arc<AgentLink> {
        let (events, inbox) = channel();
        Arc::new(AgentLink {
            dirs,
            app_version: app_version.to_string(),
            sink,
            launcher: Arc::from(launcher),
            peer: Mutex::new(None),
            votes: Mutex::new(DetailVotes::default()),
            sent: Mutex::new(None),
            syncing: Mutex::new(()),
            probe: Mutex::new(None),
            events,
            inbox: Mutex::new(Some(inbox)),
            transitions: Transitions::new(Supervisor::new().snapshot_now()),
            pace,
            tuning,
            quit_called: AtomicBool::new(false),
        })
    }

    /// Runs the supervisor on its own thread (the owner) and its pace.
    pub fn spawn(self: &Arc<Self>) -> std::io::Result<std::thread::JoinHandle<()>> {
        let inbox = lock(&self.inbox)
            .take()
            .ok_or_else(|| std::io::Error::other("the link is already running"))?;
        let mut executor = driver::Executor::new(self.clone(), self.events.clone())?;
        let owner = run_owner("dyno-link", Supervisor::new(), inbox, self.transitions.clone(), move |command| {
            executor.execute(command)
        })?;
        let _ = self.events.send(Event::Begin);
        spawn_ticker(
            "dyno-pace",
            self.pace,
            self.events.clone(),
            || Event::Tick,
            self.transitions.clone(),
        )?;
        Ok(owner)
    }

    /// What the supervisor published.
    pub fn transitions(&self) -> &Transitions<Snapshot> {
        &self.transitions
    }

    /// The Retry of the status chip: acts on the wait in progress (ends and relaunches the agent this app started and
    /// that does not answer, starts the app's own agent when the login service starts none, looks again when another
    /// agent holds the data folder). It names the stall shown now, so a Retry for a wait that is over does nothing.
    pub fn retry(&self) {
        let stall = self.transitions.current().stall.map(|(id, _)| id);
        let _ = self.events.send(Event::Retry { stall });
    }

    /// How to ask the OS about the login service. Without one, no service supervises the agent.
    pub fn set_service_probe(&self, probe: ServiceProbe) {
        *lock(&self.probe) = Some(probe);
    }

    pub(crate) fn probe(&self) -> Option<ServiceProbe> {
        lock(&self.probe).clone()
    }

    /// The login service may have changed (the switch was turned, or a connection was made): the supervisor asks the OS,
    /// then hands the agent over to the service, or takes it back from a service that was removed. Returns at once.
    pub fn refresh_service(&self) {
        let _ = self.events.send(Event::ServiceChanged);
    }

    /// Records whether a service keeps the agent alive (decides what quitting does).
    pub fn set_service_installed(&self, installed: bool) {
        let _ = self.events.send(Event::ServiceInstalled(installed));
    }

    /// Whether this app started the agent it talks to.
    pub fn started_by_this_app(&self) -> bool {
        self.transitions.current().started_by_this_app()
    }

    /// Whether the link is connected.
    pub fn connected(&self) -> bool {
        self.transitions.current().connected()
    }

    /// Waits until the link is connected; false when `bound` passed first (a caller with a recorded product limit).
    pub fn wait_connected(&self, bound: Duration) -> bool {
        self.transitions.wait_until_for(bound, Snapshot::connected).is_some()
    }

    // ------------------------------------------------------------------------------------------------ requests

    /// Sends one request and returns the reply. A `Subscribe` is not forwarded as is: it is a window's wish
    /// ([`AgentLink::set_detail`]). Never call it on the owner thread: it may wait for the connection.
    pub fn call(&self, request: Request) -> Result<Reply, CommandError> {
        debug_assert!(
            std::thread::current().name() != Some("dyno-link"),
            "a request on the supervisor's owner thread would block it"
        );
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

    /// The connection. Before the very first one has been made, a request waits for it, or for a shown stall or a failure
    /// (the window asks for data the moment it opens, usually before the agent has answered; the wait is on the state, not
    /// on a clock). After a first connection, requests fail fast instead.
    fn peer_or_wait(&self) -> Option<Arc<Peer<Message>>> {
        // A connection the supervisor has published (`UsePeer`) is used at once. Its snapshot follows the step that made it,
        // so a request that starts in between (the worker that subscribes right after a connect) must not read the older
        // snapshot, which may say "settled, not connected" (a stall had been shown) and fail it as unavailable.
        let live = || lock(&self.peer).clone().filter(|p| !p.is_closed());
        if let Some(p) = live() {
            return Some(p);
        }
        // (a link that was spawned but has not taken its first step is still Idle: that is not an answer)
        let spawned = lock(&self.inbox).is_none();
        let snapshot = self
            .transitions
            .wait_until(|s| s.settled() && !(spawned && s.phase == machine::Phase::Idle));
        if !snapshot.connected() {
            return None;
        }
        live()
    }

    /// A window's wish for the live detail; the connection asks for the most any window wants.
    pub fn set_detail(&self, window: &str, detail: LiveDetail) -> Result<(), CommandError> {
        lock(&self.votes).set(window, detail);
        self.sync_detail()
    }

    /// A window is gone: it no longer wants anything. Returns at once (it is called from the UI thread): the subscription is
    /// brought up to date on a worker, because that is a request to the agent.
    pub fn drop_window(self: &Arc<Self>, window: &str) {
        lock(&self.votes).remove(window);
        let me = self.clone();
        let _ = std::thread::Builder::new().name("dyno-detail".into()).spawn(move || {
            let _ = me.sync_detail();
        });
    }

    /// Brings the agent's subscription up to what the windows want. Syncs are serialised (one request at a time, so two
    /// cannot cross on the wire), but `sent` itself is held only to read or set it, never across the request (ER-17).
    fn sync_detail(&self) -> Result<(), CommandError> {
        let _one_at_a_time = lock(&self.syncing);
        let wanted = lock(&self.votes).effective();
        if *lock(&self.sent) == Some(wanted) {
            return Ok(());
        }
        self.call(Request::Subscribe { detail: wanted })?;
        *lock(&self.sent) = Some(wanted);
        Ok(())
    }

    /// A connection was made (run off the owner thread): the subscription starts anew, and the shell reads what it needs.
    pub(crate) fn on_connected(self: &Arc<Self>) {
        *lock(&self.sent) = None;
        if let Err(e) = self.sync_detail() {
            logging::info(&format!("could not subscribe: {}", e.message));
        }
        self.sink.connected(self);
    }

    /// The quit decision for this link: ends the agent this app started unless a service keeps it, decided once at the
    /// moment of the call (see [`stop_agent_on_quit`]). Waits until the supervisor has stopped and returns whether an agent
    /// of this app was stopped. Only the first call answers `true`.
    pub fn quit(&self) -> bool {
        if self.quit_called.swap(true, Ordering::SeqCst) {
            return false;
        }
        if lock(&self.inbox).is_some() {
            return false; // never run: nothing to stop
        }
        let _ = self.events.send(Event::Quit);
        let snapshot = self.transitions.wait_until(Snapshot::stopped);
        snapshot.outcome().is_some_and(|o| o.stopped_own)
    }
}
