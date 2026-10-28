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
pub fn run_with_timeout(cmd: Command, timeout: Duration) -> Result<ToolOutput, Error> {
    let started = Instant::now();
    run_until(cmd, timeout.as_secs(), |_| started.elapsed() >= timeout)
}

/// Runs `cmd` to completion unless `give_up(pid)` says to stop first; then the tool and every program it started are
/// ended and `TimedOut` (naming `secs`) is returned. The tool runs in a session of its own on Unix (it leads its process
/// group and has no controlling terminal, so an interactive login shell started from a terminal is not stopped by
/// `SIGTTIN`) and in a kill-on-close job object on Windows (which also holds helpers that left its tree).
pub(crate) fn run_until(mut cmd: Command, secs: u64, mut give_up: impl FnMut(u32) -> bool) -> Result<ToolOutput, Error> {
    let program = Path::new(cmd.get_program())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    tree::own_session(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| Error::Os {
        what: "starting a program",
        reason: format!("{program}: {e}"),
    })?;
    let tree = tree::Tree::of(&child);
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if give_up(child.id()) => {
                tree.end(&mut child);
                return Err(Error::TimedOut { program, secs });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                tree.end(&mut child);
                return Err(Error::Os {
                    what: "waiting for a program",
                    reason: e.to_string(),
                });
            }
        }
    };
    // On Windows, closing the job ends helpers the tool left running; their pipes then close.
    tree.close();
    let text = |h: Drain| String::from_utf8_lossy(&h.and_then(|h| h.join().ok()).unwrap_or_default()).into_owned();
    Ok(ToolOutput {
        success: status.success(),
        code: status.code(),
        stdout: text(out),
        stderr: text(err),
    })
}

/// `taskkill.exe` in the system folder, by full path (never whatever `taskkill` the `PATH` finds first).
#[cfg(windows)]
pub fn taskkill_exe() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::PathBuf::from(root).join("System32").join("taskkill.exe")
}

/// A tool's process tree: its own session on Unix, a kill-on-close job object on Windows.
mod tree {
    #![allow(unsafe_code)]
    use std::process::{Child, Command};

    /// Makes the tool start a session of its own (`setsid`): it leads a new process group, so `killpg` reaches every
    /// program it starts, and it has no controlling terminal to be stopped by.
    #[cfg(unix)]
    pub fn own_session(cmd: &mut Command) {
        use std::os::unix::process::CommandExt;
        // SAFETY: the closure runs between fork and exec and only calls setsid, which is async-signal-safe and touches no
        // memory of the parent.
        unsafe {
            cmd.pre_exec(|| nix::unistd::setsid().map(|_| ()).map_err(std::io::Error::from));
        }
    }

    #[cfg(not(unix))]
    pub fn own_session(_cmd: &mut Command) {}

    /// What ends the tree.
    pub struct Tree {
        #[cfg(windows)]
        job: Option<windows::Win32::Foundation::HANDLE>,
    }

    impl Tree {
        /// The tree of a just-started tool. On Windows the tool joins a kill-on-close job (a program it starts later
        /// joins too); when that fails (an old Windows, a job that forbids it) `taskkill /T` is the fallback.
        #[cfg(windows)]
        pub fn of(child: &Child) -> Tree {
            Tree { job: job_for(child) }
        }

        #[cfg(not(windows))]
        pub fn of(_child: &Child) -> Tree {
            Tree {}
        }

        /// The tool finished: on Windows the job is closed, which ends any helper it left running.
        pub fn close(self) {}

        /// Ends the tool and everything it started, and reaps the tool.
        pub fn end(&self, child: &mut Child) {
            #[cfg(unix)]
            {
                // The tool leads its own process group (`own_session`).
                if let Ok(pid) = i32::try_from(child.id()) {
                    let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGKILL);
                }
            }
            #[cfg(windows)]
            match self.job {
                // SAFETY: a job handle this value owns and has not closed.
                Some(job) => unsafe {
                    let _ = windows::Win32::System::JobObjects::TerminateJobObject(job, 1);
                },
                None => taskkill_tree(child.id()),
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    #[cfg(windows)]
    impl Drop for Tree {
        fn drop(&mut self) {
            if let Some(job) = self.job.take() {
                // SAFETY: the handle is owned here and closed once; closing it ends what is left in the job.
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(job);
                }
            }
        }
    }

    /// A new kill-on-close job holding `child`, or `None` when Windows refuses.
    #[cfg(windows)]
    fn job_for(child: &Child) -> Option<windows::Win32::Foundation::HANDLE> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: no attributes and no name; the handle is closed below on failure or owned by the `Tree`.
        let job = unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` is a valid structure of the size passed, for the class passed; both handles are valid while
        // `child` lives.
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0),
            )
            .and_then(|()| AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())))
        };
        if ok.is_err() {
            // SAFETY: the job was created above and is not used again.
            unsafe {
                let _ = CloseHandle(job);
            }
            return None;
        }
        Some(job)
    }

    /// `taskkill /PID n /T /F`, run directly (not through `run_with_timeout`, which would recurse) and waited for a few
    /// seconds at most.
    #[cfg(windows)]
    fn taskkill_tree(pid: u32) {
        use std::process::Stdio;
        use std::time::{Duration, Instant};
        let mut kill = super::quiet_command(&super::taskkill_exe());
        kill.args(["/PID", &pid.to_string(), "/T", "/F"])
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
    /// them: killing only the tool left its children running, holding its output pipes open. The test gives up on the
    /// tool once its tree is complete (no time budget), then waits until every member is gone.
    #[test]
    fn a_tool_that_hangs_is_stopped_with_the_programs_it_started() {
        // Unix: `; wait` keeps the shell from replacing itself with `sleep`, so the tool has a child of its own.
        // Windows: `start /b` runs a second ping beside the first, both children of `cmd`.
        let script = if cfg!(windows) {
            "start /b ping -n 300 127.0.0.1 >nul & ping -n 300 127.0.0.1 >nul"
        } else {
            "sleep 300 & wait"
        };
        // The shell and its sleep; on Windows `cmd` and its two pings.
        let complete = if cfg!(windows) { 3 } else { 2 };
        let guard = Instant::now() + HANG_GUARD;
        let mut members = vec![];
        let err = run_until(shell(script), 0, |pid| {
            members = tree_members(pid);
            members.len() >= complete || Instant::now() > guard
        })
        .unwrap_err();
        assert!(matches!(err, Error::TimedOut { .. }), "{err}");
        assert!(members.len() >= complete, "the tool started its programs: {members:?}");
        let gone = eventually_gone(&members);
        if !gone {
            for pid in &members {
                kill_pid(*pid);
            }
        }
        assert!(gone, "the tool's tree outlived it: {members:?}");
    }

    /// Ends a hung test, never a product interval.
    const HANG_GUARD: Duration = Duration::from_secs(60);

    /// The tool `pid` and its programs: its process group on Unix, its descendants on Windows.
    #[cfg(unix)]
    fn tree_members(pid: u32) -> Vec<u32> {
        let out = quiet_command(Path::new("pgrep")).args(["-g", &pid.to_string()]).output().unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect()
    }

    #[cfg(windows)]
    fn tree_members(pid: u32) -> Vec<u32> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        let mut tree = vec![Pid::from_u32(pid)];
        loop {
            let before = tree.len();
            for (p, proc_) in sys.processes() {
                if !tree.contains(p) && proc_.parent().is_some_and(|pp| tree.contains(&pp)) {
                    tree.push(*p);
                }
            }
            if tree.len() == before {
                break;
            }
        }
        // `cmd` gets a console host of its own, which is not one of the tool's programs.
        tree.into_iter()
            .filter(|p| {
                sys.process(*p)
                    .is_some_and(|q| !q.name().to_string_lossy().eq_ignore_ascii_case("conhost.exe"))
            })
            .map(|p| p.as_u32())
            .collect()
    }

    /// Whether every one of `pids` is gone (a zombie counts as gone), waiting on that condition up to the hang guard.
    fn eventually_gone(pids: &[u32]) -> bool {
        let guard = Instant::now() + HANG_GUARD;
        loop {
            if pids.iter().all(|p| !running(*p)) {
                return true;
            }
            if Instant::now() > guard {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    fn running(pid: u32) -> bool {
        let out = quiet_command(Path::new("ps"))
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        !stat.trim().is_empty() && !stat.trim().starts_with('Z')
    }

    #[cfg(windows)]
    fn running(pid: u32) -> bool {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
        sys.process(Pid::from_u32(pid)).is_some()
    }

    #[cfg(unix)]
    fn kill_pid(pid: u32) {
        if let Ok(p) = i32::try_from(pid) {
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), nix::sys::signal::Signal::SIGKILL);
        }
    }

    #[cfg(windows)]
    fn kill_pid(pid: u32) {
        let _ = quiet_command(&taskkill_exe()).args(["/PID", &pid.to_string(), "/F"]).output();
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
