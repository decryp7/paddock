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
