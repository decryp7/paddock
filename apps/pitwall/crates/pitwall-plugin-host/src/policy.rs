//! The pure failure policy of one plugin: its failures within a sliding window, the backoff owed before the next
//! start, and the quarantine after [`Policy::max_failures`] failures within [`Policy::window`]. Time is passed in, so
//! every rule is unit-tested without clocks or processes.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How failures are judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Failures within [`Policy::window`] that quarantine the plugin.
    pub max_failures: usize,
    /// The sliding window failures are counted in.
    pub window: Duration,
    /// The backoff after the first failure; it doubles with each further one.
    pub backoff_base: Duration,
    /// The longest backoff.
    pub backoff_max: Duration,
}

impl Default for Policy {
    /// 3 failures in 10 minutes quarantine; backoff 250 ms doubling to 30 s.
    fn default() -> Policy {
        Policy {
            max_failures: 3,
            window: Duration::from_secs(600),
            backoff_base: Duration::from_millis(250),
            backoff_max: Duration::from_secs(30),
        }
    }
}

/// Delay before a restart after `recent` failures: `base`, doubling per failure, at most `max`; zero after none.
pub fn backoff_delay(base: Duration, max: Duration, recent: usize) -> Duration {
    if recent == 0 {
        return Duration::ZERO;
    }
    let exp = u32::try_from(recent - 1).unwrap_or(u32::MAX).min(31);
    base.saturating_mul(2u32.saturating_pow(exp)).min(max)
}

/// The failure history of one plugin across restarts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    failures: VecDeque<Instant>,
    quarantined: Option<String>,
    last: Option<String>,
    restarts: u32,
}

impl Ledger {
    fn prune(&mut self, policy: &Policy, now: Instant) {
        while self.failures.front().is_some_and(|t| now.saturating_duration_since(*t) > policy.window) {
            self.failures.pop_front();
        }
    }

    /// Records a failure at `now` for `reason`; returns whether the plugin is quarantined by it.
    pub fn record_failure(&mut self, policy: &Policy, now: Instant, reason: &str) -> bool {
        self.failures.push_back(now);
        // Bounded: only the window's failures matter, and at most `max_failures` of them.
        while self.failures.len() > policy.max_failures.max(1) {
            self.failures.pop_front();
        }
        self.prune(policy, now);
        self.last = Some(reason.to_string());
        if self.quarantined.is_none() && self.failures.len() >= policy.max_failures.max(1) {
            let minutes = (policy.window.as_secs() / 60).max(1);
            self.quarantined = Some(format!(
                "it failed {} times in {minutes} minutes (last: {reason}) and was stopped; reset it to try again",
                self.failures.len()
            ));
            return true;
        }
        false
    }

    /// Failures within the window at `now`.
    pub fn recent(&mut self, policy: &Policy, now: Instant) -> usize {
        self.prune(policy, now);
        self.failures.len()
    }

    /// How long a start must still wait at `now` (zero when it may start).
    pub fn retry_after(&mut self, policy: &Policy, now: Instant) -> Duration {
        let recent = self.recent(policy, now);
        let Some(last) = self.failures.back() else {
            return Duration::ZERO;
        };
        backoff_delay(policy.backoff_base, policy.backoff_max, recent).saturating_sub(now.saturating_duration_since(*last))
    }

    /// Why the plugin is quarantined, if it is.
    pub fn quarantined(&self) -> Option<&str> {
        self.quarantined.as_deref()
    }

    /// The last failure.
    pub fn last_failure(&self) -> Option<&str> {
        self.last.as_deref()
    }

    /// Counts a restart (a start after a failure).
    pub fn count_restart(&mut self) {
        self.restarts = self.restarts.saturating_add(1);
    }

    /// Restarts so far.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    /// Lifts the quarantine and forgets the failures (the user asked to try again).
    pub fn reset(&mut self) {
        self.failures.clear();
        self.quarantined = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            max_failures: 3,
            window: Duration::from_secs(60),
            backoff_base: Duration::from_millis(100),
            backoff_max: Duration::from_millis(350),
        }
    }

    #[test]
    fn backoff_doubles_up_to_its_maximum() {
        let (b, m) = (Duration::from_millis(100), Duration::from_secs(1));
        assert_eq!(backoff_delay(b, m, 0), Duration::ZERO);
        assert_eq!(backoff_delay(b, m, 1), b);
        assert_eq!(backoff_delay(b, m, 2), b * 2);
        assert_eq!(backoff_delay(b, m, 3), b * 4);
        assert_eq!(backoff_delay(b, m, 10), m);
        assert_eq!(backoff_delay(b, m, usize::MAX), m);
    }

    #[test]
    fn three_failures_in_the_window_quarantine() {
        let p = policy();
        let t0 = Instant::now();
        let mut l = Ledger::default();
        assert!(!l.record_failure(&p, t0, "crash 1"));
        assert!(!l.record_failure(&p, t0 + Duration::from_secs(1), "crash 2"));
        assert_eq!(l.quarantined(), None);
        assert!(l.record_failure(&p, t0 + Duration::from_secs(2), "hang"));
        let why = l.quarantined().unwrap();
        assert!(why.contains("3 times in 1 minutes") && why.contains("last: hang"), "{why}");
        // Further failures keep the first reason.
        assert!(!l.record_failure(&p, t0 + Duration::from_secs(3), "x"));
        assert!(l.quarantined().unwrap().contains("hang"));
        assert_eq!(l.last_failure(), Some("x"));
        l.reset();
        assert_eq!(l.quarantined(), None);
        assert_eq!(l.recent(&p, t0 + Duration::from_secs(3)), 0);
    }

    #[test]
    fn failures_outside_the_window_are_forgotten() {
        let p = policy();
        let t0 = Instant::now();
        let mut l = Ledger::default();
        l.record_failure(&p, t0, "a");
        l.record_failure(&p, t0 + Duration::from_secs(10), "b");
        assert_eq!(l.recent(&p, t0 + Duration::from_secs(30)), 2);
        assert_eq!(l.recent(&p, t0 + Duration::from_secs(65)), 1);
        // A third failure late enough is not a quarantine.
        assert!(!l.record_failure(&p, t0 + Duration::from_secs(65), "c"));
        assert_eq!(l.recent(&p, t0 + Duration::from_secs(65)), 2);
    }

    #[test]
    fn the_backoff_is_owed_from_the_last_failure() {
        let p = policy();
        let t0 = Instant::now();
        let mut l = Ledger::default();
        assert_eq!(l.retry_after(&p, t0), Duration::ZERO, "a clean history starts at once");
        l.record_failure(&p, t0, "a");
        assert_eq!(l.retry_after(&p, t0), Duration::from_millis(100));
        assert_eq!(l.retry_after(&p, t0 + Duration::from_millis(40)), Duration::from_millis(60));
        assert_eq!(l.retry_after(&p, t0 + Duration::from_millis(500)), Duration::ZERO);
        l.record_failure(&p, t0 + Duration::from_millis(500), "b");
        assert_eq!(l.retry_after(&p, t0 + Duration::from_millis(500)), Duration::from_millis(200));
        // Out of the window: nothing owed.
        assert_eq!(l.retry_after(&p, t0 + Duration::from_secs(120)), Duration::ZERO);
        l.count_restart();
        assert_eq!(l.restarts(), 1);
        assert_eq!(Policy::default().max_failures, 3);
    }

    #[test]
    fn a_policy_of_zero_failures_still_needs_one() {
        let p = Policy { max_failures: 0, ..policy() };
        let mut l = Ledger::default();
        assert!(l.record_failure(&p, Instant::now(), "x"));
    }
}
