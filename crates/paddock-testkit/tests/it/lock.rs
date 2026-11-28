//! `lock::await_free` and `await_held`: a lock-state read in a test settles before it is judged.
#![allow(clippy::unwrap_used)]
use paddock_testkit::lock::{await_free, await_free_within, await_held, await_held_within};
use std::cell::Cell;
use std::time::Duration;

/// A just-released lock can read held for a few reads (a spawn's copy of the descriptor): the wait goes on until it reads free.
#[test]
fn await_free_waits_out_a_few_held_reads() {
    let reads = Cell::new(0u32);
    await_free("a just-released lock", || {
        reads.set(reads.get() + 1);
        reads.get() <= 3
    });
    assert_eq!(
        reads.get(),
        4,
        "it read held three times and free once, then stopped"
    );
}

/// A lock that is free at the first read costs one read.
#[test]
fn await_free_returns_at_once_on_a_free_lock() {
    let reads = Cell::new(0u32);
    await_free("a free lock", || {
        reads.set(reads.get() + 1);
        false
    });
    assert_eq!(reads.get(), 1);
}

/// The positive control (ER-33): a lock that stays held is a failure with the name of what was waited for, never a pass.
#[test]
#[should_panic(expected = "waiting for a real holder to be free")]
fn await_free_fails_naming_what_it_waited_for_when_the_lock_stays_held() {
    await_free_within(
        "a real holder to be free",
        Duration::from_millis(30),
        || true,
    );
}

/// A lock that reads free later is waited for.
#[test]
fn await_held_waits_for_the_lock_to_read_held() {
    let reads = Cell::new(0u32);
    await_held("a lock being taken", || {
        reads.set(reads.get() + 1);
        reads.get() > 2
    });
    assert_eq!(reads.get(), 3);
}

/// The positive control: a lock that never reads held is a failure.
#[test]
#[should_panic(expected = "waiting for a lock nobody holds to be held")]
fn await_held_fails_naming_what_it_waited_for_when_the_lock_stays_free() {
    await_held_within(
        "a lock nobody holds to be held",
        Duration::from_millis(30),
        || false,
    );
}
