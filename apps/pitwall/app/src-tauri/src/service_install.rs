//! Registering the Pitwall server with the OS from the app. Elevation and approval are always the OS's own: UAC on
//! Windows (the installer does this once; the app can repeat it), the Login Items approval on macOS (`SMAppService`
//! with the launch agent inside the app bundle); on Linux nothing is elevated (a systemd user unit written by
//! `pitwall-server install`). Pitwall never asks for or handles a password. Adapted from Pitcrew's service_install.

use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The service's state as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstall {
    /// `windows`, `macos` or `linux`.
    pub platform: &'static str,
    /// Registered with the OS.
    pub installed: bool,
    /// The server answers.
    pub running: bool,
    /// The OS integration can be switched on from the app.
    pub can_install: bool,
    /// The user has not been asked yet: show the "run Pitwall's service?" question.
    pub ask_consent: bool,
    /// What to do next, when something is off.
    pub message: Option<String>,
}

/// The server program's file name.
pub const SERVER_EXE: &str = if cfg!(windows) { "pitwall-server.exe" } else { "pitwall-server" };

/// The server program in `dir` (Tauri puts sidecars next to the app's executable), if it is really there.
pub fn server_exe_in(dir: &Path) -> Result<PathBuf, CommandError> {
    let p = dir.join(SERVER_EXE);
    if p.metadata().is_ok_and(|m| m.len() > 0) {
        Ok(p)
    } else {
        Err(CommandError::new(
            ErrorCode::NotAvailable,
            format!(
                "{} is missing; reinstall Pitwall (development builds: build pitwall-server and copy it there)",
                p.display()
            ),
        ))
    }
}

/// The server program next to this app.
pub fn server_exe() -> Result<PathBuf, CommandError> {
    let exe =
        std::env::current_exe().map_err(|e| CommandError::new(ErrorCode::Internal, format!("the app's folder cannot be found: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| CommandError::new(ErrorCode::Internal, "the app's folder cannot be found"))?;
    server_exe_in(dir)
}

/// What the OS says about the registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsState {
    /// Registered.
    pub installed: bool,
    /// What to do next.
    pub message: Option<String>,
}

/// Puts the OS state, the server's answer and the first-run decision together.
pub fn compose(os: OsState, running: bool, ask_consent: bool) -> ServiceInstall {
    let message = os.message.or_else(|| match (os.installed, running) {
        (true, false) => {
            Some("The service is installed but the server does not answer yet; give it a moment, or restart the service.".into())
        }
        (false, true) => Some("The server is running outside the service (started by hand); it stops when you log out.".into()),
        _ => None,
    });
    ServiceInstall {
        platform: PLATFORM,
        installed: os.installed,
        running,
        can_install: true,
        ask_consent: ask_consent && !os.installed && !running,
        message,
    }
}

/// The platform name the UI sees.
pub const PLATFORM: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};

#[cfg(windows)]
mod imp {
    //! SCM queries are safe wrappers; elevation is `ShellExecuteExW` with the "runas" verb, the one unsafe call.
    #![allow(unsafe_code)]

    use super::*;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    /// The service's name in the Service Control Manager (`pitwall_server::service::spec::WINDOWS_SERVICE_NAME`).
    const SERVICE_NAME: &str = "Pitwall";

    /// Whether the service is registered.
    pub fn os_state() -> OsState {
        let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT) else {
            return OsState {
                installed: false,
                message: Some("the Service Control Manager cannot be asked".into()),
            };
        };
        match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
            Ok(s) => {
                let running = s
                    .query_status()
                    .map(|st| st.current_state == ServiceState::Running)
                    .unwrap_or(false);
                OsState {
                    installed: true,
                    message: (!running).then(|| "installed but not running; Windows restarts it, or start it from Services".to_string()),
                }
            }
            Err(_) => OsState {
                installed: false,
                message: None,
            },
        }
    }

    /// Runs `pitwall-server <verb>` with Windows' elevation prompt and waits for it (two minutes at most).
    fn elevated(verb: &str) -> Result<(), CommandError> {
        const ERROR_CANCELLED: i32 = 1223;
        let exe = server_exe()?;
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
                return Err(CommandError::new(ErrorCode::Forbidden, "Windows' permission prompt was declined"));
            }
            Err(e) => {
                return Err(CommandError::new(
                    ErrorCode::Internal,
                    format!("the server program could not be started: {e}"),
                ))
            }
        }
        if info.hProcess.is_invalid() {
            return Ok(());
        }
        // SAFETY: a valid process handle we own; waited on and closed once.
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
            Err(CommandError::new(
                ErrorCode::Internal,
                format!("pitwall-server {verb} failed (exit code {code}); the service log in %ProgramData%\\Pitwall\\logs has details"),
            ))
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
    //! `SMAppService` (macOS 13+). The plist is `Contents/Library/LaunchAgents/net.decryptology.pitwall.service.plist`
    //! in the app bundle (bundle.macOS.files in tauri.macos.conf.json), with `BundleProgram` pointing at the bundled
    //! server. The Objective-C calls are the only unsafe code; each is a plain message send on objects we own.
    #![allow(unsafe_code, unused_unsafe)]

    use super::*;
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    const PLIST: &str = "net.decryptology.pitwall.service.plist";

    fn service() -> objc2::rc::Retained<SMAppService> {
        let name = NSString::from_str(PLIST);
        // SAFETY: a class method taking a valid NSString.
        unsafe { SMAppService::agentServiceWithPlistName(&name) }
    }

    /// Whether the launch agent is registered.
    pub fn os_state() -> OsState {
        // SAFETY: a property read on a valid object.
        let status = unsafe { service().status() };
        if status == SMAppServiceStatus::Enabled {
            OsState {
                installed: true,
                message: None,
            }
        } else if status == SMAppServiceStatus::RequiresApproval {
            OsState {
                installed: true,
                message: Some("allow Pitwall in System Settings > General > Login Items".to_string()),
            }
        } else if status == SMAppServiceStatus::NotFound {
            OsState {
                installed: false,
                message: Some("run Pitwall from /Applications (the launch agent is part of the app bundle)".to_string()),
            }
        } else {
            OsState {
                installed: false,
                message: None,
            }
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
                        ErrorCode::Forbidden,
                        "macOS needs your approval: allow Pitwall in System Settings > General > Login Items (opened for you)",
                    ));
                }
                Err(CommandError::new(
                    ErrorCode::Internal,
                    format!("the launch agent could not be registered: {}", e.localizedDescription()),
                ))
            }
        }
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        // SAFETY: a method call on a valid object.
        unsafe { service().unregisterAndReturnError() }.map_err(|e| {
            CommandError::new(
                ErrorCode::Internal,
                format!("the launch agent could not be removed: {}", e.localizedDescription()),
            )
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    //! A systemd **user** service: the app runs `pitwall-server install` / `uninstall` (as the user, no elevation),
    //! which writes `~/.config/systemd/user/pitwall.service` and runs `systemctl --user`. The server program has the
    //! timeouts; this side only bounds the wait for it.
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const UNIT: &str = "pitwall.service";

    fn unit_file() -> Option<PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        env("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("systemd").join("user").join(UNIT))
    }

    /// Whether the unit file exists.
    pub fn os_state() -> OsState {
        OsState {
            installed: unit_file().is_some_and(|p| p.exists()),
            message: None,
        }
    }

    /// Runs `pitwall-server <verb>`, waiting at most 90 s; its standard error becomes the message.
    fn run_verb(verb: &str) -> Result<(), CommandError> {
        let exe = server_exe()?;
        // The one place the app starts a program: it is our own server, bounded by the wait loop below (no console
        // window exists on Linux).
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new(&exe)
            .arg(verb)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the server program could not be started: {e}")))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if started.elapsed() > Duration::from_secs(90) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandError::new(
                        ErrorCode::Internal,
                        format!("pitwall-server {verb} took too long and was stopped"),
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    return Err(CommandError::new(
                        ErrorCode::Internal,
                        format!("waiting for pitwall-server {verb} failed: {e}"),
                    ))
                }
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
        Err(CommandError::new(
            ErrorCode::Internal,
            if reason.is_empty() {
                format!("pitwall-server {verb} failed")
            } else {
                reason.to_string()
            },
        ))
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

pub use imp::{install, os_state, uninstall};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_program_must_exist_and_not_be_an_empty_placeholder() {
        let d = tempfile::tempdir().unwrap();
        let e = server_exe_in(d.path()).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotAvailable);
        assert!(e.message.contains("reinstall Pitwall"));
        std::fs::write(d.path().join(SERVER_EXE), b"").unwrap();
        assert!(server_exe_in(d.path()).is_err(), "the build placeholder is empty");
        std::fs::write(d.path().join(SERVER_EXE), b"binary").unwrap();
        assert_eq!(server_exe_in(d.path()).unwrap(), d.path().join(SERVER_EXE));
    }

    #[test]
    fn the_state_tells_the_user_what_is_off() {
        let none = OsState {
            installed: false,
            message: None,
        };
        let yes = OsState {
            installed: true,
            message: None,
        };
        // Fresh machine: ask.
        let s = compose(none.clone(), false, true);
        assert!(s.ask_consent && !s.installed && !s.running && s.message.is_none());
        // The question is asked once.
        assert!(!compose(none.clone(), false, false).ask_consent);
        // Never asked when something already runs the server.
        assert!(!compose(none.clone(), true, true).ask_consent);
        assert!(!compose(yes.clone(), false, true).ask_consent);
        // Hints.
        assert!(compose(yes.clone(), false, false).message.unwrap().contains("restart the service"));
        assert!(compose(none, true, false).message.unwrap().contains("started by hand"));
        assert!(compose(yes.clone(), true, false).message.is_none());
        // The OS's own message wins.
        let os = OsState {
            installed: true,
            message: Some("allow it".into()),
        };
        assert_eq!(compose(os, false, false).message.as_deref(), Some("allow it"));
        assert!(["windows", "macos", "linux"].contains(&PLATFORM));
    }
}
