//! Retries: a pure table plus a loop that takes its clock and sleeper as parameters, so tests never wait. Connect errors,
//! `503 overloaded` and `429` with `Retry-After` are retried with exponential backoff and jitter (250 ms to 8 s; 4 attempts for
//! interactive calls, 8 for batch); other 4xx are never retried; a streamed answer is never retried after its first delta;
//! cancel and the call's deadline always win.

use crate::error::PitwallError;
use crate::loopback::CancelToken;
use crate::model::Priority;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, Instant};

/// Longest `Retry-After` honoured.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// How often and how long to wait between attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Attempts in all (1: no retry).
    pub attempts: u32,
    /// The first wait.
    pub base: Duration,
    /// No wait is longer than this (except an honoured `Retry-After`, capped at [`MAX_RETRY_AFTER`]).
    pub max: Duration,
}

impl Policy {
    /// One attempt.
    pub const NONE: Policy = Policy {
        attempts: 1,
        base: Duration::from_millis(250),
        max: Duration::from_secs(8),
    };
    /// Four attempts: something the user waits for.
    pub const INTERACTIVE: Policy = Policy {
        attempts: 4,
        base: Duration::from_millis(250),
        max: Duration::from_secs(8),
    };
    /// Eight attempts: bulk work that can afford to wait.
    pub const BATCH: Policy = Policy {
        attempts: 8,
        base: Duration::from_millis(250),
        max: Duration::from_secs(8),
    };

    /// The policy for a priority (background work retries like interactive).
    pub fn for_priority(p: Priority) -> Policy {
        match p {
            Priority::Batch => Policy::BATCH,
            _ => Policy::INTERACTIVE,
        }
    }

    /// The wait before attempt `next` (2 for the second attempt): `Retry-After` when Pitwall sent one, else exponential with
    /// `jitter` (0 to 1000 per mille of the upper half: the wait lies between half and all of the nominal value).
    pub fn delay(&self, next: u32, retry_after: Option<Duration>, jitter: u32) -> Duration {
        if let Some(r) = retry_after {
            return r.min(MAX_RETRY_AFTER);
        }
        let exp = next.saturating_sub(2).min(16);
        let nominal = self.base.saturating_mul(1u32 << exp).min(self.max);
        let jitter = u64::from(jitter.min(1000));
        let half = nominal / 2;
        half + Duration::from_nanos(
            u64::try_from(half.as_nanos()).unwrap_or(u64::MAX) / 1000 * jitter,
        )
    }
}

/// Waits between attempts. The real one sleeps in short slices and returns early on cancel; tests record instead.
pub trait Sleeper: Send + Sync {
    /// Waits `d`, or less when `cancel` is cancelled.
    fn sleep(&self, d: Duration, cancel: &CancelToken);
}

/// The real sleeper.
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, d: Duration, cancel: &CancelToken) {
        let end = Instant::now() + d;
        while !cancel.is_cancelled() {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            std::thread::sleep(left.min(Duration::from_millis(50)));
        }
    }
}

/// A random number in `0..=1000` for the jitter (no dependency: the standard library's randomly seeded hasher).
pub fn jitter() -> u32 {
    let mut h = RandomState::new().build_hasher();
    h.write_u8(1);
    u32::try_from(h.finish() % 1001).unwrap_or(0)
}

/// Runs `attempt` until it succeeds, fails with an error that is not transient, runs out of attempts, is cancelled or passes
/// `deadline`. `waited` receives each wait (for tests and "Still working" displays).
pub fn run<T>(
    policy: Policy,
    cancel: &CancelToken,
    sleeper: &dyn Sleeper,
    deadline: Option<Instant>,
    mut attempt: impl FnMut() -> Result<T, PitwallError>,
) -> Result<T, PitwallError> {
    let mut n = 1;
    loop {
        if cancel.is_cancelled() {
            return Err(PitwallError::Cancelled);
        }
        let err = match attempt() {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };
        if !err.is_transient() || n >= policy.attempts {
            return Err(err);
        }
        let retry_after = match &err {
            PitwallError::RateLimited { retry_after, .. }
            | PitwallError::Overloaded { retry_after, .. } => *retry_after,
            _ => None,
        };
        // A 429 without Retry-After is not retried: Pitwall did not say when.
        if matches!(
            err,
            PitwallError::RateLimited {
                retry_after: None,
                ..
            }
        ) {
            return Err(err);
        }
        n += 1;
        let wait = policy.delay(n, retry_after, jitter());
        if deadline.is_some_and(|d| Instant::now() + wait >= d) {
            return Err(PitwallError::Timeout("the call's deadline"));
        }
        sleeper.sleep(wait, cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<Duration>>);

    impl Sleeper for Recorder {
        fn sleep(&self, d: Duration, _: &CancelToken) {
            self.0.lock().unwrap().push(d);
        }
    }

    #[test]
    fn the_backoff_table() {
        let p = Policy::INTERACTIVE;
        // Nominal waits 250 ms, 500 ms, 1 s, ... capped at 8 s; the jitter keeps half to all of it.
        let nominal = |n| p.delay(n, None, 1000);
        assert_eq!(
            [2, 3, 4, 5, 6, 7, 8, 9, 40].map(nominal),
            [250, 500, 1000, 2000, 4000, 8000, 8000, 8000, 8000].map(Duration::from_millis)
        );
        assert_eq!(p.delay(2, None, 0), Duration::from_millis(125));
        assert_eq!(
            p.delay(2, None, 500),
            Duration::from_millis(187) + Duration::from_micros(500)
        );
        // Retry-After wins over the table, within its cap.
        assert_eq!(
            p.delay(2, Some(Duration::from_secs(5)), 0),
            Duration::from_secs(5)
        );
        assert_eq!(
            p.delay(2, Some(Duration::from_secs(900)), 0),
            MAX_RETRY_AFTER
        );
        // A jitter beyond the range is clamped.
        assert_eq!(p.delay(2, None, 5000), Duration::from_millis(250));
        assert_eq!(
            (
                Policy::for_priority(Priority::Interactive).attempts,
                Policy::for_priority(Priority::Background).attempts
            ),
            (4, 4)
        );
        assert_eq!(Policy::for_priority(Priority::Batch).attempts, 8);
        assert!(jitter() <= 1000);
    }

    #[test]
    fn transient_errors_are_retried_until_they_clear_and_nothing_else_is() {
        let rec = Recorder::default();
        let cancel = CancelToken::new();
        let mut calls = 0;
        let r = run(Policy::INTERACTIVE, &cancel, &rec, None, || {
            calls += 1;
            if calls < 3 {
                Err(PitwallError::Overloaded {
                    message: "busy".into(),
                    retry_after: None,
                })
            } else {
                Ok(calls)
            }
        });
        assert_eq!((r, rec.0.lock().unwrap().len()), (Ok(3), 2));

        // A 429 with Retry-After waits as told; without it the call fails at once.
        let rec = Recorder::default();
        let mut calls = 0;
        let r = run(Policy::INTERACTIVE, &cancel, &rec, None, || {
            calls += 1;
            if calls == 1 {
                Err(PitwallError::RateLimited {
                    message: "m".into(),
                    retry_after: Some(Duration::from_secs(7)),
                })
            } else {
                Ok(())
            }
        });
        assert_eq!(
            (r, rec.0.lock().unwrap().clone()),
            (Ok(()), vec![Duration::from_secs(7)])
        );
        let rec = Recorder::default();
        let mut calls = 0;
        let r: Result<(), _> = run(Policy::INTERACTIVE, &cancel, &rec, None, || {
            calls += 1;
            Err(PitwallError::RateLimited {
                message: "m".into(),
                retry_after: None,
            })
        });
        assert!(matches!(r, Err(PitwallError::RateLimited { .. })));
        assert_eq!((calls, rec.0.lock().unwrap().len()), (1, 0));

        // 4xx are never retried.
        for e in [
            PitwallError::Invalid("x".into()),
            PitwallError::KeyRevoked,
            PitwallError::Forbidden("x".into()),
            PitwallError::NoModelFits("x".into()),
        ] {
            let mut calls = 0;
            let r: Result<(), _> = run(Policy::BATCH, &cancel, &rec, None, || {
                calls += 1;
                Err(e.clone())
            });
            assert_eq!((r, calls), (Err(e), 1));
        }
    }

    #[test]
    fn attempts_are_bounded_by_priority() {
        let rec = Recorder::default();
        let cancel = CancelToken::new();
        let count = |policy: Policy| {
            let mut calls = 0;
            let r: Result<(), _> = run(policy, &cancel, &rec, None, || {
                calls += 1;
                Err(PitwallError::NotRunning)
            });
            assert_eq!(r, Err(PitwallError::NotRunning));
            calls
        };
        assert_eq!(
            (
                count(Policy::NONE),
                count(Policy::INTERACTIVE),
                count(Policy::BATCH)
            ),
            (1, 4, 8)
        );
    }

    #[test]
    fn cancel_and_the_deadline_always_win() {
        let rec = Recorder::default();
        let cancel = CancelToken::new();
        let c = cancel.clone();
        let mut calls = 0;
        // Cancelled during the first attempt: the retry never happens.
        let r: Result<(), _> = run(Policy::BATCH, &cancel, &rec, None, || {
            calls += 1;
            c.cancel();
            Err(PitwallError::NotRunning)
        });
        assert_eq!((r, calls), (Err(PitwallError::Cancelled), 1));
        // A deadline that the next wait would pass ends the call.
        let fresh = CancelToken::new();
        let r: Result<(), _> = run(Policy::BATCH, &fresh, &rec, Some(Instant::now()), || {
            Err(PitwallError::NotRunning)
        });
        assert_eq!(r, Err(PitwallError::Timeout("the call's deadline")));
        // The real sleeper returns at once for a cancelled token and for a zero wait.
        let done = CancelToken::new();
        done.cancel();
        ThreadSleeper.sleep(Duration::from_secs(3600), &done);
        ThreadSleeper.sleep(Duration::ZERO, &CancelToken::new());
        // A real (one millisecond) wait ends by itself.
        ThreadSleeper.sleep(Duration::from_millis(1), &CancelToken::new());
    }
}
