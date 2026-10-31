//! Scratch folders for fixtures that hand out a plain path: removed when the test's thread ends, also after a panic
//! (thread-local values are dropped on unwind). CI fails on `debrief-*` folders left in the temp folder
//! (docs/testing/temp-leaks.md).

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    static KEPT: RefCell<Vec<tempfile::TempDir>> = const { RefCell::new(Vec::new()) };
}

/// A fresh empty folder named `debrief-<prefix>-...` in the OS temp folder, removed when the calling thread ends.
pub fn scratch_dir(prefix: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("debrief-{prefix}-"))
        .tempdir()
        .expect("create a scratch folder");
    let path = dir.path().to_path_buf();
    KEPT.with(|k| k.borrow_mut().push(dir));
    path
}

thread_local! {
    static FAIL_NEXT_REMOVAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Makes the next [`remove_or_defer`] on this thread fail without touching the folder: the seam a test uses to stage a
/// failed removal. File modes cannot stage one portably (root ignores a read-only folder, as in the Linux CI container).
pub fn fail_next_removal() {
    FAIL_NEXT_REMOVAL.with(|f| f.set(true));
}

/// Folders whose removal failed when their owner dropped them: removed again when the test process exits.
static DEFERRED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

extern "C" fn remove_deferred_at_exit() {
    remove_deferred();
}

/// Removes `path` (a fixture's folder, when its owner is dropped). A removal that fails (an error on a busy machine,
/// anything that holds the folder for a moment) is said on stderr and done again when the test process exits, so the
/// folder never outlives the process silently (CI 7108 left `debrief-prop-*` folders behind; docs/testing/temp-leaks.md).
pub fn remove_or_defer(path: &std::path::Path) {
    let staged = FAIL_NEXT_REMOVAL.with(|f| f.replace(false));
    let removed = if staged {
        Err(std::io::Error::other("a removal failure staged by fail_next_removal"))
    } else {
        std::fs::remove_dir_all(path)
    };
    match removed {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!(
                "could not remove {} now ({e}); removing it when the test process exits",
                path.display()
            );
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
    }
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

#[test]
fn scratch_folders_are_removed_when_the_thread_ends_even_after_a_panic() {
    let kept = std::thread::spawn(|| scratch_dir("probe")).join().unwrap();
    assert!(!kept.exists());
    let panicked = std::thread::spawn(|| {
        let p = scratch_dir("probe-panic");
        std::fs::write(p.join("f"), b"x").unwrap();
        std::panic::resume_unwind(Box::new(p));
    })
    .join()
    .unwrap_err()
    .downcast::<PathBuf>()
    .unwrap();
    assert!(!panicked.exists());
}
