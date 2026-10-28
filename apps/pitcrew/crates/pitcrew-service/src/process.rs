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

/// Runs `cmd` to completion, stopping it after `timeout`, together with every program it started (a login shell's
/// `~/.zshrc`, `cmd /c`). Error messages name the program only, never its arguments (a registration token is an
/// argument).
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<ToolOutput, Error> {
    let program = Path::new(cmd.get_program())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A process group of its own, so a timeout ends the programs it started too (`end_tree`).
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
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
                end_tree(&mut child);
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

/// Ends a timed-out tool and everything it started, and reaps it.
fn end_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The tool leads its own process group (`run_with_timeout`).
        if let Ok(pid) = i32::try_from(child.id()) {
            let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        // `taskkill /T` ends the tool's descendants; it runs directly (not through `run_with_timeout`, which would
        // recurse) and is not waited for longer than a few seconds.
        let mut kill = quiet_command(Path::new("taskkill.exe"));
        kill.args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Ok(mut k) = kill.spawn() {
            let deadline = Instant::now() + Duration::from_secs(10);
            while matches!(k.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = k.kill();
            let _ = k.wait();
        }
    }
    let _ = child.kill();
    let _ = child.wait();
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

    /// A tool that started programs of its own (a login shell running `~/.zshrc`, `cmd /c`) is stopped with all of
    /// them: killing only the tool left its children running, holding its output pipes open.
    #[cfg(unix)]
    #[test]
    fn a_tool_that_hangs_is_stopped_with_the_programs_it_started() {
        let dir = std::env::temp_dir().join(format!("pitcrew-tree-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("pid");
        // `; wait` keeps the shell from replacing itself with `sleep`: the shell has a child of its own.
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
        let err = run_with_timeout(shell(&script), Duration::from_millis(500)).unwrap_err();
        assert!(matches!(err, Error::TimedOut { .. }), "{err}");
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let gone = wait_gone(pid);
        if !gone {
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGKILL);
        }
        assert!(gone, "the tool's own child {pid} still runs");
    }

    /// Whether `pid` is gone (or a zombie) within a few seconds of being killed: a condition, not a fixed wait.
    #[cfg(unix)]
    fn wait_gone(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let out = quiet_command(Path::new("ps"))
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&out.stdout);
            if stat.trim().is_empty() || stat.trim().starts_with('Z') {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
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
