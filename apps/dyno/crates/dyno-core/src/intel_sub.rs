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
#[serde(rename_all = "camelCase")]
pub enum Schedule {
    /// Only when the user presses "Update now".
    Manual,
    /// Every `every`, and once at start-up when a run was missed.
    Automatic {
        /// The interval.
        every: Hours,
    },
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
}
