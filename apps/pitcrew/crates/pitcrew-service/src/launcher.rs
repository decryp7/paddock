//! Starting runners where they should run (ADR 0003), behind a trait so the supervisor can be tested with any
//! process.
//!
//! - Windows as LocalSystem: an interactive runner starts in the console user's session with their token
//!   (`platform::win_session`); a service-mode runner (or the logged-off fallback) is a child of the service.
//! - Windows as an ordinary user (development), macOS (the service is the user's launch agent) and Linux: every
//!   runner is a child of the service, which already runs in the user's session.

use crate::launch::{spawn_child, LaunchSpec, LineSink, RunnerProcess};
use pitcrew_core::session::LaunchTarget;

/// Who is logged on, as far as launching is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Console {
    /// The console user's session (`None`: nobody is logged on).
    pub session: Option<u32>,
}

/// Starts runner processes.
pub trait Launcher: Send {
    /// The console user now.
    fn console(&self) -> Console;
    /// Starts `spec` at `target`.
    fn launch(&self, target: LaunchTarget, spec: &LaunchSpec, sink: LineSink) -> Result<Box<dyn RunnerProcess>, String>;
}

/// The launcher of this OS.
#[derive(Debug, Default)]
pub struct OsLauncher;

impl Launcher for OsLauncher {
    fn console(&self) -> Console {
        #[cfg(windows)]
        {
            use crate::platform::win_session::{console_user, ConsoleUser};
            match console_user() {
                ConsoleUser::Session(s) => Console { session: Some(s) },
                // Not LocalSystem: we are the user; session 0 stands for "our own session" (plain children).
                ConsoleUser::NotSystem => Console { session: Some(0) },
                ConsoleUser::Nobody => Console { session: None },
            }
        }
        #[cfg(not(windows))]
        {
            Console { session: Some(0) }
        }
    }

    fn launch(&self, target: LaunchTarget, spec: &LaunchSpec, sink: LineSink) -> Result<Box<dyn RunnerProcess>, String> {
        match target {
            #[cfg(windows)]
            LaunchTarget::UserSession { session_id } if session_id != 0 => {
                let p = crate::platform::win_session::spawn_in_session(session_id, spec, sink)?;
                Ok(Box::new(p))
            }
            _ => Ok(Box::new(spawn_child(spec, sink)?)),
        }
    }
}
