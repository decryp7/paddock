//! Waiting on a condition in a test (ADR 0041 §3): poll the condition, never sleep a guessed time. The poll interval is the only
//! real time here and the bound is a hang guard, not a verdict: a test that passes or fails never depends on how long it took,
//! only a condition that never holds fails, with the name of what was waited for.
//!
//! Two chained waits in one failing test can together pass nextest's 60 s kill, which then ends the test without this message.
//!
//! This crate owns real time for tests (conformance C71 skips it); a test calls these instead of `thread::sleep`.

use std::time::{Duration, Instant};

/// The default bound: over any loaded runner's slowest correct wait and under CI's 60 s test kill, so a real hang fails with
/// its own message instead of being killed first.
pub const HANG_GUARD: Duration = Duration::from_secs(40);

/// The first poll interval and the longest one: the interval doubles from the first to the last.
const FIRST: Duration = Duration::from_micros(200);
const LAST: Duration = Duration::from_millis(20);

/// Polls `f` until it returns `Some(value)`; `None` after `guard`. The last poll happens after the bound too, so a value that arrived
/// while the thread slept is not lost.
pub fn poll_for<T>(guard: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + guard;
    let mut pause = FIRST;
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if Instant::now() >= end {
            return f();
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(LAST);
    }
}

/// Polls until `f` becomes true, for at most `guard`; returns whether it did (it does not mean the condition *stays* true). A caller
/// that ignores the answer writes `let _ =`: most callers check the effect afterwards.
#[must_use]
pub fn poll_until_within(guard: Duration, mut f: impl FnMut() -> bool) -> bool {
    poll_for(guard, || f().then_some(())).is_some()
}

/// Retries an action: runs `f` (which does something and returns whether it is done) first at once, then every `every`, until it
/// returns true or `guard` has passed; returns whether it did. Use it for actions that change things (a delete, a command that logs),
/// where [`poll_until_within`]'s fast poll would run them back to back.
#[must_use]
pub fn retry_within(guard: Duration, every: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + guard;
    loop {
        if f() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(every);
    }
}

/// Waits until `f` holds (at most [`HANG_GUARD`]); panics naming `what` when it never does.
pub fn wait_until(what: &str, f: impl FnMut() -> bool) {
    wait_until_within(what, HANG_GUARD, f);
}

/// [`wait_until`] with a bound of the caller's.
pub fn wait_until_within(what: &str, guard: Duration, f: impl FnMut() -> bool) {
    assert!(poll_until_within(guard, f), "timed out after {guard:?} waiting for {what}");
}

/// Waits until `f` gives a value (at most [`HANG_GUARD`]) and returns it; panics naming `what` when it never does.
pub fn wait_for<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_for_within(what, HANG_GUARD, f)
}

/// [`wait_for`] with a bound of the caller's.
pub fn wait_for_within<T>(what: &str, guard: Duration, f: impl FnMut() -> Option<T>) -> T {
    poll_for(guard, f).unwrap_or_else(|| panic!("timed out after {guard:?} waiting for {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn a_condition_that_holds_at_once_is_not_polled_again() {
        let calls = AtomicUsize::new(0);
        wait_until("a true condition", || {
            calls.fetch_add(1, Ordering::SeqCst);
            true
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_condition_set_by_another_thread_ends_the_wait() {
        let flag = Arc::new(AtomicBool::new(false));
        let setter = flag.clone();
        let t = std::thread::spawn(move || setter.store(true, Ordering::SeqCst));
        wait_until("the flag", || flag.load(Ordering::SeqCst));
        t.join().unwrap();
    }

    #[test]
    fn a_condition_that_never_holds_gives_up_after_the_bound_and_is_polled_once_more() {
        let calls = AtomicUsize::new(0);
        let held = poll_until_within(Duration::from_millis(30), || {
            calls.fetch_add(1, Ordering::SeqCst);
            false
        });
        assert!(!held);
        assert!(calls.load(Ordering::SeqCst) >= 2, "polled at least at the start and after the bound");
    }

    #[test]
    fn a_value_is_returned_and_a_missing_one_is_none() {
        let mut n = 0;
        let v = poll_for(Duration::from_secs(5), || {
            n += 1;
            (n == 3).then_some(n * 10)
        });
        assert_eq!(v, Some(30));
        assert_eq!(poll_for::<u8>(Duration::from_millis(10), || None), None);
        assert_eq!(wait_for("three", || Some(3)), 3);
    }

    #[test]
    #[should_panic(expected = "waiting for the impossible")]
    fn a_condition_that_never_holds_panics_naming_what_was_waited_for() {
        wait_until_within("the impossible", Duration::from_millis(5), || false);
    }

    #[test]
    #[should_panic(expected = "waiting for a value that never comes")]
    fn a_value_that_never_comes_panics_naming_what_was_waited_for() {
        let _: u8 = wait_for_within("a value that never comes", Duration::from_millis(5), || None);
    }

    #[test]
    fn a_retried_action_runs_at_once_then_at_its_own_interval() {
        let mut runs = 0;
        assert!(retry_within(Duration::from_secs(5), Duration::from_millis(1), || {
            runs += 1;
            runs == 3
        }));
        assert_eq!(runs, 3);
        let mut never = 0;
        assert!(!retry_within(Duration::from_millis(20), Duration::from_millis(10), || {
            never += 1;
            false
        }));
        assert!((2..=4).contains(&never), "a fixed interval, not a fast poll: {never}");
    }
}
