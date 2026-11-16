//! Asking one process to stop, and ending it when it does not (ADR 0012). Used for the leftovers of finished jobs and
//! for "Stop now" on the Processes tab; the runner's own stop is `launch::graceful_stop`.
//!
//! Unlike the runner's stop, which signals the runner's whole process group, this addresses **one pid**: a leftover
//! is usually still in the runner's group, and signalling the group would stop the runner too. The caller has
//! already checked that the pid belongs to the runner's tree (`pitcrew_core::process_track::check_stop_target`).

/// What the service can do to a process. A process is named by its pid **and the start time the service read for it**
/// (milliseconds since the epoch, whole seconds): the implementation checks that the pid still belongs to that process
/// immediately before acting, and never acts on a bare pid (rule 53, ADR 0033).
pub trait Terminator: Send {
    /// Asks it to stop: SIGTERM on Unix, a close request on Windows. An error says why it was not asked, including
    /// that the pid now belongs to another process.
    fn ask(&self, pid: u32, started_ms: u64) -> Result<(), String>;
    /// Ends it now (SIGKILL, `TerminateProcess`); returns whether it was this process and it was ended or already gone.
    /// A pid handed on to another process is left alone.
    fn end(&self, pid: u32, started_ms: u64) -> bool;
}

/// The operating system's way, through `paddock_proc` (which compares the start time of the pid's holder before it signals).
pub struct OsTerminator;

/// Start times come from the process table in whole seconds ([`paddock_proc::ProcessId::of_reported`]).
const START_RESOLUTION_MS: u64 = 1000;

impl Terminator for OsTerminator {
    fn ask(&self, pid: u32, started_ms: u64) -> Result<(), String> {
        let id = paddock_proc::ProcessId::of_reported(pid, started_ms, START_RESOLUTION_MS).ok_or(NOT_THE_SAME)?;
        if paddock_proc::ask(id) {
            Ok(())
        } else {
            Err("the stop request could not be sent (the process ended meanwhile, or the system refused it)".into())
        }
    }

    fn end(&self, pid: u32, started_ms: u64) -> bool {
        let Some(id) = paddock_proc::ProcessId::of_reported(pid, started_ms, START_RESOLUTION_MS) else {
            return false;
        };
        matches!(paddock_proc::end(id), paddock_proc::Ending::Ended | paddock_proc::Ending::Gone)
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

    /// The start time the service would have listed for `child`: the process table's, in whole seconds.
    #[cfg(unix)]
    fn listed_start_ms(child: &Reaped) -> u64 {
        let id = paddock_proc::ProcessId::of(child.0.id()).expect("an unreaped child has a start time");
        id.started().as_micros() / 1_000_000 * 1000
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
        let child = stand_in("exec sleep 60", false);
        let started = listed_start_ms(&child);
        let mut child = child;
        assert!(OsTerminator.ask(child.0.id(), started).is_ok());
        let status = child.0.wait().unwrap();
        assert_eq!(status.signal(), Some(libc_sigterm()));
        // Ending one that ignores SIGTERM: a shell that ignores it and then becomes `sleep` (an ignored signal stays
        // ignored across exec), so it is one process with no children of its own to leave behind. It says when the
        // signal is ignored; the test waits for that line instead of a fixed time.
        let mut child = stand_in("trap '' TERM; echo ready; exec sleep 60", true);
        let mut ready = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "ready");
        let (pid, started) = (child.0.id(), listed_start_ms(&child));
        OsTerminator.ask(pid, started).unwrap();
        assert!(OsTerminator.end(pid, started));
        // Had the request not been ignored, SIGTERM (sent first) would have ended it.
        let status = child.0.wait().unwrap();
        assert_eq!(status.signal(), Some(libc_sigkill()), "it ignored the request");
    }

    #[cfg(unix)]
    #[test]
    fn a_reused_process_id_is_neither_asked_nor_ended() {
        // The pid names a live stand-in that started at another time than the one the service listed: the id was reused.
        let mut child = stand_in("exec sleep 60", false);
        let listed_then = listed_start_ms(&child).saturating_sub(3_600_000);
        let err = OsTerminator.ask(child.0.id(), listed_then).unwrap_err();
        assert!(err.contains("no longer belongs"), "{err}");
        assert!(!OsTerminator.end(child.0.id(), listed_then));
        assert!(child.0.try_wait().unwrap().is_none(), "the stand-in was not touched");
        // The same process by its own start time is stopped.
        assert!(OsTerminator.ask(child.0.id(), listed_start_ms(&child)).is_ok());
        child.0.wait().unwrap();
        // A pid nobody holds is an error, not a panic.
        assert!(OsTerminator.ask(i32::MAX as u32, 1_000).is_err());
        assert!(!OsTerminator.end(i32::MAX as u32, 1_000));
        assert!(OsTerminator.ask(u32::MAX, 1_000).is_err(), "out of range");
    }

    #[cfg(unix)]
    fn libc_sigterm() -> i32 {
        nix::sys::signal::Signal::SIGTERM as i32
    }

    #[cfg(unix)]
    fn libc_sigkill() -> i32 {
        nix::sys::signal::Signal::SIGKILL as i32
    }
}
