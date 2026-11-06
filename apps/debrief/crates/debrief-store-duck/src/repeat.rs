//! "Log on change" for a loop that retries a failing step every few hundred milliseconds: the first failure of a cause is
//! logged with its chain, the repeats of the same cause are only counted, a different cause is logged as a new first failure,
//! and the recovery is one line with the count and the time it lasted. ADR 0055 §6's general limiter, when it lands, does not
//! say "recovered"; this does.

use std::time::{Duration, Instant};

/// What a failure means for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The first failure, or one with a different cause than the run before: log it. `ended` is the run of the previous
    /// cause that this one ended, when there was one.
    First { ended: Option<Run> },
    /// The same cause again: counted, not logged.
    Repeat,
}

/// A run of failures with one cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    pub failures: u64,
    pub lasted: Duration,
}

/// The state of one loop's failures.
#[derive(Debug, Default)]
pub(crate) struct RepeatGuard {
    cause: Option<String>,
    failures: u64,
    since: Option<Instant>,
}

impl RepeatGuard {
    /// A failure with `cause` (its message) at `now`.
    pub fn failed(&mut self, cause: &str, now: Instant) -> Failure {
        if self.cause.as_deref() == Some(cause) {
            self.failures += 1;
            return Failure::Repeat;
        }
        let ended = self.take(now);
        self.cause = Some(cause.to_string());
        self.failures = 1;
        self.since = Some(now);
        Failure::First { ended }
    }

    /// The loop worked again at `now`: the run that just ended, when there was one.
    pub fn recovered(&mut self, now: Instant) -> Option<Run> {
        self.take(now)
    }

    fn take(&mut self, now: Instant) -> Option<Run> {
        let since = self.since.take()?;
        self.cause = None;
        Some(Run {
            failures: std::mem::take(&mut self.failures),
            lasted: now.saturating_duration_since(since),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hundred_identical_failures_log_once_and_the_recovery_counts_them() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut g = RepeatGuard::default();
        assert_eq!(g.recovered(at(0)), None, "nothing failed: nothing to report");
        let logged = (0..100u64)
            .filter(|i| matches!(g.failed("disk full", at(i * 500)), Failure::First { .. }))
            .count();
        assert_eq!(logged, 1, "one warning for a hundred repeats");
        assert_eq!(
            g.recovered(at(49_500 + 500)),
            Some(Run {
                failures: 100,
                lasted: Duration::from_millis(50_000)
            })
        );
        assert_eq!(g.recovered(at(60_000)), None, "reported once");
    }

    #[test]
    fn a_new_cause_is_logged_and_ends_the_run_of_the_old_one() {
        let t0 = Instant::now();
        let mut g = RepeatGuard::default();
        assert_eq!(g.failed("a", t0), Failure::First { ended: None });
        assert_eq!(g.failed("a", t0 + Duration::from_secs(1)), Failure::Repeat);
        assert_eq!(
            g.failed("b", t0 + Duration::from_secs(2)),
            Failure::First {
                ended: Some(Run {
                    failures: 2,
                    lasted: Duration::from_secs(2)
                })
            }
        );
        assert_eq!(
            g.failed("a", t0 + Duration::from_secs(3)),
            Failure::First {
                ended: Some(Run {
                    failures: 1,
                    lasted: Duration::from_secs(1)
                })
            }
        );
    }
}
