//! Restart with backoff and quarantine, as a pure state machine over instants.

use crate::config::Policy;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// What to do after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Try again at this instant.
    RetryAt(Instant),
    /// Too many failures: quarantine.
    Quarantine,
}

/// The failure history of one plugin.
#[derive(Debug, Default)]
pub(crate) struct Supervision {
    failures: VecDeque<Instant>,
    in_a_row: u32,
    retry_at: Option<Instant>,
    quarantined: bool,
}

impl Supervision {
    /// Records a failure at `now`.
    pub(crate) fn record_failure(&mut self, now: Instant, policy: &Policy) -> Verdict {
        self.failures.push_back(now);
        while self
            .failures
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > policy.window)
        {
            self.failures.pop_front();
        }
        self.in_a_row = self.in_a_row.saturating_add(1);
        if self.failures.len() >= policy.max_failures.max(1) {
            self.quarantined = true;
            self.retry_at = None;
            return Verdict::Quarantine;
        }
        let at = now + backoff(self.in_a_row, policy);
        self.retry_at = Some(at);
        Verdict::RetryAt(at)
    }

    /// The plugin started and answered: the backoff starts over (the failure window still counts).
    pub(crate) fn record_success(&mut self) {
        self.in_a_row = 0;
        self.retry_at = None;
    }

    /// Whether it may be started at `now`.
    pub(crate) fn may_start(&self, now: Instant) -> bool {
        !self.quarantined && self.retry_at.is_none_or(|t| now >= t)
    }

    /// When the next start is allowed, if it has to wait.
    pub(crate) fn retry_at(&self) -> Option<Instant> {
        self.retry_at
    }

    /// Whether it is quarantined.
    pub(crate) fn is_quarantined(&self) -> bool {
        self.quarantined
    }

    /// The user cleared the quarantine: a clean slate.
    pub(crate) fn clear(&mut self) {
        *self = Supervision::default();
    }
}

/// `initial * 2^(n-1)`, at most `max`.
fn backoff(n: u32, policy: &Policy) -> Duration {
    let factor = 1u32.checked_shl(n.saturating_sub(1)).unwrap_or(u32::MAX);
    policy.backoff_initial.saturating_mul(factor).min(policy.backoff_max)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn the_backoff_doubles_up_to_its_maximum() {
        let p = Policy {
            max_failures: 100,
            ..Policy::default()
        };
        let t0 = Instant::now();
        let mut s = Supervision::default();
        assert!(s.may_start(t0));
        assert_eq!(s.record_failure(t0, &p), Verdict::RetryAt(t0 + S));
        assert!(!s.may_start(t0) && s.may_start(t0 + S));
        assert_eq!(s.record_failure(t0, &p), Verdict::RetryAt(t0 + 2 * S));
        assert_eq!(s.record_failure(t0, &p), Verdict::RetryAt(t0 + 4 * S));
        for _ in 0..40 {
            s.record_failure(t0, &p);
        }
        assert_eq!(s.retry_at(), Some(t0 + 60 * S));
        s.record_success();
        assert!(s.may_start(t0));
        assert_eq!(s.record_failure(t0, &p), Verdict::RetryAt(t0 + S));
    }

    #[test]
    fn three_failures_in_ten_minutes_quarantine_until_cleared() {
        let p = Policy::default();
        let t0 = Instant::now();
        let mut s = Supervision::default();
        assert!(matches!(s.record_failure(t0, &p), Verdict::RetryAt(_)));
        s.record_success();
        assert!(matches!(s.record_failure(t0 + 60 * S, &p), Verdict::RetryAt(_)));
        assert_eq!(s.record_failure(t0 + 9 * 60 * S, &p), Verdict::Quarantine);
        assert!(s.is_quarantined() && !s.may_start(t0 + 3600 * S));
        s.clear();
        assert!(!s.is_quarantined() && s.may_start(t0));
    }

    #[test]
    fn failures_older_than_the_window_are_forgotten() {
        let p = Policy::default();
        let t0 = Instant::now();
        let mut s = Supervision::default();
        for i in 0..10 {
            assert!(
                matches!(s.record_failure(t0 + i * 6 * 60 * S, &p), Verdict::RetryAt(_)),
                "failure {i}"
            );
        }
        assert!(!s.is_quarantined());
    }
}
