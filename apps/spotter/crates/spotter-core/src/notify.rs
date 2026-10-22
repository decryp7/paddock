//! Notification policy: when a new release is worth telling the user about.

use crate::time::QuietHours;
use serde::{Deserialize, Serialize};

/// Which releases notify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotifyMode {
    /// Every release newer than the last one acknowledged.
    #[default]
    EveryNew,
    /// Only releases newer than the version the user has installed (every new one while that is unknown).
    NewerThanInstalled,
    /// Never (the watch still shows new releases).
    Off,
}

/// A watch's notification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotificationPolicy {
    /// Which releases notify.
    #[serde(default)]
    pub mode: NotifyMode,
    /// Repeat the notification at every check until the release is acknowledged (the old app's "naggy" switch).
    #[serde(default)]
    pub nag: bool,
    /// Hours when notifications wait (they are delivered at the first check after).
    #[serde(default)]
    pub quiet_hours: Option<QuietHours>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies_round_trip_with_defaults() {
        let p: NotificationPolicy = toml::from_str("").unwrap();
        assert_eq!(p, NotificationPolicy::default());
        let q: NotificationPolicy =
            toml::from_str("mode = \"newer-than-installed\"\nnag = true\nquiet_hours = { start = \"22:00\", end = \"07:00\" }").unwrap();
        assert_eq!(q.mode, NotifyMode::NewerThanInstalled);
        assert!(q.nag && q.quiet_hours.is_some());
        assert_eq!(toml::from_str::<NotificationPolicy>(&toml::to_string(&q).unwrap()).unwrap(), q);
    }
}
