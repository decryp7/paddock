//! Child processes: every external program runs without a console window and with a timeout.

use crate::error::Error;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Most output kept from a short-lived tool (e.g. `gitea-runner --version`).
pub const MAX_TOOL_OUTPUT: usize = 64 * 1024;

/// Windows process creation flag: no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Windows process creation flag: a new process group (so Ctrl+Break reaches the runner only).
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// A command that opens no console window on Windows. The one place the service creates `Command`s.
pub fn quiet_command(program: &Path) -> Command {
    #[allow(clippy::disallowed_methods)]
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    };
    cmd
}

/// The finished tool's output (bounded) and whether it succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Exit status was success.
    pub success: bool,
    /// Its exit code, when it had one.
    pub code: Option<i32>,
    /// Standard output, at most [`MAX_TOOL_OUTPUT`] bytes, lossily decoded.
    pub stdout: String,
    /// Standard error, at most [`MAX_TOOL_OUTPUT`] bytes, lossily decoded.
    pub stderr: String,
}

type Drain = Option<std::thread::JoinHandle<Vec<u8>>>;

fn drain(stream: Option<impl Read + Send + 'static>) -> Drain {
    stream.map(|out| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut limited = out.take(MAX_TOOL_OUTPUT as u64);
            let _ = limited.read_to_end(&mut buf);
            // Keep reading (and dropping) so the child never blocks on a full pipe.
            let mut rest = limited.into_inner();
            let _ = std::io::copy(&mut rest, &mut std::io::sink());
            buf
        })
    })
}

/// Runs `cmd` to completion, stopping it after `timeout`. Error messages name the program only, never its
/// arguments (a registration token is an argument).
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<ToolOutput, Error> {
    let program = Path::new(cmd.get_program())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| Error::Os {
        what: "starting a program",
        reason: format!("{program}: {e}"),
    })?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
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
    let text = |h: Drain| String::from_utf8_lossy(&h.and_then(|h| h.join().ok()).unwrap_or_default()).into_owned();
    Ok(ToolOutput {
        success: status.success(),
        code: status.code(),
        stdout: text(out),
        stderr: text(err),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn shell(script: &str) -> Command {
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
        let out = run_with_timeout(shell("echo gitea-runner version v5.2.0 && echo oops 1>&2"), Duration::from_secs(20)).unwrap();
        assert!(out.success);
        assert_eq!(out.code, Some(0));
        assert!(out.stdout.contains("v5.2.0"), "{out:?}");
        assert!(out.stderr.contains("oops"), "{out:?}");
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
    fn a_missing_program_is_an_error_that_names_no_arguments() {
        let mut cmd = quiet_command(Path::new("pitcrew-no-such-program"));
        cmd.args(["--token", "SECRET"]);
        let err = run_with_timeout(cmd, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, Error::Os { .. }));
        assert!(!err.to_string().contains("SECRET"));
    }
}
