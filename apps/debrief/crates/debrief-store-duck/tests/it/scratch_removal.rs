//! A scratch folder whose removal fails or is raced is removed on a later attempt, or at the end of the process
//! (docs/testing/temp-leaks.md; CI 7813 left `debrief-duck-diff-*` folders).

use crate::common::scratch::{fail_next_removals, remove_deferred, remove_or_defer};
use crate::common::TempDir;

#[test]
fn a_removal_that_fails_a_few_times_is_tried_again() {
    let path = {
        let dir = TempDir::new("retry");
        std::fs::write(dir.0.join("f"), b"x").unwrap();
        fail_next_removals(3);
        dir.0.clone()
    };
    assert!(!path.exists(), "left behind: {}", path.display());
}

#[test]
fn a_removal_that_never_works_is_deferred_to_the_end_of_the_process() {
    let dir = TempDir::new("deferred");
    let path = dir.0.clone();
    std::fs::write(path.join("f"), b"x").unwrap();
    fail_next_removals(u32::MAX);
    remove_or_defer(&path);
    fail_next_removals(0);
    assert!(path.exists(), "the staged failures kept it");
    remove_deferred();
    assert!(!path.exists(), "the exit handler's removal");
}
