//! A scratch folder that cannot be removed is never a silent leak: the removal says so on stderr (past libtest's capture),
//! leaves a marker file `check-temp-leaks.mjs` reports, and only then is retried at the end of the process
//! (docs/testing/temp-leaks.md; CI 7813 left `debrief-duck-diff-*` and `debrief-maintain-reorder-skew-*`).

use debrief_store_duck::test_scratch::{fail_next_removal, marker_file, remove_deferred, remove_or_defer_marking_in};

#[test]
fn a_removal_that_works_leaves_no_marker() {
    let markers = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap().keep();
    std::fs::write(dir.join("f"), b"x").unwrap();
    remove_or_defer_marking_in(&dir, markers.path());
    assert!(!dir.exists());
    assert_eq!(std::fs::read_dir(markers.path()).unwrap().count(), 0);
}

#[test]
fn a_removal_that_fails_leaves_a_marker_naming_the_folder_the_error_and_what_was_in_it_and_the_exit_handler_keeps_it() {
    let markers = tempfile::tempdir().unwrap();
    let dir = tempfile::Builder::new().prefix("debrief-probe-").tempdir().unwrap().keep();
    std::fs::create_dir(dir.join("index")).unwrap();
    std::fs::write(dir.join("index/seg.bin"), b"x").unwrap();
    fail_next_removal();
    remove_or_defer_marking_in(&dir, markers.path());
    assert!(dir.exists(), "the staged failure kept the folder");
    let marker = marker_file(&dir, markers.path());
    let text = std::fs::read_to_string(&marker).unwrap_or_else(|e| panic!("no marker {}: {e}", marker.display()));
    assert!(text.contains(&dir.display().to_string()), "{text}");
    assert!(text.contains("a removal failure staged"), "{text}");
    assert!(text.contains("index/seg.bin"), "{text}");
    assert!(
        marker.file_name().unwrap().to_string_lossy().starts_with("debrief-"),
        "the gate matches debrief-*"
    );
    remove_deferred();
    assert!(!dir.exists(), "the exit handler's removal");
    assert!(marker.exists(), "the marker stays after the folder is gone: the run is still red");
}
