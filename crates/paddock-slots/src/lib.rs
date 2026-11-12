//! Numbered slots over operating-system file locks (ADR 0022, the CI kit's job isolation).
//!
//! A *slot* is `s1` to `sN`. Taking one means holding an exclusive lock on `<root>/slots/sN.lock` (`File::try_lock`:
//! `flock` on Unix, `LockFileEx` on Windows). The kernel drops the lock when the holder's process ends, however it ends,
//! so there is no stale lock to reclaim and therefore no reclaim race: two holders of one slot cannot exist.
//!
//! - [`SlotId`]: the typed `sN` name.
//! - [`SlotLocks`] (a port) and [`acquire_first`] (pure): take the first free slot; [`slot_states`] reads which are busy;
//!   [`with_idle`] runs work on an idle slot while holding its lock (deleting its kept folders without a check-then-act race).
//! - [`acquire_within`]: [`acquire_first`] repeated for a grace period when every slot looked busy (only a releaser's or an idle
//!   sweep's instant of holding the lock is left to wait out: a status probe never takes it).
//! - Tokens: a holder has a [`fresh_token`]; `release` presents it, and a holder honours only its own, so a slow release can
//!   never free the next job's slot.
//! - [`FsLocks`]: the adapter over lock files, plus the holder protocol [`hold`]: a long-lived process takes a slot,
//!   announces it, and keeps it until its owner folder disappears or a release is requested ([`request_release`]).
//!
//! Reading a slot's state does not touch the lock acquirers contend on: a holder also keeps `sN.live` locked, and a status call
//! looks at that file only (a shared lock, taken and dropped at once), so a probe cannot make an acquirer skip a slot.
//!
//! # Processes that start children
//!
//! A lock belongs to an open file description. A program that holds a slot ([`FsGuard`]) or has a status probe running and
//! starts a child process by fork-then-exec (what `std::process::Command` does) hands the child a copy of the open lock file
//! for the moment between the fork and the exec, because another thread's file is not yet closed-on-exec at that point.
//! If the child is slow to exec, the slot looks held after its guard was dropped. Library consumers that spawn children on
//! one thread while another thread takes slots should serialise the two (a read-write lock around slot work and spawning;
//! this crate's own tests do exactly that), or spawn from a process that holds no slots, as the holder protocol does.

mod error;
mod fs;
mod hold;
mod slot;

pub use error::SlotError;
pub use fs::{FsGuard, FsLocks, OwnerInfo};
pub use hold::{
    ACQUIRE_GRACE, Announcement, HoldEnd, HoldProbe, ReleaseLocks, acquire_within, fresh_token,
    hold, hold_until, hold_with, release, wait_free,
};
pub use slot::{SlotId, SlotLocks, SlotState, acquire_first, slot_states, with_idle};
