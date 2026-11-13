//! The `dyno-agent` command line: `run` (the service entry), `status`, `stop`, `service …`, `once` and `bench`.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::discovery::Discovery;
use crate::paths::Dirs;
use crate::wire;
use crate::{Running, StartError};
use dyno_core::protocol::{LiveDetail, Message, Reply, Request};
use paddock_ipc::{BodyOptions, BodyPeer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Set by SIGINT/SIGTERM.
static SIGNALLED: AtomicBool = AtomicBool::new(false);
/// `run` has finished starting: from here its own loop sees [`SIGNALLED`] and stops the agent cleanly.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// `run` has stopped the agent and is about to return.
static STOPPED: AtomicBool = AtomicBool::new(false);

/// How long a stop after a signal may take before the process ends anyway (ADR 0031 class S: a signal means the system is
/// ending the process, and macOS follows TERM with KILL; ending must not depend on a store that will not close).
// cap: the one deliberate time limit of the agent's stop.
const SIGNAL_STOP_GRACE: Duration = Duration::from_secs(10);

/// What to do about a signal that arrived: wait (for a start in progress to finish, after which the main loop stops the agent
/// cleanly, or for that clean stop), or end because the start (`EndNow`) or the stop (`EndAnyway`) took longer than the grace.
/// A start is not ended at once: it may already have opened the store and written the discovery file (CI job 33005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnSignal {
    EndNow,
    WaitForStop,
    EndAnyway,
}

fn on_signal_decision(running: bool, stopped: bool, waited_too_long: bool) -> OnSignal {
    match (running, stopped) {
        (_, true) => OnSignal::WaitForStop,
        (false, false) if waited_too_long => OnSignal::EndNow,
        (false, false) => OnSignal::WaitForStop,
        (true, false) if waited_too_long => OnSignal::EndAnyway,
        (true, false) => OnSignal::WaitForStop,
    }
}

/// Ends the process now, without running destructors or at-exit handlers (they may be what hangs, and the main thread may be
/// inside them): the OS releases the lock and the sockets, as for a crash.
#[allow(unsafe_code)]
fn end_process(code: i32) -> ! {
    #[cfg(unix)]
    // SAFETY: `_exit` only ends this process; it takes no pointers.
    unsafe {
        libc::_exit(code)
    }
    #[cfg(not(unix))]
    std::process::exit(code)
}

/// Watches for a signal on a thread of its own, so that TERM ends a start that hangs too (the main thread only looks at the flag
/// once the agent runs). Once the agent runs, the main loop stops it cleanly (store closed, lock released, discovery file removed);
/// this thread only makes sure the process ends if the start and that stop take longer than [`SIGNAL_STOP_GRACE`].
fn spawn_signal_watch() {
    let spawned = std::thread::Builder::new().name("dyno-signal".into()).spawn(|| {
        while !SIGNALLED.load(Ordering::SeqCst) {
            // pace: how often the flag is looked at (a signal handler cannot wake a thread).
            std::thread::sleep(Duration::from_millis(100));
        }
        let signalled_at = Instant::now();
        loop {
            let late = signalled_at.elapsed() >= SIGNAL_STOP_GRACE;
            match on_signal_decision(RUNNING.load(Ordering::SeqCst), STOPPED.load(Ordering::SeqCst), late) {
                OnSignal::EndNow => {
                    eprintln!("dyno-agent: ended by a signal; the start did not finish in time");
                    log::error!(GraceMs = SIGNAL_STOP_GRACE.as_millis() as u64; "Agent ended by a signal: the start did not finish in time");
                    end_process(143);
                }
                OnSignal::EndAnyway => {
                    // Rule 56 (ER-27): which part of the stop was still running, so a forced end can be explained from the log.
                    let waiting_on = crate::stop_waiting_on();
                    eprintln!("dyno-agent: ended by a signal; the stop did not finish in time (waiting on {waiting_on})");
                    log::error!(
                        GraceMs = SIGNAL_STOP_GRACE.as_millis() as u64,
                        WaitingOn = waiting_on.as_str();
                        "Agent ended by a signal: the stop did not finish in time"
                    );
                    end_process(143);
                }
                OnSignal::WaitForStop if STOPPED.load(Ordering::SeqCst) => return,
                OnSignal::WaitForStop => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    if let Err(e) = spawned {
        log::error!(error:% = e; "the signal watch could not start; a TERM during a slow start is only seen once the agent runs");
    }
}

const USAGE: &str = "dyno-agent — measures your computer and keeps its history\n\n\
usage:\n  dyno-agent run                 run in the foreground (what the service starts)\n  dyno-agent status              is an agent running?\n  \
dyno-agent stop                ask the running agent to exit\n  dyno-agent once                print one measurement as JSON (changes nothing)\n  \
dyno-agent bench [seconds] [none|tray|summary|processes]   run for a while and report Dyno's own CPU and memory as JSON\n  \
dyno-agent service install|uninstall|status|ensure   register it to start at login\n";

/// The exit code of `run` when another agent already runs for the data folder: this one started nothing and changed
/// nothing (the app then connects to the running one).
pub const EXIT_ALREADY_RUNNING: i32 = 3;

/// The exit code of `run` (and of the app) when the data folder is refused: nothing was started, opened or written in it, not even the log (the refusal is logged in the OS's own logs folder, `logging::log_refusal`).
/// The text on standard error says the cause and the next step; the app makes the same check and shows the same text. 78 is the code
/// `Dirs::current` already uses for "no usable data folder" (`EX_CONFIG`).
pub const EXIT_DATA_FOLDER_REFUSED: i32 = 78;

/// Runs the command line; returns the exit code.
pub fn main(args: Vec<String>) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = match args.as_slice() {
        ["run"] => return run_exit_code(run()),
        ["status"] => status(),
        ["stop"] => stop(),
        ["once"] => once(),
        ["view"] => view(),
        ["bench"] => bench(60, "processes"),
        ["bench", n] => n
            .parse()
            .map_err(|_| "bench takes a number of seconds".to_string())
            .and_then(|n| bench(n, "processes")),
        ["bench", n, who] => n
            .parse()
            .map_err(|_| "bench takes a number of seconds".to_string())
            .and_then(|n| bench(n, who)),
        ["service", cmd] => service(cmd),
        ["--version"] | ["-V"] => {
            println!("{}", version_line(env!("CARGO_PKG_VERSION"), option_env!("PADDOCK_BUILD_COMMIT")));
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            return 2;
        }
    };
    match out {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("dyno-agent: {e}");
            1
        }
    }
}

/// `dyno-agent 1.1.0 (build abc1234)`: the version and the commit the build embeds (`PADDOCK_BUILD_COMMIT`), so a VM run or a
/// bug report names the exact build; without a commit (a local build) only the version.
pub fn version_line(version: &str, commit: Option<&str>) -> String {
    match commit.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => format!("dyno-agent {version} (build {c})"),
        None => format!("dyno-agent {version}"),
    }
}

/// Connects to the agent named in the discovery file.
pub fn connect(dirs: &Dirs) -> Result<(BodyPeer<Message>, Discovery), String> {
    let d = Discovery::read(&dirs.discovery()).ok_or("No agent is running.")?;
    let token = d.token().ok_or("The discovery file is damaged.")?;
    let conn =
        wire::connect(&d.endpoint, &token).map_err(|e| format!("The agent in {} did not answer ({e}).", dirs.discovery().display()))?;
    let peer = BodyPeer::start(conn, BodyOptions::new("the agent")).map_err(|e| e.to_string())?;
    Ok((peer, d))
}

/// Sends one request and unwraps the reply.
pub fn ask(peer: &BodyPeer<Message>, request: Request) -> Result<Reply, String> {
    ask_within(peer, request, Duration::from_secs(15))
}

/// Like [`ask`] with an explicit deadline, for requests that read slow system sources (hardware inventory, diagnostics).
pub fn ask_within(peer: &BodyPeer<Message>, request: Request, within: Duration) -> Result<Reply, String> {
    match peer.call(Message::Request(Box::new(request)), within) {
        Ok(Message::Reply(r)) => (*r).map_err(|e| e.message),
        Ok(_) => Err("The agent sent something unexpected.".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Starts the job `spec`, waits until it ends and returns what it produced, or the message it ended with. For callers that
/// want the answer and have nothing to show meanwhile (the CLI, the tests); the app shows the progress instead.
pub fn run_job(peer: &BodyPeer<Message>, spec: dyno_core::jobs::JobSpec, within: Duration) -> Result<dyno_core::jobs::JobOutput, String> {
    use paddock_jobs::wire::WireState;
    let started = std::time::Instant::now();
    let Reply::Job(job) = ask(peer, Request::StartJob { spec })? else {
        return Err("The agent sent something unexpected.".into());
    };
    loop {
        let Reply::Jobs(all) = ask(peer, Request::Jobs)? else {
            return Err("The agent sent something unexpected.".into());
        };
        match all.iter().find(|s| s.id == job.id).map(|s| &s.state) {
            Some(WireState::Done) => break,
            Some(WireState::Failed { message, .. }) => return Err(message.clone()),
            Some(WireState::Cancelled(_)) => return Err("The job was cancelled.".into()),
            Some(_) => {}
            None => return Err("The agent forgot the job.".into()),
        }
        if started.elapsed() > within {
            return Err("The job did not end in time.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    match ask(peer, Request::JobResult { id: job.id })? {
        Reply::JobOutput(o) => Ok(*o),
        _ => Err("The agent sent something unexpected.".into()),
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn install_signal_handlers() {
    extern "C" fn on_signal(_: libc::c_int) {
        SIGNALLED.store(true, Ordering::SeqCst);
    }
    // SAFETY: the handler only stores into an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
}

#[cfg(not(unix))]
fn install_signal_handlers() {}

fn reveal() -> crate::agent::Reveal {
    Box::new(dyno_platform::reveal)
}

/// The OS ports, and a second process source for on-demand details.
fn platforms(dirs: &Dirs) -> (dyno_platform::Platform, Box<dyn dyno_core::ports::ProcessSource>) {
    let secrets = dirs.secrets_folder();
    let platform = dyno_platform::platform(&secrets);
    // What the support bundle needs to tell a real reader from a gap: the adapter behind every port (ADR 0033, ER-27).
    log::info!(Ports = platform.wired_line().as_str(); "Platform wired");
    let details = dyno_platform::platform(&secrets).processes;
    (platform, details)
}

/// The exit code for `run`'s outcome, with its message on stderr.
fn run_exit_code(out: Result<(), StartError>) -> i32 {
    match out {
        Ok(()) => 0,
        Err(e @ StartError::AlreadyRunning(_)) => {
            eprintln!("dyno-agent: {e}");
            EXIT_ALREADY_RUNNING
        }
        Err(StartError::DataFolderRefused(e)) => {
            eprintln!("dyno-agent: {e}");
            EXIT_DATA_FOLDER_REFUSED
        }
        Err(StartError::Failed(e)) => {
            eprintln!("dyno-agent: {e}");
            log::error!(ErrorCode = "StartFailed", ErrorChain = e.as_str(); "Agent could not start");
            1
        }
    }
}

fn run() -> Result<(), StartError> {
    let dirs = Dirs::current();
    // Before the log, the lock and the markers: each of them writes into the data folder, and a refused one is never written to.
    if let Err(text) = crate::paths::check_root(&dirs) {
        // Not in the refused folder: the OS's own logs folder, so a start that leaves nothing else still leaves its reason (ER-27).
        crate::logging::log_refusal(&dirs);
        return Err(StartError::DataFolderRefused(text));
    }
    let dirs = dirs.clone().with_log_folder(crate::logging::log_dir(&dirs));
    // First: the agent's own dated log (and its panic hook), so every step below is in a file even under launchd.
    let _log = crate::logging::install(&dirs);
    log::info!(
        ParentPid = crate::logging::parent_pid(),
        ParentName = crate::logging::parent_name(),
        Arch = crate::logging::arch(),
        OsVersion = crate::logging::os_version().as_str(),
        Version = env!("CARGO_PKG_VERSION");
        "Agent starting"
    );
    install_signal_handlers();
    spawn_signal_watch();
    let (platform, details) = platforms(&dirs);
    // Takes the data folder's agent lock first: a second agent exits here, before it opens or writes anything.
    let root = dirs.root().to_path_buf();
    let running = Running::start_os(dirs, platform, details, reveal())?;
    // The lock is held, so this is the one agent: say whether the last one died, and leave this run's marker.
    let (run_marker, _previous) =
        paddock_startup::run_marker::begin(&root, "agent", std::process::id(), &paddock_startup::lifecycle::os_probe);
    eprintln!("dyno-agent {} running (pid {})", env!("CARGO_PKG_VERSION"), running.discovery.pid);
    log::info!(ElapsedMs = crate::logging::elapsed_ms(), Pid = running.discovery.pid; "Agent running");
    log_socket_view();
    RUNNING.store(true, Ordering::SeqCst);
    while !running.stopped() && !SIGNALLED.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let reason = if SIGNALLED.load(Ordering::SeqCst) {
        "signal"
    } else {
        "a client asked it to shut down"
    };
    log::info!(Reason = reason; "Agent stopping");
    running.stop();
    STOPPED.store(true, Ordering::SeqCst);
    run_marker.end();
    log::info!(ElapsedMs = crate::logging::elapsed_ms(); "Agent stopped");
    Ok(())
}

fn status() -> Result<(), String> {
    let (peer, d) = connect(&Dirs::current())?;
    ask(&peer, Request::Settings)?;
    println!("running: pid {}, version {}, protocol {}", d.pid, d.version, d.protocol);
    Ok(())
}

fn stop() -> Result<(), String> {
    let dirs = Dirs::current();
    let (peer, d) = connect(&dirs)?;
    ask(&peer, Request::Shutdown)?;
    for _ in 0..50 {
        if !dirs.discovery().exists() {
            println!("stopped (pid {})", d.pid);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("The agent was asked to stop but is still running.".into())
}

/// Rule 56 (ER-27): on macOS the start line says which code-signing identifier this process has and how much of the kernel's
/// socket list the OS shows it, so a "only its own sockets" page can be explained from the log alone (Dyno ADR 0026).
fn log_socket_view() {
    #[cfg(target_os = "macos")]
    match dyno_platform::macos::pcblist::socket_view() {
        Ok(v) => log::info!(
            SigningIdentifier = v.identifier.as_deref().unwrap_or("unreadable"),
            Announced = v.announced,
            Sent = v.sent,
            CutDown = v.cut_down,
            ParentName = crate::logging::parent_name();
            "Agent socket view"
        ),
        Err(e) => log::warn!(Error = e.to_string().as_str(); "Agent socket view unreadable"),
    }
}

/// `dyno-agent view`: prints the socket view as one line (`identifier=… announced=… sent=… cut_down=…`), for the bundle check.
fn view() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let v = dyno_platform::macos::pcblist::socket_view().map_err(|e| e.to_string())?;
        println!(
            "identifier={} announced={} sent={} cut_down={}",
            v.identifier.as_deref().unwrap_or("unreadable"),
            v.announced,
            v.sent,
            v.cut_down
        );
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err("the socket view is a macOS check".into())
}

#[expect(
    clippy::disallowed_methods,
    reason = "`dyno-agent once` is a diagnostic command: its platform is rooted at a scratch folder"
)]
fn once() -> Result<(), String> {
    let mut p = dyno_platform::platform(&std::env::temp_dir());
    let t = crate::agent::now();
    p.sensors.sample(t);
    std::thread::sleep(Duration::from_millis(1_000));
    let sample = p.sensors.sample(crate::agent::now());
    println!("{}", serde_json::to_string_pretty(&sample).map_err(|e| e.to_string())?);
    Ok(())
}

fn service(cmd: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find this program: {e}"))?;
    crate::service::refuse_inside_app_bundle(crate::service::Os::current(), &exe)?;
    let mut verbs = crate::service::verbs_for_current_user(exe)?;
    crate::service::run_verb(cmd, verbs.as_mut()).map(|line| println!("{line}"))
}

/// Runs the agent in-process against this machine with a subscriber asking for the processes (the heaviest case the
/// app creates), and reports Dyno's own cost.
fn bench(seconds: u64, who: &str) -> Result<(), String> {
    let detail = match who {
        "none" => None,
        "summary" => Some(LiveDetail::Summary),
        "processes" => Some(LiveDetail::Processes),
        "tray" => Some(LiveDetail::Tray),
        _ => return Err("bench subscribers: none, tray, summary or processes".into()),
    };
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let tmp = tempfile_dir()?;
    let dirs = Dirs::at(tmp.join("data"));
    let (platform, details) = platforms(&dirs);
    let running = Running::start_os(dirs.clone(), platform, details, Box::new(|_| Ok(())))?;
    let (peer, _) = connect(&dirs)?;
    let seen = Arc::new(Mutex::new((0u64, 0f32, 0u64)));
    let s = seen.clone();
    let listener = {
        let (conn_peer, _) = connect(&dirs)?;
        drop(conn_peer);
        let d = Discovery::read(&dirs.discovery()).ok_or("agent vanished")?;
        let conn = wire::connect(&d.endpoint, &d.token().ok_or("bad token")?).map_err(|e| e.to_string())?;
        BodyPeer::start(
            conn,
            BodyOptions::new("the agent").on_push(move |m| {
                if let Message::Live(l) = m {
                    let mut g = crate::agent::lock(&s);
                    g.0 += 1;
                    g.1 = l.overhead.agent_cpu;
                    g.2 = l.overhead.agent_memory;
                }
            }),
        )
        .map_err(|e| e.to_string())?
    };
    if let Some(detail) = detail {
        ask(&listener, Request::Subscribe { detail })?;
    }
    let me = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = System::new().map_err(|e| format!("The OS process table could not be read ({e})."))?;
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), true, kind);
    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(seconds.max(5)));
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), true, kind);
    let (cpu, rss) = sys.process(me).map(|p| (p.cpu_usage(), p.memory())).unwrap_or((0.0, 0));
    let (updates, reported_cpu, reported_mem) = *crate::agent::lock(&seen);
    drop((peer, listener));
    running.stop();
    paddock_proc::cleanup::remove_dir_all(&tmp);
    let report = serde_json::json!({
        "subscriber": who,
        "seconds": started.elapsed().as_secs_f32(),
        "liveUpdates": updates,
        "cpuPercentOfOneCore": cpu,
        "residentMegabytes": rss as f64 / 1_048_576.0,
        "agentReportedCpuPercentOfOneCore": reported_cpu,
        "agentReportedMegabytes": reported_mem as f64 / 1_048_576.0,
        "logicalCores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "note": "in-process agent + one subscriber asking for the full process list; the sampling loop adapts to the load",
    });
    println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "the bench command's own scratch folder, removed when the command ends"
)]
fn tempfile_dir() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("dyno-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::LockError;

    /// Keeps every record of the process: the `Platform wired` test reads the one it needs.
    struct Capture(std::sync::Mutex<Vec<(String, String)>>);

    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, r: &log::Record) {
            let mut ports = String::new();
            struct V<'a>(&'a mut String);
            impl<'kvs> log::kv::VisitSource<'kvs> for V<'_> {
                fn visit_pair(&mut self, k: log::kv::Key<'kvs>, v: log::kv::Value<'kvs>) -> Result<(), log::kv::Error> {
                    if k.as_str() == "Ports" {
                        self.0.push_str(&v.to_string());
                    }
                    Ok(())
                }
            }
            let _ = r.key_values().visit(&mut V(&mut ports));
            self.0.lock().unwrap().push((r.args().to_string(), ports));
        }
        fn flush(&self) {}
    }

    /// Reads the line `platforms()` logs, the one function `run()` builds its platform with. It does not start the agent: `run()` also installs the
    /// agent's own logger, takes the instance lock and blocks until a signal, so a test cannot go through it cheaply; a `run()` that stopped calling
    /// `platforms()` would not be caught here. The global logger needs one process per test (nextest's default; `cargo test` is fine while no other
    /// `dyno-agent` lib test installs a logger or builds a platform).
    #[test]
    fn the_agent_logs_one_platform_wired_line_naming_every_port_at_start() {
        static CAPTURE: Capture = Capture(std::sync::Mutex::new(Vec::new()));
        log::set_logger(&CAPTURE).expect("no other test in this process installs a logger");
        log::set_max_level(log::LevelFilter::Info);
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let (platform, _details) = platforms(&Dirs::at(tmp.path().join("data")));
        let wired: Vec<_> = CAPTURE
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m == "Platform wired")
            .cloned()
            .collect();
        assert_eq!(wired.len(), 1, "exactly one line: {wired:?}");
        assert_eq!(wired[0].1, platform.wired_line(), "it carries every port=adapter pair");
        for port in ["sensors=", "locks=", "startup=", "facts=", "wol.udp="] {
            assert!(wired[0].1.contains(port), "{port} missing from {}", wired[0].1);
        }
    }

    #[test]
    fn the_version_line_names_the_build_commit_when_there_is_one() {
        assert_eq!(version_line("1.1.0", Some("abc1234")), "dyno-agent 1.1.0 (build abc1234)");
        assert_eq!(version_line("1.1.0", Some(" ")), "dyno-agent 1.1.0");
        assert_eq!(version_line("1.1.0", None), "dyno-agent 1.1.0");
    }

    #[test]
    fn a_refused_second_agent_exits_with_its_own_code_and_other_failures_with_one() {
        assert_eq!(run_exit_code(Ok(())), 0);
        let held = StartError::AlreadyRunning(LockError::Held { folder: "/x/Dyno".into() });
        let message = held.to_string();
        assert!(message.contains("already running for /x/Dyno"), "{message}");
        assert!(
            message.contains("another computer") && message.contains("this one exits"),
            "{message}"
        );
        assert_eq!(run_exit_code(Err(held)), EXIT_ALREADY_RUNNING);
        assert_eq!(run_exit_code(Err(StartError::Failed("boom".into()))), 1);
    }

    #[test]
    fn a_signal_lets_a_start_finish_stops_cleanly_and_ends_only_after_the_grace() {
        use super::{on_signal_decision, OnSignal};
        // Still starting: the start may already have opened the store and written the discovery file, so the watch lets it
        // finish (the main loop then stops it cleanly, CI job 33005)...
        assert_eq!(on_signal_decision(false, false, false), OnSignal::WaitForStop);
        // ...and ends the process only if the start and its stop take longer than the grace.
        assert_eq!(on_signal_decision(false, false, true), OnSignal::EndNow);
        // Running: the main loop stops it cleanly; the watch only waits...
        assert_eq!(on_signal_decision(true, false, false), OnSignal::WaitForStop);
        // ...and ends the process if that stop hangs.
        assert_eq!(on_signal_decision(true, false, true), OnSignal::EndAnyway);
        // Stopped: nothing left to do, however late.
        assert_eq!(on_signal_decision(true, true, true), OnSignal::WaitForStop);
    }
}
