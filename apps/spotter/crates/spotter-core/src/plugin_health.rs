//! Plugin quarantine: a plugin that crashes, hangs, exceeds its memory limit or breaks the protocol gets a strike;
//! [`MAX_STRIKES`] strikes within [`STRIKE_WINDOW_MS`] quarantine it until the user re-enables it. Ordinary errors
//! the plugin reports (unreachable, unauthorized, …) are not strikes.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Strikes that quarantine a plugin.
pub const MAX_STRIKES: usize = 3;
/// The window strikes are counted in (30 minutes).
pub const STRIKE_WINDOW_MS: i64 = 30 * 60 * 1000;

/// A plugin's health.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginHealth {
    /// Recent strikes.
    pub strikes: Vec<Timestamp>,
    /// Quarantined: not started until reset.
    pub quarantined: bool,
    /// The last problem, for the plugin manager.
    pub last_problem: Option<String>,
}

impl PluginHealth {
    /// Records a strike; returns whether this one quarantined the plugin.
    pub fn strike(&mut self, now: Timestamp, problem: String) -> bool {
        self.strikes.retain(|t| now.0 - t.0 < STRIKE_WINDOW_MS);
        self.strikes.push(now);
        self.last_problem = Some(problem);
        if !self.quarantined && self.strikes.len() >= MAX_STRIKES {
            self.quarantined = true;
            return true;
        }
        false
    }

    /// Lifts the quarantine (the user's "Enable again").
    pub fn reset(&mut self) {
        *self = PluginHealth::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_strikes_quarantine_and_old_ones_expire() {
        let mut h = PluginHealth::default();
        assert!(!h.strike(Timestamp(0), "crash".into()));
        assert!(
            !h.strike(Timestamp(STRIKE_WINDOW_MS + 1), "crash".into()),
            "the first strike expired"
        );
        assert!(!h.strike(Timestamp(STRIKE_WINDOW_MS + 2), "hang".into()));
        assert!(h.strike(Timestamp(STRIKE_WINDOW_MS + 3), "memory".into()));
        assert!(h.quarantined);
        assert!(
            !h.strike(Timestamp(STRIKE_WINDOW_MS + 4), "again".into()),
            "only the first quarantine is news"
        );
        assert_eq!(h.last_problem.as_deref(), Some("again"));
        h.reset();
        assert_eq!(h, PluginHealth::default());
    }
}
