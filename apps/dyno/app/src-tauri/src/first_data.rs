//! How long after a connection to the agent the first live update arrived: the last step of the login start the log tells
//! (a slow connect is the app's wait, a slow first update is the agent's). Pure over `Instant`s, so it is tested without a clock.

use std::sync::Mutex;
use std::time::Instant;

/// When the link last connected and has not yet seen a live update.
#[derive(Debug, Default)]
pub struct FirstData(Mutex<Option<Instant>>);

impl FirstData {
    /// A connection was made at `now`: the next update is the first one of it.
    pub fn connected(&self, now: Instant) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
    }

    /// A live update arrived at `now`: the milliseconds since the connection when it is the first since it, else `None`.
    pub fn first_update(&self, now: Instant) -> Option<u64> {
        let since = self.0.lock().unwrap_or_else(|e| e.into_inner()).take()?;
        Some(u64::try_from(now.saturating_duration_since(since).as_millis()).unwrap_or(u64::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn the_first_update_after_each_connection_reports_its_delay_once() {
        let t0 = Instant::now();
        let tracker = FirstData::default();
        assert_eq!(tracker.first_update(t0), None, "no connection yet: nothing to report");
        tracker.connected(t0);
        assert_eq!(tracker.first_update(t0 + Duration::from_millis(1_234)), Some(1_234));
        assert_eq!(
            tracker.first_update(t0 + Duration::from_millis(2_000)),
            None,
            "only the first update"
        );
        tracker.connected(t0 + Duration::from_secs(60));
        assert_eq!(
            tracker.first_update(t0 + Duration::from_secs(61)),
            Some(1_000),
            "a new connection starts again"
        );
    }
}
