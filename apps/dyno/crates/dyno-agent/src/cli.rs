//! The `dyno-agent` command line: `run` (the service entry), `status`, `stop`, `service …`, `once` and `bench`.

use crate::discovery::Discovery;
use crate::paths::Dirs;
use crate::service::{Service, Status};
use crate::Running;
use dyno_core::protocol::{LiveDetail, Message, Reply, Request};
use dyno_ipc::{Client, Peer, PeerOptions};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Set by SIGINT/SIGTERM.
static SIGNALLED: AtomicBool = AtomicBool::new(false);

const USAGE: &str = "dyno-agent — measures your PC and keeps its history\n\n\
usage:\n  dyno-agent run                 run in the foreground (what the service starts)\n  dyno-agent status              is an agent running?\n  \
dyno-agent stop                ask the running agent to exit\n  dyno-agent once                print one measurement as JSON (changes nothing)\n  \
dyno-agent bench [seconds] [none|summary|processes]   run for a while and report Dyno's own CPU and memory as JSON\n  \
dyno-agent service install|uninstall|status|ensure   register it to start at login\n";

/// Runs the command line; returns the exit code.
pub fn main(args: Vec<String>) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = match args.as_slice() {
        ["run"] => run(),
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
    let platform = dyno_platform::platform_with(dirs.process_control());
    let details = dyno_platform::platform_with(dirs.root().join("process-control-details.json")).processes;
    (platform, details)
}

fn run() -> Result<(), String> {
    let dirs = Dirs::current();
    if connect(&dirs).is_ok() {
        println!("An agent is already running; nothing to do.");
        return Ok(());
    }
    install_signal_handlers();
    let (platform, details) = platforms(&dirs);
    let running = Running::start(dirs, platform, details, reveal())?;
    eprintln!("dyno-agent {} running (pid {})", env!("CARGO_PKG_VERSION"), running.discovery.pid);
    while !running.stopped() && !SIGNALLED.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
    running.stop();
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
    let mut p = dyno_platform::platform_with(std::env::temp_dir().join("dyno-once-state.json"));
    let t = crate::agent::now();
    p.sensors.sample(t);
    std::thread::sleep(Duration::from_millis(1_000));
    let sample = p.sensors.sample(crate::agent::now());
    println!("{}", serde_json::to_string_pretty(&sample).map_err(|e| e.to_string())?);
    Ok(())
}

fn service(cmd: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find this program: {e}"))?;
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
    let (platform, details) = platforms(&dirs);
    let running = Running::start(dirs.clone(), platform, details, Box::new(|_| Ok(())))?;
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
