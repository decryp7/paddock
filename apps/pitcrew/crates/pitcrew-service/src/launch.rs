//! Starting a runner process with its standard output and error piped to the service, and stopping it.
//!
//! - Every line the runner writes goes to a [`LineSink`] (the supervisor: log file, tail, classifier). Lines are cut
//!   at `MAX_LINE_LEN` and decoded lossily; nothing the runner writes can make the service allocate without limit.
//! - Graceful stop: on Unix SIGTERM to the runner's own process group; on Windows Ctrl+Break to its process group,
//!   sent by a short-lived helper (`pitcrew-service ctrl-break <pid>`) that attaches to the runner's hidden console,
//!   since a service has no console of its own (ADR 0003). The runner then stops polling and finishes a running job.
//! - Kill (only after the stop timeout and only while no job runs): the whole process tree.

use crate::process::quiet_command;
#[cfg(windows)]
use crate::process::{run_detached, run_with_timeout};
use pitcrew_core::logline::MAX_LINE_LEN;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::Arc;
#[cfg(windows)]
use std::time::Duration;

/// Receives the runner's output, one line at a time (from reader threads).
pub type LineSink = Arc<dyn Fn(String) + Send + Sync>;

/// What to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    /// The program.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its working directory.
    pub dir: PathBuf,
    /// Variables set (over the service's own environment) for the process: the login shell's `PATH` on macOS and
    /// Linux, then the runner's `env` table.
    pub env: Vec<(String, String)>,
}

/// A running runner process.
pub trait RunnerProcess: Send {
    /// Its process id.
    fn pid(&self) -> u32;
    /// `None` while it runs; `Some(code)` once it exited.
    fn try_wait(&mut self) -> Option<Option<i32>>;
    /// Asks it to stop gracefully.
    fn stop_gracefully(&mut self) -> Result<(), String>;
    /// Ends it and its children.
    fn kill(&mut self);
}

/// Reads `stream` line by line on a thread until it closes.
pub fn read_lines(stream: impl Read + Send + 'static, sink: LineSink) {
    let spawned = std::thread::Builder::new().name("runner-output".into()).spawn(move || {
        let mut stream = stream;
        let mut buf = [0u8; 8192];
        let mut line: Vec<u8> = Vec::with_capacity(256);
        let mut overflow = false;
        let flush = |line: &mut Vec<u8>, overflow: &mut bool| {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            sink(String::from_utf8_lossy(line).into_owned());
            line.clear();
            *overflow = false;
        };
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for &b in &buf[..n] {
                if b == b'\n' {
                    flush(&mut line, &mut overflow);
                } else if line.len() < MAX_LINE_LEN {
                    line.push(b);
                } else {
                    overflow = true;
                }
            }
        }
        if !line.is_empty() || overflow {
            flush(&mut line, &mut overflow);
        }
    });
    // A thread that cannot be spawned only loses output; the process itself is still supervised.
    drop(spawned);
}

/// A runner started as an ordinary child of the service (service account, or the user's own session when the
/// service runs as that user: macOS, development).
pub struct ChildProcess {
    child: Started,
}

/// How the process was started: `std::process` everywhere, `posix_spawn` with the responsibility disclaim on macOS
/// (`crate::disclaim`, ADR 0020).
enum Started {
    Std(Child),
    #[cfg(target_os = "macos")]
    Spawned {
        pid: u32,
        reaped: bool,
    },
}

impl Started {
    fn id(&self) -> u32 {
        match self {
            Started::Std(c) => c.id(),
            #[cfg(target_os = "macos")]
            Started::Spawned { pid, .. } => *pid,
        }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<Option<i32>>> {
        match self {
            Started::Std(c) => Ok(c.try_wait()?.map(|s| s.code())),
            #[cfg(target_os = "macos")]
            Started::Spawned { pid, reaped } => {
                use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
                if *reaped {
                    return Ok(Some(None));
                }
                match waitpid(
                    nix::unistd::Pid::from_raw(i32::try_from(*pid).unwrap_or(0)),
                    Some(WaitPidFlag::WNOHANG),
                ) {
                    Ok(WaitStatus::StillAlive) => Ok(None),
                    Ok(WaitStatus::Exited(_, code)) => {
                        *reaped = true;
                        Ok(Some(Some(code)))
                    }
                    // Killed by a signal: no exit code.
                    Ok(_) | Err(nix::errno::Errno::ECHILD) => {
                        *reaped = true;
                        Ok(Some(None))
                    }
                    Err(e) => Err(std::io::Error::from_raw_os_error(e as i32)),
                }
            }
        }
    }

    // The bare-pid kill exists only in the macOS `Spawned` arm, so the expectation is fulfilled only there.
    #[cfg_attr(
        target_os = "macos",
        expect(
            clippy::disallowed_methods,
            reason = "ADR 0033 WP-6: kills a spawned runner without the reaped guard; use paddock_proc"
        )
    )]
    fn kill(&mut self) {
        match self {
            Started::Std(c) => {
                let _ = c.kill();
            }
            #[cfg(target_os = "macos")]
            Started::Spawned { pid, .. } => {
                if let Ok(p) = i32::try_from(*pid) {
                    let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), nix::sys::signal::Signal::SIGKILL);
                }
            }
        }
    }
}

/// Whether runners are started as their own responsible processes (macOS: so a job that reads `~/Documents` does not
/// make macOS ask about Pitcrew.app; always false elsewhere, where nothing like it exists).
pub fn disclaim_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::disclaim::disclaim_supported()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Starts `spec` as a child with piped output, in its own process group, without a console window.
pub fn spawn_child(spec: &LaunchSpec, sink: LineSink) -> Result<ChildProcess, String> {
    #[cfg(target_os = "macos")]
    {
        let s = crate::disclaim::spawn(&spec.program, &spec.args, &spec.dir, &spec.env, crate::disclaim::find_disclaim())?;
        read_lines(s.stdout, sink.clone());
        read_lines(s.stderr, sink);
        Ok(ChildProcess {
            child: Started::Spawned {
                pid: u32::try_from(s.pid).unwrap_or(0),
                reaped: false,
            },
        })
    }
    #[cfg(not(target_os = "macos"))]
    spawn_std(spec, sink)
}

/// The portable start: `std::process`.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn spawn_std(spec: &LaunchSpec, sink: LineSink) -> Result<ChildProcess, String> {
    let mut cmd = quiet_command(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.dir)
        .envs(spec.env.iter().map(|(k, v)| (k, v)));
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::process::CREATE_NO_WINDOW | crate::process::CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{} could not be started: {e}", spec.program.display()))?;
    if let Some(out) = child.stdout.take() {
        read_lines(out, sink.clone());
    }
    if let Some(err) = child.stderr.take() {
        read_lines(err, sink);
    }
    Ok(ChildProcess {
        child: Started::Std(child),
    })
}

impl RunnerProcess for ChildProcess {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn try_wait(&mut self) -> Option<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(code)) => Some(code),
            Ok(None) => None,
            // The handle is unusable: treat the process as gone so supervision goes on.
            Err(_) => Some(None),
        }
    }

    fn stop_gracefully(&mut self) -> Result<(), String> {
        graceful_stop(self.child.id(), None)
    }

    fn kill(&mut self) {
        kill_tree(self.child.id());
        self.child.kill();
    }
}

static STOP_HELPER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Uses `exe` as the Ctrl+Break helper instead of the running executable (tests run from a test binary).
pub fn set_stop_helper(exe: PathBuf) {
    let _ = STOP_HELPER.set(exe);
}

/// Asks process `pid` (a group leader) to stop. On Windows `session` is the user session it runs in, if any.
#[cfg_attr(
    unix,
    expect(
        clippy::disallowed_methods,
        reason = "ADR 0033 WP-6: kill_tree signals a recorded pid (adopted runners); kill by ProcessId"
    )
)]
pub fn graceful_stop(pid: u32, session: Option<u32>) -> Result<(), String> {
    #[cfg(unix)]
    {
        let _ = session;
        let pid = i32::try_from(pid).map_err(|_| "process id out of range".to_string())?;
        nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGTERM)
            .map_err(|e| format!("the stop signal could not be sent: {e}"))
    }
    #[cfg(windows)]
    {
        let exe = match STOP_HELPER.get() {
            Some(p) => p.clone(),
            None => std::env::current_exe().map_err(|e| format!("the service's own program cannot be found: {e}"))?,
        };
        let args = vec!["ctrl-break".to_string(), pid.to_string()];
        match session {
            Some(s) => std::thread::Builder::new()
                .name("stop-in-session".into())
                .spawn(move || {
                    // Not waited for (B1); the stop timeout ends a runner whose helper failed.
                    let _ = crate::platform::win_session::run_in_session(s, &exe, &args, Duration::from_secs(15));
                })
                .map(|_| ())
                .map_err(|e| format!("the stop request could not start: {e}")),
            None => {
                // B1: the helper takes a moment (it attaches to the runner's console); the supervisor thread that asks
                // for the stop must not wait for it. A helper that fails is not reported: the stop timeout ends the
                // runner anyway, and the supervisor says so.
                let mut cmd = quiet_command(&exe);
                cmd.args(&args);
                run_detached(cmd, Duration::from_secs(15), |_| {}).map_err(|e| format!("the stop request could not start: {e}"))
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, session);
        Err("stopping runners is not supported on this platform".into())
    }
}

/// Ends `pid` and every process it started.
#[cfg_attr(
    unix,
    expect(
        clippy::disallowed_methods,
        reason = "ADR 0033 WP-6: kill_tree signals a recorded pid (adopted runners); kill by ProcessId"
    )
)]
pub fn kill_tree(pid: u32) {
    #[cfg(unix)]
    {
        if let Ok(p) = i32::try_from(pid) {
            let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(p), nix::sys::signal::Signal::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let mut cmd = quiet_command(&crate::process::taskkill_exe());
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        let _ = run_with_timeout(cmd, Duration::from_secs(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    fn collect() -> (LineSink, Arc<Mutex<Vec<String>>>) {
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        (Arc::new(move |l| g.lock().unwrap().push(l)), got)
    }

    fn wait_for(got: &Arc<Mutex<Vec<String>>>, n: usize) -> Vec<String> {
        for _ in 0..400 {
            if got.lock().unwrap().len() >= n {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        got.lock().unwrap().clone()
    }

    #[test]
    fn splits_lines_and_bounds_them() {
        let (sink, got) = collect();
        let mut data = b"one\r\ntwo\n".to_vec();
        data.extend(vec![b'x'; MAX_LINE_LEN + 10]);
        data.extend(b"\nlast \xff");
        read_lines(std::io::Cursor::new(data), sink);
        let lines = wait_for(&got, 4);
        assert_eq!(lines[0], "one");
        assert_eq!(lines[1], "two");
        assert_eq!(lines[2].len(), MAX_LINE_LEN);
        assert_eq!(lines[3], "last \u{fffd}");
    }

    #[test]
    fn runs_a_child_and_reads_both_streams() {
        let (sink, got) = collect();
        let spec = if cfg!(windows) {
            LaunchSpec {
                program: "cmd.exe".into(),
                args: vec!["/d".into(), "/c".into(), "echo out && echo err 1>&2".into()],
                dir: std::env::temp_dir(),
                env: vec![],
            }
        } else {
            LaunchSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "echo out; echo err 1>&2".into()],
                dir: std::env::temp_dir(),
                env: vec![],
            }
        };
        let mut p = spawn_child(&spec, sink).unwrap();
        assert!(p.pid() > 0);
        let mut code = None;
        for _ in 0..500 {
            if let Some(c) = p.try_wait() {
                code = Some(c);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(code, Some(Some(0)));
        let mut lines = wait_for(&got, 2);
        lines.iter_mut().for_each(|l| *l = l.trim().to_string());
        lines.sort();
        assert_eq!(lines, vec!["err", "out"]);
        p.kill();
    }

    #[test]
    fn a_missing_binary_is_a_message() {
        let (sink, _) = collect();
        let err = spawn_child(
            &LaunchSpec {
                program: "/no/such/gitea-runner".into(),
                args: vec![],
                dir: std::env::temp_dir(),
                env: vec![],
            },
            sink,
        )
        .err()
        .unwrap();
        assert!(err.contains("could not be started"), "{err}");
    }
}
