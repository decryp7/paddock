//! The app's shared state: the engine and what the commands need next to it. Everything else (watches, history,
//! settings) lives in the engine; the app holds no state of its own.

use spotter_engine::Engine;
use spotter_sources::http::Http;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Environment variable that points the app at another data folder (tests, e2e runs, a second profile). The
/// production folder is never touched by anything that sets it.
pub const DATA_DIR_ENV: &str = "SPOTTER_DATA_DIR";

/// What the commands share.
pub struct AppState {
    /// The engine.
    pub engine: Engine,
    /// Where `spotter.toml`, `spotter.db`, packs and a downloaded Chromium live.
    pub data_dir: PathBuf,
    /// Where downloads go when the settings name no folder.
    pub default_downloads: PathBuf,
    /// HTTP for small requests (checksum files).
    pub http: Http,
    /// The app's one downloader (the shell's: its per-host caps hold across every download).
    pub downloader: Arc<paddock_download::Downloader>,
    /// Downloads as shell jobs (release files, the browser).
    pub downloads: crate::jobs::Downloads,
    /// The wizard's test that runs now: a newer test stops it (the user edited the form again).
    pub last_test: std::sync::Mutex<Option<paddock_jobs::JobId>>,
    /// The page the tray or a toast asked the main window to show, for a window that could not listen yet.
    pub pending_route: std::sync::Mutex<Option<String>>,
    /// Tells the background tasks to stop.
    pub stop: tokio::sync::watch::Sender<bool>,
}

/// The state as the commands receive it.
pub type Shared = Arc<AppState>;

/// The data folder: the override when it is an absolute path, else the platform default.
pub fn resolve_data_dir(override_dir: Option<&str>, default: PathBuf) -> PathBuf {
    match override_dir.map(str::trim).filter(|d| !d.is_empty()).map(Path::new) {
        Some(p) if p.is_absolute() => p.to_path_buf(),
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_must_be_absolute() {
        let default = PathBuf::from("/default");
        let abs = if cfg!(windows) { r"C:\scratch\spotter" } else { "/scratch/spotter" };
        assert_eq!(resolve_data_dir(Some(abs), default.clone()), PathBuf::from(abs));
        assert_eq!(resolve_data_dir(Some("relative"), default.clone()), default);
        assert_eq!(resolve_data_dir(Some("  "), default.clone()), default);
        assert_eq!(resolve_data_dir(None, default.clone()), default);
    }
}
