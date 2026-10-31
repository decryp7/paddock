//! One agent per data folder. The agent holds an exclusive lock on `agent.lock` in its data folder for as long as it runs
//! (`File::try_lock`: `flock` on Unix, `LockFileEx` on Windows). The kernel drops the lock when the process ends, however
//! it ends, so a lock is never stale and nothing has to reclaim it. A second agent on the same folder (started by the app
//! while the first was still starting from a slow disk, or by the login service beside the app's own) cannot take it,
//! refuses to start before it opens the history database, and exits with [`crate::cli::EXIT_ALREADY_RUNNING`].
//!
//! The lock is the agent's identity for "is one running here?" (rule 53: hold the thing itself, never trust a pid read
//! earlier); the discovery file only says how to reach it.

use crate::paths::Dirs;
use paddock_proc::Claim;
use std::fs::OpenOptions;
use std::path::PathBuf;

/// The held lock: dropping it (or the process ending) frees the data folder.
#[derive(Debug)]
pub struct InstanceLock {
    _lock: paddock_proc::InstanceLock,
}

/// Why the lock was not taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another agent holds it.
    #[error(
        "another Dyno agent is already running for {} (it may be on another computer that shares this folder, for example a network home)",
        folder.display()
    )]
    Held {
        /// The data folder.
        folder: PathBuf,
    },
    /// The lock file could not be opened or locked.
    #[error("the agent lock {} could not be taken: {source}", path.display())]
    Io {
        /// The lock file.
        path: PathBuf,
        /// What failed.
        source: std::io::Error,
    },
}

impl InstanceLock {
    /// Takes the lock for `dirs`; a free lock is taken at once, a refusal takes about 400 ms (5 checks, 100 ms apart) before it
    /// counts as held (the shared single-instance lock, `paddock_proc::InstanceLock`).
    pub fn acquire(dirs: &Dirs) -> Result<InstanceLock, LockError> {
        let path = dirs.instance_lock();
        match paddock_proc::InstanceLock::claim(&path) {
            Ok(Claim::Acquired(_lock)) => Ok(InstanceLock { _lock }),
            Ok(Claim::Held) => Err(LockError::Held {
                folder: dirs.root().to_path_buf(),
            }),
            Ok(Claim::Unsupported) => Err(LockError::Io {
                path,
                source: std::io::Error::new(std::io::ErrorKind::Unsupported, "file locking is not supported here"),
            }),
            Err(e) => Err(LockError::Io {
                path: e.path,
                source: e.source,
            }),
        }
    }
}

/// The agent log is moved aside to `agent.log.1` once it grows past this.
const LOG_MAX_BYTES: u64 = 1024 * 1024;

/// Appends `line` to the agent's own log file (`logs/agent.log` in the data folder), so a refusal or a missing lock is
/// found even when the agent was started with its stderr discarded (by the app) or sent elsewhere (by a service).
/// Bounded: a line equal to the file's last one is not written again (a service retrying a refused agent every 10 s
/// would otherwise write thousands a day), and the file is rotated past [`LOG_MAX_BYTES`], keeping one older
/// generation (and nothing is written while the file cannot be moved aside). Failures are ignored: logging never stops the
/// agent. No line carries a timestamp; if one ever does, the repeat check must compare the text without it.
pub fn note(dirs: &Dirs, line: &str) {
    use std::io::Write;
    let path = dirs.logs().join("agent.log");
    let _ = std::fs::create_dir_all(dirs.logs());
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > LOG_MAX_BYTES {
            // Moved aside, replacing an older generation. When it cannot be moved (a scanner holds the file), nothing is
            // written: the file never grows past the bound by more than the line that crossed it.
            if std::fs::rename(&path, path.with_extension("log.1")).is_err() {
                return;
            }
        } else if std::fs::read_to_string(&path).is_ok_and(|t| t.lines().last() == Some(line)) {
            return;
        }
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// Whether an agent holds the lock for `dirs` right now. A free lock is taken and dropped at once to find out, so an agent
/// starting at that very instant may be refused (it exits; the caller starts or finds one on its next pass).
pub fn is_held(dirs: &Dirs) -> bool {
    paddock_proc::is_held(&dirs.instance_lock())
}

/// [`is_held`] at one check, without the re-check's wait: for the app's supervisor, which judges the lock over its own looks
/// at its own pace (`paddock_proc::is_held_now`).
pub fn is_held_now(dirs: &Dirs) -> bool {
    paddock_proc::is_held_now(&dirs.instance_lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_per_folder_and_the_lock_ends_with_its_holder() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        assert!(!is_held(&dirs));
        let first = InstanceLock::acquire(&dirs).unwrap();
        assert!(is_held(&dirs));
        assert!(is_held_now(&dirs));
        let err = InstanceLock::acquire(&dirs).unwrap_err();
        assert!(matches!(err, LockError::Held { .. }), "{err}");
        assert!(err.to_string().contains("already running"));
        let other = Dirs::at(tmp.path().join("other"));
        drop(InstanceLock::acquire(&other).unwrap());
        drop(first);
        assert!(!is_held(&dirs));
        drop(InstanceLock::acquire(&dirs).unwrap());
    }

    #[test]
    fn repeated_refusals_do_not_grow_the_log_without_bound() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let log = dirs.logs().join("agent.log");
        for _ in 0..50 {
            note(&dirs, "dyno-agent: another Dyno agent is already running");
        }
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().lines().count(),
            1,
            "the same refusal is written once"
        );
        note(&dirs, "dyno-agent: something else");
        note(&dirs, "dyno-agent: another Dyno agent is already running");
        assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 3);
        // Past the bound the file is moved aside, so it never grows beyond it by more than a line.
        std::fs::write(&log, vec![b'x'; usize::try_from(LOG_MAX_BYTES).unwrap() + 1]).unwrap();
        note(&dirs, "after rotation");
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "after rotation");
        assert!(dirs.logs().join("agent.log.1").exists());
    }

    #[test]
    fn rotation_replaces_an_older_generation_and_a_log_that_cannot_move_is_not_written() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let log = dirs.logs().join("agent.log");
        let older = dirs.logs().join("agent.log.1");
        let big = || vec![b'x'; usize::try_from(LOG_MAX_BYTES).unwrap() + 1];
        std::fs::create_dir_all(dirs.logs()).unwrap();
        std::fs::write(&older, b"older generation").unwrap();
        std::fs::write(&log, big()).unwrap();
        note(&dirs, "first after rotation");
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "first after rotation");
        assert!(
            std::fs::metadata(&older).unwrap().len() > LOG_MAX_BYTES,
            "the older generation was replaced"
        );
        // The move fails (the target is a folder here; on Windows a scanner holding the file does the same): no write.
        std::fs::remove_file(&older).unwrap();
        std::fs::create_dir(&older).unwrap();
        std::fs::write(&log, big()).unwrap();
        let before = std::fs::metadata(&log).unwrap().len();
        note(&dirs, "must not grow the file");
        assert_eq!(std::fs::metadata(&log).unwrap().len(), before);
    }

    #[test]
    fn a_folder_that_cannot_hold_the_lock_file_says_where() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-folder");
        std::fs::write(&file, b"x").unwrap();
        let err = InstanceLock::acquire(&Dirs::at(&file)).unwrap_err();
        assert!(matches!(err, LockError::Io { .. }), "{err}");
        assert!(err.to_string().contains("agent.lock"));
    }
}
