//! Registering the Pitcrew service with the OS from the app (ADR 0003). Elevation and approval are always the OS's
//! own: UAC on Windows (the installer does this once; the app can repeat it), the Login Items approval on macOS;
//! on Linux nothing is elevated (a systemd user service).
//! Pitcrew never asks for or handles a password.

use crate::dto::{CommandError, CommandErrorCode, ServiceInstall};

/// The installed service binary next to the app's executable (Tauri puts sidecars there).
pub fn service_exe() -> Result<std::path::PathBuf, CommandError> {
    let exe = std::env::current_exe().map_err(|e| CommandError::internal(format!("the app's folder cannot be found: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| CommandError::internal("the app's folder cannot be found"))?;
    let name = if cfg!(windows) { "pitcrew-service.exe" } else { "pitcrew-service" };
    let p = dir.join(name);
    if p.metadata().is_ok_and(|m| m.len() > 0) {
        Ok(p)
    } else {
        Err(CommandError::new(
            CommandErrorCode::NotImplemented,
            format!(
                "{} is missing; reinstall Pitcrew (development builds: build pitcrew-service and copy it there)",
                p.display()
            ),
        ))
    }
}

#[cfg(windows)]
mod imp {
    //! SCM queries are safe wrappers; elevation is `ShellExecuteExW` with the "runas" verb, the one unsafe call.
    #![allow(unsafe_code)]

    use super::*;
    use pitcrew_core::service_spec::WINDOWS_SERVICE_NAME;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    /// Whether the service is registered and running.
    pub fn state() -> ServiceInstall {
        let base = ServiceInstall {
            platform: "windows",
            installed: false,
            running: false,
            can_install: true,
            message: None,
        };
        let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT) else {
            return ServiceInstall {
                message: Some("the Service Control Manager cannot be asked".into()),
                ..base
            };
        };
        match manager.open_service(WINDOWS_SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
            Ok(s) => {
                let running = s
                    .query_status()
                    .map(|st| st.current_state == ServiceState::Running)
                    .unwrap_or(false);
                ServiceInstall {
                    installed: true,
                    running,
                    message: (!running).then(|| "installed but not running; Windows restarts it, or start it from Services".to_string()),
                    ..base
                }
            }
            Err(_) => base,
        }
    }

    /// Runs `pitcrew-service <verb>` with Windows' elevation prompt and waits for it.
    pub fn elevated(verb: &str) -> Result<(), CommandError> {
        const ERROR_CANCELLED: i32 = 1223;
        let exe = service_exe()?;
        let op = HSTRING::from("runas");
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(verb);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: windows::core::PCWSTR(op.as_ptr()),
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: every pointer in `info` refers to an HSTRING that outlives the call.
        match unsafe { ShellExecuteExW(&mut info) } {
            Ok(()) => {}
            Err(e) if e.code().0 & 0xFFFF == ERROR_CANCELLED => {
                return Err(CommandError::new(
                    CommandErrorCode::Cancelled,
                    "Windows' permission prompt was declined",
                ))
            }
            Err(e) => return Err(CommandError::internal(format!("the service program could not be started: {e}"))),
        }
        if info.hProcess.is_invalid() {
            return Ok(());
        }
        // SAFETY: a valid process handle we own; waited on (2 minutes at most) and closed once.
        let code = unsafe {
            let waited = WaitForSingleObject(info.hProcess, 120_000);
            let mut code = 1u32;
            if waited == WAIT_OBJECT_0 {
                let _ = GetExitCodeProcess(info.hProcess, &mut code);
            }
            let _ = CloseHandle(info.hProcess);
            code
        };
        if code == 0 {
            Ok(())
        } else {
            Err(CommandError::internal(format!(
                "pitcrew-service {verb} failed (exit code {code}); the service log has details"
            )))
        }
    }

    /// Registers and starts the service (the OS asks the user).
    pub fn install() -> Result<(), CommandError> {
        elevated("install")
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        elevated("uninstall")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    //! `SMAppService` (macOS 13+). The plist is `Contents/Library/LaunchAgents/net.decryptology.pitcrew.service.plist`
    //! in the app bundle (bundle.macOS.files in tauri.macos.conf.json), with `BundleProgram` pointing at the bundled
    //! service. The Objective-C calls are the only unsafe code; each is a plain message send on objects we own.
    #![allow(unsafe_code, unused_unsafe)]

    use super::*;
    use crate::service_client::ServiceClient;
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    const PLIST: &str = "net.decryptology.pitcrew.service.plist";

    fn service() -> objc2::rc::Retained<SMAppService> {
        let name = NSString::from_str(PLIST);
        // SAFETY: a class method taking a valid NSString.
        unsafe { SMAppService::agentServiceWithPlistName(&name) }
    }

    /// Whether the service is registered and running.
    pub fn state() -> ServiceInstall {
        // SAFETY: a property read on a valid object.
        let status = unsafe { service().status() };
        let running = crate::service_client::LocalServiceClient
            .request(pitcrew_core::protocol::Request::Status)
            .is_ok();
        let (installed, message) = if status == SMAppServiceStatus::Enabled {
            (true, None)
        } else if status == SMAppServiceStatus::RequiresApproval {
            (true, Some("allow Pitcrew in System Settings ▸ General ▸ Login Items".to_string()))
        } else if status == SMAppServiceStatus::NotFound {
            (
                false,
                Some("run Pitcrew from /Applications (the launch agent is part of the app bundle)".to_string()),
            )
        } else {
            (false, None)
        };
        ServiceInstall {
            platform: "macos",
            installed,
            running,
            can_install: true,
            message,
        }
    }

    /// Registers and starts the service (the OS asks the user).
    pub fn install() -> Result<(), CommandError> {
        let s = service();
        // SAFETY: a method call on a valid object; the error is returned as a Retained NSError.
        match unsafe { s.registerAndReturnError() } {
            Ok(()) => Ok(()),
            Err(e) => {
                // SAFETY: as above.
                if unsafe { s.status() } == SMAppServiceStatus::RequiresApproval {
                    // SAFETY: a class method without arguments.
                    unsafe { SMAppService::openSystemSettingsLoginItems() };
                    return Err(CommandError::new(
                        CommandErrorCode::PermissionDenied,
                        "macOS needs your approval: allow Pitcrew in System Settings ▸ General ▸ Login Items (opened for you)",
                    ));
                }
                Err(CommandError::internal(format!(
                    "the launch agent could not be registered: {}",
                    e.localizedDescription()
                )))
            }
        }
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        // SAFETY: a method call on a valid object.
        unsafe { service().unregisterAndReturnError() }
            .map_err(|e| CommandError::internal(format!("the launch agent could not be removed: {}", e.localizedDescription())))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    //! A systemd **user** service (ADR 0009): the app runs `pitcrew-service install` / `uninstall` (as the user, no
    //! elevation), which writes `~/.config/systemd/user/pitcrew.service` and runs `systemctl --user`. The service
    //! program has the timeouts; this side only bounds the wait for it.
    use super::*;
    use crate::service_client::ServiceClient;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const UNIT: &str = pitcrew_core::service_spec::SYSTEMD_UNIT;

    fn unit_file() -> Option<std::path::PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
        env("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("systemd").join("user").join(UNIT))
    }

    /// Whether the service is registered and running.
    pub fn state() -> ServiceInstall {
        let installed = unit_file().is_some_and(|p| p.exists());
        let running = crate::service_client::LocalServiceClient
            .request(pitcrew_core::protocol::Request::Status)
            .is_ok();
        let message = match (installed, running) {
            (true, false) => Some(format!(
                "installed but not running: run `systemctl --user status {UNIT}` for the reason"
            )),
            (false, true) => Some("running outside systemd (started by hand)".to_string()),
            _ => None,
        };
        ServiceInstall {
            platform: "linux",
            installed,
            running,
            can_install: true,
            message,
        }
    }

    /// Runs `pitcrew-service <verb>`, waiting at most 90 s; its standard error becomes the message.
    fn run_verb(verb: &str) -> Result<(), CommandError> {
        let exe = service_exe()?;
        // The one place the app starts a program: it is our own service, bounded by the wait loop below (no console
        // window exists on Linux).
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new(&exe)
            .arg(verb)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CommandError::internal(format!("the service program could not be started: {e}")))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if started.elapsed() > Duration::from_secs(90) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandError::internal(format!(
                        "pitcrew-service {verb} took too long and was stopped"
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(CommandError::internal(format!("waiting for pitcrew-service {verb} failed: {e}"))),
            }
        };
        if status.success() {
            return Ok(());
        }
        let mut message = String::new();
        if let Some(mut err) = child.stderr.take() {
            use std::io::Read;
            let _ = err.by_ref().take(2048).read_to_string(&mut message);
        }
        let reason = message.trim();
        Err(CommandError::internal(if reason.is_empty() {
            format!("pitcrew-service {verb} failed")
        } else {
            reason.to_string()
        }))
    }

    /// Registers and starts the service (as the user).
    pub fn install() -> Result<(), CommandError> {
        run_verb("install")
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        run_verb("uninstall")
    }
}

pub use imp::{install, state, uninstall};
