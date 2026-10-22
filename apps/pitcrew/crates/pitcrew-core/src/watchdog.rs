//! Stall detection: the rule of the PowerShell watchdog Pitcrew replaces.
//!
//! A runner can stay alive while it no longer gets work: its polls for tasks keep timing out (seen after the
//! server or the network hiccuped, and under heavy disk and CPU load on the build machine). The rule: **at least
//! `threshold` "fetching a task timed out" events within `window`, while no job is running, means the runner is
//! stalled and is restarted.** A running job always wins: a busy runner is never restarted for stalling.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;

/// The stall rule's parameters (per runner, from the configuration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StallRule {
    /// How many timed-out polls make a stall.
    pub threshold: u32,
    /// The window they must fall in, in seconds.
    pub window_secs: u64,
}

impl Default for StallRule {
    /// Six timed-out polls in 15 minutes, as on the build machine the rule comes from (runner `fetch_timeout` 30 s).
    fn default() -> Self {
        StallRule {
            threshold: 6,
            window_secs: 15 * 60,
        }
    }
}

impl StallRule {
    fn window(&self) -> Duration {
        Duration::from_secs(self.window_secs)
    }
}

/// What the watchdog concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StallVerdict {
    /// Nothing to do.
    Healthy,
    /// Polls time out, but a job is running: wait.
    SlowButBusy {
        /// Timed-out polls inside the window.
        timeouts: u32,
    },
    /// Restart the runner.
    Stalled {
        /// Timed-out polls inside the window.
        timeouts: u32,
    },
}

/// Counts timed-out polls in a sliding window. Feed it [`StallDetector::record_timeout`] for every
/// [`crate::logline::RunnerLogEvent::FetchTimedOut`] and ask [`StallDetector::evaluate`] on every tick.
#[derive(Debug, Clone)]
pub struct StallDetector {
    rule: StallRule,
    timeouts: VecDeque<Timestamp>,
}

impl StallDetector {
    /// A detector with no history. A threshold of 0 is treated as 1 (a rule that always fires is a mistake).
    pub fn new(rule: StallRule) -> Self {
        let rule = StallRule {
            threshold: rule.threshold.max(1),
            ..rule
        };
        StallDetector {
            rule,
            timeouts: VecDeque::new(),
        }
    }

    /// Records a timed-out poll.
    pub fn record_timeout(&mut self, at: Timestamp) {
        self.timeouts.push_back(at);
        // Never keep more than the rule can use: memory stays bounded however chatty the runner is.
        while self.timeouts.len() > self.rule.threshold as usize {
            self.timeouts.pop_front();
        }
    }

    /// Forgets the history (after a restart: the new process starts with a clean slate).
    pub fn reset(&mut self) {
        self.timeouts.clear();
    }

    /// Applies the rule at `now`.
    pub fn evaluate(&mut self, now: Timestamp, job_running: bool) -> StallVerdict {
        let window = self.rule.window();
        while let Some(&first) = self.timeouts.front() {
            if now.since(first) > window {
                self.timeouts.pop_front();
            } else {
                break;
            }
        }
        let n = u32::try_from(self.timeouts.len()).unwrap_or(u32::MAX);
        if n >= self.rule.threshold {
            if job_running {
                StallVerdict::SlowButBusy { timeouts: n }
            } else {
                StallVerdict::Stalled { timeouts: n }
            }
        } else {
            StallVerdict::Healthy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn min(m: u64) -> Timestamp {
        Timestamp(m * 60_000)
    }

    #[test]
    fn six_timeouts_in_fifteen_minutes_while_idle_is_a_stall() {
        let mut d = StallDetector::new(StallRule::default());
        for m in 0..5 {
            d.record_timeout(min(m * 2));
        }
        assert_eq!(d.evaluate(min(10), false), StallVerdict::Healthy);
        d.record_timeout(min(12));
        assert_eq!(d.evaluate(min(12), false), StallVerdict::Stalled { timeouts: 6 });
    }

    #[test]
    fn a_running_job_is_never_restarted_for_stalling() {
        let mut d = StallDetector::new(StallRule::default());
        for m in 0..6 {
            d.record_timeout(min(m));
        }
        assert_eq!(d.evaluate(min(6), true), StallVerdict::SlowButBusy { timeouts: 6 });
        assert_eq!(d.evaluate(min(6), false), StallVerdict::Stalled { timeouts: 6 });
    }

    #[test]
    fn timeouts_older_than_the_window_do_not_count() {
        let mut d = StallDetector::new(StallRule::default());
        for m in [0, 1, 2, 20, 21, 22] {
            d.record_timeout(min(m));
        }
        assert_eq!(d.evaluate(min(22), false), StallVerdict::Healthy);
        // Exactly at the window's edge still counts.
        let mut edge = StallDetector::new(StallRule {
            threshold: 2,
            window_secs: 60,
        });
        edge.record_timeout(min(0));
        edge.record_timeout(min(1));
        assert_eq!(edge.evaluate(min(1), false), StallVerdict::Stalled { timeouts: 2 });
        assert_eq!(edge.evaluate(Timestamp(min(1).0 + 1), false), StallVerdict::Healthy);
    }

    #[test]
    fn reset_clears_history_and_memory_stays_bounded() {
        let mut d = StallDetector::new(StallRule::default());
        for s in 0..10_000 {
            d.record_timeout(Timestamp(s));
        }
        assert_eq!(d.timeouts.len(), 6);
        d.reset();
        assert_eq!(d.evaluate(Timestamp(10_000), false), StallVerdict::Healthy);
    }

    #[test]
    fn a_zero_threshold_is_treated_as_one() {
        let mut d = StallDetector::new(StallRule {
            threshold: 0,
            window_secs: 60,
        });
        assert_eq!(d.evaluate(min(0), false), StallVerdict::Healthy);
        d.record_timeout(min(0));
        assert_eq!(d.evaluate(min(0), false), StallVerdict::Stalled { timeouts: 1 });
    }
}
