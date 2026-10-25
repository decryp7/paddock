//! Time as a dependency, for the whole family (plan X20): one [`Clock`] port with a monotonic and a wall reading and a
//! cancellable wait, the real [`SystemClock`], a [`FakeClock`] that only moves when a test moves it, a [`Deadline`] on
//! any clock, and the restart/reconnect [`Backoff`].
//!
//! Code that waits, times out, throttles, retries or stamps a time takes an `Arc<dyn Clock>`; tests pass a
//! [`FakeClock`] and drive it with [`FakeClock::advance`], so no test sleeps and no test asserts how long something
//! took on the wall clock (lessons: "Tests that measure the wall clock fail under load"). `paddock-log` and
//! `paddock-health` re-export these types instead of defining their own.

use crate::cancel::{CancelToken, Cancelled};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The longest single OS wait (a wait for longer loops); keeps `Condvar::wait_timeout` away from overflowing durations.
const MAX_OS_WAIT: Duration = Duration::from_secs(24 * 3600);

/// The time source of everything that waits, times out, throttles or stamps a time.
///
/// Contract: [`monotonic`](Self::monotonic) never goes backwards and is the only reading used for durations, deadlines
/// and back-off; [`wall`](Self::wall) is the calendar time for stamps and file names and may jump; [`wait`](Self::wait)
/// returns once `d` has passed on this clock (`Ok`) or the token is cancelled (`Err`), whichever comes first.
pub trait Clock: Send + Sync {
    /// Time since the clock's fixed origin; never goes backwards.
    fn monotonic(&self) -> Duration;

    /// The calendar time.
    fn wall(&self) -> SystemTime;

    /// Waits until `d` has passed on this clock, or returns `Err` as soon as `cancel` is cancelled.
    fn wait(&self, d: Duration, cancel: &CancelToken) -> Result<(), Cancelled>;

    /// [`monotonic`](Self::monotonic) in whole milliseconds (saturating).
    fn now_ms(&self) -> u64 {
        u64::try_from(self.monotonic().as_millis()).unwrap_or(u64::MAX)
    }

    /// [`wall`](Self::wall) as milliseconds since the Unix epoch (negative before 1970, saturating).
    fn wall_ms(&self) -> i64 {
        match self.wall().duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
            Err(e) => i64::try_from(e.duration().as_millis()).map_or(i64::MIN, |ms| -ms),
        }
    }

    /// Waits `d` with no way to stop early.
    fn sleep(&self, d: Duration) {
        let _ = self.wait(d, &CancelToken::new());
    }
}

fn origin() -> Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(Instant::now)
}

/// The real clock: the OS monotonic clock (origin: first use in the process) and the system time.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn monotonic(&self) -> Duration {
        origin().elapsed()
    }

    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }

    fn wait(&self, d: Duration, cancel: &CancelToken) -> Result<(), Cancelled> {
        // A cancel wakes the wait at once (no polling); the token's own deadline bounds it.
        let signal = Arc::new((Mutex::new(false), Condvar::new()));
        let s = signal.clone();
        let _registered = cancel.on_cancel(move || {
            *s.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
            s.1.notify_all();
        });
        let end = Instant::now().checked_add(d);
        loop {
            cancel.check()?;
            let mut left = end.map_or(MAX_OS_WAIT, |e| e.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                return Ok(());
            }
            if let Some(r) = cancel.deadline_remaining() {
                left = left.min(r.max(Duration::from_millis(1)));
            }
            let woken = signal.0.lock().unwrap_or_else(|p| p.into_inner());
            if !*woken {
                drop(
                    signal
                        .1
                        .wait_timeout(woken, left.min(MAX_OS_WAIT))
                        .unwrap_or_else(|p| p.into_inner()),
                );
            }
        }
    }
}

#[derive(Debug)]
struct FakeState {
    monotonic: Duration,
    wall: SystemTime,
    waiting: usize,
    /// The waits in progress: their id, their end on the monotonic reading and their token.
    active: Vec<(u64, Duration, CancelToken)>,
    /// Waits started since the clock was made: a test's step boundary ("the second wait has begun").
    started: u64,
    /// Bumped on every move and every cancel of a token a wait is on, so no wake-up is lost.
    generation: u64,
}

#[derive(Debug)]
struct FakeInner {
    state: Mutex<FakeState>,
    moved: Condvar,
}

/// A clock that only moves when a test moves it. Clones share the time.
///
/// Waits on it block until [`advance`](Self::advance) moves the clock past their end (or their token is cancelled), so a
/// test drives timeouts, back-off and throttling step by step: start the work, [`wait_for_waiters`](Self::wait_for_waiters)
/// until it is blocked on the clock, advance, and assert the outcome. Both readings start at zero (the wall at the Unix
/// epoch) unless set with [`at_wall`](Self::at_wall).
#[derive(Debug, Clone)]
pub struct FakeClock(Arc<FakeInner>);

impl Default for FakeClock {
    fn default() -> Self {
        Self::at_wall(UNIX_EPOCH)
    }
}

impl FakeClock {
    /// A clock at zero, its wall at the Unix epoch.
    pub fn new() -> Self {
        Self::default()
    }

    /// A clock whose wall reading starts at `wall` (monotonic at zero).
    pub fn at_wall(wall: SystemTime) -> Self {
        FakeClock(Arc::new(FakeInner {
            state: Mutex::new(FakeState {
                monotonic: Duration::ZERO,
                wall,
                waiting: 0,
                active: Vec::new(),
                started: 0,
                generation: 0,
            }),
            moved: Condvar::new(),
        }))
    }

    /// A clock whose wall reading starts at `ms` milliseconds after the Unix epoch (before it when negative).
    pub fn at_wall_ms(ms: i64) -> Self {
        Self::at_wall(wall_from_ms(ms))
    }

    fn state(&self) -> MutexGuard<'_, FakeState> {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Moves both readings forward by `d` and wakes every wait that has reached its end.
    pub fn advance(&self, d: Duration) {
        let mut s = self.state();
        s.monotonic = s.monotonic.saturating_add(d);
        s.wall = s.wall.checked_add(d).unwrap_or(s.wall);
        s.generation += 1;
        self.0.moved.notify_all();
    }

    /// [`advance`](Self::advance) by `ms` milliseconds.
    pub fn advance_ms(&self, ms: u64) {
        self.advance(Duration::from_millis(ms));
    }

    /// Sets the wall reading (a user changing the system time); the monotonic reading does not move.
    pub fn set_wall(&self, wall: SystemTime) {
        let mut s = self.state();
        s.wall = wall;
        s.generation += 1;
        self.0.moved.notify_all();
    }

    /// How many threads are blocked in [`Clock::wait`] on this clock now.
    pub fn waiters(&self) -> usize {
        self.state().waiting
    }

    /// How many waits are blocked on this clock with time still to run: not yet at their end and not cancelled. A wait
    /// that the last [`advance`](Self::advance) ended, or whose token was cancelled, no longer counts even before its
    /// thread has left, so this is the step boundary that cannot race (unlike [`waiters`](Self::waiters), and unlike
    /// [`waits_started`](Self::waits_started) when other threads start and end short waits meanwhile).
    pub fn pending(&self) -> usize {
        let s = self.state();
        s.active
            .iter()
            .filter(|(_, end, t)| *end > s.monotonic && !t.cancel_requested())
            .count()
    }

    /// Blocks until at least `n` waits are [`pending`](Self::pending), so the next [`advance`](Self::advance) reaches
    /// exactly those. False if that did not happen within `guard` of real time (a hang guard).
    pub fn wait_for_pending(&self, n: usize, guard: Duration) -> bool {
        let s = self.state();
        let (s, _) = self
            .0
            .moved
            .wait_timeout_while(s, guard, |s| {
                s.active
                    .iter()
                    .filter(|(_, end, t)| *end > s.monotonic && !t.cancel_requested())
                    .count()
                    < n
            })
            .unwrap_or_else(|p| p.into_inner());
        s.active
            .iter()
            .filter(|(_, end, t)| *end > s.monotonic && !t.cancel_requested())
            .count()
            >= n
    }

    /// How many waits have started on this clock so far (finished ones included).
    pub fn waits_started(&self) -> u64 {
        self.state().started
    }

    /// Blocks until at least `n` waits have started on this clock in total, so a test advances exactly when the work
    /// has begun its `n`th wait (an earlier wait that has not left yet does not count twice). Returns false if that did
    /// not happen within `guard` of real time (a hang guard).
    pub fn wait_for_started(&self, n: u64, guard: Duration) -> bool {
        let s = self.state();
        let (s, _) = self
            .0
            .moved
            .wait_timeout_while(s, guard, |s| s.started < n)
            .unwrap_or_else(|p| p.into_inner());
        s.started >= n
    }

    /// Blocks until at least `n` threads wait on this clock, so a test advances only once the work is blocked on it.
    /// Returns false if that did not happen within `guard` of real time (a hang guard, not a timing assertion).
    pub fn wait_for_waiters(&self, n: usize, guard: Duration) -> bool {
        let mut s = self.state();
        let end = Instant::now().checked_add(guard);
        while s.waiting < n {
            let left = end.map_or(guard, |e| e.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                return false;
            }
            s = self
                .0
                .moved
                .wait_timeout(s, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        true
    }
}

impl Clock for FakeClock {
    fn monotonic(&self) -> Duration {
        self.state().monotonic
    }

    fn wall(&self) -> SystemTime {
        self.state().wall
    }

    fn wait(&self, d: Duration, cancel: &CancelToken) -> Result<(), Cancelled> {
        let (end, id) = {
            let mut s = self.state();
            s.waiting += 1;
            s.started += 1;
            let end = s.monotonic.saturating_add(d);
            let id = s.started;
            s.active.push((id, end, cancel.clone()));
            self.0.moved.notify_all();
            (end, id)
        };
        let me = self.clone();
        let registered = cancel.on_cancel(move || {
            let mut s = me.state();
            s.generation += 1;
            me.0.moved.notify_all();
        });
        let outcome = loop {
            let generation = self.state().generation;
            // The token is read without the clock's lock: a deadline token on this clock reads the clock.
            if let Err(c) = cancel.check() {
                break Err(c);
            }
            let s = self.state();
            if s.monotonic >= end {
                break Ok(());
            }
            if s.generation == generation {
                drop(self.0.moved.wait(s).unwrap_or_else(|p| p.into_inner()));
            }
        };
        drop(registered);
        let mut s = self.state();
        s.waiting -= 1;
        s.active.retain(|(i, _, _)| *i != id);
        self.0.moved.notify_all();
        outcome
    }
}

fn wall_from_ms(ms: i64) -> SystemTime {
    let d = Duration::from_millis(ms.unsigned_abs());
    if ms >= 0 {
        UNIX_EPOCH.checked_add(d).unwrap_or(UNIX_EPOCH)
    } else {
        UNIX_EPOCH.checked_sub(d).unwrap_or(UNIX_EPOCH)
    }
}

/// A point in time on a [`Clock`]'s monotonic reading: a time limit that a fake clock can run out.
#[derive(Clone)]
pub struct Deadline {
    clock: Arc<dyn Clock>,
    at: Option<Duration>,
}

impl fmt::Debug for Deadline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deadline")
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

impl Deadline {
    /// `limit` from now on `clock`; a limit too large to represent never passes.
    pub fn after(clock: Arc<dyn Clock>, limit: Duration) -> Deadline {
        let at = clock.monotonic().checked_add(limit);
        Deadline { clock, at }
    }

    /// Whether the time limit has been reached.
    pub fn passed(&self) -> bool {
        self.at.is_some_and(|at| self.clock.monotonic() >= at)
    }

    /// Time left (zero once passed; `None` for a deadline that never passes).
    pub fn remaining(&self) -> Option<Duration> {
        self.at.map(|at| at.saturating_sub(self.clock.monotonic()))
    }
}

/// The limits of a [`Backoff`]. The defaults are Pitwall's engine restarts (1 s doubling to 60 s, reset after five
/// healthy minutes, give up after more than five failures in ten minutes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    /// The first delay.
    pub initial: Duration,
    /// The longest delay.
    pub max: Duration,
    /// Healthy this long resets the delay to `initial`.
    pub reset_after: Duration,
    /// Give up when more than this many failures fall in `window`; `None` never gives up (a reconnecting client).
    pub give_up: Option<GiveUp>,
}

/// When a [`Backoff`] stops trying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GiveUp {
    /// Failures allowed in the window; one more gives up.
    pub max_failures: usize,
    /// The window failures are counted in.
    pub window: Duration,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        BackoffPolicy {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(60),
            reset_after: Duration::from_secs(5 * 60),
            give_up: Some(GiveUp {
                max_failures: 5,
                window: Duration::from_secs(10 * 60),
            }),
        }
    }
}

impl BackoffPolicy {
    /// A policy that never gives up: `initial` doubling to `max`, reset after `reset_after` healthy.
    pub fn retry_forever(initial: Duration, max: Duration, reset_after: Duration) -> Self {
        BackoffPolicy {
            initial,
            max,
            reset_after,
            give_up: None,
        }
    }
}

/// What to do after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// Try again after this delay.
    After(Duration),
    /// Too many failures in the window: stop and tell the user.
    GiveUp,
}

/// Exponential back-off for restarting a process or reconnecting a link: a pure state machine over a clock's monotonic
/// readings (pass `clock.monotonic()`), so every case is a unit test without waiting.
#[derive(Debug, Clone)]
pub struct Backoff {
    policy: BackoffPolicy,
    failures: VecDeque<Duration>,
    attempt: u32,
    healthy_since: Option<Duration>,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(BackoffPolicy::default())
    }
}

impl Backoff {
    /// A fresh history under `policy`.
    pub fn new(policy: BackoffPolicy) -> Backoff {
        Backoff {
            policy,
            failures: VecDeque::new(),
            attempt: 0,
            healthy_since: None,
        }
    }

    /// The policy.
    pub fn policy(&self) -> &BackoffPolicy {
        &self.policy
    }

    /// The process or link was healthy at `now`; healthy for `reset_after` in a row resets the delay.
    pub fn healthy(&mut self, now: Duration) {
        let since = *self.healthy_since.get_or_insert(now);
        if now.saturating_sub(since) >= self.policy.reset_after {
            self.attempt = 0;
        }
    }

    /// It failed (crashed, disconnected, refused) at `now`: wait, or give up.
    pub fn failed(&mut self, now: Duration) -> Retry {
        self.healthy(now);
        self.healthy_since = None;
        if let Some(g) = self.policy.give_up {
            self.failures.retain(|t| now.saturating_sub(*t) < g.window);
            self.failures.push_back(now);
            if self.failures.len() > g.max_failures {
                return Retry::GiveUp;
            }
        }
        let factor = 1u32.checked_shl(self.attempt.min(31)).unwrap_or(u32::MAX);
        let delay = self
            .policy
            .initial
            .saturating_mul(factor)
            .min(self.policy.max);
        self.attempt = self.attempt.saturating_add(1);
        Retry::After(delay)
    }

    /// Failures counted in the current window (always zero for a policy that never gives up).
    pub fn recent_failures(&self) -> usize {
        self.failures.len()
    }

    /// A clean slate (the user pressed Retry or Start).
    pub fn reset(&mut self) {
        *self = Backoff::new(self.policy);
    }
}
