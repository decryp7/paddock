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
use crate::process::run_with_timeout;
use paddock_proc::ProcessId;
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
    let spawned = std::thread::Builder::new()
        .name("runner-output".into())
        .spawn(move || pump_lines(stream, &*sink));
    // A thread that cannot be spawned only loses output; the process itself is still supervised.
    drop(spawned);
}

/// Cuts `stream` into lines for `sink`, on the calling thread, until it closes. Lines longer than `MAX_LINE_LEN` are cut; bytes
/// that are not UTF-8 are replaced.
fn pump_lines(stream: impl Read, sink: &(dyn Fn(String) + Send + Sync)) {
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
}

/// A runner started as an ordinary child of the service (service account, or the user's own session when the
/// service runs as that user: macOS, development).
pub struct ChildProcess {
    child: Started,
    /// The runner by identity (pid plus start time), read right after the start while nothing could have reaped it; what is
    /// signalled afterwards is first checked to still be this process (ADR 0033).
    id: Option<ProcessId>,
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

    /// Ends the child itself (the tree goes through [`kill_tree`] by identity). A `Spawned` one is ended there only: without
    /// its identity there is nothing safe to signal.
    fn kill(&mut self) {
        match self {
            Started::Std(c) => {
                let _ = c.kill();
            }
            #[cfg(target_os = "macos")]
            Started::Spawned { .. } => {}
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
        let pid = u32::try_from(s.pid).unwrap_or(0);
        Ok(ChildProcess {
            child: Started::Spawned { pid, reaped: false },
            id: ProcessId::of(pid),
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
    let id = ProcessId::of(child.id());
    Ok(ChildProcess {
        child: Started::Std(child),
        id,
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
        match self.id {
            Some(id) => graceful_stop(id, None),
            None => Err("the runner's start time could not be read, so it cannot be told from another process".into()),
        }
    }

    fn kill(&mut self) {
        if let Some(id) = self.id {
            kill_tree(id);
        }
        self.child.kill();
    }
}

static STOP_HELPER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Uses `exe` as the Ctrl+Break helper instead of the running executable (tests run from a test binary).
pub fn set_stop_helper(exe: PathBuf) {
    let _ = STOP_HELPER.set(exe);
}

/// Asks the runner `id` to stop, if its pid still belongs to it. On Unix it is SIGTERM to the runner itself
/// ([`paddock_proc::ask`]): the runner stops polling and finishes a running job. On Windows `session` is the user session it
/// runs in, if any.
pub fn graceful_stop(id: ProcessId, session: Option<u32>) -> Result<(), String> {
    #[cfg(unix)]
    {
        let _ = session;
        if paddock_proc::ask(id) {
            Ok(())
        } else {
            Err("the stop signal could not be sent: the runner is gone or its process id belongs to another process now".into())
        }
    }
    #[cfg(windows)]
    {
        if !id.is_alive() {
            return Err("the runner is gone or its process id belongs to another process now".into());
        }
        let pid = id.pid();
        let exe = match STOP_HELPER.get() {
            Some(p) => p.clone(),
            None => std::env::current_exe().map_err(|e| format!("the service's own program cannot be found: {e}"))?,
        };
        let args = vec!["ctrl-break".to_string(), pid.to_string()];
        match session {
            Some(s) => crate::platform::win_session::run_in_session(s, &exe, &args, Duration::from_secs(15)),
            None => {
                let mut cmd = quiet_command(&exe);
                cmd.args(&args);
                let out = run_with_timeout(cmd, Duration::from_secs(15)).map_err(|e| e.to_string())?;
                if out.success {
                    Ok(())
                } else {
                    Err(format!("the stop request failed: {}", out.stderr.trim()))
                }
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (id, session);
        Err("stopping runners is not supported on this platform".into())
    }
}

/// Ends the runner `id` and every process it started, each by identity (a process whose pid was handed on since is left
/// alone; Windows ends them through handles, never `taskkill /T`).
pub fn kill_tree(id: ProcessId) {
    paddock_proc::kill_tree(id);
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
    fn the_line_pump_never_panics_on_hostile_output() {
        paddock_testkit::assert_never_panics(
            &[
                b"one\r\ntwo\nlast \xff",
                b"time=\"2026-10-22T12:10:00+00:00\" level=info msg=\"x\"\n",
            ],
            |bytes| {
                let lines = Mutex::new(0usize);
                pump_lines(std::io::Cursor::new(bytes.to_vec()), &|l: String| {
                    assert!(l.len() <= MAX_LINE_LEN * 3);
                    *lines.lock().unwrap() += 1;
                });
                lines.into_inner().unwrap()
            },
        );
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
