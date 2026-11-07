//! Pitwall's background service on macOS through the shared [`paddock_service::Manager`] (ADR 0014 §9, rules 42 and 55):
//! the launch agent inside the app bundle registered through `SMAppService`, the stale-registration repair of macOS 26, the
//! once-per-version re-registration and the Login Items switch all live in the shared crate. This module only says what
//! Pitwall's service is (its [`spec`]) and how Pitwall asks whether the server answers.

use crate::app_state;
use crate::commands::{forward, SharedClient};
use paddock_service::backend::Side;
use paddock_service::render::{launchd_plist, LaunchdProgram};
use paddock_service::{Manager, ProcessType, RepairTracker, ServiceError, ServiceSpec, Timings};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use std::sync::{Arc, OnceLock};

/// The launchd label; the bundled agent's plist is `<label>.plist` (`bundle.macOS.files` in `tauri.macos.conf.json`).
pub const LABEL: &str = "net.decryptology.pitwall.service";

/// The variable that tells the server it is supervised (it adds the login shell's PATH and watches for updates).
pub const SUPERVISED_ENV: &str = "PITWALL_SERVICE";

/// What the app is called in messages.
const APP_NAME: &str = "Pitwall";

/// Pitwall's service; the bundled plist and every other OS file are rendered from it.
pub fn spec() -> ServiceSpec {
    ServiceSpec::builder(LABEL, APP_NAME, "pitwall-server")
        .unit("pitwall")
        .bundle_id("net.decryptology.pitwall")
        .display_name("Pitwall server")
        .description("The Pitwall server: answers the app, the command line and the OpenAI-compatible API on this computer.")
        .supervised_env(SUPERVISED_ENV)
        .process_type(ProcessType::Interactive)
        .build()
        .unwrap_or_else(|e| unreachable!("Pitwall's own spec is valid: {e}"))
}

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
    Ok(Manager::new(APP_NAME, backend, &data, LABEL, env!("CARGO_PKG_VERSION"), answers)
        .with_program(&program.to_string_lossy())
        .with_tracker(tracker())
        .with_timings(timings))
}

/// The step the running repair is at (`None` when none works), for the job's phase label.
pub fn repair_step() -> Option<String> {
    // The manager stamps the notice with the shared monotonic clock (its default), so it is read on the same clock.
    tracker()
        .current(paddock_jobs::time::Clock::monotonic(&paddock_jobs::time::SystemClock))
        .and_then(|n| n.step)
}
