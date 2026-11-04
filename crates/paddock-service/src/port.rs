//! The ports: everything that touches the OS goes through one of these, so tests never touch the real launchd, Login
//! Items, systemd, registry or services (rule 7). The real adapters are [`crate::system::RealSystem`] and the native
//! glue in `native.rs`; tests use [`crate::testing`], which records every command and file write.

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

/// Runs a program with the OS's own elevation prompt (Windows UAC) and waits for it.
pub trait Elevator: Send {
    /// Runs `program args…` elevated; `Ok(exit code)`, or [`RegisterError::Declined`] when the user said no.
    fn run_elevated(
        &mut self,
        program: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<i32, RegisterError>;
}

#[cfg(test)]
mod tests {
    use super::*;

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
