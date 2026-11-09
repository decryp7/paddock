//! Removal of a test's scratch folder, shared by the integration tests' `TempDir` and the unit tests' `Dir`
//! (`maintain_tests.rs` includes this file by path). CI fails on `debrief-*` folders left in the temp folder
//! (docs/testing/temp-leaks.md): a store's background threads can still be writing into its folder for a moment after the
//! test dropped the store (CI 7813 left `debrief-duck-diff-*` and `debrief-maintain-reorder-skew-*`), so a removal that
//! fails, or whose folder comes back, is tried again, and one that never works is said on stderr and done again when the
//! test process exits.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Attempts at removing a folder before it is deferred to the end of the process; each one is a full removal.
const ATTEMPTS: u32 = 200;

thread_local! {
    static STAGED_FAILURES: Cell<u32> = const { Cell::new(0) };
}

/// Makes the next `n` removal attempts on this thread fail without touching the folder (the seam a test uses to stage a
/// busy folder: file modes cannot do it portably, root ignores them).
pub fn fail_next_removals(n: u32) {
    STAGED_FAILURES.with(|f| f.set(n));
}

static DEFERRED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

extern "C" fn remove_deferred_at_exit() {
    remove_deferred();
}

/// Removes the folders [`remove_or_defer`] could not (what the exit handler does).
pub fn remove_deferred() {
    let paths = std::mem::take(&mut *DEFERRED.lock().unwrap_or_else(|p| p.into_inner()));
    for path in paths {
        if let Err(e) = std::fs::remove_dir_all(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("could not remove {} at exit either: {e}", path.display());
            }
        }
    }
}

/// One attempt: removed and still gone.
fn attempt(path: &Path) -> std::io::Result<()> {
    if STAGED_FAILURES.with(|f| f.replace(f.get().saturating_sub(1))) > 0 {
        return Err(std::io::Error::other("a removal failure staged by fail_next_removals"));
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) if path.exists() => Err(std::io::Error::other("the folder came back after its removal")),
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Removes `path`, trying again while it fails; what still fails is said on stderr and removed when the process exits.
pub fn remove_or_defer(path: &Path) {
    let mut last = None;
    for _ in 0..ATTEMPTS {
        match attempt(path) {
            Ok(()) => return,
            Err(e) => {
                last = Some(e);
                std::thread::yield_now();
            }
        }
    }
    if let Some(e) = last {
        eprintln!("could not remove {} ({e}); removing it when the test process exits", path.display());
    }
    static AT_EXIT: std::sync::Once = std::sync::Once::new();
    AT_EXIT.call_once(|| {
        extern "C" {
            fn atexit(cb: extern "C" fn()) -> std::os::raw::c_int;
        }
        // SAFETY: registers a plain `extern "C"` function with the C runtime; it runs once, at exit.
        let registered = unsafe { atexit(remove_deferred_at_exit) };
        assert_eq!(registered, 0, "atexit failed");
    });
    DEFERRED.lock().unwrap_or_else(|p| p.into_inner()).push(path.to_path_buf());
}
