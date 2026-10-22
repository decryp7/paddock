//! The Windows Service Control Manager integration (ADR 0003): the service entry point, and registering and
//! removing the service. Registration needs an elevated caller: the app starts `pitcrew-service install` through
//! Windows' own elevation prompt and never handles passwords; the service runs as LocalSystem, which it needs to
//! start runners in the logged-on user's session.

// `define_windows_service!` expands to the `extern "system"` entry point the SCM calls, which converts the raw
// argument array inside an `unsafe` block of the macro. That is the only unsafe code; this crate writes none.
#![allow(unsafe_code)]

use crate::error::Error;
use pitcrew_core::service_spec::{SCM_RESET_AFTER, SCM_RESTART_DELAYS, WINDOWS_DESCRIPTION, WINDOWS_DISPLAY_NAME, WINDOWS_SERVICE_NAME};
use std::ffi::OsString;
use std::sync::mpsc;
use std::time::Duration;
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_EXISTS: i32 = 1073;

fn os_error(what: &'static str, e: windows_service::Error) -> Error {
    if let windows_service::Error::Winapi(io) = &e {
        if io.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
            return Error::NeedsElevation(what);
        }
    }
    Error::Os {
        what,
        reason: e.to_string(),
    }
}

fn raw_code(e: &windows_service::Error) -> Option<i32> {
    match e {
        windows_service::Error::Winapi(io) => io.raw_os_error(),
        _ => None,
    }
}

define_windows_service!(ffi_service_main, service_main);

/// Hands the process to the SCM (the `service` command). Returns when the service has stopped.
pub fn dispatch() -> Result<(), Error> {
    service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main).map_err(|e| os_error("Starting as a Windows service", e))
}

fn service_main(_arguments: Vec<OsString>) {
    // Nobody sees a service's standard error; problems go to the service log (<data>/logs/service.log).
    let _ = run_service();
}

fn status(state: ServiceState) -> ServiceStatus {
    let controls = if state == ServiceState::Running {
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN | ServiceControlAccept::SESSION_CHANGE
    } else {
        ServiceControlAccept::empty()
    };
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: controls,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(if state == ServiceState::StopPending { 30 } else { 0 }),
        process_id: None,
    }
}

fn run_service() -> Result<(), Error> {
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let stop_tx = event_tx.clone();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = event_tx.send(Event::Stop);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::SessionChange(_) => {
            let _ = event_tx.send(Event::Session);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let handle =
        service_control_handler::register(WINDOWS_SERVICE_NAME, handler).map_err(|e| os_error("Registering the control handler", e))?;
    let data_dir = crate::paths::data_dir().ok_or(Error::Os {
        what: "Finding the data folder",
        reason: "ProgramData is not set".into(),
    })?;
    let host = crate::host::Host::start(data_dir, move |_| {
        let _ = stop_tx.send(Event::Stop);
    });
    let host = match host {
        Ok(h) => h,
        Err(e) => {
            let _ = handle.set_service_status(ServiceStatus {
                exit_code: ServiceExitCode::Win32(1),
                ..status(ServiceState::Stopped)
            });
            return Err(e);
        }
    };
    handle
        .set_service_status(status(ServiceState::Running))
        .map_err(|e| os_error("Reporting the service status", e))?;
    while let Ok(Event::Session) = event_rx.recv() {
        host.send(crate::supervisor::Msg::LogonChanged);
    }
    handle
        .set_service_status(status(ServiceState::StopPending))
        .map_err(|e| os_error("Reporting the service status", e))?;
    host.stop();
    handle
        .set_service_status(status(ServiceState::Stopped))
        .map_err(|e| os_error("Reporting the service status", e))?;
    Ok(())
}

enum Event {
    Stop,
    Session,
}

/// Registers the service (automatic delayed start, LocalSystem, restart on failure) and starts it.
pub fn install() -> Result<(), Error> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
        .map_err(|e| os_error("Installing the service", e))?;
    let exe = std::env::current_exe().map_err(|source| Error::Io {
        path: "the service executable".into(),
        source,
    })?;
    let info = ServiceInfo {
        name: OsString::from(WINDOWS_SERVICE_NAME),
        display_name: OsString::from(WINDOWS_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec![OsString::from("service")],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let service = match manager.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START) {
        Ok(s) => s,
        Err(e) if raw_code(&e) == Some(ERROR_SERVICE_EXISTS) => {
            return Err(Error::Os {
                what: "Installing the service",
                reason: "it is installed already; uninstall it first".into(),
            })
        }
        Err(e) => return Err(os_error("Installing the service", e)),
    };
    service
        .set_description(WINDOWS_DESCRIPTION)
        .map_err(|e| os_error("Describing the service", e))?;
    service
        .set_delayed_auto_start(true)
        .map_err(|e| os_error("Setting delayed start", e))?;
    let actions = SCM_RESTART_DELAYS
        .iter()
        .map(|d| ServiceAction {
            action_type: ServiceActionType::Restart,
            delay: *d,
        })
        .collect();
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(SCM_RESET_AFTER),
            reboot_msg: None,
            command: None,
            actions: Some(actions),
        })
        .map_err(|e| os_error("Setting the recovery actions", e))?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(|e| os_error("Setting the recovery actions", e))?;
    service
        .start::<&std::ffi::OsStr>(&[])
        .map_err(|e| os_error("Starting the service", e))
}

/// Stops and removes the service. Removing a service that is not installed succeeds.
pub fn uninstall() -> Result<(), Error> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(|e| os_error("Removing the service", e))?;
    let service = match manager.open_service(
        WINDOWS_SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    ) {
        Ok(s) => s,
        Err(e) if raw_code(&e) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => return Ok(()),
        Err(e) => return Err(os_error("Removing the service", e)),
    };
    let state = service
        .query_status()
        .map_err(|e| os_error("Removing the service", e))?
        .current_state;
    if state != ServiceState::Stopped {
        service.stop().map_err(|e| os_error("Stopping the service", e))?;
        // Wait (a minute at most) so an installer can replace the binary afterwards; runners are asked to stop and
        // finish a running job, and a busy one is adopted by the next service.
        for _ in 0..120 {
            match service.query_status() {
                Ok(s) if s.current_state == ServiceState::Stopped => break,
                Ok(_) => std::thread::sleep(Duration::from_millis(500)),
                Err(_) => break,
            }
        }
    }
    service.delete().map_err(|e| os_error("Removing the service", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_denied_asks_for_elevation() {
        let denied = windows_service::Error::Winapi(std::io::Error::from_raw_os_error(ERROR_ACCESS_DENIED));
        assert!(matches!(os_error("Installing the service", denied), Error::NeedsElevation(_)));
        let other = windows_service::Error::Winapi(std::io::Error::from_raw_os_error(ERROR_SERVICE_EXISTS));
        assert_eq!(raw_code(&other), Some(ERROR_SERVICE_EXISTS));
        assert!(matches!(os_error("x", other), Error::Os { .. }));
    }

    #[test]
    fn status_reports_accept_stop_only_while_running() {
        assert!(status(ServiceState::Running).controls_accepted.contains(ServiceControlAccept::STOP));
        assert!(status(ServiceState::Stopped).controls_accepted.is_empty());
        assert!(status(ServiceState::StopPending).wait_hint > Duration::ZERO);
    }
}
