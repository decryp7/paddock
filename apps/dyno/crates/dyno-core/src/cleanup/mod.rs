//! Cleanup: the category catalog, the safety policy and scheduling (ADR 0005).

pub mod catalog;
pub mod policy;

pub use catalog::*;
pub use policy::*;

use crate::time::{Timestamp, DAY};
use serde::{Deserialize, Serialize};

/// How often scheduled cleanups run (only the categories the user enabled).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CleanupSchedule {
    /// Never.
    #[default]
    Off,
    /// Once a day.
    Daily,
    /// Once a week.
    Weekly,
}

/// Whether a scheduled cleanup is due.
pub fn due(schedule: CleanupSchedule, last_run: Option<Timestamp>, now: Timestamp) -> bool {
    let every = match schedule {
        CleanupSchedule::Off => return false,
        CleanupSchedule::Daily => DAY,
        CleanupSchedule::Weekly => 7 * DAY,
    };
    last_run.is_none_or(|l| now.since(l) >= every)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules() {
        assert!(!due(CleanupSchedule::Off, None, Timestamp(0)));
        assert!(due(CleanupSchedule::Daily, None, Timestamp(0)));
        assert!(!due(CleanupSchedule::Daily, Some(Timestamp(0)), Timestamp(DAY - 1)));
        assert!(due(CleanupSchedule::Daily, Some(Timestamp(0)), Timestamp(DAY)));
        assert!(!due(CleanupSchedule::Weekly, Some(Timestamp(0)), Timestamp(6 * DAY)));
    }
}
