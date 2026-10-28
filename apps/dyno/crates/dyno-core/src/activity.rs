//! The activity log: what Dyno itself reported (an alert fired, Dyno started or reset something of its own). Dyno
//! changes nothing on the machine, so nothing in the log can be undone.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// What kind of activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    /// An alert fired.
    Alert,
    /// Dyno itself (started, reset a file of its own, saw a network change).
    System,
}

/// One entry as the UI shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// Id.
    pub id: String,
    /// When.
    pub at: Timestamp,
    /// Kind.
    pub kind: ActivityKind,
    /// One line.
    pub title: String,
    /// Why or what exactly.
    pub detail: String,
}

/// The most entries the agent keeps.
pub const MAX_ENTRIES: usize = 5_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_roundtrip_as_camel_case_json() {
        let e = ActivityEntry {
            id: "a".into(),
            at: Timestamp(5),
            kind: ActivityKind::System,
            title: "Dyno started".into(),
            detail: String::new(),
        };
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["kind"], "system");
        assert_eq!(serde_json::from_value::<ActivityEntry>(j).unwrap(), e);
        const { assert!(MAX_ENTRIES > 100) };
    }
}
