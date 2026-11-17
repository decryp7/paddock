//! RFC 3339 UTC stamps with a fixed number of fractional digits, the one formatter of the family's logs, reports and
//! trails (it replaces chrono's `to_rfc3339_opts(.., true)`; the text is byte-identical: always `Z`, always the same
//! number of digits, truncated and never rounded).

use crate::time::Clock;
use jiff::Timestamp;

/// How many fractional-second digits a stamp carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// `2026-10-20T09:00:00Z`.
    Secs,
    /// `2026-10-20T09:00:00.123Z`.
    Millis,
    /// `2026-10-20T09:00:00.123456Z`.
    Micros,
}

impl Precision {
    fn format(self) -> &'static str {
        match self {
            Precision::Secs => "%Y-%m-%dT%H:%M:%SZ",
            Precision::Millis => "%Y-%m-%dT%H:%M:%S.%3fZ",
            Precision::Micros => "%Y-%m-%dT%H:%M:%S.%6fZ",
        }
    }
}

/// Formats `ts` as RFC 3339 in UTC with the given precision.
pub fn rfc3339(ts: Timestamp, precision: Precision) -> String {
    ts.strftime(precision.format()).to_string()
}

/// The calendar time of `clock` as a [`Timestamp`] (a reading outside jiff's range saturates to its bounds).
pub fn wall_timestamp(clock: &dyn Clock) -> Timestamp {
    Timestamp::try_from(clock.wall()).unwrap_or_else(|_| {
        if clock.wall_ms() < 0 {
            Timestamp::MIN
        } else {
            Timestamp::MAX
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64, nanos: i32) -> Timestamp {
        Timestamp::new(secs, nanos).unwrap()
    }

    #[test]
    fn formats_match_the_text_chrono_wrote() {
        // 2026-10-20T09:00:00Z, with a nanosecond part that must be truncated, not rounded.
        let t = at(1_792_486_800, 123_456_789);
        assert_eq!(rfc3339(t, Precision::Secs), "2026-10-20T09:00:00Z");
        assert_eq!(rfc3339(t, Precision::Millis), "2026-10-20T09:00:00.123Z");
        assert_eq!(rfc3339(t, Precision::Micros), "2026-10-20T09:00:00.123456Z");
    }

    #[test]
    fn a_whole_second_keeps_its_zero_digits() {
        let t = at(1_792_486_800, 0);
        assert_eq!(rfc3339(t, Precision::Millis), "2026-10-20T09:00:00.000Z");
        assert_eq!(rfc3339(t, Precision::Micros), "2026-10-20T09:00:00.000000Z");
    }

    #[test]
    fn the_fake_clock_reads_back_as_a_timestamp() {
        let c = crate::time::FakeClock::at_wall_ms(1_792_486_800_250);
        assert_eq!(
            rfc3339(wall_timestamp(&c), Precision::Millis),
            "2026-10-20T09:00:00.250Z"
        );
    }
}
