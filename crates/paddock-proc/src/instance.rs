//! One instance per data folder, decided by a held OS lock. The holder keeps an exclusive lock on a file for as long as
//! it runs (`File::try_lock`: `flock` on Unix, `LockFileEx` on Windows); the kernel drops the lock when the process ends,
//! however it ends, so a lock is never stale and nothing reclaims it. This is the authority for "is one running here?"
//! (rule 53: hold the thing itself, never trust a pid read earlier). A discovery file ([`crate::hint`]) only says how to
//! reach the holder.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// The held lock: dropping it (or the process ending) frees the folder.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// The result of asking for the lock.
#[derive(Debug)]
pub enum Claim {
    /// This process now holds it.
    Acquired(InstanceLock),
    /// Another process holds it.
    Held,
    /// The file system cannot lock (some network shares). Nothing is known: the caller decides, and says so.
    Unsupported,
}

/// The lock file could not be opened or locked.
#[derive(Debug, thiserror::Error)]
#[error("the lock {} could not be taken: {source}", path.display())]
pub struct InstanceError {
    /// The lock file.
    pub path: PathBuf,
    /// What failed.
    #[source]
    pub source: std::io::Error,
}

impl InstanceLock {
    /// Takes the lock on `path` (created with its folder if missing, never written or truncated) without waiting.
    pub fn claim(path: &Path) -> Result<Claim, InstanceError> {
        Self::claim_with(path, File::try_lock)
    }

    /// [`InstanceLock::claim`] with the locking call given (tests make it answer what no real file system does).
    fn claim_with(
        path: &Path,
        lock: impl FnOnce(&File) -> Result<(), TryLockError>,
    ) -> Result<Claim, InstanceError> {
        let fail = |source| InstanceError {
            path: path.to_path_buf(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(fail)?;
        match judge(lock(&file)) {
            Ok(Verdict::Free) => Ok(Claim::Acquired(InstanceLock { _file: file })),
            Ok(Verdict::Held) => Ok(Claim::Held),
            Ok(Verdict::Unsupported) => Ok(Claim::Unsupported),
            Err(e) => Err(fail(e)),
        }
    }
}

/// What `try_lock` said, apart from the file.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Free,
    Held,
    Unsupported,
}

/// Reads the answer of `File::try_lock`.
fn judge(answer: Result<(), TryLockError>) -> std::io::Result<Verdict> {
    match answer {
        Ok(()) => Ok(Verdict::Free),
        Err(TryLockError::WouldBlock) => Ok(Verdict::Held),
        Err(TryLockError::Error(e)) if e.kind() == std::io::ErrorKind::Unsupported => {
            Ok(Verdict::Unsupported)
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Whether some process holds the lock on `path` right now. A free lock is taken and dropped at once to find out, so an
/// instance starting at that very instant may be refused (it exits; the caller finds one on its next pass). Anything
/// that is not a clear "held" (unsupported, an error) is `false`.
pub fn is_held(path: &Path) -> bool {
    matches!(InstanceLock::claim(path), Ok(Claim::Held))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Waits for `cond`: a child forked by a parallel test holds a copy of every descriptor open at that moment (also the
    /// lock's) until it execs, so a lock can look held for a moment after its holder let go. Bounded only as a safety net.
    fn until(cond: impl Fn() -> bool) {
        let t = std::time::Instant::now();
        while t.elapsed() < std::time::Duration::from_secs(30) {
            if cond() {
                return;
            }
            std::thread::yield_now();
        }
        panic!("the condition never held");
    }

    #[test]
    fn one_holder_per_file_and_the_lock_ends_with_its_holder() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data").join("a.lock");
        until(|| !is_held(&path));
        let held = std::cell::RefCell::new(None);
        until(|| {
            *held.borrow_mut() = match InstanceLock::claim(&path) {
                Ok(Claim::Acquired(l)) => Some(l),
                _ => None,
            };
            held.borrow().is_some()
        });
        let first = held.take();
        assert!(is_held(&path));
        assert!(matches!(InstanceLock::claim(&path).unwrap(), Claim::Held));
        let other = tmp.path().join("b.lock");
        until(|| matches!(InstanceLock::claim(&other), Ok(Claim::Acquired(_))));
        drop(first);
        until(|| !is_held(&path));
        until(|| matches!(InstanceLock::claim(&path), Ok(Claim::Acquired(_))));
    }

    #[test]
    fn a_claim_follows_what_the_lock_call_answers() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.lock");
        let kind = |k| Err(TryLockError::Error(std::io::Error::from(k)));
        assert!(matches!(
            InstanceLock::claim_with(&path, |_| Ok(())),
            Ok(Claim::Acquired(_))
        ));
        assert!(matches!(
            InstanceLock::claim_with(&path, |_| Err(TryLockError::WouldBlock)),
            Ok(Claim::Held)
        ));
        assert!(matches!(
            InstanceLock::claim_with(&path, |_| kind(std::io::ErrorKind::Unsupported)),
            Ok(Claim::Unsupported)
        ));
        let e = InstanceLock::claim_with(&path, |_| kind(std::io::ErrorKind::PermissionDenied))
            .unwrap_err();
        assert_eq!(e.source.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(e.path, path);
    }

    #[test]
    fn the_answers_of_try_lock_are_told_apart() {
        let io = |k| TryLockError::Error(std::io::Error::from(k));
        assert_eq!(judge(Ok(())).unwrap(), Verdict::Free);
        assert_eq!(judge(Err(TryLockError::WouldBlock)).unwrap(), Verdict::Held);
        assert_eq!(
            judge(Err(io(std::io::ErrorKind::Unsupported))).unwrap(),
            Verdict::Unsupported
        );
        assert_eq!(
            judge(Err(io(std::io::ErrorKind::PermissionDenied)))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn a_path_that_cannot_hold_a_lock_file_says_where() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-folder");
        std::fs::write(&file, b"x").unwrap();
        let err = InstanceLock::claim(&file.join("a.lock")).unwrap_err();
        assert!(err.to_string().contains("a.lock"), "{err}");
        assert!(!is_held(&file.join("a.lock")), "an error is not a holder");
        // A folder where the lock file should be cannot be opened as one.
        let folder = tmp.path().join("folder.lock");
        std::fs::create_dir(&folder).unwrap();
        assert!(
            InstanceLock::claim(&folder)
                .unwrap_err()
                .to_string()
                .contains("folder.lock")
        );
    }
}
