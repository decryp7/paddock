//! The family's clock port, deadlines and back-off (plan X20). Nothing here sleeps or measures the wall clock: waits are
//! driven by a `FakeClock`, and a test advances it only once the work is blocked on it.

use crate::common;
use common::Collect;
use paddock_jobs::time::{
    Backoff, BackoffPolicy, Clock, Deadline, FakeClock, GiveUp, Retry, SystemClock,
};
use paddock_jobs::{CancelReason, CancelToken, Cancelled, JobError, JobRegistry, JobState};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);
/// Hang guard for `wait_for_waiters` (a broken test fails instead of hanging; never a timing assertion).
const GUARD: Duration = Duration::from_secs(30);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

// Pitwall's `backoff.rs` cases, unchanged in meaning.
#[test]
fn backoff_doubles_caps_resets_and_gives_up() {
    let mut b = Backoff::new(BackoffPolicy::default());
    assert_eq!(b.failed(ms(0)), Retry::After(SECOND));
    assert_eq!(b.failed(ms(1)), Retry::After(2 * SECOND));
    assert_eq!(b.failed(ms(2)), Retry::After(4 * SECOND));
    assert_eq!(b.recent_failures(), 3);
    // Healthy for five minutes: back to one second.
    b.healthy(ms(10));
    b.healthy(ms(10) + 5 * MINUTE);
    assert_eq!(b.failed(ms(11) + 5 * MINUTE), Retry::After(SECOND));

    let mut b = Backoff::new(BackoffPolicy {
        give_up: Some(GiveUp {
            max_failures: 100,
            window: 10 * MINUTE,
        }),
        ..BackoffPolicy::default()
    });
    let mut last = Retry::GiveUp;
    for i in 0..10 {
        last = b.failed(ms(i));
    }
    assert_eq!(last, Retry::After(MINUTE), "capped");

    let mut b = Backoff::new(BackoffPolicy::default());
    for i in 0..5 {
        assert!(matches!(b.failed(ms(i)), Retry::After(_)));
    }
    assert_eq!(b.failed(ms(6)), Retry::GiveUp);

    // Old failures leave the window.
    let mut b = Backoff::new(BackoffPolicy::default());
    for i in 0..5 {
        b.failed(i * 11 * MINUTE);
    }
    assert_eq!(b.recent_failures(), 1);
    assert_eq!(Backoff::default().recent_failures(), 0);
}

#[test]
fn healthy_but_not_long_enough_keeps_the_delay_growing() {
    let mut b = Backoff::default();
    b.failed(ms(0));
    b.healthy(ms(1));
    b.healthy(ms(1) + 4 * MINUTE);
    assert_eq!(b.failed(ms(2) + 4 * MINUTE), Retry::After(2 * SECOND));
}

#[test]
fn a_reconnecting_client_never_gives_up_and_never_overflows() {
    let policy = BackoffPolicy::retry_forever(ms(250), 30 * SECOND, MINUTE);
    let mut b = Backoff::new(policy);
    assert_eq!(b.policy(), &policy);
    let delays: Vec<Retry> = (0..5).map(|i| b.failed(ms(i))).collect();
    assert_eq!(
        delays,
        [ms(250), ms(500), ms(1000), ms(2000), ms(4000)].map(Retry::After)
    );
    for i in 0..1_000 {
        assert_eq!(
            b.failed(ms(10 + i)),
            Retry::After(if i < 2 {
                ms(8000 * (1 << i))
            } else {
                30 * SECOND
            })
        );
    }
    assert_eq!(
        b.recent_failures(),
        0,
        "no window is kept when it never gives up"
    );
    b.reset();
    assert_eq!(
        b.failed(ms(5_000)),
        Retry::After(ms(250)),
        "Retry starts over"
    );
}

#[test]
fn reset_clears_a_given_up_history() {
    let mut b = Backoff::default();
    for i in 0..6 {
        b.failed(ms(i));
    }
    assert_eq!(b.failed(ms(7)), Retry::GiveUp);
    b.reset();
    assert_eq!(b.failed(ms(8)), Retry::After(SECOND));
    assert_eq!(b.recent_failures(), 1);
}

#[test]
fn the_fake_clock_moves_only_when_told_and_clones_share_it() {
    let c = FakeClock::new();
    assert_eq!(c.monotonic(), Duration::ZERO);
    assert_eq!(c.wall(), UNIX_EPOCH);
    assert_eq!(c.wall_ms(), 0);
    c.clone().advance_ms(5);
    assert_eq!(c.now_ms(), 5);
    assert_eq!(c.wall_ms(), 5);
    // A user changing the system time moves the wall, never the monotonic reading.
    c.set_wall(UNIX_EPOCH + Duration::from_secs(1_000));
    assert_eq!(c.wall_ms(), 1_000_000);
    assert_eq!(c.now_ms(), 5);
    c.set_wall(UNIX_EPOCH - Duration::from_secs(1));
    assert_eq!(c.wall_ms(), -1_000);
    assert_eq!(FakeClock::at_wall_ms(-2_500).wall_ms(), -2_500);
    assert_eq!(
        FakeClock::at_wall_ms(1_700_000_000_000).wall_ms(),
        1_700_000_000_000
    );
    // Saturates instead of overflowing.
    c.advance(Duration::MAX);
    assert_eq!(c.monotonic(), Duration::MAX);
    assert_eq!(c.now_ms(), u64::MAX);
    assert!(format!("{c:?}").contains("FakeClock"));
}

#[test]
fn a_wait_on_the_fake_clock_ends_when_the_clock_reaches_it() {
    let c = FakeClock::new();
    let waiter = {
        let c = c.clone();
        std::thread::spawn(move || c.wait(10 * SECOND, &CancelToken::new()))
    };
    assert!(c.wait_for_waiters(1, GUARD));
    c.advance(9 * SECOND);
    // Still blocked: it can only leave once the clock reaches its end.
    assert_eq!(c.waiters(), 1);
    c.advance(SECOND);
    assert_eq!(waiter.join().unwrap(), Ok(()));
    assert_eq!(c.waiters(), 0);
    // A zero wait returns at once.
    assert_eq!(c.wait(Duration::ZERO, &CancelToken::new()), Ok(()));
}

#[test]
fn a_cancel_ends_a_fake_wait_without_moving_the_clock() {
    let c = FakeClock::new();
    let token = CancelToken::new();
    let waiter = {
        let (c, token) = (c.clone(), token.clone());
        std::thread::spawn(move || c.wait(Duration::MAX, &token))
    };
    assert!(c.wait_for_waiters(1, GUARD));
    token.cancel();
    assert_eq!(waiter.join().unwrap(), Err(Cancelled(CancelReason::User)));
    assert_eq!(c.monotonic(), Duration::ZERO);
}

#[test]
fn a_deadline_on_the_same_fake_clock_ends_a_longer_wait() {
    let c = FakeClock::new();
    let token = CancelToken::with_deadline_on(Arc::new(c.clone()), 5 * SECOND);
    let waiter = {
        let (c, token) = (c.clone(), token.clone());
        std::thread::spawn(move || {
            c.sleep(SECOND);
            c.wait(MINUTE, &token)
        })
    };
    assert!(c.wait_for_waiters(1, GUARD));
    c.advance(SECOND);
    assert!(c.wait_for_waiters(1, GUARD));
    assert!(!token.is_cancelled());
    c.advance(4 * SECOND);
    assert_eq!(
        waiter.join().unwrap(),
        Err(Cancelled(CancelReason::Deadline))
    );
}

#[test]
fn waiting_for_waiters_gives_up_after_its_guard() {
    assert!(!FakeClock::new().wait_for_waiters(1, Duration::ZERO));
    assert!(FakeClock::new().wait_for_waiters(0, Duration::ZERO));
}

/// The acceptance case: a fake clock drives a job's time limit, without sleeping.
#[test]
fn a_fake_clock_runs_out_a_jobs_time_limit() {
    let clock = FakeClock::new();
    let sink = Collect::new();
    let reg = JobRegistry::with_clock(sink.clone(), Arc::new(clock.clone()), 0);
    let job = reg.register_with_deadline("ai", "Answer", None, 2 * MINUTE);
    let runner = {
        let (job, clock) = (job.clone(), clock.clone());
        std::thread::spawn(move || {
            job.run::<()>(|ctx| {
                loop {
                    // A body that polls something every second until its time limit.
                    clock.wait(SECOND, ctx.cancel_token())?;
                }
            })
        })
    };
    // One second short of the limit, in uneven steps: the body keeps polling and the job is not cancelled.
    for step in [SECOND, 30 * SECOND, MINUTE, 28 * SECOND] {
        assert!(clock.wait_for_waiters(1, GUARD));
        clock.advance(step);
        assert!(!job.cancel_token().is_cancelled());
    }
    assert!(clock.wait_for_waiters(1, GUARD));
    clock.advance(SECOND);
    assert!(matches!(
        runner.join().unwrap(),
        Err(JobError::Cancelled(CancelReason::Deadline))
    ));
    assert!(matches!(
        sink.all().last().map(|s| s.state.clone()),
        Some(JobState::Cancelled {
            reason: CancelReason::Deadline
        })
    ));
}

#[test]
fn deadlines_pass_on_their_clock() {
    let c = FakeClock::new();
    let d = Deadline::after(Arc::new(c.clone()), 3 * SECOND);
    assert!(!d.passed());
    assert_eq!(d.remaining(), Some(3 * SECOND));
    c.advance(2 * SECOND);
    assert_eq!(d.clone().remaining(), Some(SECOND));
    c.advance(5 * SECOND);
    assert!(d.passed());
    assert_eq!(d.remaining(), Some(Duration::ZERO));
    assert!(format!("{d:?}").contains("Deadline"));
    // A limit too large to represent never passes.
    c.advance(SECOND);
    let never = Deadline::after(Arc::new(c.clone()), Duration::MAX);
    c.advance(Duration::MAX);
    assert!(!never.passed());
    assert_eq!(never.remaining(), None);
}

#[test]
fn the_system_clock_reads_the_os_and_honours_cancel() {
    let s = SystemClock;
    let a = s.monotonic();
    assert!(s.monotonic() >= a, "monotonic");
    assert!(s.wall_ms() > 1_700_000_000_000, "after 2023");
    assert!(s.wall() > SystemTime::UNIX_EPOCH);
    assert_eq!(s.wait(Duration::ZERO, &CancelToken::new()), Ok(()));
    assert_eq!(s.wait(Duration::from_nanos(1), &CancelToken::new()), Ok(()));
    s.sleep(Duration::ZERO);
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert_eq!(
        s.wait(MINUTE, &cancelled),
        Err(Cancelled(CancelReason::User))
    );
    // An endless wait ends with its token (another thread cancels it).
    let token = CancelToken::new();
    let canceller = {
        let token = token.clone();
        std::thread::spawn(move || token.cancel())
    };
    assert_eq!(
        s.wait(Duration::MAX, &token),
        Err(Cancelled(CancelReason::User))
    );
    canceller.join().unwrap();
    let as_dyn: Arc<dyn Clock> = Arc::new(SystemClock);
    assert!(as_dyn.now_ms() <= SystemClock.now_ms());
}
