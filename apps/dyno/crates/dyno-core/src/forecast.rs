//! "Disk full in N days": a least-squares line through the free space of the last days.

use crate::stats::linear_fit;
use crate::time::{Timestamp, DAY, HOUR};
use serde::{Deserialize, Serialize};

/// When a disk will be full.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Forecast {
    /// At the current trend, full in this many days.
    FullInDays(f32),
    /// Free space is stable or growing (or full is more than a year away).
    NotFilling,
    /// Less than 12 hours or fewer than 6 points of history.
    NotEnoughData,
}

/// One free-space reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskTrendPoint {
    /// When.
    pub at: Timestamp,
    /// Free bytes.
    pub free: u64,
}

/// A disk's free-space trend and forecast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskTrend {
    /// Disk name.
    pub disk: String,
    /// Capacity in bytes.
    pub total: u64,
    /// Readings.
    pub points: Vec<DiskTrendPoint>,
    /// The forecast.
    pub forecast: Forecast,
}

/// Days further than this count as "not filling".
pub const HORIZON_DAYS: f32 = 365.0;

/// Forecasts from readings (any order); uses the last 14 days.
pub fn disk_full(points: &[DiskTrendPoint]) -> Forecast {
    let Some(last) = points.iter().max_by_key(|p| p.at) else {
        return Forecast::NotEnoughData;
    };
    let recent: Vec<&DiskTrendPoint> = points.iter().filter(|p| last.at.since(p.at) <= 14 * DAY).collect();
    let first = recent.iter().map(|p| p.at).min().unwrap_or(last.at);
    if recent.len() < 6 || last.at.since(first) < 12 * HOUR {
        return Forecast::NotEnoughData;
    }
    let xy: Vec<(f64, f64)> = recent
        .iter()
        .map(|p| ((p.at.since(first) as f64) / DAY as f64, p.free as f64))
        .collect();
    match linear_fit(&xy) {
        Some(fit) if fit.slope < 0.0 => {
            let days = (last.free as f64 / -fit.slope) as f32;
            if days > HORIZON_DAYS {
                Forecast::NotFilling
            } else {
                Forecast::FullInDays((days * 10.0).round() / 10.0)
            }
        }
        _ => Forecast::NotFilling,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(days: i64, start_free: u64, per_day: i64) -> Vec<DiskTrendPoint> {
        (0..=days * 4)
            .map(|q| DiskTrendPoint {
                at: Timestamp(q * 6 * HOUR),
                free: (start_free as i64 + per_day * q / 4) as u64,
            })
            .collect()
    }

    #[test]
    fn a_shrinking_disk_has_a_date() {
        let gb = 1_000_000_000;
        // 100 GB free, losing 2 GB a day: the last reading (after 10 days) has 80 GB → 40 days.
        match disk_full(&series(10, 100 * gb, -2 * gb as i64)) {
            Forecast::FullInDays(d) => assert!((d - 40.0).abs() < 0.5, "{d}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn stable_slow_or_short_series() {
        let gb = 1_000_000_000u64;
        assert_eq!(disk_full(&series(10, 100 * gb, 0)), Forecast::NotFilling);
        assert_eq!(disk_full(&series(10, 100 * gb, 1_000_000)), Forecast::NotFilling);
        assert_eq!(disk_full(&series(10, 100 * gb, -100_000)), Forecast::NotFilling, "beyond a year");
        assert_eq!(disk_full(&[]), Forecast::NotEnoughData);
        assert_eq!(disk_full(&series(1, gb, -1)[..3]), Forecast::NotEnoughData);
        let same_time: Vec<_> = (0..10).map(|_| DiskTrendPoint { at: Timestamp(0), free: 5 }).collect();
        assert_eq!(disk_full(&same_time), Forecast::NotEnoughData);
    }
}
