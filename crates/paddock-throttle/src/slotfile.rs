//! The OS adapter of the slot locks: a slot is held by an open file with an exclusive advisory lock (`flock` on macOS and Linux,
//! `LockFileEx` on Windows, both through `std::fs::File::try_lock`). The kernel frees the lock when the holder's last handle closes,
//! so a holder that exits or is killed frees its slot at once and nobody has to decide whether an owner is still alive.
//!
//! Rules this module keeps (each has a test):
//! - the lock belongs to the open file description: it is **not inherited** by a child (`std` opens every file close-on-exec on Unix and
//!   non-inheritable on Windows), so a command that outlives its `throttle` never keeps the slot;
//! - a held slot is never probed by taking it: [`is_held`] asks with a *shared* try-lock and lets go at once, so a probe never makes a
//!   real holder out of the prober, and two probes never exclude each other;
//! - nothing here reads or writes the holder's description: that is the sidecar file of [`crate::lock`] (a mandatory byte-range lock on
//!   Windows would also stop other handles from reading the locked file itself).

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// An exclusive lock on a slot file, held by this handle. Dropping it (or the process ending, however it ends) frees the slot.
#[derive(Debug)]
pub struct Held {
    file: File,
}

impl Held {
    /// Gives the slot back now (dropping does the same; this names the intent and the failure).
    pub fn release(self) -> io::Result<()> {
        self.file.unlock()
    }
}

/// Tries to take the slot file `path` (created when missing). `Ok(None)`: another handle holds it. `Err`: the file cannot be opened or
/// the file system does not do locks (the message of the OS).
pub fn take(path: &Path) -> io::Result<Option<Held>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Held { file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Whether some handle holds the slot file `path` now. A missing file is free. The probe takes a shared lock for an instant and
/// lets go, so it never changes who holds the slot (a taker that tries in that instant simply tries again at its next poll).
pub fn is_held(path: &Path) -> io::Result<bool> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            Ok(false)
        }
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_slot_is_held_and_free_again_when_dropped() {
        let d = tempfile::tempdir().expect("tempdir");
        let p = d.path().join("slot.lock");
        paddock_testkit::lock::await_free("a missing file is free", || is_held(&p).expect("probe"));
        let a = take(&p).expect("take").expect("free");
        paddock_testkit::lock::await_held("a taken slot is held", || is_held(&p).expect("probe"));
        assert!(
            take(&p).expect("take").is_none(),
            "a second handle is refused"
        );
        drop(a);
        paddock_testkit::lock::await_free("a dropped slot is free", || is_held(&p).expect("probe"));
        let b = take(&p).expect("take");
        assert!(b.is_some());
        b.expect("held").release().expect("release");
        paddock_testkit::lock::await_free("a released slot is free", || {
            is_held(&p).expect("probe")
        });
    }

    #[test]
    fn probes_do_not_exclude_each_other_or_become_holders() {
        let d = tempfile::tempdir().expect("tempdir");
        let p = d.path().join("slot.lock");
        std::fs::write(&p, b"").expect("file");
        paddock_testkit::lock::await_free("the first probe sees a free slot", || {
            is_held(&p).expect("probe")
        });
        paddock_testkit::lock::await_free("the second probe sees a free slot", || {
            is_held(&p).expect("probe")
        });
        assert!(
            take(&p).expect("take").is_some(),
            "probing left nothing behind"
        );
    }
}
