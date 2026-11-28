//! Reading a lock's state in a test (lessons.md, "A just-released lock can look held"; user, 2026-11-28, in chat): a test reads a
//! lock's state only through these waits, never once.
//!
//! Why: on Unix a `flock` belongs to the open file description, and a spawn on any thread gives its child a copy of the descriptor
//! table until it execs. A lock that was just released (or a handle just dropped) can therefore read held for a moment, and a single
//! read after the release judges the wrong state. The wrong reading is only ever *held*: a free reading is true. So a test that
//! expects a lock free waits until a probe reads it free ([`await_free`]), and one that expects a lock held waits until it reads held
//! ([`await_held`], a taken lock reads held at once, so this only covers a lock another process or thread is about to take).
//!
//! The probe is the caller's own read (`slotfile::is_held`, `InstanceLock::claim`, `try_lock().is_err()`): it returns whether the
//! lock reads held. The wait is [`crate::wait`]'s, bounded by the hang detector ([`HANG_GUARD`]), and never a judgement of speed; a lock
//! that stays in the wrong state is a failure that names what was waited for. Conformance C181 (report-only) flags a test that
//! asserts on a lock-state read outside these waits.

use crate::wait::{HANG_GUARD, poll_until_within};
use std::time::Duration;

/// Waits until `held` (the caller's probe: does the lock read held?) reads false, at most [`HANG_GUARD`]; panics naming `what` when it
/// never does. Use it for every "the lock is free now" expectation after a release, a drop or an exit.
pub fn await_free(what: &str, held: impl FnMut() -> bool) {
    await_free_within(what, HANG_GUARD, held);
}

/// [`await_free`] with a bound of the caller's.
pub fn await_free_within(what: &str, deadline: Duration, mut held: impl FnMut() -> bool) {
    assert!(
        poll_until_within(deadline, || !held()),
        "timed out after {deadline:?} waiting for {what} (the lock still reads held)"
    );
}

/// Waits until `held` reads true, at most [`HANG_GUARD`]; panics naming `what` when it never does. Use it for every "the lock is held
/// now" expectation, so a holder another thread or process has not yet taken is waited for.
pub fn await_held(what: &str, held: impl FnMut() -> bool) {
    await_held_within(what, HANG_GUARD, held);
}

/// [`await_held`] with a bound of the caller's.
pub fn await_held_within(what: &str, deadline: Duration, held: impl FnMut() -> bool) {
    assert!(
        poll_until_within(deadline, held),
        "timed out after {deadline:?} waiting for {what} (the lock still reads free)"
    );
}
