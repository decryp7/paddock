use crate::{FsLocks, OwnerInfo, SlotError, SlotId, SlotLocks, acquire_first};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

/// Why a hold ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldEnd {
    /// A release was requested.
    Released,
    /// The owner folder is gone: the job ended.
    OwnerGone,
}

/// What a holder watches. Injected so the loop is testable without files or sleeping.
pub trait HoldProbe {
    /// A release was requested.
    fn release_requested(&mut self) -> bool;
    /// The job's folder still exists.
    fn owner_present(&mut self) -> bool;
    /// Wait until the next look.
    fn pause(&mut self);
}

/// Waits until the owner is gone or a release is asked for (release wins when both hold).
pub fn hold_until(probe: &mut impl HoldProbe) -> HoldEnd {
    loop {
        if probe.release_requested() {
            return HoldEnd::Released;
        }
        if !probe.owner_present() {
            return HoldEnd::OwnerGone;
        }
        probe.pause();
    }
}

struct FsProbe<'a> {
    locks: &'a FsLocks,
    id: SlotId,
    owner: &'a Path,
    poll: Duration,
}

impl HoldProbe for FsProbe<'_> {
    fn release_requested(&mut self) -> bool {
        self.locks.release_requested(self.id)
    }
    fn owner_present(&mut self) -> bool {
        self.owner.exists()
    }
    fn pause(&mut self) {
        std::thread::sleep(self.poll);
    }
}

/// The holder process's whole job: take the first free slot of `max`, write its name and a newline to `announce` (or
/// `none` when all are busy, returning `Ok(None)`), then keep it until `owner` disappears or a release is requested, and
/// let go. If the process is killed, the kernel releases the lock; the owner and release files left behind are only
/// information and are overwritten by the next holder.
pub fn hold(
    locks: &FsLocks,
    max: u32,
    owner: &Path,
    poll: Duration,
    mut announce: impl Write,
) -> Result<Option<(SlotId, HoldEnd)>, SlotError> {
    let Some((id, guard)) = acquire_first(locks, max)? else {
        let _ = announce
            .write_all(b"none\n")
            .and_then(|()| announce.flush());
        return Ok(None);
    };
    locks.clear_release(id);
    locks.write_owner(
        id,
        &OwnerInfo {
            owner: owner.to_path_buf(),
            pid: std::process::id(),
        },
    )?;
    // The parent may have gone already; the hold does not depend on it.
    let _ = announce
        .write_all(format!("{id}\n").as_bytes())
        .and_then(|()| announce.flush());
    drop(announce);
    let end = hold_until(&mut FsProbe {
        locks,
        id,
        owner,
        poll,
    });
    locks.clear_owner(id);
    locks.clear_release(id);
    drop(guard);
    Ok(Some((id, end)))
}

/// Waits (up to `timeout`) until slot `id` can be locked, that is, its holder is gone. Returns whether it is free.
pub fn wait_free(
    locks: &FsLocks,
    id: SlotId,
    timeout: Duration,
    poll: Duration,
) -> Result<bool, SlotError> {
    let deadline = Instant::now() + timeout;
    loop {
        if locks.try_lock(id)?.is_some() {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(poll);
    }
}

/// Asks the holder of `id` to let go and waits until it has. Releasing a free slot is fine and returns at once.
pub fn release(
    locks: &FsLocks,
    id: SlotId,
    timeout: Duration,
    poll: Duration,
) -> Result<(), SlotError> {
    if wait_free(locks, id, Duration::ZERO, poll)? {
        return Ok(());
    }
    locks.request_release(id)?;
    if wait_free(locks, id, timeout, poll)? {
        locks.clear_release(id);
        Ok(())
    } else {
        Err(SlotError::ReleaseTimeout(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Script {
        release_after: Option<u32>,
        owner_gone_after: Option<u32>,
        looks: u32,
    }

    impl HoldProbe for Script {
        fn release_requested(&mut self) -> bool {
            self.release_after.is_some_and(|n| self.looks >= n)
        }
        fn owner_present(&mut self) -> bool {
            !self.owner_gone_after.is_some_and(|n| self.looks >= n)
        }
        fn pause(&mut self) {
            self.looks += 1;
        }
    }

    #[test]
    fn the_loop_ends_on_release_or_owner_and_release_wins_a_tie() {
        let s = |r, o| Script {
            release_after: r,
            owner_gone_after: o,
            looks: 0,
        };
        assert_eq!(hold_until(&mut s(Some(2), None)), HoldEnd::Released);
        assert_eq!(hold_until(&mut s(None, Some(3))), HoldEnd::OwnerGone);
        assert_eq!(hold_until(&mut s(Some(1), Some(1))), HoldEnd::Released);
    }
}
