//! The ports: everything that touches the OS goes through one of these, so tests never touch the real launchd, Login
//! Items, systemd, registry or services (rule 7). The real adapters are [`crate::system::RealSystem`] and the native
//! glue in `native.rs`; tests use [`crate::testing`], which records every command and file write.

use paddock_proc::RunState;
use std::path::Path;
use std::time::Duration;

/// What a helper program printed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    /// The exit code (`None` when it was ended by a signal).
    pub code: Option<i32>,
    /// Standard output (lossy UTF-8, bounded).
    pub stdout: String,
    /// Standard error (lossy UTF-8, bounded).
    pub stderr: String,
}

impl Output {
    /// Exit code 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// An output with only an exit code (tests).
    pub fn code(code: i32) -> Output {
        Output {
            code: Some(code),
            ..Output::default()
        }
    }

    /// Exit code 0 with `stdout` (tests).
    pub fn ok(stdout: &str) -> Output {
        Output {
            code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    /// Standard error or, when it is empty, standard output: the tool's own words, first line.
    pub fn reason(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        let line = text.lines().map(str::trim).find(|l| !l.is_empty());
        match (line, self.code) {
            (Some(l), _) => l.to_string(),
            (None, Some(c)) => format!("it exited with code {c}"),
            (None, None) => "it was stopped".to_string(),
        }
    }
}

/// Why a helper program gave no output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// It could not be started (missing, not executable).
    NotStarted(String),
    /// It did not finish in time and its tree was ended.
    TimedOut(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::NotStarted(why) => write!(f, "could not be started: {why}"),
            RunError::TimedOut(what) => write!(f, "{what} did not finish in time and was stopped"),
        }
    }
}

impl std::error::Error for RunError {}

/// Helper programs, files of the OS's registrations and waiting.
pub trait System: Send {
    /// Runs `program args…` to completion with a timeout and bounded output.
    fn run(&mut self, program: &str, args: &[&str]) -> Result<Output, RunError>;
    /// Starts `program args…` detached (no console window), without waiting (a `Run` entry's agent after registering).
    fn spawn(&mut self, program: &Path, args: &[String]) -> Result<(), String>;
    /// The process that holds `pid` now, by identity (pid and start time, rule 53); `None` when there is none. Tests answer from
    /// a table, so what they see never depends on the processes of the machine they run on (ER-14).
    fn process(&mut self, pid: u32) -> Option<paddock_proc::ProcessId> {
        paddock_proc::ProcessId::of(pid)
    }
    /// Reads a small text file (a plist, a unit).
    fn read(&mut self, path: &Path) -> std::io::Result<String>;
    /// Writes a file atomically, creating its folder.
    fn write(&mut self, path: &Path, text: &str) -> std::io::Result<()>;
    /// [`write`](Self::write) for a file only its owner may read or change (0600 on Unix): a job definition launchd loads.
    /// The default writes like `write` (a fake keeps its files in memory).
    fn write_private(&mut self, path: &Path, text: &str) -> std::io::Result<()> {
        self.write(path, text)
    }
    /// Removes a file; a missing file is `NotFound`.
    fn remove(&mut self, path: &Path) -> std::io::Result<()>;
    /// Whether `path` is a non-empty file (a build placeholder is empty).
    fn is_program(&mut self, path: &Path) -> bool;
    /// Waits (the fake only advances its clock).
    fn pause(&mut self, how_long: Duration);
    /// The CPU time the process `pid` (and its children) used so far, `None` when the OS did not say or there is no such
    /// process. For judging a start that is taking long; `pid` was just read from the OS's own report. Not known by default.
    fn cpu_time(&mut self, _pid: u32) -> Option<Duration> {
        None
    }
    /// Whether the process `pid` is stopped by the OS or another app ([`RunState::Stopped`]), read by process identity.
    /// [`RunState::Unknown`] when the OS does not say (the default).
    fn run_state(&mut self, _pid: u32) -> RunState {
        RunState::Unknown
    }
}

/// `SMAppService.status`, without the Objective-C type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppServiceStatus {
    /// Registered and allowed.
    Enabled,
    /// Not registered (never installed, removed, or switched off).
    NotRegistered,
    /// Registered, but the user has to allow it in Login Items (or switched it off there).
    RequiresApproval,
    /// Background Task Management has no record of the agent: the app never registered it (the normal state of a fresh
    /// install on macOS 13 to 26), or it is not where it can be registered from. Only `register` tells the two apart.
    NotFound,
}

/// Why registering failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// macOS needs the user's approval in Login Items. Retrying cannot help.
    NeedsApproval,
    /// Administrator rights are needed. Retrying cannot help.
    NeedsElevation,
    /// The user declined the OS's prompt. Retrying cannot help.
    Declined,
    /// It cannot be registered from here (the app outside /Applications, no systemd user manager). Retrying cannot help.
    Unavailable(String),
    /// Anything else, in words; worth another try.
    Other(String),
    /// The write reported success, but reading the registration back shows something else (ADR 0054 section 1.1). It
    /// carries what the OS holds; it is never `Ok`.
    NotAsWritten(crate::backend::Inspection),
}

impl RegisterError {
    /// Whether another attempt can help.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            RegisterError::Other(_) | RegisterError::NotAsWritten(_)
        )
    }
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::NeedsApproval => f.write_str("macOS needs approval in Login Items"),
            RegisterError::NeedsElevation => f.write_str("administrator rights are needed"),
            RegisterError::Declined => f.write_str("the permission prompt was declined"),
            RegisterError::Unavailable(why) | RegisterError::Other(why) => f.write_str(why),
            RegisterError::NotAsWritten(look) => write!(
                f,
                "the registration was written, but reading it back shows registration {:?}, job {:?}{}",
                look.registration,
                look.job,
                if look.definition_current == Some(false) {
                    ", and another definition is in force"
                } else {
                    ""
                }
            ),
        }
    }
}

/// `SMAppService` for one bundled plist (macOS 13+).
pub trait AppService: Send {
    /// The registration's status.
    fn status(&mut self) -> AppServiceStatus;
    /// Registers the launch agent from the bundle.
    fn register(&mut self) -> Result<(), RegisterError>;
    /// Unregisters it.
    fn unregister(&mut self) -> Result<(), String>;
    /// Opens System Settings > General > Login Items (the user switches the item; the app never does).
    fn open_login_items(&mut self);
}

/// How the wait for an elevated program ended, as the adapter observed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevatedEnd {
    /// The program exited with this code.
    Exited(u32),
    /// The OS gave no process to watch (it finished before it could be): nothing says it failed.
    NoProcess,
    /// It still ran when the backstop ceiling came; it was not stopped.
    StillRunning(Duration),
    /// The wait itself failed (Windows error code): nothing is known about the program.
    WaitFailed(u32),
}

/// What one slice of waiting for an elevated program saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slice {
    /// The program exited with this code.
    Exited(u32),
    /// It still runs.
    Running,
    /// The wait itself failed (Windows error code).
    Failed(u32),
}

/// Waits for an elevated program in slices of `slice`, up to `ceiling`: `wait(slice)` waits one slice and says what it saw,
/// `working(waited)` is called after each slice in which the program still ran (the log's "still working"). A failed wait
/// ends at once with its error, never spun on. The adapter supplies the OS calls; this is the logic.
pub fn wait_in_slices(
    ceiling: Duration,
    slice: Duration,
    wait: &mut dyn FnMut(Duration) -> Slice,
    working: &mut dyn FnMut(Duration),
) -> ElevatedEnd {
    let slice = slice.min(ceiling).max(Duration::from_millis(1));
    let mut waited = Duration::ZERO;
    loop {
        let seen = wait(slice);
        waited += slice;
        match seen {
            Slice::Exited(code) => return ElevatedEnd::Exited(code),
            Slice::Failed(code) => return ElevatedEnd::WaitFailed(code),
            Slice::Running if waited >= ceiling => return ElevatedEnd::StillRunning(waited),
            Slice::Running => working(waited),
        }
    }
}

/// What an elevated run's end means for the caller: the exit code, or, for a program still running at the ceiling, an
/// error that says so (ER-19: a run that has not ended is not a failed one, and its caller must not report it failed).
pub fn elevated_result(program: &Path, end: ElevatedEnd) -> Result<i32, RegisterError> {
    match end {
        ElevatedEnd::Exited(code) => Ok(i32::try_from(code).unwrap_or(1)),
        ElevatedEnd::NoProcess => Ok(0),
        ElevatedEnd::WaitFailed(code) => Err(RegisterError::Other(format!(
            "waiting for {} failed (Windows error {code}); it may still be running. Check the service in the Services window, then press Repair again if it did not finish",
            program.display()
        ))),
        ElevatedEnd::StillRunning(waited) => Err(RegisterError::Other(format!(
            "{} was still running after {} minutes and has not been stopped; it may still finish. Check the service in the Services window, then press Repair again if it did not",
            program.display(),
            waited.as_secs() / 60
        ))),
    }
}

/// Runs a program with the OS's own elevation prompt (Windows UAC) and waits for it.
pub trait Elevator: Send {
    /// Runs `program args…` elevated and waits for it to end, up to `ceiling` (a backstop, not a judge: a program that is
    /// still running then is reported as such by [`elevated_result`], never as failed; the adapter says it is still working
    /// in the log while it waits). `Ok(exit code)`, or [`RegisterError::Declined`] when the user said no.
    fn run_elevated(
        &mut self,
        program: &Path,
        args: &[&str],
        ceiling: Duration,
    ) -> Result<i32, RegisterError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elevated_program_still_running_is_not_reported_failed() {
        let p = Path::new("C:\\app\\server.exe");
        assert_eq!(elevated_result(p, ElevatedEnd::Exited(0)), Ok(0));
        assert_eq!(elevated_result(p, ElevatedEnd::Exited(5)), Ok(5));
        assert_eq!(elevated_result(p, ElevatedEnd::Exited(u32::MAX)), Ok(1));
        assert_eq!(elevated_result(p, ElevatedEnd::NoProcess), Ok(0));
        let why = elevated_result(p, ElevatedEnd::StillRunning(Duration::from_secs(900)))
            .unwrap_err()
            .to_string();
        assert!(
            why.contains("still running after 15 minutes")
                && why.contains("not been stopped")
                && !why.contains("failed"),
            "{why}"
        );
    }

    #[test]
    fn the_wait_in_slices_ends_on_exit_failure_or_the_ceiling_and_never_spins_on_a_failure() {
        let run = |seen: Vec<Slice>| {
            let mut seen = seen.into_iter();
            let mut waits = 0u32;
            let mut notes = Vec::new();
            let end = wait_in_slices(
                Duration::from_secs(90),
                Duration::from_secs(30),
                &mut |s| {
                    waits += 1;
                    assert_eq!(s, Duration::from_secs(30));
                    seen.next().unwrap_or(Slice::Running)
                },
                &mut |w| notes.push(w.as_secs()),
            );
            (end, waits, notes)
        };
        assert_eq!(
            run(vec![Slice::Exited(0)]),
            (ElevatedEnd::Exited(0), 1, vec![])
        );
        assert_eq!(
            run(vec![Slice::Running, Slice::Exited(3)]),
            (ElevatedEnd::Exited(3), 2, vec![30])
        );
        // A failed wait (WAIT_FAILED) ends the loop at once with its error code.
        assert_eq!(
            run(vec![Slice::Running, Slice::Failed(6)]),
            (ElevatedEnd::WaitFailed(6), 2, vec![30])
        );
        // Running to the ceiling: three slices, still working said after the first two.
        assert_eq!(
            run(vec![]),
            (
                ElevatedEnd::StillRunning(Duration::from_secs(90)),
                3,
                vec![30, 60]
            )
        );
        // A ceiling shorter than a slice is one short slice.
        let mut waited = Vec::new();
        let mut notes = Vec::new();
        let end = wait_in_slices(
            Duration::from_secs(5),
            Duration::from_secs(30),
            &mut |s| {
                waited.push(s);
                Slice::Running
            },
            &mut |w| notes.push(w),
        );
        assert_eq!(
            (end, waited),
            (
                ElevatedEnd::StillRunning(Duration::from_secs(5)),
                vec![Duration::from_secs(5)]
            )
        );
        assert!(notes.is_empty());
        let why = elevated_result(Path::new("C:\\s.exe"), ElevatedEnd::WaitFailed(6))
            .unwrap_err()
            .to_string();
        assert!(
            why.contains("Windows error 6") && why.contains("Repair again"),
            "{why}"
        );
    }

    #[test]
    fn outputs_give_the_tools_own_words() {
        let mut o = Output::code(3);
        assert!(!o.success());
        assert_eq!(o.reason(), "it exited with code 3");
        o.stdout = "\n out line \n".into();
        assert_eq!(o.reason(), "out line");
        o.stderr = "err line\nmore".into();
        assert_eq!(o.reason(), "err line");
        assert_eq!(Output::default().reason(), "it was stopped");
        assert!(Output::ok("x").success());
        assert!(
            RunError::NotStarted("gone".into())
                .to_string()
                .contains("gone")
        );
        assert!(
            RunError::TimedOut("launchctl".into())
                .to_string()
                .contains("in time")
        );
    }

    #[test]
    fn a_system_that_cannot_read_cpu_times_or_run_states_does_not_know_them() {
        struct Bare;
        impl System for Bare {
            fn run(&mut self, _: &str, _: &[&str]) -> Result<Output, RunError> {
                Ok(Output::default())
            }
            fn spawn(&mut self, _: &Path, _: &[String]) -> Result<(), String> {
                Ok(())
            }
            fn read(&mut self, _: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }
            fn write(&mut self, _: &Path, _: &str) -> std::io::Result<()> {
                Ok(())
            }
            fn remove(&mut self, _: &Path) -> std::io::Result<()> {
                Ok(())
            }
            fn is_program(&mut self, _: &Path) -> bool {
                false
            }
            fn pause(&mut self, _: Duration) {}
        }
        assert_eq!(Bare.cpu_time(std::process::id()), None);
        assert_eq!(Bare.run_state(std::process::id()), RunState::Unknown);
    }

    #[test]
    fn only_other_register_errors_are_retried() {
        assert!(RegisterError::Other("busy".into()).retryable());
        for e in [
            RegisterError::NeedsApproval,
            RegisterError::NeedsElevation,
            RegisterError::Declined,
            RegisterError::Unavailable("x".into()),
        ] {
            assert!(!e.retryable());
            assert!(!e.to_string().is_empty());
        }
        assert_eq!(RegisterError::Other("busy".into()).to_string(), "busy");
    }
}
