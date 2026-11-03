//! The CPU test's sustained run (ADR 0030 section 1): the lengths offered, the trace of one point a second, the judgement of that
//! trace (peak, sustained, how much was kept, when it first fell) and the thermal safety stop. Pure: the trace and the
//! temperatures come in, words and numbers go out. An episode is judged by `throttle::ThrottleTracker`, the same rules as in
//! monitoring, in the agent.

use serde::{Deserialize, Serialize};

/// The lengths of a sustained run offered, minutes. The default is 5.
pub const SUSTAINED_MINUTES: [u32; 4] = [2, 5, 10, 20];
/// The default length, minutes.
pub const DEFAULT_SUSTAINED_MINUTES: u32 = 5;
/// The longest run where no temperature can be read, minutes.
pub const NO_SENSOR_MAX_MINUTES: u32 = 5;
/// The peak is the best window of this many seconds in the first [`PEAK_WITHIN_SECS`].
pub const PEAK_WINDOW_SECS: usize = 10;
/// The peak is looked for in this many first seconds.
pub const PEAK_WITHIN_SECS: usize = 30;
/// The sustained value is the median of this many last seconds.
pub const SUSTAINED_TAIL_SECS: usize = 60;
/// A fall of more than this fraction below the peak counts as the first drop.
pub const DROP_FRACTION: f64 = 0.10;
/// The run stops when the hottest sensor stays within the hot margin of its critical value for this long, seconds.
pub const HOT_STOP_SECS: u32 = 10;
/// What the person is told when no temperature can be read and the length is limited.
pub const NO_SENSOR_NOTE: &str =
    "Dyno cannot read this computer's temperature; the hardware protects itself. The sustained run is limited to 5 minutes.";

/// One second of a sustained run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TracePoint {
    /// Seconds since the run began.
    pub secs: u32,
    /// Work done that second, in the metric's unit (GFLOPS).
    pub throughput: f64,
    /// The clock, MHz, when readable.
    pub freq_mhz: Option<u32>,
    /// The hottest temperature, Celsius, when readable.
    pub temp_c: Option<f32>,
}

/// Refuses a length the machine may not run: not one of the offered lengths, or longer than the limit where no temperature is
/// readable.
pub fn check_length(minutes: u32, sensor_readable: bool) -> Result<(), String> {
    if !SUSTAINED_MINUTES.contains(&minutes) {
        return Err(format!("The sustained run lasts 2, 5, 10 or 20 minutes, not {minutes}."));
    }
    if !sensor_readable && minutes > NO_SENSOR_MAX_MINUTES {
        return Err(NO_SENSOR_NOTE.into());
    }
    Ok(())
}

/// What a sustained trace says.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SustainedSummary {
    /// The best [`PEAK_WINDOW_SECS`]-second average in the first [`PEAK_WITHIN_SECS`] seconds.
    pub peak: f64,
    /// The median of the last [`SUSTAINED_TAIL_SECS`] seconds.
    pub sustained: f64,
    /// `sustained / peak`, percent.
    pub kept_percent: f64,
    /// Seconds until the throughput first fell more than [`DROP_FRACTION`] below the peak (after the peak window), if it did.
    pub first_drop_secs: Option<u32>,
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// Judges a trace; `None` when there are too few points to say (fewer than one peak window).
pub fn summarize(trace: &[TracePoint]) -> Option<SustainedSummary> {
    let v: Vec<f64> = trace.iter().map(|p| p.throughput).filter(|t| t.is_finite()).collect();
    if v.len() < PEAK_WINDOW_SECS {
        return None;
    }
    let head = &v[..v.len().min(PEAK_WITHIN_SECS)];
    let (mut peak, mut peak_end) = (0.0_f64, PEAK_WINDOW_SECS);
    for (i, w) in head.windows(PEAK_WINDOW_SECS).enumerate() {
        let m = mean(w);
        if m > peak {
            peak = m;
            peak_end = i + PEAK_WINDOW_SECS;
        }
    }
    let tail = &v[v.len().saturating_sub(SUSTAINED_TAIL_SECS)..];
    let mut sorted = tail.to_vec();
    sorted.sort_by(f64::total_cmp);
    let sustained = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    };
    let first_drop_secs = v
        .windows(PEAK_WINDOW_SECS)
        .enumerate()
        .skip(peak_end - PEAK_WINDOW_SECS + 1)
        .find(|(_, w)| mean(w) < peak * (1.0 - DROP_FRACTION))
        .map(|(i, _)| u32::try_from(i + PEAK_WINDOW_SECS).unwrap_or(u32::MAX));
    Some(SustainedSummary {
        peak,
        sustained,
        kept_percent: if peak > 0.0 { sustained / peak * 100.0 } else { 0.0 },
        first_drop_secs,
    })
}

/// The reason for a heat stop, in the words the person sees.
pub const HEAT_STOP_REASON: &str = "Stopped for safety: the processor stayed close to its critical temperature.";
/// The reason when the OS itself reports a critical thermal state.
pub const OS_CRITICAL_REASON: &str = "Stopped for safety: the operating system reported a critical thermal state.";

/// The OS has cut the processor's speed to this percent or less for heat: counted as a critical thermal state. macOS does not tell
/// `pmset` its `NSProcessInfo.thermalState`, so this is the nearest reading Dyno has: a thermal limit in effect with the speed at or
/// below half.
pub const OS_CRITICAL_SPEED_PERCENT: u8 = 50;

/// Whether what the OS reports amounts to a critical thermal state (see [`OS_CRITICAL_SPEED_PERCENT`]).
pub fn os_critical(os: &crate::hardware::OsThrottle) -> bool {
    os.thermal == Some(true) && os.speed_limit_percent.is_some_and(|p| p <= OS_CRITICAL_SPEED_PERCENT)
}

/// Watches the temperature during a sustained run and says when to stop.
#[derive(Debug, Default)]
pub struct HeatWatch {
    hot_since: Option<u32>,
}

impl HeatWatch {
    /// A watch with no history.
    pub fn new() -> HeatWatch {
        HeatWatch::default()
    }

    /// Feeds one second. `hot_margin` is how close to critical counts as hot (`throttle::HOT_MARGIN_C`); a sensor without a
    /// critical value is judged against `throttle::HOT_DEFAULT_C`. Returns the reason to stop.
    pub fn push(
        &mut self,
        secs: u32,
        temp_c: Option<f32>,
        critical_c: Option<f32>,
        os_critical: bool,
        hot_margin: f32,
        hot_default: f32,
    ) -> Option<&'static str> {
        if os_critical {
            return Some(OS_CRITICAL_REASON);
        }
        let hot = temp_c.is_some_and(|t| t >= critical_c.map_or(hot_default, |c| c - hot_margin));
        if !hot {
            self.hot_since = None;
            return None;
        }
        let since = *self.hot_since.get_or_insert(secs);
        (secs.saturating_sub(since) >= HOT_STOP_SECS).then_some(HEAT_STOP_REASON)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(v: &[f64]) -> Vec<TracePoint> {
        v.iter()
            .enumerate()
            .map(|(i, t)| TracePoint {
                secs: i as u32 + 1,
                throughput: *t,
                freq_mhz: None,
                temp_c: None,
            })
            .collect()
    }

    #[test]
    fn lengths_are_the_offered_ones_and_limited_without_a_sensor() {
        assert!(check_length(5, true).is_ok() && check_length(20, true).is_ok());
        assert!(check_length(3, true).is_err());
        assert!(check_length(5, false).is_ok());
        assert_eq!(check_length(10, false).unwrap_err(), NO_SENSOR_NOTE);
    }

    #[test]
    fn a_steady_machine_keeps_everything() {
        let s = summarize(&trace(&[100.0; 120])).unwrap();
        assert_eq!(
            (s.peak, s.sustained, s.kept_percent, s.first_drop_secs),
            (100.0, 100.0, 100.0, None)
        );
    }

    #[test]
    fn a_machine_that_throttles_after_a_minute_shows_the_peak_the_kept_share_and_when_it_fell() {
        let mut v = vec![200.0; 60];
        v.extend(vec![150.0; 120]);
        let s = summarize(&trace(&v)).unwrap();
        assert_eq!(s.peak, 200.0);
        assert_eq!(s.sustained, 150.0);
        assert_eq!(s.kept_percent, 75.0);
        // The ten-second window first averages below 180 (10 % under the peak) once it is mostly slow.
        let drop = s.first_drop_secs.unwrap();
        assert!((61..=70).contains(&drop), "{drop}");
    }

    #[test]
    fn the_peak_is_the_best_window_in_the_first_thirty_seconds_only() {
        let mut v = vec![100.0; 30];
        v.extend(vec![500.0; 30]);
        assert_eq!(summarize(&trace(&v)).unwrap().peak, 100.0);
        let mut w = vec![50.0; 10];
        w.extend(vec![120.0; 20]);
        w.extend(vec![80.0; 40]);
        assert_eq!(summarize(&trace(&w)).unwrap().peak, 120.0, "a slow start is not the peak");
    }

    #[test]
    fn too_short_or_non_finite_traces_give_no_summary_and_never_panic() {
        assert!(summarize(&trace(&[1.0; 5])).is_none());
        assert!(summarize(&[]).is_none());
        let mut v = vec![100.0; 20];
        v[3] = f64::NAN;
        v[4] = f64::INFINITY;
        assert!(summarize(&trace(&v)).is_some());
        assert_eq!(summarize(&trace(&[0.0; 20])).unwrap().kept_percent, 0.0);
    }

    #[test]
    fn heat_stops_the_run_only_after_ten_seconds_close_to_critical() {
        let mut w = HeatWatch::new();
        // Critical 100, margin 8: 92 and above is hot.
        for s in 1..=10 {
            assert_eq!(w.push(s, Some(95.0), Some(100.0), false, 8.0, 92.0), None, "second {s}");
        }
        assert_eq!(w.push(11, Some(95.0), Some(100.0), false, 8.0, 92.0), Some(HEAT_STOP_REASON));
        // A dip below the margin starts the count again.
        let mut w = HeatWatch::new();
        for s in 1..=9 {
            w.push(s, Some(95.0), Some(100.0), false, 8.0, 92.0);
        }
        assert_eq!(w.push(10, Some(80.0), Some(100.0), false, 8.0, 92.0), None);
        for s in 11..=20 {
            assert_eq!(w.push(s, Some(95.0), Some(100.0), false, 8.0, 92.0), None);
        }
    }

    #[test]
    fn heat_uses_the_default_without_a_critical_value_and_never_guesses_without_a_reading() {
        let mut w = HeatWatch::new();
        for s in 1..=30 {
            assert_eq!(w.push(s, None, None, false, 8.0, 92.0), None);
        }
        assert_eq!(w.push(1, Some(93.0), None, false, 8.0, 92.0), None);
        assert_eq!(w.push(12, Some(93.0), None, false, 8.0, 92.0), Some(HEAT_STOP_REASON));
    }

    #[test]
    fn the_os_reading_counts_as_critical_only_with_a_thermal_limit_and_half_speed_or_less() {
        use crate::hardware::OsThrottle;
        let t = |thermal, speed| OsThrottle {
            speed_limit_percent: speed,
            thermal,
            power: None,
        };
        assert!(os_critical(&t(Some(true), Some(50))));
        assert!(os_critical(&t(Some(true), Some(10))));
        assert!(
            !os_critical(&t(Some(true), Some(80))),
            "a mild limit is for the tracker, not a stop"
        );
        assert!(!os_critical(&t(Some(false), Some(10))), "a low limit for power is not heat");
        assert!(!os_critical(&t(Some(true), None)));
        assert!(!os_critical(&OsThrottle::default()));
    }

    #[test]
    fn an_os_critical_state_stops_at_once() {
        assert_eq!(HeatWatch::new().push(1, None, None, true, 8.0, 92.0), Some(OS_CRITICAL_REASON));
    }
}
