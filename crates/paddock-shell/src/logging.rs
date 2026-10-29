//! The app's log (`paddock-log`, ER-22): every app on the shell installs it first in `run`, so the `log` lines of the
//! shared crates (the start's error, a window, dialog or pop-out that failed, the start-up watchdog) reach the daily file
//! in the OS log folder, with the level from Settings ("Diagnostics > Logging"), redaction, dedupe and the panic hook.
//! Without it every one of those lines is lost (Spotter's hang at start left nothing in its log).

use crate::AppIdentity;
use paddock_env::{AppDirs, AppId};
use paddock_log::{LogConfig, LogHandle, Logging};

/// Installs the app's log into its folders ([`AppDirs::for_app`]); `None` (said on standard error) when the folders are
/// unknown or a log is installed already. Call it first in `run`.
pub fn install(identity: &AppIdentity) -> Option<LogHandle> {
    let dirs = AppId::new(&identity.id, &identity.name)
        .and_then(|id| AppDirs::for_app(&id))
        .map_err(|e| eprintln!("{}: the log could not start ({e})", identity.name))
        .ok()?;
    install_in(identity, &dirs, true)
}

/// [`install`] into `dirs` (tests: a temporary folder, never the real one), with or without the panic hook.
pub fn install_in(identity: &AppIdentity, dirs: &AppDirs, panic_hook: bool) -> Option<LogHandle> {
    let installed = AppId::new(&identity.id, &identity.name)
        .map_err(|e| e.to_string())
        .and_then(|id| {
            let saved =
                paddock_diag::logging::stored(&dirs.config().join(paddock_settings::FILE_NAME));
            let config = LogConfig::new(&id, "app", &identity.version)
                .with_dirs(dirs)
                .with_settings(saved)
                .with_panic_hook(panic_hook);
            Logging::new(config).install().map_err(|e| e.to_string())
        });
    installed
        .map_err(|why| eprintln!("{}: the log could not start ({why})", identity.name))
        .ok()
}
