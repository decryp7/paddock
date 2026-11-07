//! The elevated side of a system service's registration: what `<program> install` and `uninstall` do when Windows' own prompt
//! started them for the app ([`crate::backend::ElevatedWindowsService`]) or the installer ran them.
//!
//! Only the service manager's work, over a [`Backend`]: no [`crate::Manager`], so no record file, no data folder and no path
//! taken from an environment variable. A process with an administrator's token must not read or write in a folder the user
//! controls (`%APPDATA%`, a `PITWALL_HOME`-style override, a `--data-dir`): a link planted there would redirect what the
//! elevated process writes. The unelevated caller, the app's [`crate::Manager`] over `ElevatedWindowsService`, writes its
//! record before and after the elevated step; a registration the installer made is taken over by the app's start check
//! ([`crate::Plan::MarkCurrent`]).
//!
//! The decisions are the replace's own ([`crate::repair::replace`]: remove what is there, register, read back); a test runs the
//! whole install and removal over a fake service manager and asserts that no file is touched.

use crate::backend::{Backend, Target, UnregisterError, WindowsService};
use crate::repair::{RepairOutcome, Step, Timings};
use crate::spec::ServiceSpec;
use crate::system::RealSystem;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// The Service Control Manager service of `spec` starting `program`, over the real system tools (found in the system folder
/// Windows names, never on `PATH`).
pub fn windows_service(spec: &ServiceSpec, program: &Path) -> WindowsService {
    WindowsService::new(
        spec.clone(),
        Target::exe(program),
        Box::new(RealSystem::new()),
    )
}

/// Registers the service, replacing whatever is registered under its name, and returns at once (the elevated step does not
/// wait for the service to answer: it cannot ask it, and the caller waits).
///
/// # Errors
/// Why it was not registered, with what to do.
pub fn install(
    b: &mut dyn Backend,
    t: &Timings,
    say: &mut dyn FnMut(Step),
) -> Result<RepairOutcome, String> {
    let cancel = AtomicBool::new(false);
    let outcome = crate::repair::replace(b, &mut || false, &cancel, &t.without_start_wait(), say);
    match outcome {
        RepairOutcome::Recovered | RepairOutcome::Registered => Ok(outcome),
        RepairOutcome::Failed(why) | RepairOutcome::Unavailable(why) => Err(why),
        RepairOutcome::NeedsElevation => {
            Err("administrator rights are needed to register the service".to_string())
        }
        RepairOutcome::Declined => Err("the registration was declined".to_string()),
        RepairOutcome::NeedsApproval => Err("the service needs the user's approval".to_string()),
        RepairOutcome::Cancelled => Err("the registration was cancelled".to_string()),
    }
}

/// Stops and removes the registration and waits, bounded by [`Timings::gone_wait`], for its job to end. Removing what is not
/// there succeeds.
///
/// # Errors
/// Why it is still registered.
pub fn uninstall(b: &mut dyn Backend, t: &Timings) -> Result<(), String> {
    remove(b, t, &|| Duration::ZERO).map_err(|e| format!("The service could not be removed: {e}"))
}

/// The removal itself, the one implementation the manager's [`crate::Manager::uninstall`] uses too: unregister, ask the job to
/// end, and wait for it until [`Timings::gone_wait`] has passed by the polls' sum or by `elapsed` (a slow OS call counts).
/// A failed unregister is an error only when the registration is still there.
pub(crate) fn remove(
    b: &mut dyn Backend,
    t: &Timings,
    elapsed: &dyn Fn() -> Duration,
) -> Result<(), UnregisterError> {
    let unregistered = b.unregister();
    if let Err(e) = b.evict() {
        log::info!(error:% = e; "removing the service's job did not succeed");
    }
    let mut polled = Duration::ZERO;
    while !b.job_gone() && polled.max(elapsed()) < t.gone_wait {
        b.pause(t.gone_poll);
        polled += t.gone_poll;
    }
    match unregistered {
        Err(e) if b.inspect().present() => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Registration;
    use crate::spec::Kind;
    use crate::testing::{Call, FakeSystem};

    const EXE: &str = r"C:\Program Files\Pitwall\pitwall-server.exe";

    fn service(sys: &FakeSystem) -> WindowsService {
        sys.emulate_scm();
        sys.put_program(EXE);
        let spec = ServiceSpec::builder(
            "net.decryptology.pitwall.service",
            "Pitwall",
            "pitwall-server",
        )
        .kind(Kind::SystemService)
        .build()
        .unwrap();
        WindowsService::new(spec, Target::exe(EXE), Box::new(sys.clone()))
    }

    fn no_file_calls(sys: &FakeSystem) {
        let files: Vec<Call> = sys
            .calls()
            .into_iter()
            .filter(|c| matches!(c, Call::Write(..) | Call::Remove(_)))
            .collect();
        assert!(
            files.is_empty(),
            "the elevated step touched files: {files:?}"
        );
    }

    /// Review of the B1b/B6 merge: the elevated child ran the whole Manager, which writes its record into the data folder
    /// (a folder the user controls). It now does only the service manager's work.
    #[test]
    fn the_elevated_install_and_removal_are_service_manager_calls_only() {
        let sys = FakeSystem::new();
        sys.answer(
            "sc query",
            crate::port::Output::ok("STATE : 4  RUNNING\r\n"),
        );
        let mut b = service(&sys);
        let mut steps = vec![];
        let outcome = install(&mut b, &Timings::default(), &mut |s| steps.push(s)).unwrap();
        assert!(outcome.registered(), "{outcome:?}");
        let cmds = sys.commands();
        assert!(
            cmds.iter().any(|c| c.starts_with("sc create Pitwall")),
            "{cmds:?}"
        );
        assert!(
            cmds.iter()
                .all(|c| c.starts_with("sc ") || c.starts_with("reg ")),
            "{cmds:?}"
        );
        no_file_calls(&sys);
        sys.answer("sc query", crate::port::Output::code(1060));
        uninstall(&mut b, &Timings::default()).unwrap();
        assert!(sys.commands().iter().any(|c| c == "sc delete Pitwall"));
        assert_eq!(b.inspect().registration, Registration::Absent);
        no_file_calls(&sys);
    }

    #[test]
    fn a_registration_that_fails_says_why() {
        let sys = FakeSystem::new();
        let mut b = service(&sys);
        sys.answer("sc create", crate::port::Output::code(5));
        let e = install(&mut b, &Timings::default(), &mut |_| {}).unwrap_err();
        assert!(e.contains("administrator"), "{e}");
        no_file_calls(&sys);
    }
}
