//! One instance per data folder, decided by a held OS lock. The holder keeps an exclusive lock on a file for as long as
//! it runs (`File::try_lock`: `flock` on Unix, `LockFileEx` on Windows); the kernel drops the lock when the process ends,
//! however it ends, so a lock is never stale and nothing reclaims it. This is the authority for "is one running here?"
//! (rule 53: hold the thing itself, never trust a pid read earlier). A discovery file ([`crate::hint`]) only says how to
//! reach the holder.

use crate::start::Pace;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A refusal is reported as [`Claim::Held`] only after the lock was seen held on this many consecutive checks. Why: on
/// macOS (measured) any spawn, `posix_spawn` included, gives the child a copy of the descriptor table until it execs, so
/// the lock can look held for a moment after its holder let go. A real holder stays held on every check; a copy goes
/// away within moments (ADR 0031: judged on observations at a pace, not on one reading or on elapsed time).
pub const HELD_CHECKS: u32 = 5;

/// The pace between two checks of a held lock.
pub const HELD_TICK: Duration = Duration::from_millis(100);

/// Waits a tick on the thread (the product's pace; tests give their own).
struct ThreadPace;

impl Pace for ThreadPace {
    fn wait(&mut self, tick: Duration) -> bool {
        std::thread::sleep(tick);
        true
    }
}

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
    /// Takes the lock on `path` (created with its folder if missing, never written or truncated). A lock that looks held
    /// is checked [`HELD_CHECKS`] times, [`HELD_TICK`] apart, before it is reported held (so a refusal takes about
    /// `(HELD_CHECKS - 1) * HELD_TICK`); a free lock is taken at once.
    pub fn claim(path: &Path) -> Result<Claim, InstanceError> {
        Self::claim_paced(path, File::try_lock, &mut ThreadPace)
    }

    /// [`InstanceLock::claim`] with the locking call and the pace given (tests drive both).
    fn claim_paced(
        path: &Path,
        mut lock: impl FnMut(&File) -> Result<(), TryLockError>,
        pace: &mut dyn Pace,
    ) -> Result<Claim, InstanceError> {
        let mut seen = 0;
        loop {
            match Self::claim_once(path, &mut lock)? {
                Claim::Held => {
                    seen += 1;
                    if seen >= HELD_CHECKS || !pace.wait(HELD_TICK) {
                        return Ok(Claim::Held);
                    }
                }
                other => return Ok(other),
            }
        }
    }

    /// One look at the lock.
    fn claim_once(
        path: &Path,
        lock: &mut impl FnMut(&File) -> Result<(), TryLockError>,
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

/// Whether some process holds the lock on `path`, settled: held on every one of [`HELD_CHECKS`] checks, so a held lock
/// costs about `(HELD_CHECKS - 1) * HELD_TICK` of waiting on the thread. A free lock is taken and dropped at once to find
/// out, so an instance starting at that very instant may be refused (it exits; the caller finds one on its next pass).
/// Anything that is not a clear "held" (unsupported, an error) is `false`.
///
/// Which check to use: for a one-shot decision on a reading (act now or not), use [`InstanceLock::claim`] or this settled
/// check. Never call it inside a loop that looks again at its own pace: the wait slows every look (CI run 7257); that loop
/// uses [`is_held_now`].
pub fn is_held_settled(path: &Path) -> bool {
    matches!(InstanceLock::claim(path), Ok(Claim::Held))
}

/// Whether some process holds the lock on `path` at this one check: no re-check and no wait, so it answers at once. A held
/// reading may be only a spawning child's copy of the descriptor (see [`HELD_CHECKS`]), so a free lock can read held for a
/// moment. A free lock is taken and dropped at once, as in [`is_held_settled`]; anything that is not a clear "held" is
/// `false`.
///
/// Which check to use: only where a held reading leads to waiting and looking again (a supervisor judging the lock over
/// its own repeated looks at its own pace), so a false "held" only delays; never where a held reading leads to a one-shot
/// action. For a one-shot decision use [`InstanceLock::claim`] or [`is_held_settled`].
pub fn is_held_now(path: &Path) -> bool {
    held_at_one_check(path, File::try_lock)
}

/// [`is_held_now`] with the locking call given.
fn held_at_one_check(path: &Path, mut lock: impl FnMut(&File) -> Result<(), TryLockError>) -> bool {
    matches!(InstanceLock::claim_once(path, &mut lock), Ok(Claim::Held))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pace that counts its waits and says "cancelled" once it has waited `limit` times.
    struct Ticks {
        waited: usize,
        limit: usize,
    }

    impl Ticks {
        fn new(limit: usize) -> Ticks {
            Ticks { waited: 0, limit }
        }
    }

    impl Pace for Ticks {
        fn wait(&mut self, tick: Duration) -> bool {
            assert_eq!(tick, HELD_TICK);
            self.waited += 1;
            self.waited < self.limit
        }
    }

    /// Answers "held" `n` times, then "free".
    fn held_times(n: usize) -> impl FnMut(&File) -> Result<(), TryLockError> {
        let mut left = n;
        move |_| {
            if left == 0 {
                Ok(())
            } else {
                left -= 1;
                Err(TryLockError::WouldBlock)
            }
        }
    }

    #[test]
    fn a_lock_that_looks_held_for_a_moment_is_taken_when_it_clears() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.lock");
        let mut pace = Ticks::new(usize::MAX);
        let got = InstanceLock::claim_paced(&path, held_times(HELD_CHECKS as usize - 1), &mut pace);
        assert!(matches!(got, Ok(Claim::Acquired(_))));
        assert_eq!(
            pace.waited,
            HELD_CHECKS as usize - 1,
            "one tick between two checks"
        );
    }

    #[test]
    fn a_lock_held_on_every_check_is_held() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.lock");
        let mut pace = Ticks::new(usize::MAX);
        let got = InstanceLock::claim_paced(&path, held_times(usize::MAX), &mut pace);
        assert!(matches!(got, Ok(Claim::Held)));
        assert_eq!(pace.waited, HELD_CHECKS as usize - 1);
    }

    /// Regression (CI run 7257): Dyno's supervisor probed the lock with the settled check on every look, so a held lock cost each
    /// look the re-check's wait (about 400 ms) and the supervisor's looks stopped following its pace. The one-check probe
    /// asks the lock once and never waits.
    #[test]
    fn the_one_check_probe_asks_the_lock_once_and_never_waits() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.lock");
        let mut calls = 0;
        let mut answer = held_times(usize::MAX);
        assert!(held_at_one_check(&path, |f: &File| {
            calls += 1;
            answer(f)
        }));
        assert_eq!(calls, 1, "a held lock was checked again");
        let mut calls = 0;
        assert!(!held_at_one_check(&path, |_: &File| {
            calls += 1;
            Ok(())
        }));
        assert_eq!(calls, 1);
        assert!(
            !held_at_one_check(&path, |_: &File| Err(TryLockError::Error(
                std::io::Error::from(std::io::ErrorKind::Unsupported)
            ))),
            "unsupported is not a holder"
        );
        let held = InstanceLock::claim(&path).unwrap();
        assert!(is_held_now(&path), "a real holder is held at one check");
        drop(held);
    }

    #[test]
    fn a_cancelled_wait_reports_what_was_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.lock");
        let mut pace = Ticks::new(1);
        let got = InstanceLock::claim_paced(&path, held_times(usize::MAX), &mut pace);
        assert!(matches!(got, Ok(Claim::Held)));
        assert_eq!(pace.waited, 1);
    }

    /// Regression (CI run 7230): a spawn gives the child a copy of the descriptor table until it execs (macOS, `posix_spawn`
    /// too), so a lock just released looked held to the next claim. One thread spawns programs in a loop while another
    /// takes and drops the lock: every claim must succeed, and a real holder stays held (see the tests above).
    #[test]
    fn a_lock_just_released_is_free_while_programs_are_being_spawned() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        /// Overlap that must have happened before the test may end, on a machine of any speed.
        const MIN_SPAWNS: usize = 50;
        const MIN_ROUNDS: usize = 200;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.lock");
        let stop = Arc::new(AtomicBool::new(false));
        let spawned = Arc::new(AtomicUsize::new(0));
        let spawner = {
            let (stop, spawned) = (stop.clone(), spawned.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let _ = crate::command("true").status();
                    spawned.fetch_add(1, Ordering::SeqCst);
                }
            })
        };
        // Takes and drops the lock until both sides did enough, so the two overlap for a minimum number of rounds.
        let mut round = 0;
        while round < MIN_ROUNDS || spawned.load(Ordering::SeqCst) < MIN_SPAWNS {
            round += 1;
            match InstanceLock::claim(&path).unwrap() {
                Claim::Acquired(lock) => drop(lock),
                other => panic!("round {round}: a released lock was reported {other:?}"),
            }
        }
        stop.store(true, Ordering::SeqCst);
        spawner.join().unwrap();
    }

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
        until(|| !is_held_now(&path));
        let held = std::cell::RefCell::new(None);
        until(|| {
            *held.borrow_mut() = match InstanceLock::claim(&path) {
                Ok(Claim::Acquired(l)) => Some(l),
                _ => None,
            };
            held.borrow().is_some()
        });
        let first = held.take();
        assert!(is_held_settled(&path));
        assert!(matches!(InstanceLock::claim(&path).unwrap(), Claim::Held));
        let other = tmp.path().join("b.lock");
        until(|| matches!(InstanceLock::claim(&other), Ok(Claim::Acquired(_))));
        drop(first);
        until(|| !is_held_now(&path));
        until(|| matches!(InstanceLock::claim(&path), Ok(Claim::Acquired(_))));
    }

    #[test]
    fn a_claim_follows_what_the_lock_call_answers() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.lock");
        let kind = |k| Err(TryLockError::Error(std::io::Error::from(k)));
        assert!(matches!(
            InstanceLock::claim_paced(&path, |_| Ok(()), &mut Ticks::new(usize::MAX)),
            Ok(Claim::Acquired(_))
        ));
        assert!(matches!(
            InstanceLock::claim_paced(
                &path,
                |_| Err(TryLockError::WouldBlock),
                &mut Ticks::new(usize::MAX)
            ),
            Ok(Claim::Held)
        ));
        assert!(matches!(
            InstanceLock::claim_paced(
                &path,
                |_| kind(std::io::ErrorKind::Unsupported),
                &mut Ticks::new(usize::MAX)
            ),
            Ok(Claim::Unsupported)
        ));
        let e = InstanceLock::claim_paced(
            &path,
            |_| kind(std::io::ErrorKind::PermissionDenied),
            &mut Ticks::new(usize::MAX),
        )
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
        assert!(
            !is_held_settled(&file.join("a.lock")),
            "an error is not a holder"
        );
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
