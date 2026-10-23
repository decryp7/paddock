//! The link to the agent (ADR 0002): finds the running agent through `agent.json`, starts one when there is none,
//! keeps one dyno-ipc connection, forwards the webview's requests and fans the agent's pushes out to a [`Sink`].
//! It reconnects with backoff when the connection drops and replaces an agent of another build.
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Every request fails after this long.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a request that reads the system logs may take (the unified log of a busy Mac takes a minute or two).
pub const LOG_READ_TIMEOUT: Duration = Duration::from_secs(150);

/// How long to wait for the agent's answer to `request`.
pub fn timeout_for(request: &Request) -> Duration {
    match request {
        Request::RescanIssues { .. } => LOG_READ_TIMEOUT,
        _ => REQUEST_TIMEOUT,
    }
}
/// How long a freshly started agent has to write `agent.json` and accept a connection.
pub const SPAWN_WAIT: Duration = Duration::from_secs(5);
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
    /// The connection state changed.
    fn agent(&self, status: AgentStatus);
    /// A connection was made: the shell reads what it needs from the agent (settings, for the tray).
    fn connected(&self, _link: &Arc<AgentLink>) {}
}

/// Starts the agent process.
pub trait Launcher: Send + Sync {
    /// Starts `dyno-agent run` detached. Returns at once; the link waits for the discovery file.
    fn launch(&self) -> Result<(), String>;
    /// Waits briefly for a process this launcher started to exit (after `Shutdown`), killing it when it does not.
    fn finish(&self, wait: Duration);
}

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
    stopping: Mutex<bool>,
    wake: Condvar,
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
            stopping: Mutex::new(false),
            wake: Condvar::new(),
        })
    }

    /// Runs the connection loop on its own thread.
    pub fn spawn(self: &Arc<Self>) -> std::io::Result<std::thread::JoinHandle<()>> {
        let me = self.clone();
        self.running.store(true, Ordering::SeqCst);
        std::thread::Builder::new().name("dyno-link".into()).spawn(move || me.run())
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

    /// Stops the loop and closes the connection. Returns whether the agent was asked to exit (see
    /// [`stop_agent_on_quit`]).
    pub fn shutdown(&self, stop_agent: bool) -> bool {
        let mut asked = false;
        if stop_agent {
            asked = self.call(Request::Shutdown).is_ok();
        }
        *lock(&self.stopping) = true;
        self.wake.notify_all();
        if let Some(p) = lock(&self.peer).take() {
            p.close();
        }
        if asked {
            self.launcher.finish(Duration::from_secs(3));
        }
        asked
    }

    /// The quit decision for this link.
    pub fn quit(&self) -> bool {
        if self.quit_done.swap(true, Ordering::SeqCst) {
            return false;
        }
        let stop = stop_agent_on_quit(self.started_by_this_app(), self.service_installed.load(Ordering::SeqCst));
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
                            Ok(p) => return Ok(p),
                            // A stale file (its process is gone): treat as no agent.
                            Err(e) => {
                                logging::info(&format!("stale agent.json: {e}"));
                                let _ = std::fs::remove_file(self.dirs.discovery());
                            }
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
                    if Discovery::read(&self.dirs.discovery()).is_some() {
                        // Its process is gone but the file stayed.
                        let _ = std::fs::remove_file(self.dirs.discovery());
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

    /// Starts the agent and waits for it to be reachable (a connection is made by the next pass of [`establish`]).
    fn start_agent(&self) -> Result<(), String> {
        logging::info("starting the agent");
        self.launcher.launch()?;
        self.started_by_us.store(true, Ordering::SeqCst);
        let end = Instant::now() + SPAWN_WAIT;
        while Instant::now() < end {
            if Discovery::read(&self.dirs.discovery()).is_some() {
                return Ok(());
            }
            if self.is_stopping() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("The Dyno agent did not start within 5 seconds.".into())
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// Starts the real agent program detached (no console, its own process group, not tied to the app's lifetime).
pub struct ProcessLauncher {
    exe: Box<dyn Fn() -> Result<std::path::PathBuf, String> + Send + Sync>,
    child: Mutex<Option<std::process::Child>>,
}

impl ProcessLauncher {
    /// A launcher whose program is found by `exe` each time (so a reinstalled app is picked up).
    pub fn new(exe: impl Fn() -> Result<std::path::PathBuf, String> + Send + Sync + 'static) -> ProcessLauncher {
        ProcessLauncher {
            exe: Box::new(exe),
            child: Mutex::new(None),
        }
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&self) -> Result<(), String> {
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
        let mut slot = lock(&self.child);
        if let Some(mut old) = slot.replace(child) {
            let _ = old.try_wait();
        }
        Ok(())
    }

    fn finish(&self, wait: Duration) {
        let Some(mut child) = lock(&self.child).take() else { return };
        let end = Instant::now() + wait;
        while Instant::now() < end {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
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
    fn only_reading_the_system_logs_may_take_minutes() {
        use dyno_core::issues::IssuesQuery;
        assert_eq!(
            timeout_for(&Request::RescanIssues {
                query: IssuesQuery::default()
            }),
            LOG_READ_TIMEOUT
        );
        assert_eq!(
            timeout_for(&Request::Issues {
                query: IssuesQuery::default()
            }),
            REQUEST_TIMEOUT
        );
        assert_eq!(timeout_for(&Request::Settings), REQUEST_TIMEOUT);
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
    }

    /// Starts an in-process agent over the fake platform when asked (what the real launcher does with a process).
    struct InProcessLauncher {
        dirs: Dirs,
        started: Mutex<Option<Running>>,
        launches: AtomicUsize,
        fail: bool,
    }

    impl Launcher for InProcessLauncher {
        fn launch(&self) -> Result<(), String> {
            self.launches.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("no agent program".into());
            }
            let fake = Fake::new();
            fake.add_process(process(100, "busy", 60.0));
            let running = Running::start(self.dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(())))?;
            *lock(&self.started) = Some(running);
            Ok(())
        }
        fn finish(&self, _: Duration) {
            if let Some(r) = lock(&self.started).take() {
                r.stop();
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
        fn finish(&self, w: Duration) {
            self.0.finish(w)
        }
    }

    fn rig(fail: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let (tx, rx) = channel();
        let sink = Arc::new(Recorder {
            live: AtomicUsize::new(0),
            statuses: tx,
        });
        let launcher = Arc::new(InProcessLauncher {
            dirs: dirs.clone(),
            started: Mutex::new(None),
            launches: AtomicUsize::new(0),
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
            .call(Request::RestoreQuarantine {
                id: "does-not-exist".into(),
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
        r.launcher.launch().unwrap();
        let before = r.launcher.launches.load(Ordering::SeqCst);
        let handle = r.link.spawn().unwrap();
        wait_for(&r.statuses, AgentState::Running);
        assert_eq!(r.launcher.launches.load(Ordering::SeqCst), before, "no second agent");
        assert!(!r.link.started_by_this_app());
        assert!(!r.link.quit(), "somebody else's agent stays");
        handle.join().unwrap();
        assert!(r.link.dirs.discovery().exists(), "it is still there");
        r.launcher.finish(Duration::ZERO);
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
