//! Bounded history of the counters being watched, for the chart with events marked.

use super::catalog::CounterUnit;
use crate::protocol::TimelineEvent;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Most points kept per counter (one hour at one reading every two seconds).
pub const MAX_POINTS: usize = 1800;
/// Most points sent to the UI per counter.
pub const MAX_SENT: usize = 600;

/// One reading in a series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterPoint {
    /// When.
    pub at: Timestamp,
    /// The value.
    pub value: f64,
}

/// A counter's readings over a window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterSeries {
    /// The counter's path.
    pub path: String,
    /// What it measures.
    pub unit: CounterUnit,
    /// The readings, oldest first.
    pub points: Vec<CounterPoint>,
}

/// The answer to "chart these counters": the series and the app's events in the same window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CounterHistory {
    /// Window start.
    pub from: Timestamp,
    /// Window end.
    pub to: Timestamp,
    /// One series per counter asked for (empty when nothing was read yet).
    pub series: Vec<CounterSeries>,
    /// Alarms, rule actions and Dyno's own changes in the window, to mark on the chart.
    pub events: Vec<TimelineEvent>,
}

/// The last [`MAX_POINTS`] readings of one counter.
#[derive(Debug, Clone, Default)]
pub struct CounterRing {
    points: VecDeque<CounterPoint>,
}

impl CounterRing {
    /// An empty ring.
    pub fn new() -> CounterRing {
        CounterRing::default()
    }

    /// Adds a reading; readings that are not after the last one are ignored (two requests in the same millisecond).
    pub fn push(&mut self, at: Timestamp, value: f64) {
        if !value.is_finite() || self.points.back().is_some_and(|p| p.at >= at) {
            return;
        }
        if self.points.len() == MAX_POINTS {
            self.points.pop_front();
        }
        self.points.push_back(CounterPoint { at, value });
    }

    /// The readings at or after `from`, thinned to at most [`MAX_SENT`].
    pub fn since(&self, from: Timestamp) -> Vec<CounterPoint> {
        thin(self.points.iter().filter(|p| p.at >= from).copied().collect(), MAX_SENT)
    }

    /// How many readings are kept.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Whether nothing was read yet.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// At most `max` points, evenly spread, always keeping the first and the last.
pub fn thin(points: Vec<CounterPoint>, max: usize) -> Vec<CounterPoint> {
    let n = points.len();
    if max < 2 || n <= max {
        return if max < 2 && n > max {
            points.into_iter().rev().take(max).collect()
        } else {
            points
        };
    }
    (0..max).map(|i| points[i * (n - 1) / (max - 1)]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t: i64) -> CounterPoint {
        CounterPoint {
            at: Timestamp(t),
            value: t as f64,
        }
    }

    #[test]
    fn the_ring_is_bounded_ordered_and_ignores_repeats_and_non_numbers() {
        let mut r = CounterRing::new();
        assert!(r.is_empty());
        for t in 1..=(MAX_POINTS as i64 + 5) {
            r.push(Timestamp(t), t as f64);
        }
        r.push(Timestamp(1), 1.0);
        r.push(Timestamp(MAX_POINTS as i64 + 6), f64::INFINITY);
        assert_eq!(r.len(), MAX_POINTS);
        let all = r.since(Timestamp(0));
        assert!(all.len() <= MAX_SENT);
        assert_eq!(all.last().unwrap().at, Timestamp(MAX_POINTS as i64 + 5));
        assert_eq!(all.first().unwrap().at, Timestamp(6));
    }

    #[test]
    fn since_filters_the_window() {
        let mut r = CounterRing::new();
        for t in [10, 20, 30] {
            r.push(Timestamp(t), 1.0);
        }
        assert_eq!(r.since(Timestamp(20)).len(), 2);
        assert!(r.since(Timestamp(31)).is_empty());
    }

    #[test]
    fn thinning_keeps_the_ends_and_the_order() {
        let pts: Vec<_> = (0..100).map(p).collect();
        let t = thin(pts.clone(), 10);
        assert_eq!(t.len(), 10);
        assert_eq!((t[0].at, t[9].at), (Timestamp(0), Timestamp(99)));
        assert!(t.windows(2).all(|w| w[0].at < w[1].at));
        assert_eq!(thin(pts.clone(), 100), pts);
        assert_eq!(thin(pts.clone(), 1), vec![p(99)]);
        assert!(thin(pts, 0).is_empty());
        assert!(thin(vec![], 5).is_empty());
    }
}
