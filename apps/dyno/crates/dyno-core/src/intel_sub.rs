//! A list the user subscribed to by URL (ADR 0031 amendment 1): the pure parts. The URL is validated here, the schedule floor is
//! computed here, and the status the page shows is defined here; the agent does the fetching through `paddock-net`'s `Egress`
//! (opt-in per subscription, refused in Offline mode) and passes time in as values.
//!
//! The list's terms are the user's responsibility: Dyno never adds a list of its own, and the add dialog says so
//! ([`TERMS_NOTICE`]).

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Shown where the user adds a URL.
pub const TERMS_NOTICE: &str =
    "The terms of a list are your responsibility: check that you may use it and how often it may be fetched. Dyno sends only the \
     request for the file, with its name and version as the User-Agent, to the address you gave.";
/// The shortest interval between two fetches of one subscription (also "Update now").
pub const MIN_INTERVAL: Duration = Duration::from_secs(3600);
/// The interval a new subscription starts with.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// The longest interval the user may choose.
pub const MAX_INTERVAL: Duration = Duration::from_secs(30 * 24 * 3600);
/// Longest address accepted, in bytes.
pub const MAX_URL: usize = 2048;
/// Largest list file read, in bytes.
pub const MAX_BYTES: u64 = 32 << 20;
/// A fetched list whose entry count fell below this share of the last good copy is not taken (the old copy stays in use).
pub const MIN_KEEP_PERCENT: u64 = 10;

/// Why an address cannot be a subscription (the text says what to change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    /// Empty, too long, or with spaces or control characters.
    #[error("The address must be 1 to {MAX_URL} characters without spaces.")]
    Shape,
    /// Not https.
    #[error("The address must start with https://. Plain http is not used.")]
    NotHttps,
    /// A user name or password in the address.
    #[error("The address must not contain a user name or password.")]
    Credentials,
    /// No host.
    #[error("The address needs a host name.")]
    Host,
}

/// Checks a subscription address. The network layer checks it again (https only, public reach, no login) before every request.
pub fn validate_url(text: &str) -> Result<String, UrlError> {
    let t = text.trim();
    if t.is_empty() || t.len() > MAX_URL || t.chars().any(|c| c.is_control() || c.is_whitespace() || !c.is_ascii()) {
        return Err(UrlError::Shape);
    }
    let Some(rest) = t.strip_prefix("https://") else {
        return Err(UrlError::NotHttps);
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(UrlError::Credentials);
    }
    if authority.is_empty() || authority.starts_with(':') {
        return Err(UrlError::Host);
    }
    Ok(t.to_string())
}

/// An update interval in whole hours, within [`MIN_INTERVAL`] and [`MAX_INTERVAL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Hours(u32);

impl Hours {
    /// Clamps to the allowed range.
    pub fn new(hours: u32) -> Hours {
        let min = (MIN_INTERVAL.as_secs() / 3600) as u32;
        let max = (MAX_INTERVAL.as_secs() / 3600) as u32;
        Hours(hours.clamp(min, max))
    }

    /// The default (daily).
    pub fn daily() -> Hours {
        Hours::new((DEFAULT_INTERVAL.as_secs() / 3600) as u32)
    }

    /// As a duration.
    pub fn duration(self) -> Duration {
        Duration::from_secs(u64::from(self.0) * 3600)
    }

    /// The number of hours.
    pub fn get(self) -> u32 {
        self.0
    }
}

/// How a subscription is refreshed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Schedule {
    /// Only when the user presses "Update now".
    Manual,
    /// Every `every`, and once at start-up when a run was missed.
    Automatic {
        /// The interval.
        every: Hours,
    },
}

/// When a subscription may be fetched again (milliseconds since the Unix epoch), or `None` when it may be fetched now. The floor
/// is [`MIN_INTERVAL`] for "Update now" and for automatic runs; an automatic run waits for the chosen interval. `last_attempt_ms`
/// is kept outside the cache folder, so clearing the cache cannot cause an early fetch.
pub fn next_allowed(last_attempt_ms: Option<i64>, now_ms: i64, floor: Duration) -> Option<i64> {
    let last = last_attempt_ms?;
    let at = last.saturating_add(i64::try_from(floor.as_millis()).unwrap_or(i64::MAX));
    // A clock that moved back (last in the future) must not lock the list for longer than the floor.
    if last > now_ms {
        return Some(now_ms.saturating_add(i64::try_from(floor.as_millis()).unwrap_or(i64::MAX)));
    }
    (at > now_ms).then_some(at)
}

/// When the next automatic run is due (ms since the epoch); a subscription never fetched is due now. A run missed while the
/// computer slept is due once, at wake.
pub fn automatic_due(schedule: Schedule, last_attempt_ms: Option<i64>, now_ms: i64) -> bool {
    match schedule {
        Schedule::Manual => false,
        Schedule::Automatic { every } => next_allowed(last_attempt_ms, now_ms, every.duration().max(MIN_INTERVAL)).is_none(),
    }
}

/// What the page shows for one subscription.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubscriptionStatus {
    /// When a list was last taken, ms since the epoch.
    pub last_fetched_ms: Option<i64>,
    /// When a fetch was last tried (the floor counts from here).
    pub last_attempt_ms: Option<i64>,
    /// Entries in the list in use.
    pub entries: u32,
    /// Exact duplicates removed from it.
    pub duplicates: u32,
    /// Entries removed because a wider entry covers them.
    pub covered: u32,
    /// Lines that were not taken.
    pub skipped: u32,
    /// The last error in words, with the next step; `None` after a good run.
    pub error: Option<String>,
    /// A note that is not an error ("The new list has 3 entries instead of 1,200; kept the previous one").
    pub note: Option<String>,
}

/// Whether a freshly fetched list with `new_entries` is a large drop against the last good copy of `old_entries`.
pub fn is_large_drop(old_entries: u32, new_entries: u32) -> bool {
    (new_entries == 0 && old_entries > 0) || (u64::from(new_entries) * 100 < u64::from(old_entries) * MIN_KEEP_PERCENT && old_entries >= 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_https_addresses_without_a_login_are_accepted() {
        assert_eq!(
            validate_url(" https://lists.example/bad.txt ").unwrap(),
            "https://lists.example/bad.txt"
        );
        assert_eq!(validate_url("http://lists.example/x"), Err(UrlError::NotHttps));
        assert_eq!(validate_url("file:///etc/passwd"), Err(UrlError::NotHttps));
        assert_eq!(validate_url("https://user:pw@lists.example/x"), Err(UrlError::Credentials));
        assert_eq!(validate_url("https:///x"), Err(UrlError::Host));
        assert_eq!(validate_url(""), Err(UrlError::Shape));
        assert_eq!(validate_url("https://a b/x"), Err(UrlError::Shape));
        assert_eq!(
            validate_url(&format!("https://a.example/{}", "x".repeat(MAX_URL))),
            Err(UrlError::Shape)
        );
    }

    #[test]
    fn the_interval_is_clamped_between_hourly_and_monthly() {
        assert_eq!(Hours::new(0).get(), 1);
        assert_eq!(Hours::new(5000).get(), 720);
        assert_eq!(Hours::daily().duration(), DEFAULT_INTERVAL);
    }

    #[test]
    fn the_floor_counts_from_the_last_attempt_and_survives_a_cleared_cache() {
        let h = 3_600_000;
        assert_eq!(next_allowed(None, 1_000, MIN_INTERVAL), None, "never fetched: now");
        assert_eq!(
            next_allowed(Some(0), 30 * 60_000, MIN_INTERVAL),
            Some(h),
            "30 minutes in: allowed at the hour"
        );
        assert_eq!(next_allowed(Some(0), h, MIN_INTERVAL), None, "exactly at the floor is allowed");
        // The clock moved back: wait at most one floor from now, never longer.
        assert_eq!(next_allowed(Some(10 * h), h, MIN_INTERVAL), Some(2 * h));
    }

    #[test]
    fn automatic_runs_follow_the_interval_and_manual_ones_never_start_alone() {
        let day = Schedule::Automatic { every: Hours::daily() };
        let h = 3_600_000;
        assert!(automatic_due(day, None, 0));
        assert!(!automatic_due(day, Some(0), 23 * h));
        assert!(automatic_due(day, Some(0), 24 * h));
        assert!(automatic_due(day, Some(0), 10 * 24 * h), "slept through ten days: due once");
        assert!(!automatic_due(Schedule::Manual, None, 0));
    }

    #[test]
    fn a_large_drop_is_not_taken() {
        assert!(is_large_drop(1200, 3));
        assert!(is_large_drop(1200, 0));
        assert!(!is_large_drop(1200, 200));
        assert!(!is_large_drop(5, 1), "tiny lists may change freely");
        assert!(!is_large_drop(0, 0));
    }
}
