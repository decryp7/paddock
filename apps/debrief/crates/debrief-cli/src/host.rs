//! How the CLI gets a store: the port [`StoreHost`] (the store worker in the executable; the DuckDB store in
//! process in tests) and [`WorkerHost`], which starts the worker the way the app does.

use debrief_core::ports::{Store, StoreError, StoreResult};
use debrief_core::Profile;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The database file inside a store folder: its presence says the folder holds a store to reuse.
pub const STORE_DB_FILE: &str = "data.duckdb";

/// A store opened for the CLI.
pub type SharedStore = Arc<dyn Store>;

/// Creates and opens stores. Implementations honour the [`Store`] contract; errors say what to fix.
pub trait StoreHost {
    /// A new, empty store in `dir` (whatever was there is replaced).
    fn create(&self, dir: &Path, profile: &Profile) -> StoreResult<SharedStore>;
    /// The store already in `dir`.
    fn open(&self, dir: &Path, profile: &Profile) -> StoreResult<SharedStore>;
}

/// The store worker process, found next to the executable (`DEBRIEF_STORE_WORKER` overrides it in a debug build only, a release
/// build ignores it; `DEBRIEF_LIB_DIR` names the folder holding libduckdb when it is not next to the worker).
pub struct WorkerHost {
    worker: debrief_store_client::StoreWorker,
}

/// Where the worker's executable is.
pub fn worker_binary() -> PathBuf {
    worker_binary_with(cfg!(debug_assertions), std::env::var_os("DEBRIEF_STORE_WORKER"))
}

/// [`worker_binary`] for a given build mode and value of `DEBRIEF_STORE_WORKER`.
fn worker_binary_with(debug: bool, env: Option<std::ffi::OsString>) -> PathBuf {
    if let Some(p) = paddock_env::dev_override::resolve(debug, "DEBRIEF_STORE_WORKER", env) {
        return PathBuf::from(p);
    }
    let name = if cfg!(windows) {
        "debrief-store-worker.exe"
    } else {
        "debrief-store-worker"
    };
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// What to tell the user when the worker is not at `exe`. Only a debug build honours `DEBRIEF_STORE_WORKER`, so only its
/// message names the variable.
fn missing_worker_message(debug: bool, exe: &Path) -> String {
    let hint = if debug { " or set DEBRIEF_STORE_WORKER" } else { "" };
    format!(
        "the store worker was not found at {}; install it next to this program{hint}",
        exe.display()
    )
}

impl WorkerHost {
    pub fn new() -> StoreResult<WorkerHost> {
        let exe = worker_binary();
        if !exe.is_file() {
            return Err(StoreError::Invalid(missing_worker_message(cfg!(debug_assertions), &exe)));
        }
        let mut cfg = debrief_store_client::WorkerConfig::new(&exe);
        cfg.lib_dir = std::env::var_os("DEBRIEF_LIB_DIR")
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .or_else(|| exe.parent().map(Path::to_path_buf));
        Ok(WorkerHost {
            worker: debrief_store_client::StoreWorker::new(cfg)?,
        })
    }

    /// Stops the worker process (also done when this process ends).
    pub fn shutdown(&self) {
        self.worker.shutdown();
    }
}

impl StoreHost for WorkerHost {
    fn create(&self, dir: &Path, profile: &Profile) -> StoreResult<SharedStore> {
        Ok(Arc::new(self.worker.open_store(dir, profile.clone(), true, None)?))
    }

    fn open(&self, dir: &Path, profile: &Profile) -> StoreResult<SharedStore> {
        Ok(Arc::new(self.worker.open_store(dir, profile.clone(), false, None)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_missing_worker_message_names_the_variable_only_where_it_works() {
        let exe = Path::new("/opt/debrief/debrief-store-worker");
        let release = missing_worker_message(false, exe);
        assert!(
            release.contains("/opt/debrief/debrief-store-worker") && release.contains("next to this program"),
            "{release}"
        );
        assert!(
            !release.contains("DEBRIEF_STORE_WORKER"),
            "a release build ignores the variable: {release}"
        );
        assert!(missing_worker_message(true, exe).contains("DEBRIEF_STORE_WORKER"));
    }

    #[test]
    fn a_release_build_ignores_debrief_store_worker() {
        let evil = Some(std::ffi::OsString::from("/evil/store-worker"));
        assert_eq!(worker_binary_with(true, evil.clone()), PathBuf::from("/evil/store-worker"));
        let release = worker_binary_with(false, evil);
        assert_ne!(release, PathBuf::from("/evil/store-worker"));
        assert!(release.file_name().unwrap().to_string_lossy().starts_with("debrief-store-worker"));
    }
}
