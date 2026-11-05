//! Native glue of the updater's adapter: what only a real desktop app does (quit, start the app again, hand the package to
//! `tauri-plugin-updater`). Each function makes one platform call or hands over to the plugin; reviewed by hand and left
//! out of the coverage job (crates/README.md).

use ::tauri::{AppHandle, Manager, Runtime};

/// Quits the app to update; `RunEvent::Exit` then runs `on_exit`, which installs.
pub(super) fn quit<R: Runtime>(app: &AppHandle<R>) {
    log::info!("Restarting to update");
    app.exit(0);
}

/// Starts this app's binary again when `action` asks for it (after an install, or after a failed one so the user sees
/// what happened).
pub(super) fn relaunch_after<R: Runtime>(app: &AppHandle<R>, action: crate::ExitAction) {
    if !matches!(
        action,
        crate::ExitAction::Installed { relaunch: true }
            | crate::ExitAction::Failed { relaunch: true }
    ) {
        return;
    }
    match ::tauri::process::current_binary(&app.env()) {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).spawn() {
                log::error!("Could not start the app again after the update: {e}");
            }
        }
        Err(e) => log::error!("Could not start the app again after the update: {e}"),
    }
}

#[cfg(feature = "install")]
pub use plugin::PluginInstaller;

#[cfg(all(windows, feature = "install"))]
pub use elevate::{ShellElevator, is_administrator};

/// The Windows glue of the per-machine hand-off: one platform call each, no decisions (they are in `install.rs`).
#[cfg(all(windows, feature = "install"))]
mod elevate {
    use crate::install::{ElevatedOutcome, Elevator, quote_arg};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_CANCELLED, GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::{
        CheckTokenMembership, CreateWellKnownSid, GetTokenInformation, TOKEN_QUERY,
        TokenElevationType, WinBuiltinAdministratorsSid,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain([0]).collect()
    }

    /// `ShellExecuteExW` with the verb `runas`: the UAC prompt names the program. A declined prompt is error 1223.
    /// The window is started hidden (`SW_HIDE`) and only the exit code is read, never the program's output, so the hand-off works
    /// the same with a console or a GUI-subsystem `paddock-setup.exe` (branch `fix/paddock-setup-no-console`) and shows no window.
    pub struct ShellElevator;

    impl Elevator for ShellElevator {
        fn run(&self, program: &Path, args: &[String], patience: Duration) -> ElevatedOutcome {
            let file = wide(&program.to_string_lossy());
            let verb = wide("runas");
            let params = wide(
                &args
                    .iter()
                    .map(|a| quote_arg(a))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            // SAFETY: a zeroed SHELLEXECUTEINFOW is valid (null pointers, zero masks); the fields set below are the ones the
            // call reads, and the wide strings outlive it.
            let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.lpParameters = params.as_ptr();
            info.nShow = SW_HIDE;
            // SAFETY: `info` is initialised above and lives through the call.
            if unsafe { ShellExecuteExW(&mut info) } == 0 {
                // SAFETY: reads the calling thread's last error.
                let code = unsafe { GetLastError() };
                return if code == ERROR_CANCELLED {
                    ElevatedOutcome::Declined
                } else {
                    ElevatedOutcome::Failed(format!("error {code}"))
                };
            }
            let process: HANDLE = info.hProcess;
            if process.is_null() {
                return ElevatedOutcome::Failed("no process handle".into());
            }
            let ms = u32::try_from(patience.as_millis()).unwrap_or(u32::MAX - 1);
            // SAFETY: `process` is the handle the call returned (NOCLOSEPROCESS), closed once below.
            let waited = unsafe { WaitForSingleObject(process, ms) };
            let outcome = match waited {
                WAIT_OBJECT_0 => {
                    let mut code = 0u32;
                    // SAFETY: as above; `code` lives through the call.
                    if unsafe { GetExitCodeProcess(process, &mut code) } == 0 {
                        ElevatedOutcome::Failed("the exit code could not be read".into())
                    } else {
                        ElevatedOutcome::Exited(code as i32)
                    }
                }
                WAIT_TIMEOUT => ElevatedOutcome::StillRunning,
                other => ElevatedOutcome::Failed(format!("wait returned {other}")),
            };
            // SAFETY: the handle is closed exactly once.
            unsafe { CloseHandle(process) };
            outcome
        }
    }

    /// Whether this account is an administrator, also with a filtered token (UAC on): a limited token has a linked full one,
    /// so `TokenElevationType` limited or full means yes; a token without a split (a standard user, the built-in
    /// Administrator, UAC off) is asked for the Administrators group.
    pub fn is_administrator() -> bool {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo handle of this process and an out parameter that lives through the call.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let mut kind = 0i32;
        let mut returned = 0u32;
        // SAFETY: `kind` is a 4-byte TOKEN_ELEVATION_TYPE and the size passed is its size.
        let got = unsafe {
            GetTokenInformation(
                token,
                TokenElevationType,
                std::ptr::from_mut(&mut kind).cast(),
                std::mem::size_of::<i32>() as u32,
                &mut returned,
            )
        } != 0;
        // SAFETY: the token was opened above.
        unsafe { CloseHandle(token) };
        // 1 = Default (no split), 2 = Full, 3 = Limited.
        if got && matches!(kind, 2 | 3) {
            return true;
        }
        let mut sid = [0u8; 68];
        let mut len = sid.len() as u32;
        // SAFETY: `sid` is SECURITY_MAX_SID_SIZE bytes and `len` says so.
        if unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                std::ptr::null_mut(),
                sid.as_mut_ptr().cast(),
                &mut len,
            )
        } == 0
        {
            return false;
        }
        let mut member = 0i32;
        // SAFETY: a null token means the calling thread's (duplicated primary) token; the SID was built above.
        let ok = unsafe {
            CheckTokenMembership(std::ptr::null_mut(), sid.as_mut_ptr().cast(), &mut member)
        } != 0;
        ok && member != 0
    }
}

#[cfg(feature = "install")]
mod plugin {
    use crate::{Installer, Package, UpdateError};
    use ::tauri::{AppHandle, Runtime};
    use tauri_plugin_updater::UpdaterExt;

    /// Installs through `tauri-plugin-updater` (registered by the app): it restores the update from the release information
    /// as received (requiring the signed version), verifies the package's signature again and installs it.
    pub struct PluginInstaller<R: Runtime> {
        app: AppHandle<R>,
        key: String,
    }

    impl<R: Runtime> PluginInstaller<R> {
        /// `key` is the same public key the updater was given.
        pub fn new(app: AppHandle<R>, key: &str) -> Self {
            PluginInstaller {
                app,
                key: key.trim().to_string(),
            }
        }
    }

    impl<R: Runtime> Installer for PluginInstaller<R> {
        fn install(&self, package: &Package<'_>, relaunch: bool) -> Result<(), UpdateError> {
            let failed = |detail: String| UpdateError::InstallFailed { detail };
            let updater = self
                .app
                .updater_builder()
                .pubkey(self.key.clone())
                .build()
                .map_err(|e| failed(e.to_string()))?;
            let update = updater
                .restore_update(package.manifest.clone())
                .map_err(|e| failed(e.to_string()))?
                .ok_or_else(|| failed("the update is no longer newer than this version".into()))?;
            let bytes = std::fs::read(package.file).map_err(|e| failed(e.to_string()))?;
            update
                .restart_after_install(relaunch)
                .install(bytes)
                .map_err(|e| match e {
                    tauri_plugin_updater::Error::Minisign(_)
                    | tauri_plugin_updater::Error::SignedVersionMismatch { .. }
                    | tauri_plugin_updater::Error::MissingSignedVersion => {
                        UpdateError::SignatureInvalid {
                            detail: e.to_string(),
                        }
                    }
                    other => failed(other.to_string()),
                })
        }
    }
}
