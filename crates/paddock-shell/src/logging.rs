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
    install_with_commit(identity, "")
}

/// [`install`] that also puts the build's commit on every line (`Commit`), so a log, a report and a VM run bind to one build.
pub fn install_with_commit(identity: &AppIdentity, commit: &str) -> Option<LogHandle> {
    let dirs = AppId::new(&identity.id, &identity.name)
        .and_then(|id| AppDirs::for_app(&id))
        .map_err(|e| eprintln!("{}: the log could not start ({e})", identity.name))
        .ok()?;
    install_in_with(identity, &dirs, true, commit)
}

/// [`install`] into `dirs` (tests: a temporary folder, never the real one), with or without the panic hook.
pub fn install_in(identity: &AppIdentity, dirs: &AppDirs, panic_hook: bool) -> Option<LogHandle> {
    install_in_with(identity, dirs, panic_hook, "")
}

fn install_in_with(
    identity: &AppIdentity,
    dirs: &AppDirs,
    panic_hook: bool,
    commit: &str,
) -> Option<LogHandle> {
    let installed = AppId::new(&identity.id, &identity.name)
        .map_err(|e| e.to_string())
        .and_then(|id| {
            let saved = paddock_diag::logging::stored(&shell_settings_path(dirs));
            let config = LogConfig::new(&id, "app", &identity.version)
                .with_dirs(dirs)
                .with_settings(saved)
                .with_panic_hook(panic_hook)
                .with_commit(commit);
            Logging::new(config).install().map_err(|e| e.to_string())
        });
    let handle = installed
        .map_err(|why| eprintln!("{}: the log could not start ({why})", identity.name))
        .ok();
    if handle.is_some() {
        // The first line of every run: which build, on what, installed how (ER-27). Why it started (login, by hand, a relaunch) and
        // the phases up to ready are the app's and the start-up contract's lines that follow.
        log::info!(
            Version = identity.version.as_str(), Commit = commit, Os = std::env::consts::OS, Arch = std::env::consts::ARCH, Pid = std::process::id(),
            Portable = dirs.source() == paddock_env::Source::Portable, BootSecs = sysinfo::System::uptime(),
            Flags = launch_flags(std::env::args().skip(1)).as_str();
            "Starting {}", identity.name
        );
    }
    handle
}

/// The names of the command-line flags (`--hidden --new-instance`), never their values or any other argument: how this run was asked to
/// start (a login item starts hidden; a person starts by hand with none).
pub fn launch_flags(args: impl Iterator<Item = String>) -> String {
    let names: Vec<String> = args
        .take_while(|a| a != "--")
        .filter(|a| a.starts_with("--"))
        .map(|a| {
            a.split('=')
                .next()
                .unwrap_or("")
                .chars()
                .take(32)
                .collect::<String>()
        })
        .take(8)
        .collect();
    if names.is_empty() {
        "none".into()
    } else {
        names.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_line_names_flags_but_never_values_or_files() {
        let args = [
            "--hidden",
            "/Users/sam/secret.txt",
            "--export-diagnostics=/Users/sam/r.zip",
            "--",
            "--after",
        ]
        .map(String::from);
        assert_eq!(
            launch_flags(args.into_iter()),
            "--hidden --export-diagnostics"
        );
        assert_eq!(launch_flags(std::iter::empty()), "none");
    }

    #[test]
    fn the_running_apps_shell_file_is_in_its_config_folder_under_its_own_name() {
        let id = AppIdentity::new("demo", "Shell Demo", "1.0.0");
        // Only paths are computed; nothing is opened or created.
        if let Some(path) = shell_settings_path_for(&id) {
            assert_eq!(
                path.file_name().and_then(|n| n.to_str()),
                Some(SHELL_SETTINGS_FILE)
            );
        }
        let bad = AppIdentity::new("", "", "1.0.0");
        assert!(
            shell_settings_path_for(&bad).is_none(),
            "an invalid identity has no folders"
        );
    }
}
