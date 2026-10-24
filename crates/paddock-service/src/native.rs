//! Native glue (crates/README.md): code only a real desktop runs, which tests cannot reach by rule 7 (they never touch
//! the real Login Items or elevation prompt). Each function makes one platform call or builds the real adapters and
//! hands over to tested code; it is reviewed by hand and left out of the coverage job.
//!
//! - `SmAppService`: `SMAppService` (macOS 13+) for one plist in the app bundle (macOS builds only).
//! - `RunAsElevator`: Windows' own elevation prompt (`ShellExecuteExW` with `runas`; Windows builds only).
//! - [`current_uid`] and [`host_backend`]: the user, and the backend for this OS from [`crate::backend::choose`].
#![allow(unsafe_code)]

use crate::backend::{Backend, Choice, Side, Target, choose};
use crate::error::ServiceError;
use crate::spec::ServiceSpec;
use crate::system::RealSystem;
use std::path::Path;

/// The user id (`gui/<uid>` is their launchd domain); 0 on Windows.
pub fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// The backend for `spec` on this computer: `program` is the background program (next to the app, or this process
/// on the program's side), `data_dir` the app's data folder (an AppImage's program is copied there first).
pub fn host_backend(
    spec: &ServiceSpec,
    program: &Path,
    side: Side,
    data_dir: &Path,
) -> Result<Box<dyn Backend>, ServiceError> {
    let vars = paddock_env::ProcessVars;
    let appimage = std::env::var("APPIMAGE").ok();
    let sys = || Box::new(RealSystem::new());
    match choose(
        paddock_env::Os::current(),
        spec,
        program,
        side,
        appimage.as_deref(),
        &vars,
    )? {
        #[cfg(target_os = "macos")]
        Choice::Bundled => Ok(Box::new(crate::backend::LaunchdBundled::new(
            spec.clone(),
            current_uid(),
            Box::new(mac::SmAppService::new(spec)),
            sys(),
        ))),
        Choice::LaunchdFile { plist, target } => Ok(Box::new(crate::backend::LaunchdFile::new(
            spec.clone(),
            current_uid(),
            plist,
            target,
            sys(),
        ))),
        Choice::Systemd {
            unit,
            target,
            copy_from,
        } => {
            let target = match copy_from {
                Some(src) => Target {
                    exe: crate::system::stable_copy(&src, data_dir).map_err(|source| {
                        ServiceError::Io {
                            op: "copy the program to",
                            path: data_dir.join("bin"),
                            source,
                        }
                    })?,
                    ..target
                },
                None => target,
            };
            Ok(Box::new(crate::backend::Systemd::new(
                spec.clone(),
                unit,
                target,
                sys(),
            )))
        }
        Choice::WindowsRun { target } => Ok(Box::new(crate::backend::WindowsRun::new(
            spec.clone(),
            target,
            sys(),
        ))),
        Choice::WindowsService {
            elevated: false,
            target,
        } => Ok(Box::new(crate::backend::WindowsService::new(
            spec.clone(),
            target,
            sys(),
        ))),
        #[cfg(windows)]
        Choice::WindowsService {
            elevated: true,
            target,
        } => Ok(Box::new(crate::backend::ElevatedWindowsService::new(
            spec.clone(),
            target.exe,
            sys(),
            Box::new(win::RunAsElevator),
        ))),
        #[allow(unreachable_patterns)]
        _ => Err(ServiceError::Unavailable(
            "This kind of background service is not available on this system.".to_string(),
        )),
    }
}

#[cfg(target_os = "macos")]
pub use mac::SmAppService;

#[cfg(target_os = "macos")]
mod mac {
    use crate::port::{AppService, AppServiceStatus, RegisterError};
    use crate::spec::{Kind, ServiceSpec};
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService as Sm, SMAppServiceStatus};

    /// `SMAppService` for the plist `Contents/Library/Launch{Agents,Daemons}/<label>.plist` in the app bundle.
    pub struct SmAppService {
        plist: String,
        daemon: bool,
    }

    impl SmAppService {
        /// The bundled plist of `spec`.
        pub fn new(spec: &ServiceSpec) -> Self {
            Self {
                plist: format!("{}.plist", spec.label()),
                daemon: spec.kind() == Kind::SystemService,
            }
        }

        fn service(&self) -> Retained<Sm> {
            let name = NSString::from_str(&self.plist);
            // SAFETY: class methods taking a valid NSString.
            unsafe {
                if self.daemon {
                    Sm::daemonServiceWithPlistName(&name)
                } else {
                    Sm::agentServiceWithPlistName(&name)
                }
            }
        }
    }

    impl AppService for SmAppService {
        fn status(&mut self) -> AppServiceStatus {
            // SAFETY: a property read on a valid object.
            let s = unsafe { self.service().status() };
            if s == SMAppServiceStatus::Enabled {
                AppServiceStatus::Enabled
            } else if s == SMAppServiceStatus::RequiresApproval {
                AppServiceStatus::RequiresApproval
            } else if s == SMAppServiceStatus::NotFound {
                AppServiceStatus::NotFound
            } else {
                AppServiceStatus::NotRegistered
            }
        }

        fn register(&mut self) -> Result<(), RegisterError> {
            let s = self.service();
            // SAFETY: a method call on a valid object; the error comes back as a retained NSError.
            match unsafe { s.registerAndReturnError() } {
                Ok(()) => Ok(()),
                Err(e) => match self.status() {
                    AppServiceStatus::RequiresApproval => Err(RegisterError::NeedsApproval),
                    AppServiceStatus::NotFound => Err(RegisterError::Unavailable(
                        "macOS cannot find the launch agent in the app: open the app from the Applications folder."
                            .to_string(),
                    )),
                    _ => Err(RegisterError::Other(e.localizedDescription().to_string())),
                },
            }
        }

        fn unregister(&mut self) -> Result<(), String> {
            // SAFETY: a method call on a valid object.
            unsafe { self.service().unregisterAndReturnError() }
                .map_err(|e| e.localizedDescription().to_string())
        }

        fn open_login_items(&mut self) {
            // SAFETY: a class method without arguments.
            unsafe { Sm::openSystemSettingsLoginItems() };
        }
    }
}

#[cfg(windows)]
pub use win::RunAsElevator;

#[cfg(windows)]
mod win {
    use crate::port::{Elevator, RegisterError};
    use std::path::Path;
    use std::time::Duration;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, PCWSTR};

    /// Windows' elevation prompt (UAC). The app never sees a password.
    pub struct RunAsElevator;

    impl Elevator for RunAsElevator {
        fn run_elevated(
            &mut self,
            program: &Path,
            args: &[&str],
            timeout: Duration,
        ) -> Result<i32, RegisterError> {
            const ERROR_CANCELLED: i32 = 1223;
            let verb = HSTRING::from("runas");
            let file = HSTRING::from(program.as_os_str());
            let params = HSTRING::from(
                args.iter()
                    .map(|a| crate::render::windows_quote(a))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            let mut info = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
                lpVerb: PCWSTR(verb.as_ptr()),
                lpFile: PCWSTR(file.as_ptr()),
                lpParameters: PCWSTR(params.as_ptr()),
                nShow: SW_HIDE.0,
                ..Default::default()
            };
            // SAFETY: every pointer in `info` refers to an HSTRING that outlives the call.
            if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
                return Err(if e.code().0 & 0xFFFF == ERROR_CANCELLED {
                    RegisterError::Declined
                } else {
                    RegisterError::Other(format!("{} could not be started: {e}", program.display()))
                });
            }
            if info.hProcess.is_invalid() {
                return Ok(0);
            }
            let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
            // SAFETY: a valid process handle we own; waited on and closed once.
            let code = unsafe {
                let waited = WaitForSingleObject(info.hProcess, ms);
                let mut code = 1u32;
                if waited == WAIT_OBJECT_0 {
                    let _ = GetExitCodeProcess(info.hProcess, &mut code);
                }
                let _ = CloseHandle(info.hProcess);
                code
            };
            Ok(i32::try_from(code).unwrap_or(1))
        }
    }
}
