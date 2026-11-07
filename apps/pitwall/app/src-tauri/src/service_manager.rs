//! Pitwall's background service on macOS through the shared [`paddock_service::Manager`] (ADR 0014 §9, rules 42 and 55):
//! the launch agent inside the app bundle registered through `SMAppService`, the stale-registration repair of macOS 26, the
//! once-per-version re-registration and the Login Items switch all live in the shared crate. This module only says what
//! Pitwall's service is (its [`spec`]) and how Pitwall asks whether the server answers.

use crate::app_state;
use crate::commands::{forward, SharedClient};
use paddock_service::backend::Side;
use paddock_service::render::{launchd_plist, LaunchdProgram};
use paddock_service::{Manager, RepairTracker, ServiceError, StartWait, Timings};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use std::sync::{Arc, OnceLock};

pub use pitwall_service::{spec, LABEL, SUPERVISED_ENV};

/// What the app is called in messages.
const APP_NAME: &str = pitwall_service::NAME;

/// The bundled launch agent the app ships (`macos/net.decryptology.pitwall.service.plist`, checked by the app's tests).
pub fn bundled_plist() -> String {
    launchd_plist(&spec(), LaunchdProgram::Bundled)
}

/// The repair's progress, shared by every manager built here so a status read sees a repair another call started.
fn tracker() -> Arc<RepairTracker> {
    static TRACKER: OnceLock<Arc<RepairTracker>> = OnceLock::new();
    TRACKER.get_or_init(|| Arc::new(RepairTracker::new())).clone()
}

/// How a failure of the shared manager reaches the UI.
pub fn command_error(e: ServiceError) -> CommandError {
    let code = match &e {
        ServiceError::NeedsApproval | ServiceError::SwitchedOff | ServiceError::Declined => ErrorCode::Forbidden,
        ServiceError::Busy => ErrorCode::Conflict,
        ServiceError::Unavailable(_) => ErrorCode::NotAvailable,
        _ => ErrorCode::Internal,
    };
    CommandError::new(code, e.to_string())
}

/// The manager of Pitwall's service, with `client` as the check whether the server answers. Its methods block on bounded OS
/// calls: call them from a blocking thread. `timings` are the shared defaults except in tests.
pub fn manager(client: &SharedClient, timings: Timings) -> Result<Manager, CommandError> {
    let data = app_state::data_dir().ok_or_else(|| CommandError::new(ErrorCode::Internal, "Pitwall's data folder cannot be found"))?;
    let program = crate::service_install::server_exe()?;
    let backend = paddock_service::native::host_backend(&spec(), &program, Side::App, &data).map_err(command_error)?;
    let client = client.clone();
    // Asked from a blocking thread, so blocking on the async client here does not stall the runtime.
    let answers = move || tauri::async_runtime::block_on(forward(client.as_ref(), "overview", serde_json::Value::Null)).is_ok();
    Ok(manager_over(backend, &data, &program, timings, answers))
}

/// Pitwall's manager over `backend`: the one place that says how the app watches the service start (the real `manager` and the
/// test build it through here).
fn manager_over(
    backend: Box<dyn paddock_service::Backend>,
    data: &std::path::Path,
    program: &std::path::Path,
    timings: Timings,
    answers: impl FnMut() -> bool + Send + 'static,
) -> Manager {
    Manager::new(APP_NAME, backend, data, LABEL, env!("CARGO_PKG_VERSION"), answers)
        .with_program(&program.to_string_lossy())
        .with_tracker(tracker())
        // The wait for the service after a registration is judged on what launchd shows of the job (a slow start is waited
        // for, a vanished, crash-looping or stuck one fails early with what was seen) and not a fixed 120 s (ADR 0056
        // amendment, ADR 0031). Backends that cannot observe the job (systemd, Windows) keep the fixed wait.
        .with_start_watch(Box::new(StartWait::with_poll(timings.start_poll)))
        .with_timings(timings)
}

/// The step the running repair is at (`None` when none works), for the job's phase label.
pub fn repair_step() -> Option<String> {
    // The manager stamps the notice with the shared monotonic clock (its default), so it is read on the same clock.
    tracker()
        .current(paddock_jobs::time::Clock::monotonic(&paddock_jobs::time::SystemClock))
        .and_then(|n| n.step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::backend::{Systemd, Target};
    use paddock_service::testing::FakeSystem;

    #[test]
    fn pitwalls_manager_judges_the_wait_for_the_service() {
        // Over a fake systemd: nothing of the machine is touched (ER-14). The manager is the one the app builds.
        let dir = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let backend = Systemd::new(
            spec(),
            dir.path().join("pitwall.service"),
            Target::exe("/opt/pitwall/pitwall-server"),
            Box::new(sys.clone()),
        );
        let m = manager_over(
            Box::new(backend),
            dir.path(),
            std::path::Path::new("/opt/pitwall/pitwall-server"),
            Timings::default(),
            || false,
        );
        assert!(m.watches_start(), "the wait for the service is judged");
        assert!(sys.calls().is_empty(), "building the manager asks the machine nothing");
    }

    /// The app's wiring on Windows: the manager over the elevated service backend asks Windows' prompt to run the server
    /// program's own `install` and `uninstall` verbs (the elevated child is `pitwall-server`, never a password, ADR 0056).
    #[test]
    fn on_windows_install_and_uninstall_run_the_servers_verbs_through_the_prompt() {
        use paddock_service::backend::ElevatedWindowsService;
        use paddock_service::testing::{Call, FakeElevator};
        let dir = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let program = std::path::PathBuf::from(r"C:\Program Files\Pitwall\pitwall-server.exe");
        let backend = ElevatedWindowsService::new(
            pitwall_service::spec_of(true),
            program.clone(),
            Box::new(sys.clone()),
            Box::new(FakeElevator::new(&sys)),
        );
        let mut m = manager_over(
            Box::new(backend),
            dir.path(),
            &program,
            Timings::default().without_start_wait(),
            || false,
        );
        let _ = m.install(&std::sync::atomic::AtomicBool::new(false));
        let _ = m.uninstall();
        let asked: Vec<String> = sys
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Os(t) if t.starts_with("elevated ") => Some(t),
                _ => None,
            })
            .collect();
        assert!(asked.iter().any(|t| t.ends_with("pitwall-server.exe install")), "{asked:?}");
        assert!(asked.iter().any(|t| t.ends_with("pitwall-server.exe uninstall")), "{asked:?}");
    }
}
