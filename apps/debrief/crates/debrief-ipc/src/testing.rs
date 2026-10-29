//! Helpers for tests that kill or abandon a host on purpose (feature `test-support`, Unix).
//!
//! A host that ends without running destructors cannot remove the private endpoint folders it made before its children
//! connected. A test that does this on purpose makes the host (and its children) use a folder of its own, so the folder
//! goes with the test instead of staying in a shared one.

use std::path::{Path, PathBuf};

/// A short folder under `/tmp` that is the only place a process started with [`Confined::env`] makes endpoint folders.
///
/// It is short because an endpoint's socket path is limited to [`crate::endpoint::MAX_SOCKET_PATH`] bytes: in macOS' long
/// temp folder a path does not fit, and `create_named` would go on to `/tmp`, outside the folder. Removed when dropped.
pub struct Confined {
    dir: tempfile::TempDir,
    path: PathBuf,
}

/// Makes a [`Confined`] folder. Test-only: it panics (`expect`) when `/tmp` cannot hold a folder or the folder vanishes,
/// which fails the test that asked for it, the right outcome there; product code must not call it.
pub fn confined_tmp() -> Confined {
    let dir = tempfile::Builder::new()
        .prefix("dbw")
        .tempdir_in("/tmp")
        .expect("a folder under /tmp");
    let path = dir.path().canonicalize().expect("the folder exists");
    Confined { dir, path }
}

impl Confined {
    /// The folder, with symlinks resolved (as `create_named` resolves its bases).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `TMPDIR` and `XDG_RUNTIME_DIR`, the two places an endpoint is tried before `/tmp`, both set to the folder.
    pub fn env(&self) -> Vec<(String, String)> {
        let p = self.path.display().to_string();
        vec![("TMPDIR".into(), p.clone()), ("XDG_RUNTIME_DIR".into(), p)]
    }

    /// Removes the folder now and returns where it was.
    pub fn remove(self) -> PathBuf {
        let path = self.dir.path().to_path_buf();
        drop(self.dir);
        path
    }
}
