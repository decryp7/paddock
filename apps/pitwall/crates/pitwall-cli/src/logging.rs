//! The CLI's own log (ER-27, plan B4a): each run leaves a start and an end line (command name, exit code, duration) in
//! `pitwall-cli.<day>.log` in the app's log folder, in the family's format, so a bug report that starts with "the command did
//! nothing" has something to read. Never the arguments, a prompt, a key or a path: only the command's name.

use crate::args::Command;
use paddock_env::{AppDirs, AppId};
use paddock_log::{LogConfig, LogHandle, Logging};
use std::path::{Path, PathBuf};

const APP_ID: &str = "pitwall";
const APP_NAME: &str = "Pitwall";
const PROCESS: &str = "cli";

/// The command's name without anything it carries: `Chat { .. }` and `Models` read as `Chat` and `Models`.
pub fn command_name(command: &Command) -> String {
    format!("{command:?}").chars().take_while(|c| c.is_alphanumeric()).collect()
}

/// The log folder: the app's ([`AppDirs`], what Report a problem reads), or `<PITWALL_HOME>/logs` when the data folder was moved.
pub fn log_dir(moved: Option<PathBuf>, app_dirs: Option<&AppDirs>) -> Option<PathBuf> {
    match moved {
        Some(home) => Some(home.join("logs")),
        None => app_dirs.map(|a| a.logs().to_path_buf()),
    }
}

/// Installs the CLI's log; `None` when the folders are unknown or a log is installed already (the CLI works without one).
pub fn install() -> Option<LogHandle> {
    let moved = std::env::var_os("PITWALL_HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let app = AppId::new(APP_ID, APP_NAME).ok()?;
    let dir = log_dir(moved, AppDirs::for_app(&app).ok().as_ref())?;
    install_in(&dir)
}

/// [`install`] into an explicit folder (tests: a temporary one).
pub fn install_in(dir: &Path) -> Option<LogHandle> {
    let app = AppId::new(APP_ID, APP_NAME).ok()?;
    let config = LogConfig::new(&app, PROCESS, env!("CARGO_PKG_VERSION"))
        .with_log_dir(dir)
        .with_stdout(false);
    Logging::new(config).install().ok()
}

/// Logs the start of a run.
pub fn started(command: &Command) {
    log::info!(target: "pitwall_cli", "run started: command {}", command_name(command));
}

/// Logs the end of a run: the exit code and how long it took.
pub fn ended(command: &Command, exit_code: i32, ms: u128) {
    if exit_code == 0 {
        log::info!(target: "pitwall_cli", "run ended: command {}, exit {exit_code}, {ms} ms", command_name(command));
    } else {
        log::warn!(target: "pitwall_cli", "run failed: command {}, exit {exit_code}, {ms} ms", command_name(command));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_leaves_start_and_end_lines_with_the_command_name_only() {
        let dir = tempfile::tempdir().unwrap();
        let handle = install_in(dir.path()).expect("the log installs");
        started(&Command::Models);
        ended(&Command::Models, 3, 12);
        let file = handle.active_file().expect("a file");
        assert!(file.file_name().unwrap().to_string_lossy().starts_with("pitwall-cli."), "{file:?}");
        let text = std::fs::read_to_string(file).unwrap();
        assert!(text.contains("run started: command Models"), "{text}");
        assert!(text.contains("run failed: command Models, exit 3, 12 ms"), "{text}");
    }

    #[test]
    fn the_name_drops_what_the_command_carries_and_the_folder_follows_a_moved_home() {
        assert_eq!(command_name(&Command::Models), "Models");
        let chat = Command::Chat {
            model: "m".into(),
            prompt: Some("secret-prompt".into()),
            system: Some("secret-system".into()),
            max_tokens: None,
        };
        assert_eq!(command_name(&chat), "Chat");
        assert_eq!(log_dir(Some(PathBuf::from("/h")), None), Some(PathBuf::from("/h/logs")));
        assert_eq!(log_dir(None, None), None);
    }
}
