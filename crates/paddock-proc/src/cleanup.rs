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

/// The message of a panic payload (`panic!("..")` carries a `&str` or a `String`).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no text".to_owned())
}

/// Joins `thread`: the panic message when it panicked.
fn try_join<T>(thread: std::thread::JoinHandle<T>) -> Option<String> {
    thread.join().err().map(|p| panic_message(p.as_ref()))
}

/// Removes a file or a folder tree; not there is the goal, so it is no error. The one removal, behind the flag and warn forms.
fn try_remove(path: &Path, folder: bool) -> Option<std::io::Error> {
    let result = if folder {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result
        .err()
        .filter(|e| e.kind() != std::io::ErrorKind::NotFound)
}

/// Waits for a helper thread to end as a clean-up step; a thread that panicked is logged with its message. Says whether it panicked.
#[must_use = "true means the thread panicked: leftover state a caller should surface, not drop"]
pub fn join<T>(what: &str, thread: std::thread::JoinHandle<T>) -> bool {
    match try_join(thread) {
        Some(message) => {
            report(what, &format!("the thread panicked: {message}"));
            true
        }
        None => false,
    }
}

/// [`join`] for a caller with nothing to do about a panicked thread: the panic and its message are logged at warn level,
/// naming the thread.
pub fn join_warn<T>(what: &str, thread: std::thread::JoinHandle<T>) {
    if let Some(message) = try_join(thread) {
        log::warn!("{what} panicked before it was joined; its work may be unfinished: {message}");
    }
}

/// Removes the folder at `path` and everything in it as a clean-up step: not there is the goal, any other failure is logged.
#[must_use = "true means the folder is still there (logged): the caller decides whether leftovers matter"]
pub fn remove_dir_all(path: &Path) -> bool {
    remove_logged(path, true)
}

/// Removes the file at `path` as a clean-up step: a file that is not there is the goal, any other failure is logged.
#[must_use = "true means the file is still there (logged): the caller decides whether leftovers matter"]
pub fn remove_file(path: &Path) -> bool {
    remove_logged(path, false)
}

fn remove_logged(path: &Path, folder: bool) -> bool {
    match try_remove(path, folder) {
        Some(e) => {
            report(&format!("removing {}", path.display()), &e);
            true
        }
        None => false,
    }
}

/// Removes the folder at `path` as a best-effort clean-up and logs a failure (not "not there") at warn level with its cause,
/// for a caller with nothing to do about a leftover but who must not hide it. A caller that can act uses [`remove_dir_all`].
pub fn remove_dir_all_warn(path: &Path) {
    warn_if_left(path, try_remove(path, true));
}

/// [`remove_dir_all_warn`] for a file.
pub fn remove_file_warn(path: &Path) {
    warn_if_left(path, try_remove(path, false));
}

fn warn_if_left(path: &Path, error: Option<std::io::Error>) {
    if let Some(e) = error {
        log::warn!("clean-up left {} behind: {e}", path.display());
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
