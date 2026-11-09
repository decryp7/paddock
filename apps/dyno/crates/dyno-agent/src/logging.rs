//! The agent's own log: one dated paddock-log file per process (`dyno-agent.<day>.log` in the app's log folder, which the
//! Report-a-problem bundle reads by the `dyno` prefix), with the panic hook and a crash marker the diagnostics section reads.
//! A process started by launchd has no stderr anyone reads; this file is what a person has. Names, codes and millisecond counts
//! only (the logging rules): no paths of the user's files, no content.

use crate::paths::Dirs;
use paddock_env::{AppDirs, AppId};
use paddock_log::{CrashMarker, LogConfig, LogHandle, Logging};
use std::path::PathBuf;

/// The app's id (its folder and log names); the app crate takes it from here, so the two never differ.
pub const APP_ID: &str = "dyno";
/// The app's display name.
pub const APP_NAME: &str = "Dyno";
/// The process name of the agent in log file names and crash markers.
pub const PROCESS: &str = "agent";

/// Where the agent's log and crash marker go: the app's folders ([`AppDirs`]: the folder the bundle reads), or the data folder
/// itself when `DYNO_DATA_DIR` moved it (tests and tools stay in their folder).
pub fn log_places(dirs: &Dirs, data_dir_overridden: bool, app_dirs: Option<&AppDirs>) -> (PathBuf, PathBuf) {
    match app_dirs.filter(|_| !data_dir_overridden) {
        Some(a) => (a.logs().to_path_buf(), CrashMarker::path_for(a.data(), PROCESS)),
        None => (dirs.logs(), CrashMarker::path_for(dirs.root(), PROCESS)),
    }
}

/// The folder the agent's and the app's dated logs are in: the app's log folder, or the data folder's own `logs` when
/// `DYNO_DATA_DIR` moved it (the same rule the install uses).
pub fn log_dir(dirs: &Dirs) -> PathBuf {
    let overridden = std::env::var_os("DYNO_DATA_DIR").is_some_and(|v| !v.is_empty());
    let app_dirs = AppId::new(APP_ID, APP_NAME).ok().and_then(|app| AppDirs::for_app(&app).ok());
    log_places(dirs, overridden, app_dirs.as_ref()).0
}

/// Installs the agent's log as the first thing `dyno-agent run` does. `None` when a log is installed already or the id is
/// refused: the agent then runs without a file log (and says so on standard error).
pub fn install(dirs: &Dirs) -> Option<LogHandle> {
    let _ = elapsed_ms();
    let app = AppId::new(APP_ID, APP_NAME).ok()?;
    let overridden = std::env::var_os("DYNO_DATA_DIR").is_some_and(|v| !v.is_empty());
    let app_dirs = AppDirs::for_app(&app).ok();
    let (log_dir, marker) = log_places(dirs, overridden, app_dirs.as_ref());
    let config = LogConfig::new(&app, PROCESS, env!("CARGO_PKG_VERSION"))
        .with_log_dir(&log_dir)
        .with_crash_marker(&marker);
    match Logging::new(config).install() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("dyno-agent: the log could not start ({e})");
            None
        }
    }
}

static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Milliseconds since the agent process started its work (the first call, made by `dyno-agent run` first thing): the `ElapsedMs`
/// of every start line, so the gaps of a slow start read straight off the log.
pub fn elapsed_ms() -> u64 {
    u64::try_from(STARTED.get_or_init(std::time::Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The OS version for the start lines ("unknown" when the OS does not say).
pub fn os_version() -> String {
    sysinfo::System::long_os_version().unwrap_or_else(|_| "unknown".into())
}

/// The CPU architecture for the start lines.
pub fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// The parent process id (1 under launchd or the service manager), for the start line.
pub fn parent_pid() -> u32 {
    #[cfg(unix)]
    {
        std::os::unix::process::parent_id()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// What the parent process is, for the start lines: the service manager when the parent is pid 1 (launchd under SMAppService,
/// init or systemd elsewhere), else the app or a shell that started the agent.
pub fn parent_name() -> &'static str {
    match parent_pid() {
        1 if cfg!(target_os = "macos") => "launchd",
        1 => "init",
        0 => "unknown",
        _ => "another program",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_and_the_crash_marker_go_where_the_bundle_reads_them() {
        let dirs = Dirs::at(std::path::PathBuf::from("/tmp/dyno-test-data"));
        let app = AppId::new(APP_ID, APP_NAME).unwrap();
        let a = AppDirs::for_app(&app).unwrap();
        // Normal: the app's log folder and the data folder the diagnostics' crash section reads (`AppDirs::data`).
        let (log, marker) = log_places(&dirs, false, Some(&a));
        assert_eq!(log, a.logs());
        assert_eq!(marker, CrashMarker::path_for(a.data(), "agent"));
        // Moved by DYNO_DATA_DIR (tests, tools): everything stays in that folder.
        let (log, marker) = log_places(&dirs, true, Some(&a));
        assert_eq!(log, dirs.logs());
        assert_eq!(marker, CrashMarker::path_for(dirs.root(), "agent"));
        // The app's folders unknown: the data folder.
        assert_eq!(log_places(&dirs, false, None).0, dirs.logs());
    }
}
