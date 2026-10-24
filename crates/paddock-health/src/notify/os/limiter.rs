//! The rate limit: one toast per cause per cool-down, and at most a burst of toasts in a window, whatever their
//! causes. Debrief's `RateLimiter` keyed by the notification centre's cause key instead of a closed kind, in epoch
//! milliseconds from the injected clock. Bounded: it remembers at most [`MAX_CAUSES`] causes.

use super::settings::OsSettings;
use std::collections::{HashMap, VecDeque};

/// The most causes remembered; older ones are forgotten first (a forgotten cause may notify again, which is the safe side).
pub const MAX_CAUSES: usize = 512;

/// Why a toast was held back by the limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limited {
    /// The same cause notified within its cool-down.
    Cooldown,
    /// Too many toasts in the window.
    Burst,
}

/// What [`RateLimiter::allow`] recorded, to take back when the toast could not be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    cause: String,
    previous: Option<i64>,
    at: i64,
}

/// The limiter's memory.
#[derive(Debug, Default)]
pub struct RateLimiter {
    last: HashMap<String, i64>,
    recent: VecDeque<i64>,
}

const MS: i64 = 1000;

impl RateLimiter {
    /// An empty limiter.
    pub fn new() -> RateLimiter {
        RateLimiter::default()
    }

    /// Records and allows a toast for `cause` at `now_ms`, or says which limit holds it back (and records nothing).
    pub fn allow(
        &mut self,
        settings: &OsSettings,
        cause: &str,
        now_ms: i64,
    ) -> Result<Reservation, Limited> {
        self.check(settings, cause, now_ms)?;
        let previous = self.last.get(cause).copied();
        if self.last.len() >= MAX_CAUSES && previous.is_none() {
            self.forget_old(now_ms, i64::from(settings.cooldown_secs) * MS);
        }
        self.last.insert(cause.to_string(), now_ms);
        self.recent.push_back(now_ms);
        Ok(Reservation {
            cause: cause.to_string(),
            previous,
            at: now_ms,
        })
    }

    /// Whether [`RateLimiter::allow`] would allow a toast for `cause` at `now_ms`, without recording it. A clock that
    /// moved backwards forgets what it remembers instead of holding toasts back until it catches up.
    pub fn check(
        &mut self,
        settings: &OsSettings,
        cause: &str,
        now_ms: i64,
    ) -> Result<(), Limited> {
        if self.recent.back().is_some_and(|t| *t > now_ms) {
            self.recent.clear();
        }
        let window = i64::from(settings.burst_window_secs) * MS;
        while self.recent.front().is_some_and(|t| now_ms - *t >= window) {
            self.recent.pop_front();
        }
        let cooldown = i64::from(settings.cooldown_secs) * MS;
        if self
            .last
            .get(cause)
            .is_some_and(|t| *t <= now_ms && now_ms - *t < cooldown)
        {
            return Err(Limited::Cooldown);
        }
        if self.recent.len() >= settings.burst as usize {
            return Err(Limited::Burst);
        }
        Ok(())
    }

    /// Takes back a reservation (the OS refused the toast, so it must not count).
    pub fn undo(&mut self, r: Reservation) {
        match r.previous {
            Some(t) => self.last.insert(r.cause, t),
            None => self.last.remove(&r.cause),
        };
        if let Some(i) = self.recent.iter().rposition(|t| *t == r.at) {
            self.recent.remove(i);
        }
    }

    /// Makes room: causes past their cool-down first, then the oldest.
    fn forget_old(&mut self, now_ms: i64, cooldown: i64) {
        self.last
            .retain(|_, t| *t > now_ms || now_ms - *t < cooldown);
        while self.last.len() >= MAX_CAUSES {
            let Some(oldest) = self
                .last
                .iter()
                .min_by_key(|(_, t)| **t)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.last.remove(&oldest);
        }
    }

    /// How many causes are remembered.
    pub fn remembered(&self) -> usize {
        self.last.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(cooldown: u32, burst: u32, window: u32) -> OsSettings {
        OsSettings {
            cooldown_secs: cooldown,
            burst,
            burst_window_secs: window,
            ..OsSettings::default()
        }
    }

    #[test]
    fn a_cause_is_quiet_for_its_cooldown_and_others_are_independent() {
        let (mut l, set) = (RateLimiter::new(), s(30, 10, 300));
        assert!(l.allow(&set, "a", 0).is_ok());
        assert_eq!(l.allow(&set, "a", 29_999), Err(Limited::Cooldown));
        assert!(l.allow(&set, "b", 10_000).is_ok());
        assert!(
            l.allow(&set, "a", 30_000).is_ok(),
            "the cool-down is over exactly at its length"
        );
    }

    #[test]
    fn a_burst_of_one_hundred_posts_of_one_cause_gives_one_toast() {
        let (mut l, set) = (RateLimiter::new(), s(30, 5, 300));
        let shown = (0..100)
            .filter(|i| l.allow(&set, "job:1", i64::from(*i) * 100).is_ok())
            .count();
        assert_eq!(shown, 1);
    }

    #[test]
    fn a_hundred_causes_give_the_burst_and_the_window_slides() {
        let (mut l, set) = (RateLimiter::new(), s(30, 5, 300));
        let shown = (0..100)
            .filter(|i| l.allow(&set, &format!("c{i}"), i64::from(*i) * 100).is_ok())
            .count();
        assert_eq!(shown, 5);
        assert_eq!(l.allow(&set, "late", 9_999), Err(Limited::Burst));
        assert!(
            l.allow(&set, "later", 300_000).is_ok(),
            "the first toast left the window"
        );
    }

    #[test]
    fn a_refused_toast_is_not_recorded_and_undo_restores_the_state() {
        let (mut l, set) = (RateLimiter::new(), s(30, 2, 300));
        let r = l.allow(&set, "a", 0).unwrap();
        l.undo(r);
        assert!(
            l.allow(&set, "a", 1).is_ok(),
            "the failed toast did not start a cool-down"
        );
        let before = l.allow(&set, "b", 2).unwrap();
        assert_eq!(l.allow(&set, "c", 3), Err(Limited::Burst));
        l.undo(before);
        assert!(
            l.allow(&set, "c", 4).is_ok(),
            "and gave its burst slot back"
        );
        // Undoing a repeat restores the earlier time, not nothing.
        let (mut l, set) = (RateLimiter::new(), s(10, 9, 300));
        l.allow(&set, "a", 0).unwrap();
        let again = l.allow(&set, "a", 20_000).unwrap();
        l.undo(again);
        assert_eq!(
            l.allow(&set, "a", 25_000),
            Ok(Reservation {
                cause: "a".into(),
                previous: Some(0),
                at: 25_000
            })
        );
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_hold_toasts_back() {
        let (mut l, set) = (RateLimiter::new(), s(30, 1, 300));
        assert!(l.allow(&set, "a", 1_000_000).is_ok());
        assert!(
            l.allow(&set, "a", 5_000).is_ok(),
            "an hour back: not 'within the cool-down'"
        );
        assert!(
            l.allow(&set, "b", 5_001).is_err(),
            "but the burst still counts from the new time"
        );
    }

    #[test]
    fn the_memory_is_bounded() {
        let (mut l, set) = (RateLimiter::new(), s(3600, 50, 3600));
        for i in 0..(MAX_CAUSES * 3) {
            let t = i64::try_from(i).unwrap();
            let _ = l.allow(&set, &format!("c{i}"), t);
            // Keep the burst window empty so every cause is recorded.
            l.recent.clear();
            assert!(l.remembered() <= MAX_CAUSES);
        }
        assert_eq!(l.remembered(), MAX_CAUSES);
        assert!(
            l.allow(&set, &format!("c{}", MAX_CAUSES * 3 - 1), 99_999)
                .is_err(),
            "the newest are kept"
        );
    }
}
