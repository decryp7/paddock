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

/// Whether an agent holds the lock for `dirs` at this one check, without waiting (`paddock_proc::is_held_now`). Only for a
/// caller where a held reading leads to waiting and looking again (the app's supervisor, which judges the lock over its own
/// looks at its own pace), so a false "held" (a spawning child's copy of the descriptor) only delays; never for a one-shot
/// action, which uses [`InstanceLock::acquire`] or `paddock_proc::is_held_settled`. There is deliberately no waiting
/// variant here: a re-checking probe in the supervisor's look slowed every look about 400 ms (CI run 7257).
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
        assert!(!paddock_proc::is_held_settled(&dirs.instance_lock()));
        let first = InstanceLock::acquire(&dirs).unwrap();
        assert!(paddock_proc::is_held_settled(&dirs.instance_lock()));
        assert!(is_held_now(&dirs));
        let err = InstanceLock::acquire(&dirs).unwrap_err();
        assert!(matches!(err, LockError::Held { .. }), "{err}");
        assert!(err.to_string().contains("already running"));
        let other = Dirs::at(tmp.path().join("other"));
        drop(InstanceLock::acquire(&other).unwrap());
        drop(first);
        assert!(!paddock_proc::is_held_settled(&dirs.instance_lock()));
        drop(InstanceLock::acquire(&dirs).unwrap());
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
