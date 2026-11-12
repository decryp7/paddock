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

/// The first poll interval and the longest one: the interval doubles from the first to the last (also the pace of
/// `paddock_testkit_start`'s waits).
pub const FIRST: Duration = Duration::from_micros(200);
pub const LAST: Duration = Duration::from_millis(20);

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
    assert!(
        poll_until_within(guard, f),
        "timed out after {guard:?} waiting for {what}"
    );
}

/// Waits until `f` gives a value (at most [`HANG_GUARD`]) and returns it; panics naming `what` when it never does.
pub fn wait_for<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_for_within(what, HANG_GUARD, f)
}

/// [`wait_for`] with a bound of the caller's.
pub fn wait_for_within<T>(what: &str, guard: Duration, f: impl FnMut() -> Option<T>) -> T {
    poll_for(guard, f).unwrap_or_else(|| panic!("timed out after {guard:?} waiting for {what}"))
}

/// Waits for a value on `rx` (at most [`HANG_GUARD`]); panics naming `what` when none comes or when every sender is gone
/// without sending (a dropped sender is a failed test, not a longer wait).
pub fn recv<T>(what: &str, rx: &std::sync::mpsc::Receiver<T>) -> T {
    recv_within(what, HANG_GUARD, rx)
}

/// [`recv`] with a bound of the caller's.
pub fn recv_within<T>(what: &str, guard: Duration, rx: &std::sync::mpsc::Receiver<T>) -> T {
    match rx.recv_timeout(guard) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("timed out after {guard:?} waiting for {what}")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("every sender was dropped before sending {what}")
        }
    }
}

/// A counter of work done, for [`guarded`]: the work calls [`Progress::tick`] for every unit it finishes (a file scanned, a line
/// read), from any thread. Cloning shares the count.
#[derive(Debug, Clone, Default)]
pub struct Progress(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl Progress {
    /// A counter at zero.
    pub fn new() -> Progress {
        Progress::default()
    }

    /// One more unit done.
    pub fn tick(&self) {
        self.add(1);
    }

    /// `n` more units done.
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }

    /// Units done so far.
    pub fn get(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Follows work that shows its own progress as a level (rows committed, bytes read; any change counts, so a counter that
    /// starts again in a new phase is progress too): asks `look` with the kit's backoff, ticks whenever the level it gives
    /// changes, and returns once `look` says the work is done. It has no bound of its own: call it inside [`guarded`], whose
    /// stall limit ends work whose level stops changing.
    pub fn follow(&self, mut look: impl FnMut() -> (u64, bool)) {
        let mut last = None;
        let mut pause = FIRST;
        loop {
            let (level, done) = look();
            if last != Some(level) {
                last = Some(level);
                self.tick();
            }
            if done {
                return;
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(LAST);
        }
    }
}

/// The pure part of a progress guard (ADR 0071): stalled only when the count has not grown for `limit`. Time is a number the
/// caller passes, so the rule is tested with no clock.
#[derive(Debug, Clone, Copy)]
pub struct StallGuard {
    limit: Duration,
    last: u64,
    since: Duration,
}

impl StallGuard {
    /// A guard that calls a count stalled after `limit` without growth, counted from `now`, when the count stood at `progress`.
    pub fn new(limit: Duration, now: Duration, progress: u64) -> StallGuard {
        StallGuard {
            limit,
            last: progress,
            since: now,
        }
    }

    /// Looks at the count at time `now`; `true` when it has not grown for the whole limit.
    pub fn stalled(&mut self, progress: u64, now: Duration) -> bool {
        if progress != self.last {
            self.last = progress;
            self.since = now;
            return false;
        }
        now.saturating_sub(self.since) >= self.limit
    }
}

/// Runs `work` on its own thread and waits for it under a progress guard instead of a total time: a slow disk that keeps
/// finishing units passes however long it takes, and only work that finishes no unit for `stall_limit` fails, naming `what`
/// and how far it got (ADR 0071). The work reports through the [`Progress`] it is given.
///
/// A hung worker is left running (it cannot be stopped); the panic ends the test and the process ends it.
// conformance: hang-guard
pub fn guarded<T: Send + 'static>(
    what: &str,
    stall_limit: Duration,
    work: impl FnOnce(Progress) -> T + Send + 'static,
) -> T {
    let progress = Progress::new();
    let (tx, rx) = std::sync::mpsc::channel();
    let mine = progress.clone();
    std::thread::spawn(move || {
        // A send fails only when the waiter has given up.
        drop(
            tx.send(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || work(mine),
            ))),
        );
    });
    let began = Instant::now();
    let mut guard = StallGuard::new(stall_limit, Duration::ZERO, progress.get());
    loop {
        match rx.recv_timeout(LAST) {
            Ok(Ok(value)) => return value,
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if guard.stalled(progress.get(), began.elapsed()) {
                    panic!(
                        "{what} is hung: no progress for {stall_limit:?} after {} units (a slow host passes; only work that finishes nothing fails)",
                        progress.get()
                    );
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                panic!("the worker of {what} ended without a result")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn recv_returns_the_value_and_names_a_dropped_sender() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(7).unwrap();
        assert_eq!(recv("a value", &rx), 7);
        drop(tx);
        let e = std::panic::catch_unwind(|| recv("the gone value", &rx)).unwrap_err();
        assert!(
            e.downcast_ref::<String>()
                .unwrap()
                .contains("every sender was dropped before sending the gone value")
        );
    }

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
        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "polled at least at the start and after the bound"
        );
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
        let _: u8 = wait_for_within("a value that never comes", Duration::from_millis(5), || {
            None
        });
    }

    #[test]
    fn a_retried_action_runs_at_once_then_at_its_own_interval() {
        let mut runs = 0;
        assert!(retry_within(
            Duration::from_secs(5),
            Duration::from_millis(1),
            || {
                runs += 1;
                runs == 3
            }
        ));
        assert_eq!(runs, 3);
        let mut never = 0;
        assert!(!retry_within(
            Duration::from_millis(20),
            Duration::from_millis(10),
            || {
                never += 1;
                false
            }
        ));
        assert!(
            (2..=4).contains(&never),
            "a fixed interval, not a fast poll: {never}"
        );
    }

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn a_count_that_keeps_growing_is_never_stalled_however_long_it_takes() {
        let mut g = StallGuard::new(S(60), S(0), 0);
        // Two hours of work, one unit every 59 s: far over the limit in total, never over it between units.
        for n in 1..=122u64 {
            assert!(!g.stalled(n, S(n * 59)), "unit {n}");
        }
    }

    #[test]
    fn a_count_that_stops_is_stalled_after_the_limit_and_not_before() {
        let mut g = StallGuard::new(S(60), S(0), 0);
        assert!(!g.stalled(5, S(100)));
        assert!(!g.stalled(5, S(159)));
        assert!(g.stalled(5, S(160)));
    }

    #[test]
    fn a_guard_starts_from_the_count_it_is_given() {
        // A count that already stood at 5 and never moves is stalled at the limit; a guard that assumed 0 would call 5 growth.
        let mut g = StallGuard::new(S(60), S(10), 5);
        assert!(!g.stalled(5, S(69)));
        assert!(g.stalled(5, S(70)));
    }

    #[test]
    fn a_slow_but_finishing_job_passes_and_returns_its_value() {
        let v = guarded("a counted job", S(30), |p| {
            for _ in 0..1000 {
                p.tick();
            }
            p.get()
        });
        assert_eq!(v, 1000);
    }

    #[test]
    #[should_panic(expected = "a job that finishes nothing is hung")]
    fn a_job_that_finishes_no_unit_is_reported_as_hung_naming_what_it_was() {
        let (_keep, held) = std::sync::mpsc::channel::<()>();
        // conformance: hang-guard — the stall limit is the thing under test; the worker is blocked on a channel that is never sent
        let _: () = guarded(
            "a job that finishes nothing",
            Duration::from_millis(40),
            move |_| {
                let _ = held.recv();
            },
        );
    }

    #[test]
    fn following_a_level_ticks_for_every_change_and_ends_when_the_work_is_done() {
        let p = Progress::new();
        // Levels as a job shows them: a repeat is no progress, a counter that starts again in a new phase is.
        let levels = [
            (0, false),
            (0, false),
            (5, false),
            (5, false),
            (9, false),
            (2, false),
            (2, true),
        ];
        let mut at = 0;
        p.follow(|| {
            let l = levels[at];
            at += 1;
            l
        });
        assert_eq!(at, levels.len(), "asked until the work said it was done");
        assert_eq!(p.get(), 4, "one tick per new level: 0, 5, 9, 2");
    }

    #[test]
    #[should_panic(expected = "a job whose level stops changing is hung")]
    fn a_followed_level_that_stops_changing_is_a_stall_of_the_guard() {
        // conformance: hang-guard — the stall limit is the thing under test; the level never changes and the work never ends
        let _: () = guarded(
            "a job whose level stops changing",
            Duration::from_millis(40),
            |p| p.follow(|| (7, false)),
        );
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn a_panic_in_the_work_is_the_tests_panic() {
        guarded("a job", S(30), |_| panic!("boom"));
    }
}
