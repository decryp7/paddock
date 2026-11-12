use crate::{FsLocks, OwnerInfo, SlotError, SlotId, SlotLocks};
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
    /// The holder reached its lifetime cap (a job that never ended and never released).
    Expired,
}

/// What a holder watches. Injected so the loop is testable without files or sleeping.
pub trait HoldProbe {
    /// A release was requested.
    fn release_requested(&mut self) -> bool;
    /// The job's folder still exists.
    fn owner_present(&mut self) -> bool;
    /// The holder has lived as long as it may.
    fn expired(&mut self) -> bool;
    /// Wait until the next look.
    fn pause(&mut self);
}

/// Waits until the owner is gone, a release is asked for (release wins when both hold) or the lifetime cap is reached.
pub fn hold_until(probe: &mut impl HoldProbe) -> HoldEnd {
    loop {
        if probe.release_requested() {
            return HoldEnd::Released;
        }
        if !probe.owner_present() {
            return HoldEnd::OwnerGone;
        }
        if probe.expired() {
            return HoldEnd::Expired;
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
    until: Option<Instant>,
}

impl HoldProbe for FsProbe<'_> {
    fn release_requested(&mut self) -> bool {
        self.locks.release_requested(self.id, self.token)
    }
    fn owner_present(&mut self) -> bool {
        self.owner.exists()
    }
    fn expired(&mut self) -> bool {
        self.until.is_some_and(|t| Instant::now() >= t)
    }
    fn pause(&mut self) {
        std::thread::sleep(self.poll);
    }
}

/// How long a holder keeps trying when every slot looks busy. A reader (a status probe) takes a lock only for an instant, but on
/// a loaded machine the instant can last seconds; a full house that stays full is refused after this wait.
pub const ACQUIRE_GRACE: Duration = Duration::from_secs(5);
const ACQUIRE_PAUSE: Duration = Duration::from_millis(10);

/// [`crate::acquire_first`] repeated until `grace` has passed: the first free slot, or `None` when every slot stayed busy for the
/// whole wait. It tries once even with a zero wait and returns at once on an error.
pub fn acquire_within<L: SlotLocks>(
    locks: &L,
    max: u32,
    grace: Duration,
    pause: Duration,
) -> Result<Option<(SlotId, L::Guard)>, SlotError> {
    let deadline = Instant::now() + grace;
    loop {
        if let Some(got) = crate::acquire_first(locks, max)? {
            return Ok(Some(got));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(pause);
    }
}

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

/// The holder process's whole job: take the first free slot of `max` (retrying for [`ACQUIRE_GRACE`], see [`acquire_within`]), write
/// [`Announcement`] to `announce`, then keep the slot until `owner` disappears or a release with the holder's token is
/// requested, or `max_hold` has passed (`None`: no cap), and let go. If the process is killed, the kernel releases the lock; the owner and release files left behind
/// are only information and are overwritten by the next holder.
pub fn hold(
    locks: &FsLocks,
    max: u32,
    owner: &Path,
    poll: Duration,
    max_hold: Option<Duration>,
    announce: &mut dyn Write,
) -> Result<Option<(SlotId, HoldEnd)>, SlotError> {
    let Some((id, guard)) = acquire_within(locks, max, ACQUIRE_GRACE, ACQUIRE_PAUSE)? else {
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
        until: max_hold.map(|d| Instant::now() + d),
    });
    locks.clear_owner(id);
    locks.withdraw_release(id, &token);
    drop(guard);
    Ok(Some((id, end)))
}

/// The release protocol's side of a lock store: who holds a slot and the request channel to the holder. A port, so
/// [`release`] can be tested with a store whose locks fail on demand.
pub trait ReleaseLocks: SlotLocks {
    /// Who holds the slot, if its record is readable.
    fn read_owner(&self, id: SlotId) -> Option<OwnerInfo>;
    /// Asks the holder with `token` to let go.
    fn request_release(&self, id: SlotId, token: &str) -> Result<(), SlotError>;
    /// Takes back a request carrying `token`.
    fn withdraw_release(&self, id: SlotId, token: &str);
}

impl ReleaseLocks for FsLocks {
    fn read_owner(&self, id: SlotId) -> Option<OwnerInfo> {
        FsLocks::read_owner(self, id)
    }
    fn request_release(&self, id: SlotId, token: &str) -> Result<(), SlotError> {
        FsLocks::request_release(self, id, token)
    }
    fn withdraw_release(&self, id: SlotId, token: &str) {
        FsLocks::withdraw_release(self, id, token);
    }
}

/// Waits (up to `timeout`) until slot `id` can be locked, that is, its holder is gone. Returns whether it is free.
pub fn wait_free(
    locks: &impl SlotLocks,
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
    locks: &impl ReleaseLocks,
    id: SlotId,
    token: &str,
    timeout: Duration,
    poll: Duration,
) -> Result<(), SlotError> {
    let ours = || locks.read_owner(id).is_some_and(|o| o.token == token);
    if wait_free(locks, id, Duration::ZERO, poll)? || !ours() {
        return Ok(());
    }
    locks.request_release(id, token)?;
    let deadline = Instant::now() + timeout;
    loop {
        if wait_free(locks, id, Duration::ZERO, poll)? {
            locks.withdraw_release(id, token);
            return Ok(());
        }
        if !ours() {
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

    /// A status probe that stalls (a loaded machine) while it holds the slot's lock must not make an acquirer refuse: the
    /// acquirer keeps trying within its wait (1 run in 25 at load 85 was refused with "All 1 slots are busy").
    #[test]
    fn a_probe_that_holds_the_lock_for_a_while_does_not_make_acquire_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let locks = FsLocks::new(dir.path());
        let probe = locks.try_lock(SlotId::new(1).unwrap()).unwrap().unwrap();
        let owner = dir.path().join("job");
        std::fs::create_dir(&owner).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(probe);
            let _ = tx.send(());
        });
        let mut announced = Vec::new();
        let got = hold(
            &locks,
            1,
            &owner,
            Duration::from_millis(5),
            Some(Duration::ZERO),
            &mut announced,
        )
        .unwrap();
        assert!(
            got.is_some(),
            "refused: {}",
            String::from_utf8_lossy(&announced)
        );
        rx.recv().unwrap();
    }

    /// A full house that stays full is still refused, after the wait.
    #[test]
    fn a_full_house_is_refused_after_the_wait() {
        let dir = tempfile::tempdir().unwrap();
        let locks = FsLocks::new(dir.path());
        let _held = locks.try_lock(SlotId::new(1).unwrap()).unwrap().unwrap();
        let started = Instant::now();
        let got = acquire_within(
            &locks,
            1,
            Duration::from_millis(150),
            Duration::from_millis(5),
        )
        .unwrap();
        assert!(got.is_none() && started.elapsed() >= Duration::from_millis(150));
    }

    struct Script {
        release_after: Option<u32>,
        owner_gone_after: Option<u32>,
        expires_after: Option<u32>,
        looks: u32,
    }

    impl HoldProbe for Script {
        fn release_requested(&mut self) -> bool {
            self.release_after.is_some_and(|n| self.looks >= n)
        }
        fn owner_present(&mut self) -> bool {
            !self.owner_gone_after.is_some_and(|n| self.looks >= n)
        }
        fn expired(&mut self) -> bool {
            self.expires_after.is_some_and(|n| self.looks >= n)
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
            expires_after: None,
            looks: 0,
        };
        assert_eq!(hold_until(&mut s(Some(2), None)), HoldEnd::Released);
        assert_eq!(hold_until(&mut s(None, Some(3))), HoldEnd::OwnerGone);
        assert_eq!(hold_until(&mut s(Some(1), Some(1))), HoldEnd::Released);
        let mut capped = s(None, None);
        capped.expires_after = Some(4);
        assert_eq!(hold_until(&mut capped), HoldEnd::Expired);
        assert_eq!(capped.looks, 4);
    }
}
