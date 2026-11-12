use crate::{SlotError, SlotId, SlotLocks};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// The slots over lock files in `<root>/slots/`. `sN.lock` carries the lock (never read or written, because Windows
/// locks block other handles' reads), `sN.owner` says who holds it, `sN.release` asks the holder to let go.
#[derive(Debug, Clone)]
pub struct FsLocks {
    dir: PathBuf,
    /// How long a holder waits to mark its slot live (see `try_lock`).
    live_grace: std::time::Duration,
}

/// A held slot: dropping it closes the files and so releases both locks.
#[derive(Debug)]
pub struct FsGuard {
    _file: File,
    /// `sN.live`, locked exclusively for as long as the slot is held: what a status probe looks at.
    _live: File,
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
            live_grace: crate::hold::ACQUIRE_GRACE,
        }
    }

    /// With another wait for a holder to mark its slot live (tests make it nothing).
    pub fn with_live_grace(mut self, grace: std::time::Duration) -> FsLocks {
        self.live_grace = grace;
        self
    }

    fn open(&self, path: &Path) -> Result<File, SlotError> {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(io(path))
    }

    /// A status probe: `Some(file)` when no holder has `sN.live`, with a shared lock on it that ends when the file is dropped;
    /// `None` when a holder has it. It never touches `sN.lock`, the lock acquirers contend on, so a probe cannot make an acquirer
    /// skip a slot (a probe that stalls on a loaded machine got `ci slot acquire` refused, 1 run in 25 at load 85). A holder that
    /// is between taking `sN.lock` and `sN.live` looks free for that instant; it is a status, not an arbiter.
    fn live_probe(&self, id: SlotId) -> Result<Option<File>, SlotError> {
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        let p = self.file(id, "live");
        let file = self.open(&p)?;
        match file.try_lock_shared() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(io(&p)(e)),
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
        let Some(lock) = lock_outcome(file.try_lock(), file, &p)? else {
            return Ok(None);
        };
        // The holder has the slot; it now says so on `sN.live`. A probe holds that file for an instant only, but flock and
        // LockFileEx let new shared locks in ahead of a waiting exclusive one, so a stream of probes could starve a blocking
        // `lock()` for good (ER-32): the holder tries for at most ACQUIRE_GRACE and then says so.
        let live_path = self.file(id, "live");
        let live = self.open(&live_path)?;
        let got = lock_within(self.live_grace, LIVE_PAUSE, || match live.try_lock() {
            Ok(()) => Ok(true),
            Err(TryLockError::WouldBlock) => Ok(false),
            Err(TryLockError::Error(e)) => Err(e),
        })
        .map_err(io(&live_path))?;
        if !got {
            return Err(SlotError::LiveBusy {
                slot: id.to_string(),
                waited_ms: u64::try_from(self.live_grace.as_millis()).unwrap_or(u64::MAX),
            });
        }
        Ok(Some(FsGuard {
            _file: lock.into_file(),
            _live: live,
        }))
    }

    fn is_held(&self, id: SlotId) -> Result<bool, SlotError> {
        Ok(self.live_probe(id)?.is_none())
    }
}

const LIVE_PAUSE: std::time::Duration = std::time::Duration::from_millis(5);

/// Calls `attempt` (`Ok(true)`: it took the lock) until it succeeds or `grace` has passed: `Ok(false)` then. It tries once even
/// with no grace, and an error ends it at once. The pause between tries is a short sleep.
fn lock_within(
    grace: std::time::Duration,
    pause: std::time::Duration,
    mut attempt: impl FnMut() -> Result<bool, std::io::Error>,
) -> Result<bool, std::io::Error> {
    let deadline = std::time::Instant::now() + grace;
    loop {
        if attempt()? {
            return Ok(true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(pause);
    }
}

/// The file whose lock was taken.
#[derive(Debug)]
struct Locked(File);

impl Locked {
    fn into_file(self) -> File {
        self.0
    }
}

/// The lock when it was taken, `None` when somebody else holds it; any other failure is an error.
fn lock_outcome(
    result: Result<(), TryLockError>,
    file: File,
    path: &Path,
) -> Result<Option<Locked>, SlotError> {
    match result {
        Ok(()) => Ok(Some(Locked(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(io(path)(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A stream of probes that never lets the holder's lock through must end in an answer, not a wait with no end: the fake lock
    /// is busy forever and the grace is nothing, so the holder gives up after exactly one try. (A blocking `lock()` hangs here.)
    #[test]
    fn a_live_file_that_stays_locked_ends_the_holders_wait() {
        let mut tries = 0;
        let got = lock_within(Duration::ZERO, Duration::ZERO, || {
            tries += 1;
            Ok(false)
        })
        .unwrap();
        assert!(!got);
        assert_eq!(tries, 1);
        // A lock that frees after 50 tries is taken within a long grace, with no sleep in the test.
        let mut left = 50;
        let got = lock_within(Duration::from_secs(3600), Duration::ZERO, || {
            left -= 1;
            Ok(left < 0)
        })
        .unwrap();
        assert!(got);
        // An error ends it at once.
        assert!(
            lock_within(Duration::from_secs(3600), Duration::ZERO, || Err(
                std::io::Error::other("disk")
            ))
            .is_err()
        );
    }

    /// The real files: probes that keep `sN.live` locked make the holder's take end in an error that says so, instead of waiting
    /// for ever (a blocking `lock()` never returns here), and the slot is free again for the next try.
    #[test]
    fn probes_that_keep_the_live_file_locked_end_a_holders_take_with_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let locks = FsLocks::new(dir.path()).with_live_grace(Duration::ZERO);
        let s1 = SlotId::new(1).unwrap();
        let _probe = locks
            .live_probe(s1)
            .unwrap()
            .expect("a free slot looks free");
        let err = locks.try_lock(s1).unwrap_err();
        assert!(matches!(err, SlotError::LiveBusy { .. }), "{err}");
        drop(_probe);
        assert!(
            locks.try_lock(s1).unwrap().is_some(),
            "the slot was not left locked by the failed take"
        );
    }

    /// A status probe in flight (it has `sN.live` locked shared) leaves the lock acquirers contend on alone: no sleep, no timing.
    #[test]
    fn a_probe_in_flight_does_not_touch_the_lock_acquirers_take() {
        let dir = tempfile::tempdir().unwrap();
        let locks = FsLocks::new(dir.path());
        let s1 = SlotId::new(1).unwrap();
        let _probe = locks
            .live_probe(s1)
            .unwrap()
            .expect("a free slot looks free");
        let lock = locks.open(&locks.file(s1, "lock")).unwrap();
        assert!(lock.try_lock().is_ok(), "the probe did not take sN.lock");
    }

    #[test]
    fn a_held_slot_looks_held_to_a_probe_and_a_released_one_free() {
        let dir = tempfile::tempdir().unwrap();
        let locks = FsLocks::new(dir.path());
        let s1 = SlotId::new(1).unwrap();
        assert!(!locks.is_held(s1).unwrap());
        let guard = locks.try_lock(s1).unwrap().unwrap();
        assert!(locks.is_held(s1).unwrap());
        assert!(
            locks.try_lock(s1).unwrap().is_none(),
            "a second acquirer is refused by sN.lock"
        );
        drop(guard);
        assert!(!locks.is_held(s1).unwrap());
    }

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
