//! Persisting a store through `paddock-env`.

use crate::retention::Retention;
use crate::series::Series;
use crate::store::TieredStore;
use paddock_env::{Problem, Result, VersionedFile};
use std::path::PathBuf;

/// The state-file version of a persisted store.
pub const STORE_VERSION: u32 = 1;

/// A store's file: versioned, written atomically with a backup, a damaged or newer file gives an empty store and a
/// [`Problem`] to report once. The file is at most 8 MB (the default of `VersionedFile`; the retention bound keeps
/// real stores far below it).
pub struct StoreFile<P: Series> {
    file: VersionedFile<TieredStore<P>>,
    retention: Retention,
}

impl<P: Series> StoreFile<P> {
    /// The file at `path` for a store with `retention`.
    pub fn new(path: impl Into<PathBuf>, retention: Retention) -> Self {
        StoreFile {
            file: VersionedFile::new(path, STORE_VERSION),
            retention,
        }
    }

    /// Reads the store fitted to the retention (see [`TieredStore::fit`]), and why the file was not used as it is.
    pub fn load(&self) -> (TieredStore<P>, Option<Problem>) {
        let loaded = self.file.load();
        (loaded.value.fit(&self.retention), loaded.problem)
    }

    /// Writes the store.
    pub fn save(&self, store: &TieredStore<P>) -> Result<()> {
        self.file.save(store)
    }

    /// The file's path.
    pub fn path(&self) -> &std::path::Path {
        self.file.path()
    }
}
