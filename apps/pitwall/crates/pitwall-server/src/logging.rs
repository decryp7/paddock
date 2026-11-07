//! The server's own log (ER-27, plan B4a): one dated paddock-log file per process, `pitwall-server.<day>.log` in the app's log
//! folder (the folder Report a problem reads), with the family's line format, redaction, repeat dedupe, the panic hook and a
//! crash marker. The server runs as a service nobody watches: this file is what a person has. Names, codes, counts and
//! millisecond times only; never the text of a chat, a prompt or a path of the user's files.

use crate::paths::Dirs;
use paddock_env::{AppDirs, AppId};
use paddock_log::{CrashMarker, LogConfig, LogHandle, Logging};
use std::path::{Path, PathBuf};

/// The app's id (its folder and log names); the app crate uses the same.
pub const APP_ID: &str = "pitwall";
/// The app's display name.
pub const APP_NAME: &str = "Pitwall";
/// The process name in log file names and crash markers.
pub const PROCESS: &str = "server";

/// Where the log and the crash marker go: the app's folders ([`AppDirs`], what Report a problem reads), or the data folder
/// itself when it was moved (`--data-dir`, `PITWALL_HOME`: tests and tools stay in their folder).
pub fn log_places(dirs: &Dirs, data_dir_moved: bool, app_dirs: Option<&AppDirs>) -> (PathBuf, PathBuf) {
    match app_dirs.filter(|_| !data_dir_moved) {
        Some(a) => (a.logs().to_path_buf(), CrashMarker::path_for(a.data(), PROCESS)),
        None => (dirs.logs(), CrashMarker::path_for(&dirs.root, PROCESS)),
    }
}

/// Installs the server's log as the first thing `pitwall-server` does after reading its options. `None` (said on standard
/// error) when a log is installed already or the folders are unknown: the server then runs without a file log.
pub fn install(dirs: &Dirs, data_dir_moved: bool) -> Option<LogHandle> {
    let app_dirs = AppId::new(APP_ID, APP_NAME).ok().and_then(|app| AppDirs::for_app(&app).ok());
    let (log_dir, marker) = log_places(dirs, data_dir_moved, app_dirs.as_ref());
    install_in(&log_dir, &marker)
}

/// [`install`] into explicit folders (tests: temporary ones, never the real log folder).
pub fn install_in(log_dir: &Path, crash_marker: &Path) -> Option<LogHandle> {
    let app = AppId::new(APP_ID, APP_NAME).ok()?;
    let config = LogConfig::new(&app, PROCESS, env!("CARGO_PKG_VERSION"))
        .with_log_dir(log_dir)
        .with_crash_marker(crash_marker)
        .with_stdout(false);
    match Logging::new(config).install() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("pitwall-server: the log could not start ({e})");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_goes_to_the_app_folders_or_stays_with_a_moved_data_folder() {
        let dirs = Dirs::at("/data/pitwall");
        let (log, marker) = log_places(&dirs, true, None);
        assert_eq!(log, PathBuf::from("/data/pitwall/logs"));
        assert!(marker.starts_with("/data/pitwall"), "{marker:?}");
        let app = AppId::new(APP_ID, APP_NAME).unwrap();
        if let Ok(app_dirs) = AppDirs::for_app(&app) {
            assert_eq!(log_places(&dirs, false, Some(&app_dirs)).0, app_dirs.logs());
            assert_eq!(log_places(&dirs, true, Some(&app_dirs)).0, PathBuf::from("/data/pitwall/logs"));
        }
    }
}
