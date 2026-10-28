//! Numbered slots over operating-system file locks (ADR 0022, the CI kit's job isolation).
//!
//! A *slot* is `s1` to `sN`. Taking one means holding an exclusive lock on `<root>/slots/sN.lock` (`File::try_lock`:
//! `flock` on Unix, `LockFileEx` on Windows). The kernel drops the lock when the holder's process ends, however it ends,
//! so there is no stale lock to reclaim and therefore no reclaim race: two holders of one slot cannot exist.
//!
//! - [`SlotId`]: the typed `sN` name.
//! - [`SlotLocks`] (a port) and [`acquire_first`] (pure): take the first free slot; [`slot_states`] reads which are busy;
//!   [`with_idle`] runs work on an idle slot while holding its lock (deleting its kept folders without a check-then-act race).
//! - [`acquire_retry`]: [`acquire_first`] that tries a few times when everything looked busy (a status probe holds a lock for
//!   an instant).
//! - Tokens: a holder has a [`fresh_token`]; `release` presents it, and a holder honours only its own, so a slow release can
//!   never free the next job's slot.
//! - [`FsLocks`]: the adapter over lock files, plus the holder protocol [`hold`]: a long-lived process takes a slot,
//!   announces it, and keeps it until its owner folder disappears or a release is requested ([`request_release`]).
//!
//! Reading a slot's state tries its lock, so a status call can make a concurrent acquirer skip that slot for a moment.

mod error;
mod fs;
mod hold;
mod slot;

pub use error::SlotError;
pub use fs::{FsGuard, FsLocks, OwnerInfo};
pub use hold::{
    ACQUIRE_ATTEMPTS, Announcement, HoldEnd, HoldProbe, fresh_token, hold, hold_until, release,
    wait_free,
};
pub use slot::{
    SlotId, SlotLocks, SlotState, acquire_first, acquire_retry, slot_states, with_idle,
};
