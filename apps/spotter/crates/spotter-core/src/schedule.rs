//! When to check next: the watch's interval with jitter (so many watches on one server do not fire together) and
//! exponential backoff after failures (so an offline share or a rate-limited API is not hammered).

use crate::limits::{MAX_BACKOFF_SECS, MAX_INTERVAL_SECS, MIN_INTERVAL_SECS};

/// Clamps an interval to the allowed range.
pub fn clamp_interval(secs: u32) -> u32 {
    secs.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS)
}

/// Seconds until the next check after `failures` consecutive failures (0 after a success). `jitter` is a random
/// number in `[0, 1)` from the caller; it spreads the delay by ±10 %. After failures the delay doubles per failure
/// up to [`MAX_BACKOFF_SECS`] (or the interval, when that is longer).
pub fn next_delay_secs(interval_secs: u32, failures: u32, jitter: f64) -> u64 {
    let interval = u64::from(clamp_interval(interval_secs));
    let base = if failures == 0 {
        interval
    } else {
        let cap = interval.max(u64::from(MAX_BACKOFF_SECS));
        interval.saturating_mul(1u64 << failures.min(16)).min(cap)
    };
    let jitter = if jitter.is_finite() { jitter.clamp(0.0, 1.0) } else { 0.5 };
    let factor = 0.9 + 0.2 * jitter;
    ((base as f64) * factor).round().max(f64::from(MIN_INTERVAL_SECS)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_jitter_and_back_off() {
        assert_eq!(next_delay_secs(60, 0, 0.5), 60);
        assert_eq!(next_delay_secs(60, 0, 0.0), 54);
        assert_eq!(next_delay_secs(60, 0, 1.0), 66);
        assert_eq!(next_delay_secs(60, 1, 0.5), 120);
        assert_eq!(next_delay_secs(60, 3, 0.5), 480);
        assert_eq!(next_delay_secs(60, 40, 0.5), u64::from(MAX_BACKOFF_SECS));
        assert_eq!(
            next_delay_secs(MAX_INTERVAL_SECS, 5, 0.5),
            u64::from(MAX_INTERVAL_SECS),
            "long intervals cap at themselves"
        );
        assert_eq!(next_delay_secs(1, 0, 0.0), u64::from(MIN_INTERVAL_SECS), "never below the minimum");
        assert_eq!(next_delay_secs(60, 0, f64::NAN), 60);
        assert_eq!(next_delay_secs(60, 0, 7.0), 66);
        assert_eq!(clamp_interval(u32::MAX), MAX_INTERVAL_SECS);
    }
}
