//! The real ports of the supervisor (ADR 0025): the launcher that holds the agent process, and the readers of the world
//! (the discovery file, the agent lock, the login service, a connection) that answer a [`machine::Command`] with an event.
//!
//! Nothing here decides anything: a decision is a step of the pure machine. The adapters are thin and run on workers.

use super::machine::{Generation, HintSeen, LockSeen, Look, Seq};
use super::{plan, Plan, ServiceProbe, Sink};
use dyno_agent::discovery::Discovery;
use dyno_agent::paths::Dirs;
use dyno_core::protocol::{Message, PROTOCOL_VERSION};
use dyno_ipc::{Client, Peer, PeerOptions};
use paddock_proc::{tree_cpu, Cpu, ProcessId};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every guarded value stays consistent if a holder panicked.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The agent process a [`Launcher`] started, as far as it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// No more launches: the app is quitting. Must not block (it is called on the supervisor's owner thread): a launch that
    /// was queued before it sees it and does not run.
    fn close(&self) {}
    /// Waits briefly for the process this launcher started to exit (after `Shutdown`), ending it when it does not.
    fn finish(&self, wait: Duration);
}

// ---------------------------------------------------------------------------------------------------------------

/// Starts the real agent program detached (no console, its own process group, not tied to the app's lifetime).
pub struct ProcessLauncher {
    exe: Box<dyn Fn() -> Result<std::path::PathBuf, String> + Send + Sync>,
    slot: Mutex<Slot>,
    /// No more launches (an atomic, so closing never waits for a launch or an end in progress).
    closed: std::sync::atomic::AtomicBool,
}

/// The process a [`ProcessLauncher`] started, held until it has exited and been reaped.
#[derive(Default)]
struct Slot {
    child: Option<std::process::Child>,
    /// The exit code of the last one, once reaped (`Some(None)`: ended by a signal).
    exited: Option<Option<i32>>,
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
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&self) -> Result<(), String> {
        // Held across the start: a quit (`close`, `finish`) waits for it and then sees the new process.
        let mut slot = lock(&self.slot);
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
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
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn finish(&self, wait: Duration) {
        // Held throughout: nothing is launched while the process this launcher started is being ended.
        // cap: the grace a child gets to exit before it is ended through its handle (ADR 0031 class S: a quit must end);
        // `wait` is that grace, passed by the driver (3 s) and the one deliberate time limit of the supervisor.
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

/// Now, in milliseconds since the epoch (what an agent writes as `started`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The discovery file as a hint: what it names, and whether it is of this build.
pub fn hint_of(d: &Discovery, app_version: &str) -> HintSeen {
    HintSeen {
        pid: d.pid,
        started_ms: d.started,
        version: d.version.clone(),
        protocol: d.protocol,
        same_build: plan(Some(d), app_version, PROTOCOL_VERSION) == Plan::Connect,
    }
}

/// What a look is asked to read besides the discovery file and the launcher's child.
#[derive(Clone, Copy)]
pub struct Ask<'a> {
    /// The login service's probe, when the OS is to be asked.
    pub probe: Option<&'a ServiceProbe>,
    /// Read the CPU time of the held child's tree.
    pub cpu: bool,
    /// Probe the agent lock.
    pub lock: bool,
}

/// One look at the world for the machine: the discovery file (bounded read), the agent lock, the launcher's child with
/// the generation it was held under, and the login service when asked. Returns the file too, so a later `RemoveHint` and
/// `Connect` act on the very file that was seen.
pub fn look(
    seq: Seq,
    dirs: &Dirs,
    app_version: &str,
    launcher: &dyn Launcher,
    held: Option<Generation>,
    ask: Ask<'_>,
) -> (Look, Option<Discovery>) {
    let Ask { probe, cpu, lock } = ask;
    let found = Discovery::read(&dirs.discovery());
    let child = held.map(|g| (g, launcher.child()));
    let cpu = if !cpu {
        Cpu::NotRead
    } else {
        match child {
            Some((_, Child::Running { pid })) => cpu_of(pid),
            _ => Cpu::Unavailable,
        }
    };
    let look = Look {
        seq,
        now_ms: now_ms(),
        hint: found.as_ref().map(|d| hint_of(d, app_version)),
        lock: lock.then(|| {
            if dyno_agent::instance::is_held(dirs) {
                LockSeen::Held
            } else {
                LockSeen::Free
            }
        }),
        service: probe.map(|p| p()),
        child,
        cpu,
    };
    (look, found)
}

/// Connects to the agent a discovery file names and starts the peer that fans its pushes out to the sink.
pub fn connect(d: &Discovery, sink: &Arc<dyn Sink>) -> Result<Arc<Peer<Message>>, String> {
    let token = d.token().ok_or("The discovery file is damaged.")?;
    let conn = Client::connect(&d.endpoint, &token).map_err(|e| format!("The agent did not answer ({e})."))?;
    let sink = sink.clone();
    let opts = PeerOptions::new().on_push(move |m| match m {
        Message::Live(l) => sink.live(*l),
        Message::Alert(a) => sink.alert(a),
        Message::Activity(a) => sink.activity(a),
        Message::Alarm(c) => sink.alarm(c),
        Message::DiskScan(s) => sink.disk_scan(*s),
        Message::Job(j) => sink.job(*j),
        Message::DiskVolumes(v) => sink.disk_volumes(v),
        Message::WidgetLayoutChanged(surface) => sink.widget_layout_changed(surface),
        _ => {}
    });
    Peer::start(conn, opts).map(Arc::new).map_err(|e| e.to_string())
}

/// The CPU time of the tree of the child this app holds, as the start judge reads it. The pid is the held child's (the launcher
/// keeps the process unreaped, so the pid cannot have been handed on, rule 53); the identity is taken from the live process.
fn cpu_of(pid: u32) -> Cpu {
    ProcessId::of(pid).map_or(Cpu::Unavailable, tree_cpu)
}
