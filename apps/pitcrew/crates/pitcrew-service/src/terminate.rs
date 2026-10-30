//! Asking one process to stop, and ending it when it does not (ADR 0012). Used for the leftovers of finished jobs and
//! for "Stop now" on the Processes tab; the runner's own stop is `launch::graceful_stop`.
//!
//! Unlike the runner's stop, which signals the runner's whole process group, this addresses **one pid**: a leftover
//! is usually still in the runner's group, and signalling the group would stop the runner too. The caller has
//! already checked that the pid belongs to the runner's tree (`pitcrew_core::process_track::check_stop_target`).

#[cfg(windows)]
use crate::process::{quiet_command, run_with_timeout};
#[cfg(windows)]
use std::time::Duration;

/// What the service can do to a process.
pub trait Terminator: Send {
    /// Asks it to stop: SIGTERM on Unix, a close request (`taskkill`, which sends WM_CLOSE) on Windows.
    fn ask(&self, pid: u32) -> Result<(), String>;
    /// Ends it now: SIGKILL, or `taskkill /F`.
    fn end(&self, pid: u32);
}

/// The operating system's way.
pub struct OsTerminator;

impl Terminator for OsTerminator {
    #[cfg_attr(
        unix,
        expect(
            clippy::disallowed_methods,
            reason = "ADR 0033 WP-6: asks a pid from the last probe; take a ProcessId and use paddock_proc::ask"
        )
    )]
    fn ask(&self, pid: u32) -> Result<(), String> {
        #[cfg(unix)]
        {
            let p = i32::try_from(pid).map_err(|_| "process id out of range".to_string())?;
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), nix::sys::signal::Signal::SIGTERM)
                .map_err(|e| format!("the stop request could not be sent: {e}"))
        }
        #[cfg(windows)]
        {
            taskkill(pid, false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = pid;
            Err("stopping processes is not supported on this platform".into())
        }
    }

    #[cfg_attr(
        unix,
        expect(
            clippy::disallowed_methods,
            reason = "ADR 0033 WP-6: ends a pid from the last probe; take a ProcessId and use paddock_proc::end"
        )
    )]
    fn end(&self, pid: u32) {
        #[cfg(unix)]
        {
            if let Ok(p) = i32::try_from(pid) {
                let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), nix::sys::signal::Signal::SIGKILL);
            }
        }
        #[cfg(windows)]
        {
            let _ = taskkill(pid, true);
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = pid;
        }
    }
}

/// `taskkill /PID n [/T] [/F]` (Windows). Without `/F` it posts WM_CLOSE to the process's windows; console programs
/// without a window ignore it, and the stop timeout then ends them with `/F`.
#[cfg(windows)]
fn taskkill(pid: u32, force: bool) -> Result<(), String> {
    let mut cmd = quiet_command(&crate::process::taskkill_exe());
    cmd.args(["/PID", &pid.to_string()]);
    if force {
        cmd.arg("/F");
    }
    let out = run_with_timeout(cmd, Duration::from_secs(20)).map_err(|e| e.to_string())?;
    if out.success {
        Ok(())
    } else {
        Err(format!("taskkill: {}", out.stderr.trim().chars().take(200).collect::<String>()))
    }
}

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

    #[cfg(unix)]
    #[test]
    fn a_process_is_asked_then_ended() {
        use std::io::BufRead;
        use std::os::unix::process::ExitStatusExt;
        let mut c = crate::process::quiet_command(std::path::Path::new("sleep"));
        c.arg("60").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let mut child = Reaped(c.spawn().unwrap());
        assert!(OsTerminator.ask(child.0.id()).is_ok());
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
        OsTerminator.ask(pid).unwrap();
        OsTerminator.end(pid);
        // Had the request not been ignored, SIGTERM (sent first) would have ended it.
        let status = child.0.wait().unwrap();
        assert_eq!(
            status.signal(),
            Some(nix::sys::signal::Signal::SIGKILL as i32),
            "it ignored the request"
        );
        // A process that is not there is an error, not a panic.
        assert!(OsTerminator.ask(i32::MAX as u32).is_err());
        OsTerminator.end(i32::MAX as u32);
        assert!(OsTerminator.ask(u32::MAX).is_err(), "out of range");
    }
}
