//! The real [`System`]: helper programs through `paddock-proc` (no console window, a timeout that ends the whole tree,
//! bounded output), files through `paddock-env`'s atomic write.

use crate::port::{Output, RunError, System};
use paddock_proc::{Limits, Outcome};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one helper call may take (`launchctl` and `systemctl` answer in well under a second; a hung one must not
/// hang a repair).
pub const HELPER_TIMEOUT: Duration = Duration::from_secs(30);
/// The most output kept from one helper call.
pub const MAX_HELPER_OUTPUT: usize = 256 * 1024;
/// The largest registration file read back.
pub const MAX_FILE: u64 = 1024 * 1024;

/// The OS itself.
#[derive(Debug, Clone)]
pub struct RealSystem {
    timeout: Duration,
}

impl Default for RealSystem {
    fn default() -> Self {
        Self {
            timeout: HELPER_TIMEOUT,
        }
    }
}

impl RealSystem {
    /// The OS with the default helper timeout.
    pub fn new() -> Self {
        Self::default()
    }

    /// Another helper timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl System for RealSystem {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<Output, RunError> {
        let mut cmd = paddock_proc::command(program);
        cmd.args(args);
        let out =
            paddock_proc::run_bounded(cmd, Limits::new(self.timeout).max_output(MAX_HELPER_OUTPUT))
                .map_err(|e| RunError::NotStarted(format!("{program}: {e}")))?;
        if out.outcome == Outcome::TimedOut {
            return Err(RunError::TimedOut(program.to_string()));
        }
        // Without `stop_on_cap` a run only ends by exiting.
        let code = if let Outcome::Exited(status) = out.outcome {
            status.code()
        } else {
            None
        };
        Ok(Output {
            code,
            stdout: out.stdout_text(),
            stderr: out.stderr_text(),
        })
    }

    fn spawn(&mut self, program: &Path, args: &[String]) -> Result<(), String> {
        let mut cmd = paddock_proc::command(program);
        cmd.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // Detached on purpose: the agent outlives the app that registered it (the OS starts it at the next log-in).
        cmd.spawn()
            .map(drop)
            .map_err(|e| format!("{} could not be started: {e}", program.display()))
    }

    fn read(&mut self, path: &Path) -> std::io::Result<String> {
        let mut text = String::new();
        std::fs::File::open(path)?
            .take(MAX_FILE)
            .read_to_string(&mut text)?;
        Ok(text)
    }

    fn write(&mut self, path: &Path, text: &str) -> std::io::Result<()> {
        paddock_env::write_atomic(path, text.as_bytes(), false).map_err(std::io::Error::other)
    }

    fn write_private(&mut self, path: &Path, text: &str) -> std::io::Result<()> {
        paddock_env::write_atomic(path, text.as_bytes(), true).map_err(std::io::Error::other)
    }

    fn remove(&mut self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    fn is_program(&mut self, path: &Path) -> bool {
        path.metadata().is_ok_and(|m| m.is_file() && m.len() > 0)
    }

    fn pause(&mut self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

/// Copies `exe` to `<data>/bin/<file name>` (atomically, executable) and returns the copy: an AppImage's own files
/// disappear when it closes, so a unit must not point into its mount.
pub fn stable_copy(exe: &Path, data_dir: &Path) -> std::io::Result<PathBuf> {
    let name = exe.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the program has no file name",
        )
    })?;
    let bin = data_dir.join("bin");
    std::fs::create_dir_all(&bin)?;
    let target = bin.join(name);
    let mut tmp_name = name.to_os_string();
    tmp_name.push(".new");
    let tmp = bin.join(tmp_name);
    std::fs::copy(exe, &tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn a_private_write_is_owner_only_and_a_plain_one_is_not() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (private, plain) = (dir.path().join("job.plist"), dir.path().join("unit"));
        let mut sys = RealSystem::new();
        sys.write_private(&private, "x").unwrap();
        sys.write(&plain, "y").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&private), 0o600);
        assert_ne!(
            mode(&plain) & 0o044,
            0,
            "a plain write stays readable by others"
        );
        assert_eq!(sys.read(&private).unwrap(), "x");
    }

    use super::*;

    #[cfg(unix)]
    #[test]
    fn helpers_report_their_output_and_a_hung_one_is_stopped() {
        let mut s = RealSystem::new();
        let o = s
            .run("/bin/sh", &["-c", "echo hi; echo oops >&2; exit 3"])
            .unwrap();
        assert_eq!(
            (o.code, o.stdout.trim(), o.stderr.trim()),
            (Some(3), "hi", "oops")
        );
        let mut quick = RealSystem::new().with_timeout(Duration::from_millis(300));
        assert!(matches!(
            quick.run("/bin/sh", &["-c", "sleep 30"]),
            Err(RunError::TimedOut(_))
        ));
        assert!(matches!(
            s.run("/nonexistent/helper", &[]),
            Err(RunError::NotStarted(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_prints_more_than_a_pipe_holds_is_read_while_it_runs() {
        // 300 kB: more than the 64 KiB a pipe buffers. Read only after the exit, the helper would block and time out.
        let mut s = RealSystem::new().with_timeout(Duration::from_secs(20));
        let o = s
            .run("/bin/sh", &["-c", "head -c 300000 /dev/zero | tr '\\0' a"])
            .unwrap();
        // It finished (no timeout) and the output is kept up to the cap.
        assert_eq!((o.code, o.stdout.len()), (Some(0), MAX_HELPER_OUTPUT));
    }

    #[test]
    fn files_are_written_atomically_read_back_and_removed() {
        let d = tempfile::tempdir().unwrap();
        let mut s = RealSystem::default();
        let f = d.path().join("LaunchAgents").join("x.plist");
        s.write(&f, "one").unwrap();
        s.write(&f, "two").unwrap();
        assert_eq!(s.read(&f).unwrap(), "two");
        assert!(s.is_program(&f));
        s.remove(&f).unwrap();
        assert_eq!(
            s.remove(&f).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert!(!s.is_program(&f));
        std::fs::write(&f, "").unwrap();
        assert!(
            !s.is_program(&f),
            "an empty build placeholder is no program"
        );
        assert!(!s.is_program(d.path()), "a folder is no program");
        s.pause(Duration::ZERO);
        assert!(s.spawn(&d.path().join("missing-program"), &[]).is_err());
    }

    #[test]
    fn an_appimage_gets_a_stable_executable_copy() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("pitwall-server");
        std::fs::write(&src, b"one").unwrap();
        let copy = stable_copy(&src, &d.path().join("data")).unwrap();
        assert_eq!(copy, d.path().join("data/bin/pitwall-server"));
        assert_eq!(std::fs::read(&copy).unwrap(), b"one");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        std::fs::write(&src, b"two").unwrap();
        stable_copy(&src, &d.path().join("data")).unwrap();
        assert_eq!(std::fs::read(&copy).unwrap(), b"two");
        assert!(stable_copy(&d.path().join("missing"), &d.path().join("data")).is_err());
        assert!(stable_copy(Path::new("/"), &d.path().join("data")).is_err());
    }
}
