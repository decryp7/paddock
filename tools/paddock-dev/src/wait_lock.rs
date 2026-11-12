//! One waiter per target for `dev wait` (ER-32's second instance: stacked waiters on one event held the machine's load).
//!
//! A waiter takes a small lock file per target under `<git-common-dir>/paddock/waits/`, holding its pid and start time. A
//! second waiter on the same target exits at once with the first one's pid and the next step. The file is removed when the
//! waiter ends; a file whose process is proven gone (pid and start time, rule 53) is taken over. A holder that cannot be
//! looked at counts as running, so a lock is never taken from a process that might still wait.

use paddock_proc::{Presence, ProcessId, StartTime};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

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
    /// Another waiter holds it.
    Held {
        /// Its pid.
        pid: u32,
    },
    /// The lock folder or file could not be written.
    Io(String),
}

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

/// Takes the lock for `target`, or says who holds it.
pub fn acquire(locks: &Locks<'_>, target: &str) -> Result<WaitLock, LockError> {
    std::fs::create_dir_all(&locks.dir).map_err(|e| LockError::Io(format!("{}: {e}", locks.dir.display())))?;
    let path = locks.dir.join(file_name(target));
    let mine = format!("{} {}\n{target}\n", locks.me.pid(), locks.me.started().as_micros());
    // Two tries: the second after taking over a stale file.
    for _ in 0..2 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(mine.as_bytes())
                    .map_err(|e| LockError::Io(format!("{}: {e}", path.display())))?;
                return Ok(WaitLock { path, mine });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                match parse(&text) {
                    Some(id) if (locks.presence)(id) != Presence::Gone => return Err(LockError::Held { pid: id.pid() }),
                    // Unreadable (a writer is mid-write) counts as held; a parsed record of a gone process is stale.
                    None if text.is_empty() => return Err(LockError::Held { pid: 0 }),
                    _ => {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            Err(e) => return Err(LockError::Io(format!("{}: {e}", path.display()))),
        }
    }
    Err(LockError::Held { pid: 0 })
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
        assert_eq!(acquire(&b, "plock:x").unwrap_err(), LockError::Held { pid: 10 });
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
        assert_eq!(acquire(&b, "t").unwrap_err(), LockError::Held { pid: 10 });
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
}
