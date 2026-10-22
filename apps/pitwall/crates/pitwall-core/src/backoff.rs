//! Restarting engine processes (ADR 0004): exponential backoff from 1 s to 60 s, reset after a process stayed
//! healthy for five minutes, and give up after too many crashes in ten minutes (the model is marked failed and the
//! next request reports why instead of crash-looping).

use crate::time::{Millis, MINUTE, SECOND};

/// The restart policy's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// First delay.
    pub initial: Millis,
    /// Longest delay.
    pub max: Millis,
    /// Healthy this long resets the backoff.
    pub reset_after: Millis,
    /// Crashes within `window` before giving up.
    pub max_crashes: usize,
    /// The crash window.
    pub window: Millis,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            initial: SECOND,
            max: MINUTE,
            reset_after: 5 * MINUTE,
            max_crashes: 5,
            window: 10 * MINUTE,
        }
    }
}

/// What to do after a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Restart after this delay.
    RestartIn(Millis),
    /// Stop trying.
    GiveUp,
}

/// One process's crash history.
#[derive(Debug, Clone, Default)]
pub struct Backoff {
    limits: Limits,
    crashes: Vec<Millis>,
    attempt: u32,
    healthy_since: Option<Millis>,
}

impl Backoff {
    /// A fresh history.
    pub fn new(limits: Limits) -> Backoff {
        Backoff {
            limits,
            crashes: vec![],
            attempt: 0,
            healthy_since: None,
        }
    }

    /// The process reported healthy at `now`.
    pub fn healthy(&mut self, now: Millis) {
        let since = *self.healthy_since.get_or_insert(now);
        if now.saturating_sub(since) >= self.limits.reset_after {
            self.attempt = 0;
        }
    }

    /// The process crashed at `now`.
    pub fn crashed(&mut self, now: Millis) -> Decision {
        self.healthy(now);
        self.healthy_since = None;
        self.crashes.retain(|t| now.saturating_sub(*t) < self.limits.window);
        self.crashes.push(now);
        if self.crashes.len() > self.limits.max_crashes {
            return Decision::GiveUp;
        }
        let delay = self
            .limits
            .initial
            .saturating_mul(1u64 << self.attempt.min(20))
            .min(self.limits.max);
        self.attempt += 1;
        Decision::RestartIn(delay)
    }

    /// Crashes in the current window.
    pub fn recent_crashes(&self) -> usize {
        self.crashes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_caps_resets_and_gives_up() {
        let mut b = Backoff::new(Limits::default());
        assert_eq!(b.crashed(0), Decision::RestartIn(SECOND));
        assert_eq!(b.crashed(1), Decision::RestartIn(2 * SECOND));
        assert_eq!(b.crashed(2), Decision::RestartIn(4 * SECOND));
        assert_eq!(b.recent_crashes(), 3);
        // Healthy for five minutes: back to one second.
        b.healthy(10);
        b.healthy(10 + 5 * MINUTE);
        assert_eq!(b.crashed(11 + 5 * MINUTE), Decision::RestartIn(SECOND));
        let mut b = Backoff::new(Limits {
            max_crashes: 100,
            ..Limits::default()
        });
        let mut last = Decision::GiveUp;
        for i in 0..10 {
            last = b.crashed(i);
        }
        assert_eq!(last, Decision::RestartIn(MINUTE), "capped");
        let mut b = Backoff::new(Limits::default());
        for i in 0..5 {
            assert!(matches!(b.crashed(i), Decision::RestartIn(_)));
        }
        assert_eq!(b.crashed(6), Decision::GiveUp);
        // Old crashes leave the window.
        let mut b = Backoff::new(Limits::default());
        for i in 0..5 {
            b.crashed(i * 11 * MINUTE);
        }
        assert_eq!(b.recent_crashes(), 1);
        assert_eq!(Backoff::default().recent_crashes(), 0);
    }
}
