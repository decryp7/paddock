//! The real ports of the supervisor (ADR 0025): the launcher that holds the agent process, and the readers of the world
//! (the discovery file, the agent lock, the login service, a connection) that answer a [`machine::Command`] with an event.
//!
//! Nothing here decides anything: a decision is a step of the pure machine. The adapters are thin and run on workers.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use super::machine::{Generation, HintSeen, LockSeen, Look, Seq};
use super::{plan, Plan, ServiceProbe, Sink};
use dyno_agent::discovery::Discovery;
use dyno_agent::paths::Dirs;
use dyno_core::protocol::{Message, PROTOCOL_VERSION};
use paddock_ipc::{BodyOptions, BodyPeer};
use paddock_proc::{liveness_of, tree_cpu, Cpu, Liveness, ProcessId};
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
    /// The process it started is there but stopped by the OS or another app (App Tamer): waited for, never ended (ADR 0031).
    Stopped {
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
    /// How it starts the agent, for the log (rule 56): `child` (the app's own child process) or `launchd` (a launchd job).
    fn via(&self) -> &'static str {
        "child"
    }
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
                Ok(None) => return running_or_stopped(c.id()),
                Ok(Some(status)) => {
                    // Told once (the slot forgets the child below): 3 means another agent holds the data folder.
                    match status.code() {
                        Some(code) => log::warn!(ExitCode = code; "The agent this app started exited"),
                        None => log::warn!(Signal = true; "The agent this app started was ended by a signal"),
                    }
                    self.exited = Some(status.code());
                    self.child = None;
                }
                // It cannot be asked: treat it as still running (it is still held; the quit ends it).
                Err(_) => return running_or_stopped(c.id()),
            }
        }
        match self.exited {
            Some(code) => Child::Exited { code },
            None => Child::None,
        }
    }
}

/// A held child that has not exited: stopped when the OS says so ([`liveness_of`]), else running. The launcher holds the process
/// unreaped, so the pid names it reliably (rule 53) and its identity is taken from the live process.
pub(crate) fn running_or_stopped(pid: u32) -> Child {
    match ProcessId::of(pid).map(|id| liveness_of(id, None)) {
        Some(Liveness::Paused) => Child::Stopped { pid },
        _ => Child::Running { pid },
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
                    paddock_proc::cleanup::logged("killing the child that did not stop in time", child.kill());
                    break match child.wait() {
                        Ok(status) => Some(status),
                        Err(e) => {
                            paddock_proc::cleanup::report("reaping the child that did not stop in time", &e);
                            None
                        }
                    };
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        slot.child = None;
        slot.exited = Some(status.and_then(|s| s.code()));
    }
}

/// How the app starts its own agent (when no login service runs one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchRoute {
    /// As a launchd job (macOS): macOS 27 shows the app's own child only its own sockets (Dyno ADR 0026).
    Launchd,
    /// As the app's own child process (Linux, Windows).
    Child,
}

/// The route for an OS name (`std::env::consts::OS`).
pub fn launch_route(os: &str) -> LaunchRoute {
    if os == "macos" {
        LaunchRoute::Launchd
    } else {
        LaunchRoute::Child
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
    /// The look is the machine's decision look (phase `Looking`): a file beside a held lock is connected to without the service
    /// ([`service_wanted`]). Every other look that asks the service (after a connection, a change, a wait) gets its answer.
    pub deciding: bool,
    /// The look is a recheck of the service in the wait for its agent (see `Command::Look::recheck`).
    pub recheck: bool,
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
) -> (Look, Option<Discovery>, Option<u64>) {
    let Ask {
        probe,
        cpu,
        lock,
        deciding,
        recheck,
    } = ask;
    let read_world = || {
        let found = Discovery::read(&dirs.discovery());
        let hint = found.as_ref().map(|d| hint_of(d, app_version));
        let lock = lock.then(|| {
            if dyno_agent::instance::is_held(dirs) {
                LockSeen::Held
            } else {
                LockSeen::Free
            }
        });
        World { found, hint, lock }
    };
    let first = read_world();
    let child = held.map(|g| (g, launcher.child()));
    let cpu = if !cpu {
        Cpu::NotRead
    } else {
        match child {
            Some((_, Child::Running { pid })) => cpu_of(pid),
            _ => Cpu::Unavailable,
        }
    };
    let asked = std::time::Instant::now();
    let (service, World { found, hint, lock }) = ask_watching(probe, first, read_world, deciding, recheck, WATCH_POLL);
    let service_ms = service.map(|_| u64::try_from(asked.elapsed().as_millis()).unwrap_or(u64::MAX));
    let look = Look {
        seq,
        now_ms: now_ms(),
        service,
        hint,
        lock,
        child,
        cpu,
    };
    (look, found, service_ms)
}

/// What a look has read of the world besides the service: the discovery file (and its hint) and the agent lock.
pub struct World {
    /// The discovery file, as read.
    pub found: Option<Discovery>,
    /// What the file says.
    pub hint: Option<HintSeen>,
    /// Whether an agent holds the lock (when probed).
    pub lock: Option<LockSeen>,
}

/// How often a look that waits for the login service reads the discovery file and the lock again.
const WATCH_POLL: Duration = Duration::from_millis(100);

/// Whether a look must wait for the login service. A discovery file beside a held lock is decided on without it (the machine
/// connects to that agent, or asks it to exit): the service is asked again right after the connection, for the hand-over. A
/// recheck in the wait for the agent does not need it while the lock is held (it only counts rechecks that find the lock free).
/// Everything else is decided on what the service says (wait for its agent, or start one).
fn service_wanted(world: &World, deciding: bool, recheck: bool) -> bool {
    if deciding {
        !(world.hint.is_some() && world.lock == Some(LockSeen::Held))
    } else if recheck {
        world.lock != Some(LockSeen::Held)
    } else {
        true
    }
}

/// The service's answer for a look that needs it, and the world as last read. The probe can take tens of seconds at login
/// (`SMAppService.status` is an xpc round trip to smd; 21 s on the loaded VM, 2026-11-01, while launchd's agent wrote its file 11 s
/// into it), so the probe runs on its own thread and the look reads the file and the lock again every `poll`: as soon as the world
/// says the answer is not needed (an agent holds the lock) the look returns without it, instead of reporting the world it saw
/// before the wait.
fn ask_watching(
    probe: Option<&ServiceProbe>,
    mut world: World,
    mut reread: impl FnMut() -> World,
    deciding: bool,
    recheck: bool,
    poll: Duration,
) -> (Option<super::ServiceState>, World) {
    let Some(probe) = probe.filter(|_| service_wanted(&world, deciding, recheck)) else {
        return (None, world);
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let ask = probe.clone();
    let spawned = std::thread::Builder::new().name("dyno-service-ask".into()).spawn(move || {
        // silent: the receiver is gone when the caller stopped waiting for the answer; nobody reads it then
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = tx.send(ask());
    });
    if spawned.is_err() {
        return (Some(probe()), world);
    }
    loop {
        match rx.recv_timeout(poll) {
            Ok(answer) => return (Some(answer), world),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                world = reread();
                if !service_wanted(&world, deciding, recheck) {
                    return (None, world);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return (None, world),
        }
    }
}

/// A routine look (one of a wait's ticks) is told only when it took at least this long: a slow look is a finding, a fast one noise.
pub const SLOW_LOOK_MS: u64 = 1_000;

/// One look in words for the log, or `None` for a routine fast look. A decision look (the start of an attempt) is always told:
/// what the service, the discovery file and the lock said and how long it all took (`service_ms`: the login service's own part,
/// `SMAppService.status` and `launchctl`, the step that took 30 s on a busy login). Fixed words and numbers only.
pub fn describe_look(look: &Look, deciding: bool, took_ms: u64, service_ms: Option<u64>) -> Option<String> {
    if !deciding && took_ms < SLOW_LOOK_MS {
        return None;
    }
    let service = match (look.service, service_ms) {
        (Some(s), Some(ms)) => format!(
            "service asked, answered in {ms} ms ({}, {})",
            if s.installed { "installed" } else { "not installed" },
            if s.supervises { "supervises" } else { "does not supervise" }
        ),
        (Some(_), None) => "service asked".to_string(),
        (None, _) => "service not asked".to_string(),
    };
    let file = match &look.hint {
        None => "no file".to_string(),
        Some(h) => format!(
            "file of {}, {} s old",
            if h.same_build { "this build" } else { "another build" },
            (look.now_ms - h.started_ms).max(0) / 1_000
        ),
    };
    let lock = match look.lock {
        Some(LockSeen::Held) => "lock held",
        Some(LockSeen::Free) => "lock free",
        None => "lock not probed",
    };
    let child = match look.child {
        None | Some((_, Child::None)) => "child none",
        Some((_, Child::Running { .. })) => "child running",
        Some((_, Child::Stopped { .. })) => "child stopped",
        Some((_, Child::Exited { .. })) => "child exited",
    };
    let kind = if deciding { "look" } else { "look (slow)" };
    Some(format!("{kind}: {service}; {file}; {lock}; {child}; took {took_ms} ms"))
}

/// A successful connection in words: whom it was for and how long the handshake took.
pub fn describe_connect(purpose: super::machine::Purpose, handshake_ms: u64) -> String {
    match purpose {
        super::machine::Purpose::Use => format!("connected to the agent to use it: handshake {handshake_ms} ms"),
        super::machine::Purpose::AskToExit => {
            format!("connected to an agent of another build to ask it to exit: handshake {handshake_ms} ms")
        }
    }
}

/// Hands one pushed message to the sink.
pub(super) fn deliver(sink: &dyn Sink, m: Message) {
    match m {
        Message::Live(l) => sink.live(*l),
        Message::Alert(a) => sink.alert(a),
        Message::Activity(a) => sink.activity(a),
        Message::Alarm(c) => sink.alarm(c),
        Message::DiskScan(s) => sink.disk_scan(*s),
        Message::Job(j) => sink.job(*j),
        Message::DiskVolumes(v) => sink.disk_volumes(v),
        Message::WidgetLayoutChanged(surface) => sink.widget_layout_changed(surface),
        Message::PowerChanged(view) => sink.power_changed(*view),
        Message::WolChanged(view) => sink.wol_changed(*view),
        Message::WakesChanged(view) => sink.wakes_changed(*view),
        Message::EventsLost(lost) => sink.events_lost(lost),
        _ => {}
    }
}

/// Connects to the agent a discovery file names and starts the peer that fans its pushes out to the sink.
pub fn connect(d: &Discovery, sink: &Arc<dyn Sink>) -> Result<Arc<BodyPeer<Message>>, String> {
    let token = d.token().ok_or("The discovery file is damaged.")?;
    let conn = dyno_agent::wire::connect(&d.endpoint, &token).map_err(|e| format!("The agent did not answer ({e})."))?;
    let sink = sink.clone();
    let opts = BodyOptions::new("the agent").on_push(move |m| deliver(sink.as_ref(), m));
    BodyPeer::start(conn, opts).map(Arc::new).map_err(|e| e.to_string())
}

/// The CPU time of the tree of the child this app holds, as the start judge reads it. The pid is the held child's (the launcher
/// keeps the process unreaped, so the pid cannot have been handed on, rule 53); the identity is taken from the live process.
fn cpu_of(pid: u32) -> Cpu {
    ProcessId::of(pid).map_or(Cpu::Unavailable, tree_cpu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn hint(same_build: bool) -> HintSeen {
        HintSeen {
            pid: 9,
            started_ms: 500,
            version: "1.0.0".into(),
            protocol: 1,
            same_build,
        }
    }

    fn world(hint: Option<HintSeen>, lock: Option<LockSeen>) -> World {
        World { found: None, hint, lock }
    }

    /// The service's answer for a world that does not change while it is asked.
    fn ask(
        probe: Option<&ServiceProbe>,
        hint: Option<&HintSeen>,
        lock: Option<LockSeen>,
        deciding: bool,
        recheck: bool,
    ) -> Option<super::super::ServiceState> {
        ask_watching(
            probe,
            world(hint.cloned(), lock),
            || unreachable!("the probe answers at once"),
            deciding,
            recheck,
            Duration::from_secs(30),
        )
        .0
    }

    fn wanted(hint: Option<HintSeen>, lock: Option<LockSeen>) -> bool {
        service_wanted(&world(hint, lock), true, false)
    }

    /// The restore start on the VM (2026-11-01 00:07): the decision look read "no file, lock free", then asked the service for 21 s
    /// while launchd's agent took the lock and wrote its file 11 s in; the look reported the world it saw before the wait and the app
    /// connected 0.85 s after the answer, 21 s in. A look that waits for the service reads the file and the lock again, and returns as
    /// soon as they say the answer is not needed.
    #[test]
    fn a_look_waiting_for_the_service_returns_when_an_agent_appears() {
        let (release, blocked) = std::sync::mpsc::channel::<()>();
        let blocked = Mutex::new(blocked);
        let probe: ServiceProbe = Arc::new(move || {
            let _ = blocked.lock().unwrap().recv(); // the service answers only when the test ends
            super::super::ServiceState {
                installed: true,
                supervises: true,
            }
        });
        let reads = std::cell::Cell::new(0);
        let (answer, seen) = ask_watching(
            Some(&probe),
            world(None, Some(LockSeen::Free)),
            || {
                reads.set(reads.get() + 1);
                // The third read finds the agent: its file and its lock.
                if reads.get() < 3 {
                    world(None, Some(LockSeen::Free))
                } else {
                    world(Some(hint(true)), Some(LockSeen::Held))
                }
            },
            true,
            false,
            Duration::from_millis(1),
        );
        assert!(answer.is_none(), "returned without the service's answer");
        assert_eq!(
            (seen.hint.is_some(), seen.lock),
            (true, Some(LockSeen::Held)),
            "with the world it read last"
        );
        assert_eq!(reads.get(), 3);
        drop(release);
    }

    /// The answer, when it comes first, is returned with the world read before it.
    #[test]
    fn a_look_gets_the_service_answer_when_no_agent_appears() {
        let probe: ServiceProbe = Arc::new(|| super::super::ServiceState {
            installed: true,
            supervises: true,
        });
        let (answer, seen) = ask_watching(
            Some(&probe),
            world(None, Some(LockSeen::Free)),
            || world(None, Some(LockSeen::Free)),
            true,
            false,
            Duration::from_secs(30),
        );
        assert!(answer.is_some() && seen.hint.is_none());
    }

    /// The VM's login (2026-10-31): `SMAppService.status` blocked the look for about 30 s (`-[SMAppService status]` in the xpc round
    /// trip to smd) while the agent launchd had started held the lock and its file was there. A file beside a held lock gets a
    /// connection (or a request to exit) whatever the service says, so that look does not ask the service.
    #[test]
    fn a_file_beside_a_held_lock_is_decided_without_the_service() {
        assert!(!wanted(Some(hint(true)), Some(LockSeen::Held)));
        assert!(!wanted(Some(hint(false)), Some(LockSeen::Held)));
        // Anything else is decided on what the service says: wait for its agent, or start one.
        assert!(wanted(None, Some(LockSeen::Held)));
        assert!(wanted(Some(hint(true)), Some(LockSeen::Free)));
        assert!(wanted(Some(hint(true)), None));
        assert!(wanted(None, Some(LockSeen::Free)));
    }

    #[test]
    fn the_probe_is_not_called_when_the_look_does_not_need_it() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let probe: ServiceProbe = Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            super::super::ServiceState {
                installed: true,
                supervises: true,
            }
        });
        assert!(ask(Some(&probe), Some(&hint(true)), Some(LockSeen::Free), true, false).is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(ask(Some(&probe), Some(&hint(true)), Some(LockSeen::Held), true, false).is_none());
        assert!(ask(None, None, Some(LockSeen::Free), true, false).is_none(), "no probe, no answer");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the held-lock decision look never called it");
        // A look that is not the decision (after a connection, a service change, a wait) always gets the service's answer.
        assert!(ask(Some(&probe), Some(&hint(true)), Some(LockSeen::Held), false, false).is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// The restore-at-login start on the loaded VM (2026-10-31): the agent launchd started was up and its file written at 22:18:47, but
    /// the app's recheck look had begun before and asked the service for 10 s (`SMAppService.status` under load), and the next one asked
    /// again for 2 s: connected at 22:18:59, 12 s late. A recheck look that finds the lock held does not need the service (the lock says
    /// an agent runs; the service answer only counts rechecks while the lock is free), so it does not wait for it.
    #[test]
    fn a_wait_recheck_that_finds_the_lock_held_does_not_ask_the_service() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let probe: ServiceProbe = Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            super::super::ServiceState {
                installed: true,
                supervises: true,
            }
        });
        // Held, with or without the file: not asked.
        assert!(ask(Some(&probe), Some(&hint(true)), Some(LockSeen::Held), false, true).is_none());
        assert!(ask(Some(&probe), None, Some(LockSeen::Held), false, true).is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // Free (nothing runs: the service may be starting nothing) or not probed: asked, as before.
        assert!(ask(Some(&probe), None, Some(LockSeen::Free), false, true).is_some());
        assert!(ask(Some(&probe), None, None, false, true).is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    fn seen_look(hint: Option<HintSeen>, lock: Option<LockSeen>, service: Option<super::super::ServiceState>) -> Look {
        Look {
            seq: 3,
            now_ms: 10_000,
            hint,
            lock,
            service,
            child: None,
            cpu: Cpu::NotRead,
        }
    }

    /// Login-start diagnosis (2026-10-31): a bundle must show, per decision look, what the service, the discovery file and the lock
    /// said and how long it all took, with names and numbers only. Routine looks of a wait stay out of the log unless slow.
    #[test]
    fn a_decision_look_is_told_with_the_service_the_file_the_lock_and_the_times() {
        let service = super::super::ServiceState {
            installed: true,
            supervises: true,
        };
        let look = seen_look(
            Some(HintSeen {
                started_ms: 4_000,
                ..hint(true)
            }),
            Some(LockSeen::Held),
            Some(service),
        );
        let line = describe_look(&look, true, 2_345, Some(1_800)).unwrap();
        assert_eq!(
            line,
            "look: service asked, answered in 1800 ms (installed, supervises); file of this build, 6 s old; lock held; child none; took 2345 ms"
        );
        let bare = describe_look(&seen_look(None, Some(LockSeen::Free), None), true, 4, None).unwrap();
        assert_eq!(bare, "look: service not asked; no file; lock free; child none; took 4 ms");
        let other = describe_look(&seen_look(Some(hint(false)), None, None), true, 5, None).unwrap();
        assert!(
            other.contains("file of another build") && other.contains("lock not probed"),
            "{other}"
        );
    }

    #[test]
    fn a_routine_look_is_told_only_when_it_was_slow() {
        let look = seen_look(None, None, None);
        assert_eq!(describe_look(&look, false, 12, None), None);
        assert_eq!(describe_look(&look, false, SLOW_LOOK_MS - 1, None), None);
        let slow = describe_look(&look, false, SLOW_LOOK_MS, Some(SLOW_LOOK_MS)).unwrap();
        assert!(
            slow.starts_with("look (slow):") && slow.contains(&format!("took {SLOW_LOOK_MS} ms")),
            "{slow}"
        );
    }

    #[test]
    fn a_connection_is_told_with_its_purpose_and_how_long_the_handshake_took() {
        assert_eq!(
            describe_connect(super::super::machine::Purpose::Use, 37),
            "connected to the agent to use it: handshake 37 ms"
        );
        assert_eq!(
            describe_connect(super::super::machine::Purpose::AskToExit, 5),
            "connected to an agent of another build to ask it to exit: handshake 5 ms"
        );
    }
}
