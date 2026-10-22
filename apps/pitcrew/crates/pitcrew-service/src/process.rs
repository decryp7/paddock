//! Child processes: every external program runs without a console window and with a timeout.

use crate::error::Error;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Most output kept from a short-lived tool (e.g. `gitea-runner --version`).
pub const MAX_TOOL_OUTPUT: usize = 64 * 1024;

/// A command that opens no console window on Windows. The one place the service creates `Command`s.
pub fn quiet_command(program: &Path) -> Command {
    #[allow(clippy::disallowed_methods)]
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x0800_0000);
        cmd
    };
    cmd
}

/// The finished tool's standard output (bounded) and whether it succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Exit status was success.
    pub success: bool,
    /// Standard output, at most [`MAX_TOOL_OUTPUT`] bytes, lossily decoded.
    pub stdout: String,
}

/// Runs `cmd` to completion, stopping it after `timeout`.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<ToolOutput, Error> {
    let program = format!("{:?}", cmd.get_program());
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| Error::Os {
        what: "starting a program",
        reason: format!("{program}: {e}"),
    })?;
    let reader = child.stdout.take().map(|out| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out.take(MAX_TOOL_OUTPUT as u64).read_to_end(&mut buf);
            buf
        })
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::TimedOut {
                    program,
                    secs: timeout.as_secs(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                return Err(Error::Os {
                    what: "waiting for a program",
                    reason: e.to_string(),
                })
            }
        }
    };
    let stdout = reader.and_then(|h| h.join().ok()).unwrap_or_default();
    Ok(ToolOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut c = quiet_command(Path::new("cmd.exe"));
            c.args(["/d", "/c", script]);
            c
        } else {
            let mut c = quiet_command(Path::new("/bin/sh"));
            c.args(["-c", script]);
            c
        }
    }

    #[test]
    fn captures_output_of_a_finished_tool() {
        let out = run_with_timeout(shell("echo gitea-runner version v5.2.0"), Duration::from_secs(20)).unwrap();
        assert!(out.success);
        assert!(out.stdout.contains("v5.2.0"), "{out:?}");
    }

    #[test]
    fn stops_a_tool_that_hangs() {
        let hang = if cfg!(windows) { "ping -n 30 127.0.0.1 >nul" } else { "sleep 30" };
        let started = Instant::now();
        let err = run_with_timeout(shell(hang), Duration::from_millis(500)).unwrap_err();
        assert!(matches!(err, Error::TimedOut { .. }), "{err}");
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_panic() {
        let err = run_with_timeout(quiet_command(Path::new("pitcrew-no-such-program")), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, Error::Os { .. }));
    }
}
