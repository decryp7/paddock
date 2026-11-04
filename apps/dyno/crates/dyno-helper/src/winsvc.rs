//! The Windows service wrapper: the Service Control Manager starts `dyno-helper.exe service`, which reports Running,
//! serves, and stops when told. Only this module knows about the SCM; the daemon is [`crate::serve::serve`].

use crate::paths::WINDOWS_SERVICE;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

define_windows_service!(ffi_service_main, service_main);

/// Hands the process to the Service Control Manager; returns when the service has stopped.
pub fn run_as_service() -> Result<(), String> {
    service_dispatcher::start(WINDOWS_SERVICE, ffi_service_main)
        .map_err(|e| format!("This program must be started by Windows as a service: {e}"))
}

fn service_main(_arguments: Vec<OsString>) {
    let _ = serve_until_stopped();
}

fn status(state: ServiceState, accept: ServiceControlAccept, exit_code: ServiceExitCode) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code,
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    }
}

fn serve_until_stopped() -> Result<(), windows_service::Error> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = service_control_handler::register(WINDOWS_SERVICE, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            flag.store(true, Ordering::SeqCst);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    handle.set_service_status(status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        ServiceExitCode::Win32(0),
    ))?;
    // Until the data folder is verified the early lines go to the Event Log (no console, no file in the unverified folder).
    let early: crate::logfile::EarlySink = std::sync::Arc::new(crate::eventlog::report);
    let result = crate::serve::serve_with(&crate::elevate::helper_paths(), stop, Some(early));
    // A refusal is reported in full to the Event Log and by its own exit code (`sc query`: SERVICE_EXIT_CODE), so a stopped
    // service says why instead of waiting silently for Repair.
    if let Some(text) = crate::refusal::stop_event(&result) {
        crate::eventlog::report(log::Level::Error, &text);
    }
    let exit = match crate::refusal::exit_code_of(&result) {
        0 => ServiceExitCode::Win32(0),
        code => ServiceExitCode::ServiceSpecific(code),
    };
    handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty(), exit))?;
    Ok(())
}
