//! The one way Spotter starts child processes (plugins and "run a command" actions): no console window on Windows,
//! a cleared environment with only what programs commonly need, no shell, and the caller always bounds the run.

#![allow(clippy::disallowed_methods)] // this is the helper the lint points to

use std::path::Path;
use std::process::{Command, Stdio};

/// Environment variables passed through to children; everything else (tokens in the user's environment, for
/// example) is not.
const KEEP: &[&str] = &[
    "PATH",
    "SystemRoot",
    "windir",
    "SystemDrive",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "LANG",
    "LC_ALL",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// A command for `program` with no console window, a cleared environment and no standard input.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear();
    for key in KEEP {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Runs `program` with `args`, waiting at most `timeout`; the child is killed when it takes longer. Returns the
/// exit code (`None` when killed by a signal) or why it could not run.
pub fn run_bounded(program: &Path, args: &[String], timeout: std::time::Duration) -> Result<Option<i32>, String> {
    let mut child = command(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{} could not be started: {e}", program.display()))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code()),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} did not finish within {} seconds and was stopped.",
                    program.display(),
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return Err(format!("{} could not be watched: {e}", program.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn missing_programs_are_reported() {
        let e = run_bounded(Path::new("/definitely/not/here"), &[], Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("could not be started"));
    }

    #[cfg(windows)]
    #[test]
    fn commands_run_and_time_out() {
        let cmd = Path::new(r"C:\Windows\System32\cmd.exe");
        assert_eq!(
            run_bounded(cmd, &["/c".into(), "exit 3".into()], Duration::from_secs(10)),
            Ok(Some(3))
        );
        let ping = Path::new(r"C:\Windows\System32\ping.exe");
        let e = run_bounded(ping, &["-n".into(), "30".into(), "127.0.0.1".into()], Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("did not finish"));
    }

    #[cfg(unix)]
    #[test]
    fn commands_run_and_time_out() {
        assert_eq!(
            run_bounded(Path::new("/bin/sh"), &["-c".into(), "exit 3".into()], Duration::from_secs(10)),
            Ok(Some(3))
        );
        let e = run_bounded(Path::new("/bin/sh"), &["-c".into(), "sleep 30".into()], Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("did not finish"));
    }
}
