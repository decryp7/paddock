//! Running a program to completion with a timeout and an output cap. On a timeout, or when the output reaches the cap
//! and the caller asked to stop there, the whole process tree is ended and reaped, so no helper is left behind.

use crate::identity::ProcessId;
use crate::tree::kill_tree;
use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Most bytes kept of each output stream unless [`Limits::max_output`] says otherwise.
pub const DEFAULT_MAX_OUTPUT: usize = 4 * 1024 * 1024;
/// How long to wait for the pipes to close after the tree was ended (a helper that left the tree may still hold them).
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// How far a run may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The run is ended after this long.
    pub timeout: Duration,
    /// Most bytes kept of each of stdout and stderr; the rest is read and dropped so the child never blocks on a full
    /// pipe.
    pub max_output: usize,
    /// End the tree as soon as a stream reaches `max_output` (an endless writer) instead of dropping the rest.
    pub stop_on_cap: bool,
}

impl Limits {
    /// A timeout with the default output cap, dropping what exceeds it.
    pub fn new(timeout: Duration) -> Limits {
        Limits {
            timeout,
            max_output: DEFAULT_MAX_OUTPUT,
            stop_on_cap: false,
        }
    }

    /// With another output cap.
    pub fn max_output(mut self, bytes: usize) -> Limits {
        self.max_output = bytes;
        self
    }

    /// End the run when the cap is reached.
    pub fn stop_on_cap(mut self) -> Limits {
        self.stop_on_cap = true;
        self
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The program exited on its own (any status).
    Exited(ExitStatus),
    /// It did not finish within [`Limits::timeout`] and its tree was ended.
    TimedOut,
    /// It wrote more than [`Limits::max_output`] and [`Limits::stop_on_cap`] ended its tree.
    OutputCap,
}

/// What a run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// How it ended.
    pub outcome: Outcome,
    /// Standard output, at most [`Limits::max_output`] bytes.
    pub stdout: Vec<u8>,
    /// Standard error, at most [`Limits::max_output`] bytes.
    pub stderr: Vec<u8>,
    /// Some output beyond the cap was dropped.
    pub truncated: bool,
}

impl Output {
    /// It exited with status 0.
    pub fn success(&self) -> bool {
        matches!(self.outcome, Outcome::Exited(s) if s.success())
    }

    /// Standard output as lossy UTF-8.
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Standard error as lossy UTF-8.
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Why a program could not be run. Messages name the program (its file name, never its arguments) and say what to do.
#[derive(Debug)]
pub enum RunError {
    /// The program does not exist or is not on the `PATH`.
    NotFound(String),
    /// The program exists but may not be run.
    PermissionDenied(String),
    /// Starting it failed for another reason.
    Spawn(String, std::io::Error),
    /// Waiting for it failed.
    Wait(String, std::io::Error),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::NotFound(p) => write!(
                f,
                "{p} is not available on this computer. Install it or check the PATH."
            ),
            RunError::PermissionDenied(p) => {
                write!(f, "Not allowed to run {p}. Check its permissions.")
            }
            RunError::Spawn(p, e) => write!(f, "{p} could not be started: {e}"),
            RunError::Wait(p, e) => write!(f, "Waiting for {p} failed: {e}"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RunError::Spawn(_, e) | RunError::Wait(_, e) => Some(e),
            _ => None,
        }
    }
}

/// The program's file name, for messages.
fn program_name(cmd: &Command) -> String {
    std::path::Path::new(cmd.get_program())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

enum Event {
    Exit(std::io::Result<ExitStatus>),
    Cap,
    Drained,
}

fn drain(
    mut r: impl Read + Send + 'static,
    max: usize,
    stop_on_cap: bool,
    tx: mpsc::Sender<Event>,
) -> Arc<Mutex<Captured>> {
    let cap = Arc::new(Mutex::new(Captured::default()));
    let shared = cap.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        let mut told = false;
        loop {
            let n = match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut c = shared.lock().unwrap_or_else(|p| p.into_inner());
            let room = max.saturating_sub(c.bytes.len());
            c.bytes.extend_from_slice(&chunk[..n.min(room)]);
            if n > room {
                c.truncated = true;
                if stop_on_cap && !told {
                    told = true;
                    let _ = tx.send(Event::Cap);
                }
            }
        }
        let _ = tx.send(Event::Drained);
    });
    cap
}

fn take(c: &Arc<Mutex<Captured>>) -> Captured {
    std::mem::take(&mut *c.lock().unwrap_or_else(|p| p.into_inner()))
}

/// Runs `cmd` to completion with stdin closed and stdout and stderr captured under `limits`. A non-zero exit is not an
/// error: look at [`Output::outcome`]. A timeout or the output cap ends the program's whole tree (the child leads its
/// own process group on Unix) and is reported in the outcome, with what it wrote until then.
pub fn run_bounded(mut cmd: Command, limits: Limits) -> Result<Output, RunError> {
    let name = program_name(&cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::sys::own_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RunError::NotFound(name.clone()),
        std::io::ErrorKind::PermissionDenied => RunError::PermissionDenied(name.clone()),
        _ => RunError::Spawn(name.clone(), e),
    })?;
    // Taken before the waiter thread can reap the child: by the time a timeout ends the tree, the program may have exited
    // and its pid been handed on, so the tree is ended by identity (ADR 0033).
    let root = ProcessId::of(child.id());
    if root.is_none() {
        log::warn!(
            "The start time of {name} (pid {}) could not be read: a timeout or the output cap cannot end its tree",
            child.id()
        );
    }
    let (tx, rx) = mpsc::channel();
    let mut pipes = 0;
    let out = child.stdout.take().map(|s| {
        pipes += 1;
        drain(s, limits.max_output, limits.stop_on_cap, tx.clone())
    });
    let err = child.stderr.take().map(|s| {
        pipes += 1;
        drain(s, limits.max_output, limits.stop_on_cap, tx.clone())
    });
    let waiter = tx.clone();
    std::thread::spawn(move || {
        let _ = waiter.send(Event::Exit(child.wait()));
    });
    drop(tx);

    let started = Instant::now();
    let mut status: Option<std::io::Result<ExitStatus>> = None;
    let mut stopped: Option<Outcome> = None;
    let mut killed_at: Option<Instant> = None;
    while status.is_none() || pipes > 0 {
        let wait = match killed_at {
            // After the kill the tree is gone; only a helper that left it can still hold a pipe.
            Some(at) => PIPE_GRACE.saturating_sub(at.elapsed()),
            None => limits.timeout.saturating_sub(started.elapsed()),
        };
        match rx.recv_timeout(wait) {
            Ok(Event::Exit(s)) => status = Some(s),
            Ok(Event::Drained) => pipes -= 1,
            Ok(Event::Cap) => {
                if killed_at.is_none() {
                    stopped = Some(Outcome::OutputCap);
                    if let Some(r) = root {
                        kill_tree(r);
                    }
                    killed_at = Some(Instant::now());
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if killed_at.is_none() {
                    // Even when the program exited and only a helper holds the pipes open, the tree is ended.
                    if status.is_none() {
                        stopped = Some(Outcome::TimedOut);
                    }
                    if let Some(r) = root {
                        kill_tree(r);
                    }
                    killed_at = Some(Instant::now());
                } else {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let outcome = match (stopped, status) {
        (Some(o), _) => o,
        (None, Some(Ok(s))) => Outcome::Exited(s),
        (None, Some(Err(e))) => return Err(RunError::Wait(name, e)),
        // The waiter vanished without a status: report it as a wait failure.
        (None, None) => {
            return Err(RunError::Wait(
                name,
                std::io::Error::other("the process status was lost"),
            ));
        }
    };
    let (o, e) = (
        out.map(|c| take(&c)).unwrap_or_default(),
        err.map(|c| take(&c)).unwrap_or_default(),
    );
    Ok(Output {
        outcome,
        truncated: o.truncated || e.truncated,
        stdout: o.bytes,
        stderr: e.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testchild::{Mode, child, lines};

    fn limits() -> Limits {
        Limits::new(Duration::from_secs(120))
    }

    #[test]
    fn output_and_status_are_returned() {
        let o = run_bounded(child(Mode::Exit3), limits()).unwrap();
        assert!(!o.success());
        assert!(matches!(o.outcome, Outcome::Exited(s) if s.code() == Some(3)));
        assert_eq!(lines(&o.stdout_text()), ["bye"]);
        assert!(!o.truncated);
        assert!(o.stderr_text().len() < 1024 * 1024);
    }

    #[test]
    fn a_missing_program_is_reported_by_name_not_by_arguments() {
        let mut c = crate::command("/nonexistent/dir/paddock-no-such-program");
        c.arg("--secret-argument");
        let e = run_bounded(c, limits()).unwrap_err();
        assert!(matches!(e, RunError::NotFound(_)), "{e}");
        let m = e.to_string();
        assert!(
            m.contains("paddock-no-such-program")
                && !m.contains("secret-argument")
                && !m.contains("/nonexistent"),
            "{m}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_may_not_run_is_reported() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("not-executable");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        let e = run_bounded(crate::command(&p), limits()).unwrap_err();
        assert!(matches!(e, RunError::PermissionDenied(_)), "{e}");
        assert!(e.to_string().contains("Not allowed to run not-executable"));
    }

    #[test]
    fn the_other_errors_have_messages_and_sources() {
        let io = || std::io::Error::other("boom");
        let spawn = RunError::Spawn("tool".into(), io());
        assert!(
            spawn
                .to_string()
                .contains("tool could not be started: boom")
        );
        assert!(std::error::Error::source(&spawn).is_some());
        let wait = RunError::Wait("tool".into(), io());
        assert!(wait.to_string().contains("Waiting for tool failed"));
        assert!(std::error::Error::source(&wait).is_some());
        assert!(std::error::Error::source(&RunError::NotFound("t".into())).is_none());
    }

    #[test]
    fn a_hung_program_and_its_grandchild_are_ended_at_the_timeout() {
        let o = run_bounded(child(Mode::Tree), Limits::new(Duration::from_millis(1500))).unwrap();
        assert_eq!(o.outcome, Outcome::TimedOut);
        let pid: u32 = lines(&o.stdout_text())
            .iter()
            .find_map(|l| l.strip_prefix("pid "))
            .and_then(|p| p.parse().ok())
            .expect("the child told its grandchild's pid before the timeout");
        assert!(!crate::is_alive(pid), "the grandchild survived the timeout");
    }

    #[test]
    fn an_endless_writer_is_held_to_the_cap_and_ended_when_asked() {
        let o = run_bounded(
            child(Mode::Endless),
            limits().max_output(10_000).stop_on_cap(),
        )
        .unwrap();
        assert_eq!(o.outcome, Outcome::OutputCap);
        assert!(o.truncated);
        assert_eq!(o.stdout.len(), 10_000);
    }

    #[test]
    fn an_endless_writer_without_stop_on_cap_is_truncated_until_the_timeout() {
        let o = run_bounded(
            child(Mode::Endless),
            Limits::new(Duration::from_millis(800)).max_output(1000),
        )
        .unwrap();
        assert_eq!(o.outcome, Outcome::TimedOut);
        assert!(o.truncated);
        assert_eq!(o.stdout.len(), 1000, "never more than the cap is kept");
    }

    // Windows has no process group to reach a helper whose parent already exited (no job object yet).
    #[cfg(unix)]
    #[test]
    fn a_helper_left_holding_the_pipes_does_not_hang_the_run() {
        let o = run_bounded(child(Mode::Leave), Limits::new(Duration::from_millis(800))).unwrap();
        assert!(
            o.success(),
            "the program itself exited cleanly: {:?}",
            o.outcome
        );
        let pid: u32 = lines(&o.stdout_text())
            .iter()
            .find_map(|l| l.strip_prefix("pid "))
            .and_then(|p| p.parse().ok())
            .unwrap();
        // The pipes reach EOF a moment before the killed helper shows as a zombie: wait for that state (a deadline, not a sleep).
        assert!(
            crate::testchild::is_gone(pid),
            "the helper that held the pipes was ended with the tree"
        );
    }

    #[test]
    fn a_program_the_os_cannot_start_is_a_spawn_error() {
        // A name with a NUL byte can never be a program on any OS: neither "not found" nor "permission denied".
        let e = run_bounded(crate::command("paddock\0program"), limits()).unwrap_err();
        assert!(
            matches!(&e, RunError::Spawn(_, io) if io.kind() == std::io::ErrorKind::InvalidInput),
            "{e:?}"
        );
    }
}
