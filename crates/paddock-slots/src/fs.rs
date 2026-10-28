use crate::{SlotError, SlotId, SlotLocks};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// The slots over lock files in `<root>/slots/`. `sN.lock` carries the lock (never read or written, because Windows
/// locks block other handles' reads), `sN.owner` says who holds it, `sN.release` asks the holder to let go.
#[derive(Debug, Clone)]
pub struct FsLocks {
    dir: PathBuf,
}

/// A held slot: dropping it closes the file and so releases the lock.
#[derive(Debug)]
pub struct FsGuard {
    _file: File,
}

/// Who holds a slot, as written next to its lock for `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerInfo {
    /// The job folder whose disappearance ends the hold.
    pub owner: PathBuf,
    /// The holder process.
    pub pid: u32,
    /// The holder's token: only a release that presents it is honoured.
    pub token: String,
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SlotError + '_ {
    move |source| SlotError::Io {
        path: path.to_path_buf(),
        source,
    }
}

impl FsLocks {
    /// Slots under `<root>/slots/`.
    pub fn new(root: impl AsRef<Path>) -> FsLocks {
        FsLocks {
            dir: root.as_ref().join("slots"),
        }
    }

    fn file(&self, id: SlotId, ext: &str) -> PathBuf {
        self.dir.join(format!("{id}.{ext}"))
    }

    /// Records the holder next to the lock.
    pub fn write_owner(&self, id: SlotId, info: &OwnerInfo) -> Result<(), SlotError> {
        let p = self.file(id, "owner");
        let text = format!(
            "pid={}\ntoken={}\nowner={}\n",
            info.pid,
            info.token,
            info.owner.display()
        );
        std::fs::write(&p, text).map_err(io(&p))
    }

    /// The recorded holder, if any (a missing or unreadable record is `None`: it is only information).
    pub fn read_owner(&self, id: SlotId) -> Option<OwnerInfo> {
        let text = std::fs::read_to_string(self.file(id, "owner")).ok()?;
        let field = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix('=')))
        };
        Some(OwnerInfo {
            pid: field("pid")?.parse().ok()?,
            token: field("token")?.to_string(),
            owner: PathBuf::from(field("owner")?),
        })
    }

    /// Forgets the holder record.
    pub fn clear_owner(&self, id: SlotId) {
        let _ = std::fs::remove_file(self.file(id, "owner"));
    }

    /// Asks the holder whose token is `token` to release `id`. A holder with another token ignores the request.
    pub fn request_release(&self, id: SlotId, token: &str) -> Result<(), SlotError> {
        let p = self.file(id, "release");
        std::fs::write(&p, token).map_err(io(&p))
    }

    /// Whether a release was asked for with exactly this token.
    pub fn release_requested(&self, id: SlotId, token: &str) -> bool {
        std::fs::read_to_string(self.file(id, "release")).is_ok_and(|t| t.trim() == token)
    }

    /// Removes the release request file, whoever wrote it (a new holder does this before it announces itself).
    pub fn clear_release(&self, id: SlotId) {
        let _ = std::fs::remove_file(self.file(id, "release"));
    }

    /// Removes the release request only if it carries `token` (a releaser withdrawing its own request).
    pub fn withdraw_release(&self, id: SlotId, token: &str) {
        if self.release_requested(id, token) {
            self.clear_release(id);
        }
    }
}

impl SlotLocks for FsLocks {
    type Guard = FsGuard;

    fn try_lock(&self, id: SlotId) -> Result<Option<FsGuard>, SlotError> {
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        let p = self.file(id, "lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&p)
            .map_err(io(&p))?;
        lock_outcome(file.try_lock(), file, &p)
    }
}

/// The guard when the lock was taken, `None` when somebody else holds it; any other failure is an error.
fn lock_outcome(
    result: Result<(), TryLockError>,
    file: File,
    path: &Path,
) -> Result<Option<FsGuard>, SlotError> {
    match result {
        Ok(()) => Ok(Some(FsGuard { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(io(path)(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let file = || File::create(dir.path().join("x.lock")).unwrap();
        let p = Path::new("x.lock");
        assert!(lock_outcome(Ok(()), file(), p).unwrap().is_some());
        assert!(
            lock_outcome(Err(TryLockError::WouldBlock), file(), p)
                .unwrap()
                .is_none()
        );
        let err = lock_outcome(
            Err(TryLockError::Error(std::io::Error::other("disk"))),
            file(),
            p,
        )
        .unwrap_err();
        assert!(err.to_string().contains("x.lock") && err.to_string().contains("disk"));
    }
}
