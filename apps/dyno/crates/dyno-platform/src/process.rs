//! Child processes: every external program Dyno runs opens no console window and is bounded by a timeout. This is
//! the only module that creates [`Command`]s.

use dyno_core::ports::PlatformError;
use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Most bytes kept from each of a tool's output streams.
pub const MAX_TOOL_OUTPUT: usize = 4 * 1024 * 1024;

/// Windows process creation flag: no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command for `program` that opens no console window on Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
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

/// The program's file name, for messages (never its arguments).
fn program_name(cmd: &Command) -> String {
    Path::new(cmd.get_program())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Runs `cmd` to completion with stdin closed and stdout/stderr captured (each at most [`MAX_TOOL_OUTPUT`] bytes),
/// killing it after `timeout`. A non-zero exit is not an error: callers look at [`Output::status`].
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Output, PlatformError> {
    let program = program_name(&cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => PlatformError::Unsupported(format!("{program} is not available on this computer.")),
        std::io::ErrorKind::PermissionDenied => PlatformError::AccessDenied(format!("Not allowed to run {program}.")),
        _ => PlatformError::Failed(format!("Could not run {program}: {e}")),
    })?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let started = Instant::now();
    let status: ExitStatus = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PlatformError::Timeout(format!(
                    "{program} did not finish within {} s.",
                    timeout.as_secs_f32().max(0.1)
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                return Err(PlatformError::Failed(format!("Waiting for {program} failed: {e}")));
            }
        }
    };
    let bytes = |h: Drain| h.and_then(|h| h.join().ok()).unwrap_or_default();
    Ok(Output {
        status,
        stdout: bytes(out),
        stderr: bytes(err),
    })
}

/// Runs `cmd` and returns its standard output as text when it exits successfully; otherwise a
/// [`PlatformError::Failed`] with the start of its error output.
pub fn output_text(cmd: Command, timeout: Duration) -> Result<String, PlatformError> {
    let program = program_name(&cmd);
    let out = run_with_timeout(cmd, timeout)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim();
        Err(PlatformError::Failed(format!("{program} failed: {}", truncate(first, 200))))
    }
}

/// At most `max` characters of `s`.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut c = command("cmd.exe");
            c.args(["/d", "/c", script]);
            c
        } else {
            let mut c = command("/bin/sh");
            c.args(["-c", script]);
            c
        }
    }

    #[test]
    fn captures_output_of_a_finished_tool() {
        let out = run_with_timeout(shell("echo hello && echo oops 1>&2"), Duration::from_secs(20)).unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
        assert!(String::from_utf8_lossy(&out.stderr).contains("oops"));
        assert!(output_text(shell("echo hi"), Duration::from_secs(20)).unwrap().contains("hi"));
    }

    #[test]
    fn a_failing_tool_reports_its_error_output() {
        let err = output_text(shell("echo bad thing 1>&2 && exit 3"), Duration::from_secs(20)).unwrap_err();
        assert!(matches!(&err, PlatformError::Failed(m) if m.contains("bad thing")), "{err:?}");
    }

    #[test]
    fn stops_a_tool_that_hangs() {
        let hang = if cfg!(windows) { "ping -n 30 127.0.0.1 >nul" } else { "sleep 30" };
        let started = Instant::now();
        let err = run_with_timeout(shell(hang), Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, PlatformError::Timeout(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn a_missing_program_is_unsupported_and_names_no_arguments() {
        let mut cmd = command("dyno-no-such-program");
        cmd.args(["--token", "SECRET"]);
        let err = run_with_timeout(cmd, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(&err, PlatformError::Unsupported(m) if !m.contains("SECRET")), "{err:?}");
        assert_eq!(truncate("abcdef", 3), "abc");
    }
}
