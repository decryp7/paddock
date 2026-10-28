//! Numbered slots over operating-system file locks (ADR 0022, the CI kit's job isolation).
//!
//! A *slot* is `s1` to `sN`. Taking one means holding an exclusive lock on `<root>/slots/sN.lock` (`File::try_lock`:
//! `flock` on Unix, `LockFileEx` on Windows). The kernel drops the lock when the holder's process ends, however it ends,
//! so there is no stale lock to reclaim and therefore no reclaim race: two holders of one slot cannot exist.
//!
//! - [`SlotId`]: the typed `sN` name.
//! - [`SlotLocks`] (a port) and [`acquire_first`] (pure): take the first free slot; [`slot_states`] reads which are busy.
//! - [`acquire_retry`]: [`acquire_first`] that tries a few times when everything looked busy (a status probe holds a lock for
//!   an instant).
//! - Tokens: a holder has a [`fresh_token`]; `release` presents it, and a holder honours only its own, so a slow release can
//!   never free the next job's slot.
//! - [`FsLocks`]: the adapter over lock files, plus the holder protocol [`hold`]: a long-lived process takes a slot,
//!   announces it, and keeps it until its owner folder disappears or a release is requested ([`request_release`]).
//!
//! Reading a slot's state tries its lock, so a status call can make a concurrent acquirer skip that slot for a moment.
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
    ACQUIRE_ATTEMPTS, Announcement, HoldEnd, HoldProbe, ReleaseLocks, fresh_token, hold,
    hold_until, release, wait_free,
};
pub use slot::{SlotId, SlotLocks, SlotState, acquire_first, acquire_retry, slot_states};
