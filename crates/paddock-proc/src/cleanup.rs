//! Clean-up steps whose failure nothing can act on (ADR 0054 section 2: errors at OS edges are never discarded). The
//! step has already done its job or the error that led here was already reported; what is left is to say, once and in
//! the log, that the clean-up itself failed, so the next investigation does not start from a silent `let _ =`.
//!
//! Every function returns whether the step failed (a caller that cares may use it; the log line is written either way).

use std::fmt::Display;
use std::path::Path;

/// Logs `error` of a clean-up step `what` at debug level.
pub fn report(what: &str, error: &dyn Display) {
    log::debug!("clean-up step failed: {what}: {error}");
}

/// Logs the failure of a clean-up step `what` at debug level and says whether it failed.
pub fn logged<T, E: Display>(what: &str, result: Result<T, E>) -> bool {
    match result {
        Ok(_) => false,
        Err(e) => {
            report(what, &e);
            true
        }
    }
}

/// Waits for a helper thread to end as a clean-up step; a thread that panicked is logged. Says whether it panicked.
#[must_use = "true means the thread panicked: leftover state a caller should surface, not drop"]
pub fn join<T>(what: &str, thread: std::thread::JoinHandle<T>) -> bool {
    if thread.join().is_err() {
        report(what, &"the thread panicked");
        true
    } else {
        false
    }
}

/// Removes the folder at `path` and everything in it as a clean-up step: not there is the goal, any other failure is logged.
#[must_use = "true means the folder is still there (logged): the caller decides whether leftovers matter"]
pub fn remove_dir_all(path: &Path) -> bool {
    match std::fs::remove_dir_all(path) {
        Ok(()) => false,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            report(&format!("removing {}", path.display()), &e);
            true
        }
    }
}

/// Removes the file at `path` as a clean-up step: a file that is not there is the goal, any other failure is logged.
#[must_use = "true means the file is still there (logged): the caller decides whether leftovers matter"]
pub fn remove_file(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => false,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            report(&format!("removing {}", path.display()), &e);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_step_is_reported_and_a_successful_one_is_not() {
        assert!(logged("close", Err::<(), _>("boom")));
        assert!(!logged("close", Ok::<_, &str>(1)));
    }

    #[test]
    fn removing_a_missing_file_is_the_goal_and_removing_a_file_that_cannot_go_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        assert!(!remove_file(&file), "not there is fine");
        std::fs::write(&file, b"x").unwrap();
        assert!(!remove_file(&file));
        assert!(!file.exists());
        // A folder is not a file: the OS refuses, and that is reported.
        assert!(remove_file(dir.path()));
    }

    #[test]
    fn removing_a_folder_and_joining_a_thread_report_their_failures() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("a");
        assert!(!remove_dir_all(&sub), "not there is fine");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f"), b"x").unwrap();
        assert!(!remove_dir_all(&sub));
        assert!(!sub.exists());
        let file = dir.path().join("plain");
        std::fs::write(&file, b"x").unwrap();
        assert!(remove_dir_all(&file), "a file is not a folder");
        assert!(!join("fine", std::thread::spawn(|| ())));
        assert!(join(
            "boom",
            std::thread::spawn(|| panic!("expected by the test"))
        ));
        report("direct", &"words");
    }
}
