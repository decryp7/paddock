//! The link's tests. The unit tests of the pure parts and of the process launcher are the old ones; the integration tests
//! (the old 28 link tests are the specification of behaviour, ADR 0025 section 6) drive an `AgentLink` over the fake platform
//! and wait on the transitions it publishes, never on a flag and never on a sleep.

use super::machine::Phase;
use super::*;
use dyno_agent::Running;
use dyno_platform::fake::{process, Fake};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Condvar;
use std::time::Instant;

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

/// How fast the tests' links look at the world (the thresholds are counted in ticks, so only the speed changes).
const FAST: Duration = Duration::from_millis(1);

/// A hang guard for the waits below: well over the slowest wait measured under load and under CI's 60 s test kill
/// (family hang guard, 40 s). It bounds the harness only; nothing the link does depends on it.
const HANG_GUARD: Duration = Duration::from_secs(40);

/// Waits on the link's published transitions until `cond` holds.
fn wait_state(link: &AgentLink, what: &str, cond: impl Fn(&Snapshot) -> bool) -> Snapshot {
    link.transitions()
        .wait_until_for(HANG_GUARD, cond)
        .unwrap_or_else(|| panic!("never reached: {what}; the link is at {:?}", link.transitions().current()))
}

/// How many looks a test waits for while an agent holds the lock: each such look takes about 0.4 s (the paced re-check of a held
/// lock, `paddock_proc::HELD_CHECKS` x `HELD_TICK`), so a test counts few of them instead of timing them.
const LOOKS_WHILE_HELD: u64 = 3;

/// Waits until `more` further looks at the world have been answered (the link's published counter): a wait that lets the
/// link keep looking, counted instead of timed.
fn let_it_look(link: &AgentLink, more: u64) {
    let target = link.transitions().current().looks + more;
    wait_state(link, "more looks", |s| s.looks >= target);
}

/// `refresh_service` and wait for the look it asked for to be answered (and so for what it decided to be done).
fn refresh_and_wait(link: &AgentLink) {
    let seen = link.transitions().current().changes_answered;
    link.refresh_service();
    wait_state(link, "the look after the service changed", |s| s.changes_answered > seen);
}

/// Waits until a fake's own state (a counter, a thread's progress) satisfies `cond`. Only for state the link cannot publish.
fn until(bound: Duration, cond: impl Fn() -> bool) -> bool {
    let end = Instant::now() + bound;
    while !cond() {
        if Instant::now() >= end {
            return false;
        }
        // pace: how often the fake's state is looked at; the bound is the harness's hang guard.
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

/// Starts an in-process agent over the fake platform when asked (what the real launcher does with a process).
/// Its "process" is this test process: the agent's discovery file names `std::process::id()`.
struct InProcessLauncher {
    dirs: Dirs,
    started: Mutex<Option<Running>>,
    launches: AtomicUsize,
    exited: std::sync::atomic::AtomicBool,
    fail: bool,
    /// Why each launch that did not start an agent failed (for a failing assertion's message).
    refused: Mutex<Vec<String>>,
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
        let running = Running::start(self.dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(())))
            .inspect_err(|e| lock(&self.refused).push(e.to_string()))?;
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

/// Counts that never show a stall within a test: the pace is 1 ms, so the production note (50 ticks) would be 50 ms of wall
/// time, which a loaded machine easily exceeds, and a request that waits "for the connection or a stall" would then fail by
/// design. Tests about a stall use the production counts.
fn patient() -> super::machine::Tuning {
    super::machine::Tuning {
        note_after: u32::MAX,
        ..super::machine::Tuning::default()
    }
}

fn rig(fail: bool) -> Rig {
    rig_with(fail, patient())
}

fn rig_with(fail: bool, tuning: super::machine::Tuning) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let (tx, rx) = channel();
    let sink = Recorder::new(tx);
    let launcher = Arc::new(InProcessLauncher {
        dirs: dirs.clone(),
        started: Mutex::new(None),
        launches: AtomicUsize::new(0),
        exited: std::sync::atomic::AtomicBool::new(false),
        fail,
        refused: Mutex::new(Vec::new()),
    });
    let link = AgentLink::tuned(
        dirs,
        env!("CARGO_PKG_VERSION"),
        sink.clone(),
        Box::new(Shared(launcher.clone())),
        FAST,
        tuning,
    );
    Rig {
        link,
        sink,
        statuses: rx,
        launcher,
        _tmp: tmp,
    }
}

fn wait_for(rx: &Receiver<AgentStatus>, state: AgentState) -> AgentStatus {
    let end = Instant::now() + HANG_GUARD;
    while Instant::now() < end {
        if let Ok(s) = rx.recv_timeout(Duration::from_millis(200)) {
            if s.state == state {
                return s;
            }
        }
    }
    panic!("never reached {state:?}");
}

/// Waits for the Running status and for the link to publish that it is connected (the status goes out through the
/// notifier thread, the snapshot after the step's commands).
fn running(link: &AgentLink, rx: &Receiver<AgentStatus>) {
    wait_state(link, "connected", |s| s.connected());
    wait_for(rx, AgentState::Running);
}

#[test]
fn it_starts_an_agent_when_none_runs_forwards_requests_and_stops_what_it_started() {
    let r = rig(false);
    let handle = r.link.spawn().unwrap();
    running(&r.link, &r.statuses);
    assert!(r.link.started_by_this_app(), "{:?}", r.link.transitions().current());
    assert_eq!(
        r.launcher.launches.load(Ordering::SeqCst),
        1,
        "launches refused: {:?}; the link is at {:?}",
        lock(&r.launcher.refused),
        r.link.transitions().current()
    );
    // A request round-trips.
    match r.link.call(Request::Settings).unwrap() {
        Reply::Settings(_) => {}
        other => panic!("{other:?}"),
    }
    // Live updates arrive as pushes without any window asking (the tray needs them).
    assert!(
        until(HANG_GUARD, || r.sink.live.load(Ordering::SeqCst) > 0),
        "no live update arrived"
    );
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
    // No waiting for the Running status: the window's first requests arrive while the agent is still starting. The request
    // waits on the state (connected, or a shown stall), not on a clock.
    let result = r.link.call(Request::Settings);
    assert!(
        matches!(result, Ok(Reply::Settings(_))),
        "{result:?}; the link is at {:?}; statuses so far: {:?}",
        r.link.transitions().current(),
        r.statuses.try_iter().collect::<Vec<_>>()
    );
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
    running(&r.link, &r.statuses);
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
    running(&r.link, &r.statuses);
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
    running(&r.link, &r.statuses);
    r.launcher.finish(Duration::ZERO);
    let s = wait_for(&r.statuses, AgentState::Restarting);
    assert!(s.message.is_some() || s.state == AgentState::Restarting);
    running(&r.link, &r.statuses);
    assert!(r.link.call(Request::Settings).is_ok());
    r.link.quit();
    handle.join().unwrap();
}

/// An agent whose start blocks until the test opens the gate: what a cold, slow disk does to the real one (the Dyno 1.0
/// test VM took 50 s to read a 32 MB binary). Each launch is one "process" (a thread) that starts its agent once the gate
/// opens.
struct SlowLauncher {
    dirs: Dirs,
    gate: Arc<(Mutex<bool>, Condvar)>,
    launches: AtomicUsize,
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
    let link = AgentLink::paced(dirs, env!("CARGO_PKG_VERSION"), sink, Box::new(SlowShared(launcher.clone())), FAST);
    (link, launcher, rx, tmp)
}

#[test]
fn a_slow_agent_start_is_waited_for_and_never_doubled() {
    // No wall clock decides anything here: the link counts observations of the process and the discovery file.
    let (link, launcher, statuses, _tmp) = slow_rig();
    let handle = link.spawn().unwrap();
    // The wait is measured in observations, not in seconds (ER-12): the old link gave up after 50 looks and started a
    // second agent, so well over twice that many observations with the gate shut must still show exactly one launch.
    wait_state(&link, "the slow start is noted", |s| s.stall.is_some());
    let_it_look(&link, 100);
    assert_eq!(
        launcher.launches.load(Ordering::SeqCst),
        1,
        "a second agent was started while the first was still starting"
    );
    launcher.open();
    running(&link, &statuses);
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
    wait_state(&link, "the agent is launched", |s| s.child.is_some());
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
    done: std::sync::atomic::AtomicBool,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl FakeServiceManager {
    fn new(dirs: Dirs, installed: bool, supervises: bool) -> Arc<FakeServiceManager> {
        Arc::new(FakeServiceManager {
            dirs,
            state: Mutex::new(ServiceState { installed, supervises }),
            agent: Mutex::new(None),
            refused: AtomicUsize::new(0),
            done: std::sync::atomic::AtomicBool::new(false),
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
    wait_state(&r.link, "waiting for the service", |s| s.phase == Phase::WaitingForService);
    manager.start_job();
    running(&r.link, &r.statuses);
    assert_eq!(
        r.launcher.launches.load(Ordering::SeqCst),
        0,
        "the app started an agent beside the service's"
    );
    assert!(!r.link.started_by_this_app());
    // Rule 53: a refresh with the service supervising ends nothing this app did not start.
    refresh_and_wait(&r.link);
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
    running(&r.link, &r.statuses);
    assert!(r.link.started_by_this_app());
    // The switch: the service is registered and its manager starts its agent at once (refused while ours runs).
    manager.set(true, true);
    manager.start_job();
    r.link.refresh_service();
    wait_state(&r.link, "the service's agent took over", |s| {
        s.connected() && !s.started_by_this_app()
    });
    assert!(
        manager.agent_running(),
        "connected to an agent that is not the service's (refused {} times)",
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
    running(&r.link, &r.statuses);
    assert!(!r.link.started_by_this_app());
    // The switch: the service is removed. This manager leaves its agent running (as an unregister that does not
    // stop the job would): the app asks it to exit over its connection and starts its own.
    manager.set(false, false);
    r.link.refresh_service();
    wait_state(&r.link, "an agent of the app's own after the service was removed", |s| {
        s.started_by_this_app()
    });
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
    finished: AtomicUsize,
    after_launch: Child,
    state: Mutex<Child>,
}

impl StubLauncher {
    fn new(after_launch: Child) -> Arc<StubLauncher> {
        Arc::new(StubLauncher {
            launches: AtomicUsize::new(0),
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
    statuses: Receiver<AgentStatus>,
    _tmp: tempfile::TempDir,
}

fn stub_rig(after_launch: Child) -> StubRig {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let (tx, rx) = channel();
    let sink = Recorder::new(tx);
    let launcher = StubLauncher::new(after_launch);
    let link = AgentLink::paced(dirs, env!("CARGO_PKG_VERSION"), sink, Box::new(StubShared(launcher.clone())), FAST);
    StubRig {
        link,
        launcher,
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
        protocol: dyno_core::protocol::PROTOCOL_VERSION,
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
    let end = Instant::now() + HANG_GUARD;
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
    wait_state(&r.link, "waiting for the holder", |s| s.phase == Phase::WaitingForHolder);
    let_it_look(&r.link, 100);
    assert_eq!(r.launcher.launches(), 0, "the link launched agents that the holder's lock refuses");
    assert!(!r.link.connected(), "a stale file was taken for the agent");
    let seen: Vec<AgentStatus> = r.statuses.try_iter().collect();
    assert!(
        seen.iter().all(|s| !matches!(s.state, AgentState::Failed | AgentState::Restarting)),
        "a false outage was reported: {seen:?}"
    );
    // The holder writes its file: the link connects, still without another launch.
    fresh.write(&r.link.dirs.discovery()).unwrap();
    running(&r.link, &r.statuses);
    assert_eq!(r.launcher.launches(), 0);
    r.link.quit();
    handle.join().unwrap();
    holder.stop();
}

#[test]
fn an_agent_that_never_answers_is_reported_with_retry_and_retry_ends_that_process_and_launches_again() {
    let r = stub_rig(Child::Running { pid: 4242 });
    let handle = r.link.spawn().unwrap();
    let s = wait_state(&r.link, "a stall for an agent that never wrote its discovery file", |s| {
        s.stall.is_some()
    });
    assert_eq!(s.stall.map(|(_, k)| k), Some(Stall::AgentNotAnswering));
    assert_eq!(r.launcher.launches(), 1, "it keeps waiting, it does not start another");
    assert_eq!(r.launcher.finished.load(Ordering::SeqCst), 0);
    // Retry: the held process is ended through its handle, then launched again.
    r.link.retry();
    wait_state(&r.link, "the second launch", |s| s.child == Some(2));
    assert!(
        until(HANG_GUARD, || r.launcher.launches() == 2),
        "Retry did not launch the agent again"
    );
    assert_eq!(
        r.launcher.finished.load(Ordering::SeqCst),
        1,
        "the old process was not ended through the launcher"
    );
    r.link.quit();
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
    // The look that finds the lock held re-checks it at a pace (paddock-proc), so a stall may show for the look first: wait for the one
    // that names the holder.
    let s = wait_state(&r.link, "a stall that names the other agent holding the data folder", |s| {
        s.stall.map(|(_, k)| k) == Some(Stall::OtherAgentHolds)
    });
    assert_eq!(s.stall.map(|(_, k)| k), Some(Stall::OtherAgentHolds));
    assert_eq!(r.launcher.launches(), 0);
    // Retry launches although the folder is held (the loser of two exits with code 3).
    r.link.retry();
    assert!(
        until(HANG_GUARD, || r.launcher.launches() == 1),
        "Retry did not run the connection steps again"
    );
    fresh.write(&r.link.dirs.discovery()).unwrap();
    running(&r.link, &r.statuses);
    r.link.quit();
    handle.join().unwrap();
    holder.stop();
}

#[test]
fn a_login_service_that_starts_no_agent_is_waited_for_then_replaced_by_the_apps_own_with_a_note() {
    let r = rig_with(false, super::machine::Tuning::default());
    // It says it supervises; nothing ever runs (switched off in Login Items, a job launchd dropped, a unit that fails).
    let manager = FakeServiceManager::new(r.link.dirs.clone(), true, true);
    r.link.set_service_probe(manager.probe());
    let handle = r.link.spawn().unwrap();
    let seen = collect_until(&r.statuses, AgentState::Running);
    wait_state(&r.link, "connected", |s| s.connected());
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
    refresh_and_wait(&r.link);
    assert!(r.link.connected() && matches!(r.launcher.child(), Child::Running { .. }));
    assert_eq!(r.launcher.launches.load(Ordering::SeqCst), 1);
    // A registered service keeps its agent on quit, so the app's own is ended through the launcher.
    r.link.quit();
    handle.join().unwrap();
    r.launcher.finish(Duration::ZERO);
}

#[test]
fn taking_the_agent_back_waits_for_the_former_agent_to_release_the_data_folder_before_launching() {
    let r = rig(false);
    let manager = FakeServiceManager::new(r.link.dirs.clone(), true, true);
    r.link.set_service_probe(manager.probe());
    // The service's agent, started by somebody else. It releases the data folder (the lock) only when the test
    // lets it, as on Windows where the lock goes only once the process has fully exited.
    let former = other_agent(&r.link.dirs);
    let handle = r.link.spawn().unwrap();
    running(&r.link, &r.statuses);
    // The connection has been made and the service looked at: it supervises.
    wait_state(&r.link, "the service is known", |s| s.connected());
    refresh_and_wait(&r.link);
    manager.set(false, false);
    r.link.refresh_service();
    wait_state(&r.link, "the former agent is taken back", |s| {
        matches!(s.phase, Phase::TakingBack { .. })
    });
    assert!(until(HANG_GUARD, || former.stopped()), "the former agent was not asked to exit");
    // It has been asked to exit but still holds the folder: the link must wait, looking again, and launch nothing. A look that
    // finds the lock held re-checks it `HELD_CHECKS` times, `HELD_TICK` apart (about 0.4 s), so every look costs wall-clock time:
    // 50 looks took 20 s on an idle machine and more than the hang guard on a loaded CI runner (macOS, 2026-10-31: stuck at 45 looks).
    // Three answered looks after the ask show the link waiting and looking again; that no launch happens at any look is the
    // machine's invariant (the model check covers every interleaving).
    let_it_look(&r.link, LOOKS_WHILE_HELD);
    assert_eq!(
        r.launcher.launches.load(Ordering::SeqCst),
        0,
        "launched while the former agent held the folder"
    );
    former.stop();
    wait_state(
        &r.link,
        "an agent of the app's own after the former agent released the folder",
        |s| s.started_by_this_app(),
    );
    assert_eq!(r.launcher.launches.load(Ordering::SeqCst), 1);
    assert!(r.link.quit());
    handle.join().unwrap();
}

#[test]
fn quit_reports_the_agent_it_stopped_even_though_asking_it_to_exit_ended_its_process() {
    // The stub's `finish` ends the process at once, as asking the agent to exit does: the outcome was decided at the
    // moment of the quit, from the child that was held then, and is not read again afterwards.
    let r = stub_rig(Child::Running { pid: 4242 });
    let handle = r.link.spawn().unwrap();
    wait_state(&r.link, "the agent is launched", |s| s.child.is_some());
    assert!(
        r.link.quit(),
        "the process this app started was running when it quit, and quit logged that it stopped it"
    );
    handle.join().unwrap();
    assert_eq!(r.launcher.finished.load(Ordering::SeqCst), 1);
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
    r.link.quit();
    handle.join().unwrap();
}

// ------------------------------------------------------------------------------------------------ new in ADR 0025

#[test]
fn a_quit_is_taken_at_once_while_the_service_probe_never_answers() {
    // The owner thread never blocks (ADR 0025): a port that hangs for ever only delays its own answer.
    let r = stub_rig(Child::None);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let g = gate.clone();
    r.link.set_service_probe(Arc::new(move || {
        let open = lock(&g.0);
        drop(g.1.wait_while(open, |o| !*o).unwrap_or_else(|e| e.into_inner()));
        ServiceState::default()
    }));
    let handle = r.link.spawn().unwrap();
    assert!(r.link.transitions().wait_steps(10, HANG_GUARD), "the owner stopped stepping");
    assert!(!r.link.quit(), "nothing was started");
    handle.join().unwrap();
    assert!(r.link.transitions().current().stopped());
    *lock(&gate.0) = true;
    gate.1.notify_all();
}

/// A sink whose status notification blocks until the gate opens (a UI thread that is busy).
struct BlockedSink {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl Sink for BlockedSink {
    fn live(&self, _: LiveUpdate) {}
    fn alert(&self, _: Alert) {}
    fn activity(&self, _: ActivityEntry) {}
    fn agent(&self, _: AgentStatus) {
        let open = lock(&self.gate.0);
        drop(self.gate.1.wait_while(open, |o| !*o).unwrap_or_else(|e| e.into_inner()));
    }
}

#[test]
fn a_quit_is_taken_at_once_while_the_shell_is_blocked_on_a_notification() {
    let tmp = tempfile::tempdir().unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let launcher = StubLauncher::new(Child::Running { pid: 4242 });
    let link = AgentLink::paced(
        Dirs::at(tmp.path().join("data")),
        env!("CARGO_PKG_VERSION"),
        Arc::new(BlockedSink { gate: gate.clone() }),
        Box::new(StubShared(launcher.clone())),
        FAST,
    );
    let handle = link.spawn().unwrap();
    wait_state(&link, "the agent is launched", |s| s.child.is_some());
    assert!(link.quit());
    handle.join().unwrap();
    *lock(&gate.0) = true;
    gate.1.notify_all();
}

#[test]
fn a_request_uses_the_connection_that_was_just_published_even_before_the_state_that_says_so() {
    // The step that makes a connection publishes it (`UsePeer`) before its snapshot; the worker that subscribes to the live
    // stream starts in between and used to read the older snapshot, which was already "settled" without a connection (a
    // stall had been shown), so its request failed as unavailable and the subscription was never made.
    let r = rig(false);
    r.launcher.launch().unwrap();
    let found = Discovery::read(&r.link.dirs.discovery()).expect("the in-process agent wrote its file");
    let peer = ports::connect(&found, &(r.sink.clone() as Arc<dyn Sink>)).unwrap();
    *lock(&r.link.peer) = Some(peer);
    assert!(
        !r.link.connected() && r.link.transitions().current().settled(),
        "the link has not published a connection (never spawned: settled, not connected)"
    );
    assert!(r.link.call(Request::Settings).is_ok(), "a published connection is used at once");
    r.launcher.finish(Duration::ZERO);
}

/// Login-start diagnosis (2026-10-31): `SMAppService.status` took about 30 s at login, so the look that starts an attempt does not
/// ask the login service when an agent is already there to connect to (its discovery file beside a held lock). This pins the
/// whole path: the machine marks the look as a decision look, the driver passes that on, and the probe is not called while that
/// look runs. A driver that forwards `deciding: false` (the mutant) calls the probe during the first look.
#[test]
fn the_first_look_does_not_ask_the_login_service_when_an_agent_is_there_to_connect_to() {
    let r = rig(false);
    // An agent of this build is "there": its lock is held and its discovery file is written (its endpoint answers nobody).
    let _held = dyno_agent::instance::InstanceLock::acquire(&r.link.dirs).expect("the test takes the agent lock");
    Discovery {
        endpoint: "no-such-endpoint".into(),
        token: "00".into(),
        pid: std::process::id(),
        started: 0,
        version: env!("CARGO_PKG_VERSION").into(),
        protocol: dyno_core::protocol::PROTOCOL_VERSION,
    }
    .write(&r.link.dirs.discovery())
    .unwrap();
    // The probe records how many looks had been answered when it was first called: 0 means during the first look.
    let first_call_after_looks = Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let (seen, link) = (first_call_after_looks.clone(), r.link.clone());
    r.link.set_service_probe(Arc::new(move || {
        let looks = link.transitions().current().looks;
        let _ = seen.compare_exchange(u64::MAX, looks, Ordering::SeqCst, Ordering::SeqCst);
        ServiceState::default()
    }));
    let handle = r.link.spawn().unwrap();
    // Let the link look a few times (the first look decides to connect, the connect fails, the waits that follow look again).
    wait_state(&r.link, "three looks answered", |s| s.looks >= 3);
    let at = first_call_after_looks.load(Ordering::SeqCst);
    assert!(
        at != 0,
        "the service was asked during the first (decision) look, which must not wait for it"
    );
    r.link.quit();
    handle.join().unwrap();
    r.launcher.finish(Duration::ZERO);
}
