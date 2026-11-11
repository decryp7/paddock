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

/// What decides whether a program may start with the service's own token.
pub type TokenCheck = Box<dyn Fn(&LaunchSpec) -> Result<(), String> + Send>;

/// A launcher that starts a runner as the service account only when `check` accepts its program and folder, and otherwise
/// reports why and starts nothing. Runners in a user's session are not asked: they run with that user's rights.
///
/// On Windows the service runs as LocalSystem, so a program a standard user can replace would run as SYSTEM (the hole closed
/// in 1.1: see [`crate::protected`]). The check is a parameter so the rule is tested without a Windows machine.
pub struct ServiceTokenGuard<L> {
    inner: L,
    check: TokenCheck,
}

impl<L: Launcher> ServiceTokenGuard<L> {
    /// Guards `inner` with `check`.
    pub fn new(inner: L, check: TokenCheck) -> Self {
        ServiceTokenGuard { inner, check }
    }
}

impl<L: Launcher> Launcher for ServiceTokenGuard<L> {
    fn console(&self) -> Console {
        self.inner.console()
    }

    fn launch(&self, target: LaunchTarget, spec: &LaunchSpec, sink: LineSink) -> Result<Box<dyn RunnerProcess>, String> {
        if target == LaunchTarget::ServiceAccount {
            (self.check)(spec)?;
        }
        self.inner.launch(target, spec, sink)
    }
}

/// What decides whether the service may run a program (with its folder) with its own token: a runner's program at start, but also
/// its version probe, `generate-config` and `register` (everything the service starts for a runner).
pub type ProgramGate = std::sync::Arc<dyn Fn(&std::path::Path, &std::path::Path) -> Result<(), String> + Send + Sync>;

/// A gate that accepts everything: every platform where the service runs as the user, and tests.
pub fn open_gate() -> ProgramGate {
    std::sync::Arc::new(|_, _| Ok(()))
}

/// The gate of the real Windows service: when it runs as LocalSystem (its own token's user is `S-1-5-18`), the shared trust rule
/// over the real machine; when it runs as a user (development), nothing, because the token is the user's own.
#[cfg(windows)]
pub fn windows_program_gate() -> ProgramGate {
    std::sync::Arc::new(|program, dir| {
        if !crate::platform::win_session::is_local_system() {
            return Ok(());
        }
        let mut machine = paddock_service::privileged::trust_real::RealTrust;
        crate::protected::check_service_launch(&mut machine, program, dir)
    })
}

/// The program gate of this OS.
pub fn os_program_gate() -> ProgramGate {
    #[cfg(windows)]
    {
        windows_program_gate()
    }
    #[cfg(not(windows))]
    {
        open_gate()
    }
}

/// The check of the real Windows service for a runner's own start ([`windows_program_gate`] on the spec's program and folder).
#[cfg(windows)]
pub fn windows_token_check() -> TokenCheck {
    let gate = windows_program_gate();
    Box::new(move |spec| gate(&spec.program, &spec.dir))
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Counting(Arc<AtomicUsize>);

    impl Launcher for Counting {
        fn console(&self) -> Console {
            Console { session: Some(1) }
        }
        fn launch(&self, _: LaunchTarget, _: &LaunchSpec, _: LineSink) -> Result<Box<dyn RunnerProcess>, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err("started".into())
        }
    }

    fn spec() -> LaunchSpec {
        LaunchSpec {
            program: "C:\\Users\\someone\\run.exe".into(),
            args: vec![],
            dir: "C:\\Users\\someone".into(),
            env: vec![],
        }
    }

    #[test]
    fn a_service_token_start_needs_the_check_and_a_user_session_start_does_not() {
        let started = Arc::new(AtomicUsize::new(0));
        let guard = ServiceTokenGuard::new(Counting(started.clone()), Box::new(|_| Err("not protected".into())));
        let sink: LineSink = Arc::new(|_| {});
        let refused = guard.launch(LaunchTarget::ServiceAccount, &spec(), sink.clone());
        assert_eq!(refused.err().as_deref(), Some("not protected"));
        assert_eq!(started.load(Ordering::SeqCst), 0, "nothing was started");
        let user = guard.launch(LaunchTarget::UserSession { session_id: 1 }, &spec(), sink.clone());
        assert_eq!(user.err().as_deref(), Some("started"));
        let accepting = ServiceTokenGuard::new(Counting(started.clone()), Box::new(|_| Ok(())));
        assert_eq!(
            accepting.launch(LaunchTarget::ServiceAccount, &spec(), sink).err().as_deref(),
            Some("started")
        );
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(guard.console(), Console { session: Some(1) });
    }
}
