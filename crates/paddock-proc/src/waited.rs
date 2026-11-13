//! Running a program to completion with a timeout, its output going to a file or inherited (CI-in-Rust WP-1). Kept apart
//! from `run.rs` on purpose: this module creates files, and `run.rs` is code the elevated Pitwall service reaches, which must
//! hold no filesystem access (Pitwall's `the_elevated_registration_touches_no_folder_the_user_controls`).

use crate::identity::ProcessId;
use crate::run::{Outcome, PIPE_GRACE, RunError, program_name};
use crate::tree::kill_tree;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Where the output of a [`run_waited`] run goes: no pipe is read, so a program that leaves a daemon behind (which would hold
/// a pipe open for its whole life) or a long one whose output a person watches can still be bounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sink {
    /// This process's own standard output and error (a CI step's log shows every line as it comes).
    Inherit,
    /// Both streams into this file, created or truncated (the caller reads it afterwards).
    File(std::path::PathBuf),
}

/// Runs `cmd` to completion with a timeout, its output going to `sink`, standard input closed. The child leads its own
/// process group (Unix); at the timeout its whole tree is ended by identity (ADR 0033) and the outcome is
/// [`Outcome::TimedOut`]. A process the program leaves behind after it exited on its own (a server it started) is not
/// touched. `Err` only when it could not be started or waited for.
pub fn run_waited(cmd: Command, timeout: Duration, sink: Sink) -> Result<Outcome, RunError> {
    run_waited_cancellable(cmd, timeout, sink, &AtomicBool::new(false)).map(|(outcome, _)| outcome)
}

/// [`run_waited`] that also ends the tree, by identity, when `cancel` is set (a person's Ctrl+C, a total time budget of the
/// caller): the outcome is then [`Outcome::TimedOut`] and the second value is `true`. The flag is looked at every 100 ms.
pub fn run_waited_cancellable(
    mut cmd: Command,
    timeout: Duration,
    sink: Sink,
    cancel: &AtomicBool,
) -> Result<(Outcome, bool), RunError> {
    let name = program_name(&cmd);
    cmd.stdin(Stdio::null());
    if let Sink::File(path) = &sink {
        let file = std::fs::File::create(path).map_err(|e| RunError::Spawn(name.clone(), e))?;
        let copy = file
            .try_clone()
            .map_err(|e| RunError::Spawn(name.clone(), e))?;
        cmd.stdout(file).stderr(copy);
    }
    crate::sys::own_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RunError::NotFound(name.clone()),
        std::io::ErrorKind::PermissionDenied => RunError::PermissionDenied(name.clone()),
        _ => RunError::Spawn(name.clone(), e),
    })?;
    // Taken before the waiter can reap the child, so a timeout ends this tree and never a successor of its pid.
    let root = ProcessId::of(child.id());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // silent: the receiver is gone once the run has ended, and nothing waits for the event then
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = tx.send(child.wait());
    });
    let started = Instant::now();
    let status = loop {
        let left = timeout.saturating_sub(started.elapsed());
        let cancelled = cancel.load(Ordering::SeqCst);
        match rx.recv_timeout(left.min(Duration::from_millis(100))) {
            Ok(status) => break status,
            Err(RecvTimeoutError::Timeout) if !cancelled && !left.is_zero() => {}
            Err(_) => {
                if let Some(r) = root {
                    kill_tree(r);
                }
                // The tree is gone; its exit comes at once. A wait failure here still means the run timed out.
                #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
                let _ = rx.recv_timeout(PIPE_GRACE);
                log::warn!(Tool = name.as_str(), Ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX); "tool.run timed out");
                return Ok((Outcome::TimedOut, cancelled));
            }
        }
    };
    let status = status.map_err(|e| RunError::Wait(name.clone(), e))?;
    log::debug!(Tool = name.as_str(), Ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX), Exit = status.code().unwrap_or(-1); "tool.run");
    Ok((Outcome::Exited(status), false))
}

#[cfg(all(test, unix))]
mod waited_tests {
    use super::*;
    use crate::command;

    /// The identity of the process whose pid the script wrote to `pid_file`, read as soon as the file is complete.
    fn grandchild_of(pid_file: &std::path::Path) -> ProcessId {
        paddock_testkit::wait_for("the grandchild's identity", || {
            std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|t| t.trim().parse::<u32>().ok())
                .and_then(ProcessId::of)
        })
    }

    #[test]
    fn a_waited_run_reports_its_exit_and_writes_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.txt");
        let mut cmd = command("sh");
        cmd.args(["-c", "echo out; echo err >&2; exit 3"]);
        let outcome = run_waited(cmd, Duration::from_secs(30), Sink::File(file.clone())).unwrap();
        assert!(matches!(outcome, Outcome::Exited(s) if s.code() == Some(3)));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "out\nerr\n");
        let mut ok = command("true");
        ok.arg("x");
        assert!(
            matches!(run_waited(ok, Duration::from_secs(30), Sink::Inherit).unwrap(), Outcome::Exited(s) if s.success())
        );
        let missing = run_waited(
            command("/nonexistent/dir/paddock-no-such-program"),
            Duration::from_secs(1),
            Sink::Inherit,
        );
        assert!(matches!(missing, Err(RunError::NotFound(_))), "{missing:?}");
    }

    #[test]
    fn a_cancel_flag_ends_the_whole_tree_long_before_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let mut cmd = command("sh");
        cmd.arg("-c").arg(format!(
            "sleep 60 & echo $! > '{}'; wait",
            pid_file.display()
        ));
        let cancel = AtomicBool::new(false);
        let (outcome, cancelled, grandchild) = std::thread::scope(|s| {
            let run = s.spawn(|| {
                run_waited_cancellable(cmd, Duration::from_secs(600), Sink::Inherit, &cancel)
                    .unwrap()
            });
            paddock_testkit::wait_until_within(
                "the grandchild's pid",
                Duration::from_secs(10),
                || {
                    std::fs::read_to_string(&pid_file)
                        .is_ok_and(|t| t.trim().parse::<u32>().is_ok())
                },
            );
            // Held while it lives: the identity (pid and start time), never the bare pid (rule 53).
            let grandchild = grandchild_of(&pid_file);
            cancel.store(true, Ordering::SeqCst);
            let (outcome, cancelled) = run.join().unwrap();
            (outcome, cancelled, grandchild)
        });
        // The 600 s timeout cannot have elapsed: only the flag ends this run.
        assert_eq!((outcome, cancelled), (Outcome::TimedOut, true));
        paddock_testkit::wait_until_within(
            "the grandchild to be gone",
            Duration::from_secs(10),
            || grandchild.has_ended(),
        );
    }

    #[test]
    fn a_timeout_ends_the_whole_tree_grandchildren_included() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let mut cmd = command("sh");
        cmd.arg("-c").arg(format!(
            "sleep 60 & echo $! > '{}'; wait",
            pid_file.display()
        ));
        // The timeout under test, long enough that the grandchild's identity is read while it lives.
        let (outcome, grandchild) = std::thread::scope(|s| {
            let run = s.spawn(|| run_waited(cmd, Duration::from_secs(3), Sink::Inherit).unwrap());
            let grandchild = grandchild_of(&pid_file);
            (run.join().unwrap(), grandchild)
        });
        assert_eq!(outcome, Outcome::TimedOut);
        // Ended processes are reaped by init: wait for that condition, bounded.
        paddock_testkit::wait_until_within(
            "the grandchild to be gone after the timeout",
            Duration::from_secs(10),
            || grandchild.has_ended(),
        );
    }
}
