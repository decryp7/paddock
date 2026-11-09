//! Scratch folders for unit tests that clean up even when the test panics.
//!
//! A folder made here lives until the test's thread ends (thread-local drop runs on unwind too), so a helper can return a
//! plain path while the test body keeps using it. The same helper as `debrief-core`'s `test_dirs` (one per crate: a
//! crate's unit tests cannot reach another's `cfg(test)` code). CI fails on `debrief-*` folders left in the temp dir
//! (`scripts/ci/check-temp-leaks.mjs`).

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    static KEPT: RefCell<Vec<tempfile::TempDir>> = const { RefCell::new(Vec::new()) };
}

/// A fresh empty folder named `debrief-<prefix>-...` in the OS temp dir, removed when the calling thread ends.
pub(crate) fn scratch_dir(prefix: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("debrief-{prefix}-"))
        .tempdir()
        .expect("create a scratch folder");
    let path = dir.path().to_path_buf();
    KEPT.with(|k| k.borrow_mut().push(dir));
    path
}
