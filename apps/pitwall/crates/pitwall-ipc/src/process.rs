//! Starting a plugin process (ADR 0008 §2, copied and adapted from IMT's `imt-ipc::process`): an empty environment
//! plus an allow-list, no console window and below-normal priority, its sandbox applied before it runs (see
//! [`crate::sandbox`]), stdin as its lifeline (the plugin exits when it closes, i.e. when Pitwall goes away) and,
//! on Windows, a job object that kills it with Pitwall. Its standard error is kept as a short bounded tail for
//! diagnostics; its standard output is discarded.

use crate::sandbox::{Note, SandboxSpec};
use crate::IpcError;
use std::collections::VecDeque;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Environment variables a plugin inherits (nothing else of Pitwall's environment: no tokens, no proxies, no user
/// variables): what the OS and the Rust runtime need, and the coverage profile path for instrumented test builds.
pub const KEPT_ENV: &[&str] = &[
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "TEMP",
    "TMP",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "RUST_BACKTRACE",
    "LLVM_PROFILE_FILE",
    "CARGO_LLVM_COV",
];

/// How long a plugin whose lifeline was cut may take to exit before it is killed.
pub const KILL_GRACE: Duration = Duration::from_millis(200);
/// Lines of standard error kept per plugin.
pub const STDERR_LINES: usize = 40;
/// Longest standard error line kept (longer lines are cut).
pub const STDERR_LINE_BYTES: usize = 1024;

/// How to start a plugin process.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// The program.
    pub exe: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Extra environment (on top of [`SpawnSpec::keep_env`]).
    pub env: Vec<(String, String)>,
    /// The only variables of Pitwall's environment the plugin inherits (default [`KEPT_ENV`]).
    pub keep_env: &'static [&'static str],
    /// Working directory (default: the first readable folder of the sandbox).
    pub current_dir: Option<PathBuf>,
    /// Names the process in messages: `plugin "redact-filter"`.
    pub label: String,
    /// Prefix of the private address (see [`crate::endpoint::PrivateEndpoint::create_named`]).
    pub endpoint_prefix: String,
    /// How long the plugin may take to connect.
    pub connect_window: Duration,
    /// Run at below-normal priority so inference stays responsive.
    pub below_normal_priority: bool,
    /// What the plugin may do.
    pub sandbox: SandboxSpec,
}

impl SpawnSpec {
    /// A spec with the defaults: kept environment only, below-normal priority, 15 s to connect.
    pub fn new(exe: impl Into<PathBuf>, label: impl Into<String>, sandbox: SandboxSpec) -> SpawnSpec {
        SpawnSpec {
            exe: exe.into(),
            args: Vec::new(),
            env: Vec::new(),
            keep_env: KEPT_ENV,
            current_dir: None,
            label: label.into(),
            endpoint_prefix: "pitwall-plugin".into(),
            connect_window: Duration::from_secs(15),
            below_normal_priority: true,
            sandbox,
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The OS process: started by the standard library (Unix) or in an AppContainer (Windows).
enum OsChild {
    #[cfg(not(windows))]
    Std(std::process::Child),
    #[cfg(windows)]
    Contained(crate::contain::Contained),
}

impl OsChild {
    fn id(&self) -> u32 {
        match self {
            #[cfg(not(windows))]
            OsChild::Std(c) => c.id(),
            #[cfg(windows)]
            OsChild::Contained(c) => c.id(),
        }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match self {
            #[cfg(not(windows))]
            OsChild::Std(c) => c.try_wait(),
            #[cfg(windows)]
            OsChild::Contained(c) => c.try_wait(),
        }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(not(windows))]
            OsChild::Std(c) => c.kill(),
            #[cfg(windows)]
            OsChild::Contained(c) => c.kill(),
        }
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        match self {
            #[cfg(not(windows))]
            OsChild::Std(c) => c.wait(),
            #[cfg(windows)]
            OsChild::Contained(c) => c.wait(),
        }
    }
}

#[cfg(windows)]
type Job = crate::job::JobGuard;
#[cfg(not(windows))]
type Job = ();

/// A started plugin process. Killed (and reaped) on drop.
pub struct ChildProcess {
    child: Arc<Mutex<Option<OsChild>>>,
    stdin: Mutex<Option<Box<dyn Write + Send>>>,
    pid: u32,
    label: String,
    memory_limit: Option<u64>,
    /// Set once the host stopped it for using more memory than its cap (Unix watcher).
    over_memory: Arc<AtomicU64>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    notes: Vec<Note>,
    #[cfg_attr(not(windows), allow(dead_code))]
    job: Job,
}

impl ChildProcess {
    /// Starts `spec` confined by its sandbox, with a piped stdin (its lifeline). `endpoint` is the host's address
    /// (macOS allows exactly this socket through the network sandbox). Fails, without starting anything, when the
    /// sandbox cannot be applied (fail closed).
    pub fn spawn(spec: &SpawnSpec, endpoint: &str) -> Result<ChildProcess, IpcError> {
        let fail = |e: String| IpcError::Spawn(format!("cannot start {} ({}): {e}", spec.label, spec.exe.display()));
        if !SandboxSpec::valid_name(&spec.sandbox.name) {
            return Err(fail(format!("\"{}\" is not a valid sandbox name", spec.sandbox.name)));
        }
        platform::spawn(spec, endpoint).map_err(fail)
    }

    fn assemble(spec: &SpawnSpec, child: OsChild, stdin: Box<dyn Write + Send>, stderr: Option<Box<dyn Read + Send>>, job: Job) -> ChildProcess {
        let pid = child.id();
        let process = ChildProcess {
            child: Arc::new(Mutex::new(Some(child))),
            stdin: Mutex::new(Some(stdin)),
            pid,
            label: spec.label.clone(),
            memory_limit: spec.sandbox.memory_limit,
            over_memory: Arc::new(AtomicU64::new(0)),
            stderr: Arc::new(Mutex::new(VecDeque::new())),
            notes: spec.sandbox.enforcement(),
            job,
        };
        if let Some(err) = stderr {
            capture_stderr(err, process.stderr.clone());
        }
        #[cfg(not(windows))]
        if let Some(limit) = spec.sandbox.memory_limit {
            watch_memory(limit, pid, process.child.clone(), process.over_memory.clone());
        }
        process
    }

    /// The process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// How this platform sandboxes the plugin (see [`SandboxSpec::enforcement`]).
    pub fn sandbox_notes(&self) -> &[Note] {
        &self.notes
    }

    /// The last lines the plugin wrote to standard error (bounded; for diagnostics only, never logged by default).
    pub fn stderr_tail(&self) -> Vec<String> {
        lock(&self.stderr).iter().cloned().collect()
    }

    /// Writes `value` as one JSON line to the plugin's stdin (its bootstrap).
    pub fn send_line<T: serde::Serialize>(&self, value: &T) -> Result<(), IpcError> {
        let line = serde_json::to_string(value).map_err(|e| IpcError::Protocol(e.to_string()))?;
        let wrote = lock(&self.stdin)
            .as_mut()
            .map(|s| s.write_all(format!("{line}\n").as_bytes()).and_then(|_| s.flush()));
        match wrote {
            Some(Ok(())) => Ok(()),
            _ => Err(IpcError::Died(match self.exit_soon() {
                Some(s) => format!("{} exited while starting ({s})", self.label),
                None => format!("{} closed its input while starting", self.label),
            })),
        }
    }

    /// The exit status, if the plugin has exited.
    pub fn try_exit(&self) -> Option<ExitStatus> {
        lock(&self.child).as_mut().and_then(|c| c.try_wait().ok().flatten())
    }

    /// Whether the plugin has exited (or was killed).
    pub fn exited(&self) -> bool {
        lock(&self.child).as_mut().is_none_or(|c| matches!(c.try_wait(), Ok(Some(_))))
    }

    /// Whether the plugin was stopped for using more memory than its cap.
    pub fn stopped_over_memory(&self) -> bool {
        if self.over_memory.load(Ordering::SeqCst) > 0 {
            return true;
        }
        #[cfg(windows)]
        if let (Some(limit), Some(peak)) = (self.memory_limit, self.job.peak_memory()) {
            // The job refuses allocations past the cap; a plugin that died close to it ran out.
            return peak.saturating_add(limit / 16) >= limit;
        }
        false
    }

    /// Describes how the plugin ended, waiting briefly for the OS to publish its exit status.
    pub fn exit_text(&self) -> String {
        if let (true, Some(limit)) = (self.stopped_over_memory(), self.memory_limit) {
            return format!(
                "{} used more than its memory cap of {} and was stopped",
                self.label,
                crate::memory::megabytes(limit)
            );
        }
        match self.exit_soon() {
            Some(s) => format!("{} exited ({s})", self.label),
            None => format!("{} closed the connection", self.label),
        }
    }

    fn exit_soon(&self) -> Option<ExitStatus> {
        for _ in 0..20 {
            if let Some(s) = self.try_exit() {
                return Some(s);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    /// Stops the plugin and reaps it: its lifeline is closed first (a well-behaved plugin exits at once); one that
    /// has not after [`KILL_GRACE`] is killed.
    pub fn kill(&self) {
        *lock(&self.stdin) = None;
        let taken = lock(&self.child).take();
        if let Some(mut c) = taken {
            let deadline = Instant::now() + KILL_GRACE;
            while Instant::now() < deadline {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Keeps the last [`STDERR_LINES`] lines of the plugin's standard error, until it closes.
fn capture_stderr(stderr: Box<dyn Read + Send>, tail: Arc<Mutex<VecDeque<String>>>) {
    let spawned = std::thread::Builder::new().name("pitwall-plugin-stderr".into()).spawn(move || {
        let mut r = std::io::BufReader::new(stderr);
        let mut line = Vec::new();
        loop {
            line.clear();
            // Bounded: a line without an end is cut, and the rest read as further lines.
            match r.by_ref().take(STDERR_LINE_BYTES as u64).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&line).trim_end().to_string();
                    let mut t = lock(&tail);
                    if t.len() == STDERR_LINES {
                        t.pop_front();
                    }
                    t.push_back(text);
                }
            }
        }
    });
    // Without a thread the pipe is dropped with the closure: the plugin's writes fail instead of blocking.
    drop(spawned);
}

/// Unix: stops the plugin once its resident memory is over `limit`.
#[cfg(not(windows))]
fn watch_memory(limit: u64, pid: u32, child: Arc<Mutex<Option<OsChild>>>, over: Arc<AtomicU64>) {
    let spawned = std::thread::Builder::new().name("pitwall-plugin-memory".into()).spawn(move || loop {
        std::thread::sleep(crate::memory::SAMPLE_EVERY);
        let mut guard = lock(&child);
        let Some(c) = guard.as_mut() else { return };
        if matches!(c.try_wait(), Ok(Some(_))) {
            return;
        }
        if let Some(used) = crate::memory::resident_bytes(pid) {
            if crate::memory::over_cap(used, limit) {
                over.store(used, Ordering::SeqCst);
                let _ = c.kill();
                return;
            }
        }
    });
    drop(spawned);
}

#[cfg(windows)]
mod platform {
    use super::{ChildProcess, OsChild, SpawnSpec};

    pub fn spawn(spec: &SpawnSpec, _endpoint: &str) -> Result<ChildProcess, String> {
        let s = &spec.sandbox;
        let container = crate::contain::Container::open(&s.name).map_err(|e| format!("cannot prepare its sandbox: {e}"))?;
        for (paths, write) in [(&s.read, false), (&s.write, true)] {
            for p in paths {
                container.grant(p, write).map_err(|e| e.to_string())?;
            }
        }
        let mut contained = spec.clone();
        // The container can only enter folders granted to it.
        if !s.read.iter().chain(&s.write).any(|r| Some(r) == contained.current_dir.as_ref()) {
            contained.current_dir = s.read.first().cloned();
        }
        let limits = crate::job::JobLimits::of(s);
        let started = crate::contain::spawn(&contained, &container, s.network, &limits, true).map_err(|e| e.to_string())?;
        let stderr = started.stderr.map(|f| Box::new(f) as Box<dyn std::io::Read + Send>);
        Ok(ChildProcess::assemble(
            spec,
            OsChild::Contained(started.child),
            Box::new(started.stdin),
            stderr,
            started.job,
        ))
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
mod platform {
    use super::{ChildProcess, OsChild, SpawnSpec};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    /// The program and arguments that start `spec`: `sandbox-exec` with the profile in front of it.
    #[cfg(target_os = "macos")]
    fn launcher(spec: &SpawnSpec, endpoint: &str) -> Result<(PathBuf, Vec<String>), String> {
        let socket = std::path::Path::new(endpoint);
        // The sandbox matches resolved paths (/var is /private/var).
        let socket = match (socket.parent().and_then(|d| d.canonicalize().ok()), socket.file_name()) {
            (Some(dir), Some(name)) => dir.join(name),
            _ => socket.to_path_buf(),
        };
        let exe = spec.exe.canonicalize().unwrap_or_else(|_| spec.exe.clone());
        let profile = crate::sandbox::macos_profile(&spec.sandbox, &exe, &socket);
        let mut args = vec!["-p".to_string(), profile, exe.to_string_lossy().into_owned()];
        args.extend(spec.args.iter().cloned());
        Ok((PathBuf::from("/usr/bin/sandbox-exec"), args))
    }

    /// The program and arguments that start `spec` (the sandbox is applied before `exec`).
    #[cfg(target_os = "linux")]
    fn launcher(spec: &SpawnSpec, _endpoint: &str) -> Result<(PathBuf, Vec<String>), String> {
        Ok((spec.exe.clone(), spec.args.clone()))
    }

    /// The program and arguments that start `spec`: only with the network permission (no sandbox here).
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn launcher(spec: &SpawnSpec, _endpoint: &str) -> Result<(PathBuf, Vec<String>), String> {
        if spec.sandbox.network {
            Ok((spec.exe.clone(), spec.args.clone()))
        } else {
            Err("this platform has no network sandbox, and the plugin has no network permission".into())
        }
    }

    pub fn spawn(spec: &SpawnSpec, endpoint: &str) -> Result<ChildProcess, String> {
        let (exe, args) = launcher(spec, endpoint)?;
        // The one spawner of plugin processes: below-normal priority, cleared environment, sandboxed (ADR 0008).
        #[allow(clippy::disallowed_methods)]
        let mut cmd = Command::new(&exe);
        cmd.args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        cmd.env_clear();
        for k in spec.keep_env {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        cmd.envs(spec.env.iter().map(|(k, v)| (k, v)));
        if let Some(d) = spec.current_dir.as_ref().or(spec.sandbox.read.first()) {
            cmd.current_dir(d);
        }
        #[cfg(target_os = "linux")]
        let report = crate::sandbox::contain_before_exec(&mut cmd, &spec.sandbox)?;
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        // Lowered from here rather than in a `pre_exec` hook (see IMT: a hook forces fork+exec on macOS).
        if spec.below_normal_priority {
            // SAFETY: setpriority takes plain integers and has no memory effects; it only lowers our child's priority.
            unsafe {
                libc::setpriority(libc::PRIO_PROCESS, child.id() as libc::id_t, 10);
            }
        }
        let stdin = match child.stdin.take() {
            Some(s) => Box::new(s) as Box<dyn std::io::Write + Send>,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("no stdin pipe".into());
            }
        };
        let stderr = child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>);
        #[allow(unused_mut)]
        let mut process = ChildProcess::assemble(spec, OsChild::Std(child), stdin, stderr, ());
        #[cfg(target_os = "linux")]
        if let Some(r) = report {
            process.notes.insert(1, r.note());
        }
        Ok(process)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_or_a_bad_sandbox_name_is_a_spawn_error() {
        // A folder that does not exist: nothing is granted to the sandbox anywhere.
        let dir = std::env::temp_dir().join("pitwall-definitely-missing-folder");
        let spec = SpawnSpec::new(dir.join("missing.exe"), "the missing plugin", SandboxSpec::locked("pitwall.test.missing", &dir));
        assert!(matches!(ChildProcess::spawn(&spec, "x"), Err(IpcError::Spawn(m)) if m.contains("the missing plugin")));
        #[cfg(windows)]
        let _ = crate::contain::Container::delete("pitwall.test.missing");
        let bad = SpawnSpec::new("x", "p", SandboxSpec::locked("bad name", &dir));
        assert!(matches!(ChildProcess::spawn(&bad, "x"), Err(IpcError::Spawn(m)) if m.contains("sandbox name")));
    }

    #[test]
    fn the_default_environment_holds_no_secrets() {
        for k in KEPT_ENV {
            let up = k.to_ascii_uppercase();
            assert!(!up.contains("TOKEN") && !up.contains("PROXY") && !up.contains("KEY") && up != "PATH" && up != "HOME", "{k}");
        }
        let s = SpawnSpec::new("x", "p", SandboxSpec::locked("n", "/p"));
        assert!(s.below_normal_priority && s.env.is_empty());
    }
}
