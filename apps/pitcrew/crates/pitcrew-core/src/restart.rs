//! Restarting a runner that exited: exponential backoff and crash-loop protection.
//!
//! The backoff reproduces the PowerShell supervisor: 10 s after an exit, doubling up to 5 minutes while the runner
//! keeps failing, back to 10 s once it ran for 10 minutes. On top of that, a runner that exits
//! `crash_loop_restarts` times within `crash_loop_window` is in a crash loop: Pitcrew stops restarting it for
//! `crash_loop_cooldown`, tells the user, and a manual start clears the state.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;

/// Restart parameters (per runner, from the configuration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartPolicy {
    /// The first delay, in seconds.
    pub initial_delay_secs: u64,
    /// The longest delay, in seconds.
    pub max_delay_secs: u64,
    /// A run at least this long (seconds) resets the delay.
    pub reset_after_secs: u64,
    /// Exits within the window that make a crash loop.
    pub crash_loop_restarts: u32,
    /// The crash-loop window, in seconds.
    pub crash_loop_window_secs: u64,
    /// How long to wait after a crash loop before trying again by itself, in seconds.
    pub crash_loop_cooldown_secs: u64,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy {
            initial_delay_secs: 10,
            max_delay_secs: 300,
            reset_after_secs: 600,
            crash_loop_restarts: 5,
            crash_loop_window_secs: 600,
            crash_loop_cooldown_secs: 1800,
        }
    }
}

/// Why the runner is being restarted (shown in the log and the status view).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RestartReason {
    /// The process exited by itself.
    Exited,
    /// The watchdog found it stalled.
    Stalled,
}

/// What to do after an exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RestartDecision {
    /// Start again at `at`.
    RestartAt {
        /// When.
        at: Timestamp,
        /// The delay that was applied, in seconds.
        delay_secs: u64,
    },
    /// Too many exits: wait until `until` (or a manual start) and notify the user.
    CrashLoop {
        /// When Pitcrew tries again by itself.
        until: Timestamp,
        /// Exits inside the window.
        exits: u32,
    },
}

/// Remembers recent exits and the current delay of one runner.
#[derive(Debug, Clone)]
pub struct RestartTracker {
    policy: RestartPolicy,
    delay_secs: u64,
    exits: VecDeque<Timestamp>,
}

impl RestartTracker {
    /// A tracker with no history.
    pub fn new(policy: RestartPolicy) -> Self {
        let initial = policy.initial_delay_secs.max(1);
        RestartTracker {
            policy,
            delay_secs: initial,
            exits: VecDeque::new(),
        }
    }

    /// The runner started at `started` exited (or was stopped by the watchdog) at `exited`.
    pub fn on_exit(&mut self, started: Timestamp, exited: Timestamp, reason: RestartReason) -> RestartDecision {
        let p = self.policy;
        let initial = p.initial_delay_secs.max(1);
        let max = p.max_delay_secs.max(initial);
        let ran = exited.since(started);
        // A stall restart is the watchdog's decision, not a failure of the runner: it does not grow the delay.
        let delay = if ran >= Duration::from_secs(p.reset_after_secs) || reason == RestartReason::Stalled {
            initial
        } else {
            self.delay_secs
        };
        self.delay_secs = delay.saturating_mul(2).min(max);

        if reason == RestartReason::Exited {
            self.exits.push_back(exited);
        }
        let window = Duration::from_secs(p.crash_loop_window_secs);
        while let Some(&first) = self.exits.front() {
            if exited.since(first) > window {
                self.exits.pop_front();
            } else {
                break;
            }
        }
        let n = u32::try_from(self.exits.len()).unwrap_or(u32::MAX);
        if p.crash_loop_restarts > 0 && n >= p.crash_loop_restarts {
            return RestartDecision::CrashLoop {
                until: exited.plus(Duration::from_secs(p.crash_loop_cooldown_secs)),
                exits: n,
            };
        }
        RestartDecision::RestartAt {
            at: exited.plus(Duration::from_secs(delay)),
            delay_secs: delay,
        }
    }

    /// A manual start (or the end of a cool-down): history and delay start over.
    pub fn clear(&mut self) {
        self.exits.clear();
        self.delay_secs = self.policy.initial_delay_secs.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(secs: u64) -> Timestamp {
        Timestamp(secs * 1000)
    }

    fn quiet() -> RestartPolicy {
        // No crash-loop limit, to look at the backoff alone.
        RestartPolicy {
            crash_loop_restarts: 0,
            ..RestartPolicy::default()
        }
    }

    fn delay(d: RestartDecision) -> u64 {
        match d {
            RestartDecision::RestartAt { delay_secs, .. } => delay_secs,
            other => panic!("expected a restart, got {other:?}"),
        }
    }

    #[test]
    fn delay_doubles_up_to_five_minutes_and_resets_after_ten_minutes_of_running() {
        let mut t = RestartTracker::new(quiet());
        let mut now = 0;
        let mut seen = vec![];
        for _ in 0..7 {
            let d = delay(t.on_exit(s(now), s(now + 1), RestartReason::Exited));
            seen.push(d);
            now += 1 + d;
        }
        assert_eq!(seen, vec![10, 20, 40, 80, 160, 300, 300]);
        assert_eq!(delay(t.on_exit(s(now), s(now + 600), RestartReason::Exited)), 10);
        assert_eq!(delay(t.on_exit(s(now + 700), s(now + 701), RestartReason::Exited)), 20);
    }

    #[test]
    fn restart_time_is_exit_plus_delay() {
        let mut t = RestartTracker::new(quiet());
        assert_eq!(
            t.on_exit(s(0), s(5), RestartReason::Exited),
            RestartDecision::RestartAt { at: s(15), delay_secs: 10 }
        );
    }

    #[test]
    fn stall_restarts_use_the_short_delay_and_are_not_crashes() {
        let mut t = RestartTracker::new(RestartPolicy::default());
        for i in 0..10 {
            assert_eq!(delay(t.on_exit(s(i * 20), s(i * 20 + 1), RestartReason::Stalled)), 10);
        }
    }

    #[test]
    fn five_exits_in_ten_minutes_is_a_crash_loop() {
        let mut t = RestartTracker::new(RestartPolicy::default());
        for i in 0..4 {
            assert!(matches!(
                t.on_exit(s(i * 30), s(i * 30 + 1), RestartReason::Exited),
                RestartDecision::RestartAt { .. }
            ));
        }
        assert_eq!(
            t.on_exit(s(120), s(121), RestartReason::Exited),
            RestartDecision::CrashLoop {
                until: s(121 + 1800),
                exits: 5
            }
        );
        t.clear();
        assert_eq!(delay(t.on_exit(s(200), s(201), RestartReason::Exited)), 10);
    }

    #[test]
    fn exits_spread_out_are_not_a_crash_loop() {
        let mut t = RestartTracker::new(RestartPolicy::default());
        for i in 0..20 {
            assert!(matches!(
                t.on_exit(s(i * 200), s(i * 200 + 1), RestartReason::Exited),
                RestartDecision::RestartAt { .. }
            ));
        }
    }

    #[test]
    fn nonsense_policies_still_give_sane_delays() {
        let p = RestartPolicy {
            initial_delay_secs: 0,
            max_delay_secs: 0,
            ..quiet()
        };
        let mut t = RestartTracker::new(p);
        assert_eq!(delay(t.on_exit(s(0), s(1), RestartReason::Exited)), 1);
        assert_eq!(delay(t.on_exit(s(2), s(3), RestartReason::Exited)), 1);
    }
}
