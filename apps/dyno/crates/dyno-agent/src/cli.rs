//! The `dyno-agent` command line: `run` (the service entry), `status`, `stop`, `service …`, `once` and `bench`.

use crate::discovery::Discovery;
use crate::paths::Dirs;
use crate::service::{Service, Status};
use crate::{Running, StartError};
use dyno_core::protocol::{LiveDetail, Message, Reply, Request};
use dyno_ipc::{Client, Peer, PeerOptions};
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

/// What to do about a signal that arrived: end now (the agent is still starting, so nothing is open to close and the main
/// thread is in a start that does not look at the flag), wait for the clean stop, or end because the clean stop took too long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnSignal {
    EndNow,
    WaitForStop,
    EndAnyway,
}

fn on_signal_decision(running: bool, stopped: bool, waited_too_long: bool) -> OnSignal {
    match (running, stopped) {
        (_, true) => OnSignal::WaitForStop,
        (false, false) => OnSignal::EndNow,
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

/// Watches for a signal on a thread of its own, so that TERM ends a slow start too (the main thread only looks at the flag once
/// the agent runs). Once the agent runs, the main loop stops it cleanly (store closed, lock released, discovery file removed);
/// this thread only makes sure the process ends if that stop hangs.
fn spawn_signal_watch() {
    let _ = std::thread::Builder::new().name("dyno-signal".into()).spawn(|| {
        while !SIGNALLED.load(Ordering::SeqCst) {
            // pace: how often the flag is looked at (a signal handler cannot wake a thread).
            std::thread::sleep(Duration::from_millis(100));
        }
        let signalled_at = Instant::now();
        loop {
            let late = signalled_at.elapsed() >= SIGNAL_STOP_GRACE;
            match on_signal_decision(RUNNING.load(Ordering::SeqCst), STOPPED.load(Ordering::SeqCst), late) {
                OnSignal::EndNow => {
                    eprintln!("dyno-agent: ended by a signal while starting");
                    end_process(143);
                }
                OnSignal::EndAnyway => {
                    eprintln!("dyno-agent: ended by a signal; the stop did not finish in time");
                    end_process(143);
                }
                OnSignal::WaitForStop if STOPPED.load(Ordering::SeqCst) => return,
                OnSignal::WaitForStop => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
}

const USAGE: &str = "dyno-agent — measures your computer and keeps its history\n\n\
usage:\n  dyno-agent run                 run in the foreground (what the service starts)\n  dyno-agent status              is an agent running?\n  \
dyno-agent stop                ask the running agent to exit\n  dyno-agent once                print one measurement as JSON (changes nothing)\n  \
dyno-agent bench [seconds] [none|summary|processes]   run for a while and report Dyno's own CPU and memory as JSON\n  \
dyno-agent service install|uninstall|status|ensure   register it to start at login\n";

/// The exit code of `run` when another agent already runs for the data folder: this one started nothing and changed
/// nothing (the app then connects to the running one).
pub const EXIT_ALREADY_RUNNING: i32 = 3;

/// Runs the command line; returns the exit code.
pub fn main(args: Vec<String>) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = match args.as_slice() {
        ["run"] => return run_exit_code(run()),
        ["status"] => status(),
        ["stop"] => stop(),
        ["once"] => once(),
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
            println!("dyno-agent {}", env!("CARGO_PKG_VERSION"));
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

/// Connects to the agent named in the discovery file.
pub fn connect(dirs: &Dirs) -> Result<(Peer<Message>, Discovery), String> {
    let d = Discovery::read(&dirs.discovery()).ok_or("No agent is running.")?;
    let token = d.token().ok_or("The discovery file is damaged.")?;
    let conn =
        Client::connect(&d.endpoint, &token).map_err(|e| format!("The agent in {} did not answer ({e}).", dirs.discovery().display()))?;
    let peer = Peer::start(conn, PeerOptions::new()).map_err(|e| e.to_string())?;
    Ok((peer, d))
}

/// Sends one request and unwraps the reply.
pub fn ask(peer: &Peer<Message>, request: Request) -> Result<Reply, String> {
    ask_within(peer, request, Duration::from_secs(15))
}

/// Like [`ask`] with an explicit deadline, for requests that read slow system sources (hardware inventory, diagnostics).
pub fn ask_within(peer: &Peer<Message>, request: Request, within: Duration) -> Result<Reply, String> {
    match peer.call(Message::Request(Box::new(request)), within) {
        Ok(Message::Reply(r)) => (*r).map_err(|e| e.message),
        Ok(_) => Err("The agent sent something unexpected.".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Starts the job `spec`, waits until it ends and returns what it produced, or the message it ended with. For callers that
/// want the answer and have nothing to show meanwhile (the CLI, the tests); the app shows the progress instead.
pub fn run_job(peer: &Peer<Message>, spec: dyno_core::jobs::JobSpec, within: Duration) -> Result<dyno_core::jobs::JobOutput, String> {
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
            Some(WireState::Failed(message)) => return Err(message.clone()),
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
fn platforms() -> (dyno_platform::Platform, Box<dyn dyno_core::ports::ProcessSource>) {
    let platform = dyno_platform::platform();
    let details = dyno_platform::platform().processes;
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
        Err(StartError::Failed(e)) => {
            eprintln!("dyno-agent: {e}");
            log::error!(ErrorCode = "StartFailed", ErrorChain = e.as_str(); "Agent could not start");
            1
        }
    }
}

fn run() -> Result<(), StartError> {
    let dirs = Dirs::current();
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
    let (platform, details) = platforms();
    // Takes the data folder's agent lock first: a second agent exits here, before it opens or writes anything.
    let running = Running::start_os(dirs, platform, details, reveal())?;
    eprintln!("dyno-agent {} running (pid {})", env!("CARGO_PKG_VERSION"), running.discovery.pid);
    log::info!(ElapsedMs = crate::logging::elapsed_ms(), Pid = running.discovery.pid; "Agent running");
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

fn once() -> Result<(), String> {
    let mut p = dyno_platform::platform();
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
    let mut svc = Service::for_current_user(exe)?;
    match cmd {
        "install" => svc.install().map(|()| println!("installed and started")),
        "uninstall" => svc.uninstall().map(|()| println!("removed")),
        "ensure" => svc
            .ensure()
            .map(|changed| println!("{}", if changed { "registered again" } else { "nothing to do" })),
        "status" => {
            println!(
                "{}",
                match svc.status() {
                    Status::NotInstalled => "not installed".to_string(),
                    Status::Installed { running, current } => format!(
                        "installed, {}{}",
                        if running { "running" } else { "not running" },
                        if current { "" } else { ", registered for another executable" }
                    ),
                }
            );
            Ok(())
        }
        _ => Err("service takes install, uninstall, status or ensure".into()),
    }
}

/// Runs the agent in-process against this machine with a subscriber asking for the processes (the heaviest case the
/// app creates), and reports Dyno's own cost.
fn bench(seconds: u64, who: &str) -> Result<(), String> {
    let detail = match who {
        "none" => None,
        "summary" => Some(LiveDetail::Summary),
        "processes" => Some(LiveDetail::Processes),
        _ => return Err("bench subscribers: none, summary or processes".into()),
    };
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let tmp = tempfile_dir()?;
    let dirs = Dirs::at(tmp.join("data"));
    let (platform, details) = platforms();
    let running = Running::start_os(dirs.clone(), platform, details, Box::new(|_| Ok(())))?;
    let (peer, _) = connect(&dirs)?;
    let seen = Arc::new(Mutex::new((0u64, 0f32, 0u64)));
    let s = seen.clone();
    let listener = {
        let (conn_peer, _) = connect(&dirs)?;
        drop(conn_peer);
        let d = Discovery::read(&dirs.discovery()).ok_or("agent vanished")?;
        let conn = Client::connect(&d.endpoint, &d.token().ok_or("bad token")?).map_err(|e| e.to_string())?;
        Peer::start(
            conn,
            PeerOptions::new().on_push(move |m| {
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
    let mut sys = System::new();
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), true, kind);
    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(seconds.max(5)));
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), true, kind);
    let (cpu, rss) = sys.process(me).map(|p| (p.cpu_usage(), p.memory())).unwrap_or((0.0, 0));
    let (updates, reported_cpu, reported_mem) = *crate::agent::lock(&seen);
    drop((peer, listener));
    running.stop();
    let _ = std::fs::remove_dir_all(&tmp);
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

fn tempfile_dir() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("dyno-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::LockError;

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
    fn a_signal_ends_a_start_at_once_and_a_running_agent_stops_cleanly_with_a_bound() {
        use super::{on_signal_decision, OnSignal};
        // Still starting: the main thread is in a start that does not look at the flag, so the watch ends the process.
        assert_eq!(on_signal_decision(false, false, false), OnSignal::EndNow);
        // Running: the main loop stops it cleanly; the watch only waits...
        assert_eq!(on_signal_decision(true, false, false), OnSignal::WaitForStop);
        // ...and ends the process if that stop hangs.
        assert_eq!(on_signal_decision(true, false, true), OnSignal::EndAnyway);
        // Stopped: nothing left to do, however late.
        assert_eq!(on_signal_decision(true, true, true), OnSignal::WaitForStop);
    }
}
