//! Asking one process to stop, and ending it when it does not (ADR 0012). Used for the leftovers of finished jobs and
//! for "Stop now" on the Processes tab; the runner's own stop is `launch::graceful_stop`.
//!
//! Unlike the runner's stop, which signals the runner's whole process group, this addresses **one pid**: a leftover
//! is usually still in the runner's group, and signalling the group would stop the runner too. The caller has
//! already checked that the pid belongs to the runner's tree (`pitcrew_core::process_track::check_stop_target`).

/// What asking a process to stop came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// The request was delivered (SIGTERM, a close request): the process is given its stop timeout to end.
    Delivered,
    /// The process is the one named, but the system does not take a polite request for it (Windows: a console process
    /// without a window "can only be terminated forcefully"). It is tracked like a delivered ask, and ended when the stop
    /// timeout passes.
    NotTaken,
}

/// What the service can do to a process. A process is named by its [`paddock_proc::ProcessId`], the pid and the exact start
/// time the service read when it listed it; the implementation compares the pid's holder right before it signals, so a pid
/// that was handed on is never signalled (rule 53, ADR 0033).
pub trait Terminator: Send {
    /// Asks it to stop: SIGTERM on Unix, a close request on Windows. An error says why nothing was sent: the pid now belongs
    /// to another process, or the system refused.
    fn ask(&self, id: paddock_proc::ProcessId) -> Result<Asked, String>;
    /// Ends it now (SIGKILL, `TerminateProcess`) and says what that found. A pid handed on to another process is left alone
    /// ([`paddock_proc::Ending::NotOurs`]).
    fn end(&self, id: paddock_proc::ProcessId) -> paddock_proc::Ending;
}

/// The operating system's way, through `paddock_proc`.
pub struct OsTerminator;

impl Terminator for OsTerminator {
    fn ask(&self, id: paddock_proc::ProcessId) -> Result<Asked, String> {
        match paddock_proc::ask_outcome(id) {
            paddock_proc::Ending::Ended => Ok(Asked::Delivered),
            paddock_proc::Ending::Gone | paddock_proc::Ending::NotOurs(_) => Err(NOT_THE_SAME.into()),
            // The process is the one named and the system did not take the request: on Windows a console process
            // without a window cannot be closed politely, so it is ended when the timeout passes.
            paddock_proc::Ending::Refused if cfg!(windows) => Ok(Asked::NotTaken),
            paddock_proc::Ending::Refused => Err("the system refused the stop request (the process may belong to another user)".into()),
        }
    }

    fn end(&self, id: paddock_proc::ProcessId) -> paddock_proc::Ending {
        paddock_proc::end(id)
    }
}

/// Why nothing was sent: the pid is free or was handed on.
const NOT_THE_SAME: &str =
    "that process id no longer belongs to the process that was listed (it ended, or the id was reused); refresh the list";

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    /// Kills and reaps its process when dropped, so a failing assertion leaves nothing running.
    #[cfg(unix)]
    struct Reaped(std::process::Child);

    #[cfg(unix)]
    impl Drop for Reaped {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// The identity the service holds for `child`: read from the OS, exactly.
    #[cfg(unix)]
    fn held(child: &Reaped) -> paddock_proc::ProcessId {
        paddock_proc::ProcessId::of(child.0.id()).expect("an unreaped child has a start time")
    }

    /// The same pid with another start time: the id was reused by a process that started later.
    #[cfg(unix)]
    fn reused(id: paddock_proc::ProcessId) -> paddock_proc::ProcessId {
        paddock_proc::ProcessId::new(
            id.pid(),
            paddock_proc::StartTime::from_micros(id.started().as_micros().saturating_sub(3_600_000_000)),
        )
    }

    #[cfg(unix)]
    fn stand_in(script: &str, piped: bool) -> Reaped {
        let mut c = crate::process::quiet_command(std::path::Path::new("sh"));
        c.args(["-c", script]).stderr(std::process::Stdio::null());
        c.stdout(if piped {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        });
        Reaped(c.spawn().unwrap())
    }

    #[cfg(unix)]
    #[test]
    fn a_process_is_asked_then_ended_by_identity() {
        use std::io::BufRead;
        use std::os::unix::process::ExitStatusExt;
        let mut child = stand_in("exec sleep 60", false);
        assert_eq!(OsTerminator.ask(held(&child)), Ok(Asked::Delivered));
        let status = child.0.wait().unwrap();
        assert_eq!(status.signal(), Some(nix::sys::signal::Signal::SIGTERM as i32));
        // Ending one that ignores SIGTERM: a shell that ignores it and then becomes `sleep` (an ignored signal stays
        // ignored across exec), so it is one process with no children of its own to leave behind. It says when the
        // signal is ignored; the test waits for that line instead of a fixed time.
        let mut child = stand_in("trap '' TERM; echo ready; exec sleep 60", true);
        let mut ready = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "ready");
        let id = held(&child);
        assert_eq!(OsTerminator.ask(id), Ok(Asked::Delivered));
        assert_eq!(OsTerminator.end(id), paddock_proc::Ending::Ended);
        // Had the request not been ignored, SIGTERM (sent first) would have ended it.
        let status = child.0.wait().unwrap();
        assert_eq!(
            status.signal(),
            Some(nix::sys::signal::Signal::SIGKILL as i32),
            "it ignored the request"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_reused_process_id_is_neither_asked_nor_ended() {
        // The pid names a live stand-in that started at another time than the one the service listed: the id was reused.
        let mut child = stand_in("exec sleep 60", false);
        let id = held(&child);
        let err = OsTerminator.ask(reused(id)).unwrap_err();
        assert!(err.contains("no longer belongs"), "{err}");
        assert!(matches!(OsTerminator.end(reused(id)), paddock_proc::Ending::NotOurs(_)));
        assert!(child.0.try_wait().unwrap().is_none(), "the stand-in was not touched");
        // The same process by its own identity is stopped.
        assert_eq!(OsTerminator.ask(id), Ok(Asked::Delivered));
        child.0.wait().unwrap();
        // A pid nobody holds is an error, not a panic.
        let nobody = |pid| paddock_proc::ProcessId::new(pid, paddock_proc::StartTime::from_micros(1_000_000));
        assert!(OsTerminator.ask(nobody(i32::MAX as u32)).is_err());
        assert_eq!(OsTerminator.end(nobody(i32::MAX as u32)), paddock_proc::Ending::Gone);
        assert!(OsTerminator.ask(nobody(u32::MAX)).is_err(), "out of range");
    }

    /// Windows: a console process without a window cannot be closed politely (`taskkill` says it "can only be terminated
    /// forcefully"). The ask reports that the request was not taken, and the forced end still works.
    #[cfg(windows)]
    #[test]
    fn a_windowless_console_process_is_not_taken_politely_and_is_then_ended() {
        let mut c = crate::process::quiet_command(std::path::Path::new("ping"));
        c.args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = c.spawn().unwrap();
        let id = paddock_proc::ProcessId::of(child.id()).expect("an unreaped child has a start time");
        let asked = OsTerminator.ask(id);
        let ended = OsTerminator.end(id);
        let _ = child.wait();
        assert_eq!(
            asked,
            Ok(Asked::NotTaken),
            "a console process with no window refuses the close request"
        );
        assert_eq!(ended, paddock_proc::Ending::Ended);
    }
}
