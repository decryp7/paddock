use crate::{FsLocks, OwnerInfo, SlotError, SlotId, SlotLocks, acquire_retry};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    token: &'a str,
    owner: &'a Path,
    poll: Duration,
}

impl HoldProbe for FsProbe<'_> {
    fn release_requested(&mut self) -> bool {
        self.locks.release_requested(self.id, self.token)
    }
    fn owner_present(&mut self) -> bool {
        self.owner.exists()
    }
    fn pause(&mut self) {
        std::thread::sleep(self.poll);
    }
}

/// How many times a holder tries all slots when they all look busy (readers take a lock for an instant).
pub const ACQUIRE_ATTEMPTS: u32 = 5;
const ACQUIRE_PAUSE: Duration = Duration::from_millis(10);

/// A token no other holder has had: process id, clock and a counter, in hex.
pub fn fresh_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{:x}-{nanos:x}-{:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// What a holder tells its starter on one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Announcement {
    /// It holds this slot; release it with this token.
    Slot(SlotId, String),
    /// Every slot was busy.
    None,
}

impl Announcement {
    /// The line written to the starter: `sN <token>` or `none`.
    pub fn line(&self) -> String {
        match self {
            Announcement::Slot(id, token) => format!("{id} {token}\n"),
            Announcement::None => "none\n".to_string(),
        }
    }

    /// Reads a line written by [`Announcement::line`].
    pub fn parse(line: &str) -> Result<Announcement, SlotError> {
        let line = line.trim();
        if line == "none" {
            return Ok(Announcement::None);
        }
        match line.split_once(' ') {
            Some((id, token)) if !token.is_empty() && !token.contains(char::is_whitespace) => {
                Ok(Announcement::Slot(id.parse()?, token.to_string()))
            }
            _ => Err(SlotError::Invalid(line.to_string())),
        }
    }
}

/// The holder process's whole job: take the first free slot of `max` (retrying briefly, see [`acquire_retry`]), write
/// [`Announcement`] to `announce`, then keep the slot until `owner` disappears or a release with the holder's token is
/// requested, and let go. If the process is killed, the kernel releases the lock; the owner and release files left behind
/// are only information and are overwritten by the next holder.
pub fn hold(
    locks: &FsLocks,
    max: u32,
    owner: &Path,
    poll: Duration,
    announce: &mut dyn Write,
) -> Result<Option<(SlotId, HoldEnd)>, SlotError> {
    let Some((id, guard)) = acquire_retry(locks, max, ACQUIRE_ATTEMPTS, || {
        std::thread::sleep(ACQUIRE_PAUSE)
    })?
    else {
        let _ = announce
            .write_all(Announcement::None.line().as_bytes())
            .and_then(|()| announce.flush());
        return Ok(None);
    };
    let token = fresh_token();
    locks.clear_release(id);
    locks.write_owner(
        id,
        &OwnerInfo {
            owner: owner.to_path_buf(),
            pid: std::process::id(),
            token: token.clone(),
        },
    )?;
    // The parent may have gone already; the hold does not depend on it.
    let _ = announce
        .write_all(Announcement::Slot(id, token.clone()).line().as_bytes())
        .and_then(|()| announce.flush());
    let end = hold_until(&mut FsProbe {
        locks,
        id,
        token: &token,
        owner,
        poll,
    });
    locks.clear_owner(id);
    locks.withdraw_release(id, &token);
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

/// Lets go of the slot held under `token` and waits until it is free. It returns at once when the slot is free, and also
/// when the slot is now held by somebody else (another token): that holder is not the caller's to release.
pub fn release(
    locks: &FsLocks,
    id: SlotId,
    token: &str,
    timeout: Duration,
    poll: Duration,
) -> Result<(), SlotError> {
    let ours = |locks: &FsLocks| locks.read_owner(id).is_some_and(|o| o.token == token);
    if wait_free(locks, id, Duration::ZERO, poll)? || !ours(locks) {
        return Ok(());
    }
    locks.request_release(id, token)?;
    let deadline = Instant::now() + timeout;
    loop {
        if wait_free(locks, id, Duration::ZERO, poll)? {
            locks.withdraw_release(id, token);
            return Ok(());
        }
        if !ours(locks) {
            // Our holder ended by itself and the slot was taken again meanwhile: the request is stale.
            locks.withdraw_release(id, token);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(SlotError::ReleaseTimeout(id.to_string()));
        }
        std::thread::sleep(poll);
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
