//! Running a helper program (`systemctl`, the user's shell) with a hard timeout and bounded output. This is the one
//! synchronous place the service module starts programs, so a hung `.zshrc` or an unreachable user manager can never
//! hang the server or an installer.

use std::ffi::OsStr;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Most bytes kept of each output stream.
pub const MAX_OUTPUT: u64 = 64 * 1024;

/// What a helper printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Exit status 0.
    pub success: bool,
    /// Standard output (lossy UTF-8, bounded).
    pub stdout: String,
    /// Standard error (lossy UTF-8, bounded).
    pub stderr: String,
}

fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.by_ref().take(MAX_OUTPUT).read_to_end(&mut buf);
        // Keep reading (and dropping) so the child never blocks on a full pipe.
        let _ = std::io::copy(&mut r, &mut std::io::sink());
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// Runs `program args` with `env` added, killing it after `timeout`. Errors say what failed.
pub fn run_with_timeout(program: &OsStr, args: &[&str], env: &[(&str, &str)], timeout: Duration) -> Result<Output, String> {
    // The only synchronous spawn in the server: it is bounded by `timeout` below and has no console window to show.
    #[allow(clippy::disallowed_methods)]
    let mut cmd = Command::new(program);
    cmd.args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{} could not be started: {e}", program.to_string_lossy()))?;
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} did not finish within {timeout:?} and was stopped",
                    program.to_string_lossy()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("waiting for {} failed: {e}", program.to_string_lossy())),
        }
    };
    let join = |h: Option<std::thread::JoinHandle<String>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
    Ok(Output {
        success: status.success(),
        stdout: join(out),
        stderr: join(err),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn output_and_status_are_returned() {
        let o = run_with_timeout(
            OsStr::new("/bin/sh"),
            &["-c", "echo hi; echo oops >&2; exit 3"],
            &[],
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(!o.success);
        assert_eq!(o.stdout.trim(), "hi");
        assert_eq!(o.stderr.trim(), "oops");
        let o = run_with_timeout(
            OsStr::new("/bin/sh"),
            &["-c", "printf %s \"$X\""],
            &[("X", "v")],
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(o.success && o.stdout == "v");
    }

    #[test]
    fn a_hung_helper_is_stopped_and_a_missing_one_reported() {
        let started = Instant::now();
        let e = run_with_timeout(OsStr::new("/bin/sh"), &["-c", "sleep 30"], &[], Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("did not finish"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let e = run_with_timeout(OsStr::new("/nonexistent/prog"), &[], &[], Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("could not be started"), "{e}");
    }
}
