//! The app's log (`paddock-log`, ER-22): every app on the shell installs it first in `run`, so the `log` lines of the
//! shared crates (the start's error, a window, dialog or pop-out that failed, the start-up watchdog) reach the daily file
//! in the OS log folder, with the level from Settings ("Diagnostics > Logging"), redaction, dedupe and the panic hook.
//! Without it every one of those lines is lost (Spotter's hang at start left nothing in its log).

use crate::AppIdentity;
use paddock_env::{AppDirs, AppId};
use paddock_log::{LogConfig, LogHandle, Logging};
use std::path::PathBuf;

/// The shell's own settings file in the app's config folder. It is never `settings.json`: an app may keep its own
/// file of that name in the same folder in another format (Dyno's agent does), and the settings service moves a file
/// of the wrong shape aside as `.corrupt`.
pub const SHELL_SETTINGS_FILE: &str = "shell-settings.json";

/// The path of the shell's settings file for `dirs`: what the log reads before Tauri starts and what the settings
/// service opens afterwards, so both see the same file.
pub fn shell_settings_path(dirs: &AppDirs) -> PathBuf {
    dirs.config().join(SHELL_SETTINGS_FILE)
}

/// [`shell_settings_path`] for the running app, `None` when its folders are unknown.
pub(crate) fn shell_settings_path_for(identity: &AppIdentity) -> Option<PathBuf> {
    AppId::new(&identity.id, &identity.name)
        .and_then(|id| AppDirs::for_app(&id))
        .ok()
        .map(|dirs| shell_settings_path(&dirs))
}

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
            let saved = paddock_diag::logging::stored(&shell_settings_path(dirs));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_apps_shell_file_is_in_its_config_folder_under_its_own_name() {
        let id = AppIdentity::new("demo", "Shell Demo", "1.0.0");
        // Only paths are computed; nothing is opened or created.
        if let Some(path) = shell_settings_path_for(&id) {
            assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(SHELL_SETTINGS_FILE));
        }
        let bad = AppIdentity::new("", "", "1.0.0");
        assert!(shell_settings_path_for(&bad).is_none(), "an invalid identity has no folders");
    }
}
