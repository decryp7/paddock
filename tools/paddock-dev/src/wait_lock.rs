//! One waiter per target for `dev wait` (ER-32's second instance: stacked waiters on one event held the machine's load).
//!
//! A waiter takes a small lock file per target under `<git-common-dir>/paddock/waits/`, holding its pid and start time. A
//! second waiter on the same target exits at once with the first one's pid and the next step. The file is removed when the
//! waiter ends; a file whose process is proven gone (pid and start time, rule 53) is taken over. A holder that cannot be
//! looked at counts as running, so a lock is never taken from a process that might still wait.

use paddock_proc::{Presence, ProcessId, StartTime};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the locks are and who asks (the seam the tests fake).
pub struct Locks<'a> {
    /// `<git-common-dir>/paddock/waits`.
    pub dir: PathBuf,
    /// This process.
    pub me: ProcessId,
    /// Whether the process a lock file names still runs.
    pub presence: &'a dyn Fn(ProcessId) -> Presence,
}

/// A held lock; dropping it removes the file (only if it still names this process).
#[derive(Debug)]
pub struct WaitLock {
    path: PathBuf,
    mine: String,
}

/// Why a lock was not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum LockError {
    /// Another waiter holds it (`pid` 0: a lock file that cannot be read yet, or a takeover in progress).
    Held {
        /// Its pid.
        pid: u32,
        /// The lock file, for the hint.
        path: PathBuf,
    },
    /// The lock folder or file could not be written.
    Io(String),
}

/// An empty lock file older than this was left by a crashed writer and is taken over.
pub const EMPTY_STALE: Duration = Duration::from_secs(5);

/// The lock file name for a target: a hash, so any target text is a safe file name.
pub fn file_name(target: &str) -> String {
    let h = Sha256::digest(target.as_bytes());
    let hex: String = h.iter().take(12).map(|b| format!("{b:02x}")).collect();
    format!("{hex}.lock")
}

fn parse(text: &str) -> Option<ProcessId> {
    let mut it = text.split_whitespace();
    let pid = it.next()?.parse().ok()?;
    let start = it.next()?.parse().ok()?;
    Some(ProcessId::new(pid, StartTime::from_micros(start)))
}

fn older_than(path: &Path, d: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > d)
}

/// Creates `path` with `content` in one atomic step (write a private file, then hard-link it to `path`), so a reader never sees
/// an empty or half-written lock and two creators never both succeed.
fn create_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let written = std::fs::write(&tmp, content);
    let linked = written.and_then(|_| std::fs::hard_link(&tmp, path));
    let _ = std::fs::remove_file(&tmp);
    linked
}

/// Takes the lock for `target`, or says who holds it.
///
/// A stale lock (its process proven gone, or an empty file older than [`EMPTY_STALE`]) is taken over under a second file,
/// `<lock>.take`, made with an exclusive create: of two waiters that both found the lock stale only one removes it, and the
/// other reports the lock as held, so a fresh lock is never removed by the loser.
pub fn acquire(locks: &Locks<'_>, target: &str) -> Result<WaitLock, LockError> {
    std::fs::create_dir_all(&locks.dir).map_err(|e| LockError::Io(format!("{}: {e}", locks.dir.display())))?;
    let path = locks.dir.join(file_name(target));
    let mine = format!("{} {}\n{target}\n", locks.me.pid(), locks.me.started().as_micros());
    let held = |pid| LockError::Held { pid, path: path.clone() };
    for _ in 0..2 {
        match create_atomic(&path, &mine) {
            Ok(()) => return Ok(WaitLock { path, mine }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(LockError::Io(format!("{}: {e}", path.display()))),
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        match parse(&text) {
            Some(id) if (locks.presence)(id) != Presence::Gone => return Err(held(id.pid())),
            // Nothing readable in it: a writer of an older version may be mid-write; after EMPTY_STALE it was left behind.
            None if !older_than(&path, EMPTY_STALE) => return Err(held(0)),
            _ => {}
        }
        // Stale: take it over under the takeover file.
        let take = path.with_extension("take");
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&take) {
            Ok(_) => {
                let again = std::fs::read_to_string(&path).unwrap_or_default();
                let still_stale = again == text;
                if still_stale {
                    let _ = std::fs::remove_file(&path);
                }
                let _ = std::fs::remove_file(&take);
                if !still_stale {
                    return Err(held(0));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if older_than(&take, EMPTY_STALE) {
                    let _ = std::fs::remove_file(&take);
                }
                return Err(held(0));
            }
            Err(e) => return Err(LockError::Io(format!("{}: {e}", take.display()))),
        }
    }
    Err(held(0))
}

impl Drop for WaitLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).is_ok_and(|t| t == self.mine) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// The lock folder of the repository the current folder is in, if any.
pub fn dir_for(common_dir: &Path) -> PathBuf {
    common_dir.join("paddock").join("waits")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me(pid: u32) -> ProcessId {
        ProcessId::new(pid, StartTime::from_micros(1_000 + pid as u64))
    }

    #[test]
    fn a_second_waiter_on_the_same_target_is_refused_with_the_first_ones_pid() {
        let d = tempfile::tempdir().unwrap();
        let running = |_: ProcessId| Presence::Running;
        let a = Locks {
            dir: d.path().into(),
            me: me(10),
            presence: &running,
        };
        let b = Locks {
            dir: d.path().into(),
            me: me(11),
            presence: &running,
        };
        let first = acquire(&a, "plock:x").unwrap();
        assert_eq!(
            acquire(&b, "plock:x").unwrap_err(),
            LockError::Held {
                pid: 10,
                path: d.path().join(file_name("plock:x"))
            }
        );
        assert!(acquire(&b, "plock:y").is_ok(), "another target is free");
        drop(first);
        assert!(acquire(&b, "plock:x").is_ok(), "released on exit");
    }

    #[test]
    fn a_lock_of_a_dead_process_is_taken_over_and_an_unknown_one_is_kept() {
        let d = tempfile::tempdir().unwrap();
        let gone = |_: ProcessId| Presence::Gone;
        let unknown = |_: ProcessId| Presence::Unknown;
        let a = Locks {
            dir: d.path().into(),
            me: me(10),
            presence: &gone,
        };
        std::mem::forget(acquire(&a, "t").unwrap());
        let b = Locks {
            dir: d.path().into(),
            me: me(11),
            presence: &unknown,
        };
        assert!(matches!(acquire(&b, "t").unwrap_err(), LockError::Held { pid: 10, .. }));
        let c = Locks {
            dir: d.path().into(),
            me: me(12),
            presence: &gone,
        };
        let l = acquire(&c, "t").unwrap();
        let text = std::fs::read_to_string(d.path().join(file_name("t"))).unwrap();
        assert!(text.starts_with("12 "), "{text}");
        drop(l);
        assert!(!d.path().join(file_name("t")).exists());
    }

    #[test]
    fn dropping_a_lock_never_removes_a_file_another_waiter_now_owns() {
        let d = tempfile::tempdir().unwrap();
        let running = |_: ProcessId| Presence::Running;
        let a = Locks {
            dir: d.path().into(),
            me: me(10),
            presence: &running,
        };
        let l = acquire(&a, "t").unwrap();
        std::fs::write(d.path().join(file_name("t")), "99 5\nt\n").unwrap();
        drop(l);
        assert!(d.path().join(file_name("t")).exists());
    }

    fn set_age(path: &std::path::Path, secs: u64) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(secs)).unwrap();
    }

    #[test]
    fn an_empty_lock_file_is_held_while_fresh_and_taken_over_after_five_seconds() {
        let d = tempfile::tempdir().unwrap();
        let running = |_: ProcessId| Presence::Running;
        let a = Locks {
            dir: d.path().into(),
            me: me(10),
            presence: &running,
        };
        let file = d.path().join(file_name("t"));
        std::fs::write(&file, "").unwrap();
        let e = acquire(&a, "t").unwrap_err();
        assert_eq!(
            e,
            LockError::Held {
                pid: 0,
                path: file.clone()
            },
            "the error carries the lock path"
        );
        set_age(&file, 30);
        let l = acquire(&a, "t").unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("10 "));
        drop(l);
    }

    #[test]
    fn a_taker_that_loses_the_takeover_race_reports_held_and_never_removes_the_lock() {
        let d = tempfile::tempdir().unwrap();
        let gone = |_: ProcessId| Presence::Gone;
        let a = Locks {
            dir: d.path().into(),
            me: me(10),
            presence: &gone,
        };
        std::mem::forget(acquire(&a, "t").unwrap());
        let file = d.path().join(file_name("t"));
        // Another taker is in the middle of its takeover: its takeover file exists.
        let take = file.with_extension("take");
        std::fs::write(&take, "").unwrap();
        let b = Locks {
            dir: d.path().into(),
            me: me(11),
            presence: &gone,
        };
        assert!(matches!(acquire(&b, "t").unwrap_err(), LockError::Held { .. }));
        assert!(file.exists(), "the loser removes nothing");
        // A takeover file left by a crashed taker ends after five seconds; the next try then takes the lock over.
        set_age(&take, 30);
        assert!(matches!(acquire(&b, "t").unwrap_err(), LockError::Held { .. }));
        assert!(acquire(&b, "t").is_ok());
    }

    #[test]
    fn a_failed_write_leaves_no_lock_file_behind() {
        let d = tempfile::tempdir().unwrap();
        let running = |_: ProcessId| Presence::Running;
        let a = Locks {
            dir: d.path().join("sub"),
            me: me(10),
            presence: &running,
        };
        std::fs::create_dir_all(&a.dir).unwrap();
        // A directory where the private file goes makes the write fail.
        let tmp = a.dir.join(file_name("t")).with_extension(format!("tmp{}", std::process::id()));
        std::fs::create_dir(&tmp).unwrap();
        assert!(matches!(acquire(&a, "t"), Err(LockError::Io(_))));
        assert!(!a.dir.join(file_name("t")).exists(), "no half-written lock");
    }
}
