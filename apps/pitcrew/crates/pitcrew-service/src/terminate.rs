//! Asking one process to stop, and ending it when it does not (ADR 0012). Used for the leftovers of finished jobs and
//! for "Stop now" on the Processes tab; the runner's own stop is `launch::graceful_stop`.
//!
//! Unlike the runner's stop, which signals the runner's whole process group, this addresses **one pid**: a leftover
//! is usually still in the runner's group, and signalling the group would stop the runner too. The caller has
//! already checked that the pid belongs to the runner's tree (`pitcrew_core::process_track::check_stop_target`).

use paddock_proc::ProcessId;

/// Start times come from the OS in whole seconds; a holder of the pid that started later than the recorded start plus this slack
/// is another process (the pid was handed on).
const START_SLACK_MS: u64 = 1500;

/// What the service can do to a process. A process is named by its pid and the start time the last probe read (`started_ms`,
/// milliseconds since the epoch): the OS hands a freed pid on, so what is asked or ended is first checked to still be the process
/// that was meant (ADR 0033).
pub trait Terminator: Send {
    /// Asks it to stop: SIGTERM on Unix, a close request on Windows.
    fn ask(&self, pid: u32, started_ms: Option<u64>) -> Result<(), String>;
    /// Ends it now: SIGKILL, or `TerminateProcess` after the same check.
    fn end(&self, pid: u32, started_ms: Option<u64>);
}

/// The operating system's way, through `paddock_proc`.
pub struct OsTerminator;

/// The process `pid` that was running at `started_ms`, or why it cannot be told from another process.
fn identify(pid: u32, started_ms: Option<u64>) -> Result<ProcessId, String> {
    let started = started_ms.ok_or("the process's start time is not known, so it cannot be told from a reused process id")?;
    let written = i64::try_from(started.saturating_add(START_SLACK_MS)).unwrap_or(i64::MAX);
    ProcessId::of_recorded(pid, written).ok_or_else(|| "that process is gone, or its id belongs to another process now".to_string())
}

impl Terminator for OsTerminator {
    fn ask(&self, pid: u32, started_ms: Option<u64>) -> Result<(), String> {
        let id = identify(pid, started_ms)?;
        if paddock_proc::ask(id) {
            Ok(())
        } else {
            Err("the stop request could not be sent".into())
        }
    }

    fn end(&self, pid: u32, started_ms: Option<u64>) {
        if let Ok(id) = identify(pid, started_ms) {
            let _ = paddock_proc::end(id);
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    /// When process `pid` started, as the probe reads it (whole seconds, in milliseconds).
    #[cfg(unix)]
    fn started_ms(pid: u32) -> Option<u64> {
        let p = sysinfo::Pid::from_u32(pid);
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
        sys.process(p).map(|p| p.start_time() * 1000)
    }

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

    #[cfg(unix)]
    #[test]
    fn a_process_is_asked_then_ended() {
        use std::io::BufRead;
        use std::os::unix::process::ExitStatusExt;
        let mut c = crate::process::quiet_command(std::path::Path::new("sleep"));
        c.arg("60").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let mut child = Reaped(c.spawn().unwrap());
        assert!(OsTerminator.ask(child.0.id(), started_ms(child.0.id())).is_ok());
        let status = child.0.wait().unwrap();
        assert_eq!(status.signal(), Some(nix::sys::signal::Signal::SIGTERM as i32));
        // Ending one that ignores SIGTERM: a shell that ignores it and then becomes `sleep` (an ignored signal stays
        // ignored across exec), so it is one process with no children of its own to leave behind. It says when the
        // signal is ignored; the test waits for that line instead of a fixed time.
        let mut sh = crate::process::quiet_command(std::path::Path::new("sh"));
        sh.args(["-c", "trap '' TERM; echo ready; exec sleep 60"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = Reaped(sh.spawn().unwrap());
        let mut ready = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "ready");
        let pid = child.0.id();
        let started = started_ms(pid);
        OsTerminator.ask(pid, started).unwrap();
        OsTerminator.end(pid, started);
        // Had the request not been ignored, SIGTERM (sent first) would have ended it.
        let status = child.0.wait().unwrap();
        assert_eq!(
            status.signal(),
            Some(nix::sys::signal::Signal::SIGKILL as i32),
            "it ignored the request"
        );
        // A process that is not there is an error, not a panic.
        assert!(OsTerminator.ask(i32::MAX as u32, Some(5_000)).is_err());
        OsTerminator.end(i32::MAX as u32, Some(5_000));
        assert!(OsTerminator.ask(u32::MAX, Some(5_000)).is_err(), "out of range");
    }

    /// A pid whose holder started after the recorded start is another process (the id was handed on), and a process whose start
    /// time is unknown cannot be told apart: neither is asked nor ended (ADR 0033).
    #[cfg(unix)]
    #[test]
    fn a_reused_or_unidentified_pid_is_neither_asked_nor_ended() {
        let me = std::process::id();
        assert!(OsTerminator.ask(me, Some(5_000)).unwrap_err().contains("another process"));
        assert!(OsTerminator.ask(me, None).unwrap_err().contains("start time"));
        let mut c = crate::process::quiet_command(std::path::Path::new("sleep"));
        c.arg("60").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let mut child = Reaped(c.spawn().unwrap());
        OsTerminator.end(child.0.id(), Some(5_000));
        OsTerminator.end(child.0.id(), None);
        assert!(child.0.try_wait().unwrap().is_none(), "a pid with another start time is left alone");
    }
}
