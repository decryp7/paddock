//! How a live update reads for a dashboard widget, a menu-bar item or its preview: the one reader (family ADR 0035 §5, "the
//! engine decides"). It turns a [`Metric`] and a [`LiveUpdate`] into the value, how it is said, where it sits on a 0 to 100 scale
//! and its tone against the widget's thresholds. The tray icons and the dashboard both use it, so a number is never read twice
//! in two ways. Pure: no clock, no OS.

use super::dashboard::{Thresholds, ValueTone};
use super::metric::Metric;
use crate::metrics::MemoryPressure;
use crate::protocol::LiveUpdate;
use serde::{Deserialize, Serialize};

/// One metric read from one live update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MetricReading {
    /// The number: a percentage for percent metrics, bytes per second for rates, degrees Celsius, rpm; none when the PC
    /// cannot measure it.
    pub value: Option<f64>,
    /// The number as shown ("23", "5.0"); "—" when there is none.
    pub text: String,
    /// The unit as shown ("%", "MB/s", "°C", "rpm"; empty for a word).
    pub unit: String,
    /// 0 to 100 for bars and rings, none when the metric has no natural scale.
    pub percent: Option<f32>,
    /// Against the widget's thresholds.
    pub tone: ValueTone,
    /// Why there is no value.
    pub reason: Option<String>,
}

/// One metric to read with the thresholds of the widget that shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReadingQuery {
    /// What to read.
    pub metric: Metric,
    /// Its colour thresholds.
    #[serde(default)]
    pub thresholds: Thresholds,
}

/// The most queries one request may carry (a dashboard holds 32 widgets).
pub const MAX_QUERIES: usize = 64;

fn unavailable(reason: impl Into<String>) -> MetricReading {
    MetricReading {
        value: None,
        text: "—".into(),
        unit: String::new(),
        percent: None,
        tone: ValueTone::Ok,
        reason: Some(reason.into()),
    }
}

fn percent_reading(v: f64, thresholds: &Thresholds) -> MetricReading {
    let v = if v.is_finite() { v.clamp(0.0, 100.0) } else { 0.0 };
    MetricReading {
        value: Some(v),
        text: format!("{v:.0}"),
        unit: "%".into(),
        percent: Some(v as f32),
        tone: thresholds.tone(Some(v)),
        reason: None,
    }
}

/// A rate in decimal units, the way network speeds are quoted: B/s, KB/s, MB/s, GB/s.
fn rate_reading(bps: f64, thresholds: &Thresholds) -> MetricReading {
    let bps = if bps.is_finite() { bps.max(0.0) } else { 0.0 };
    let (text, unit) = if bps >= 1e9 {
        (format!("{:.1}", bps / 1e9), "GB/s")
    } else if bps >= 1e6 {
        (format!("{:.1}", bps / 1e6), "MB/s")
    } else if bps >= 1e3 {
        (format!("{:.1}", bps / 1e3), "KB/s")
    } else {
        (format!("{bps:.0}"), "B/s")
    };
    MetricReading {
        value: Some(bps),
        text,
        unit: unit.into(),
        percent: None,
        tone: thresholds.tone(Some(bps)),
        reason: None,
    }
}

/// Reads `metric` from `update` against `thresholds`. A metric the PC cannot measure says why instead of reading zero.
pub fn read_metric(metric: &Metric, update: &LiveUpdate, thresholds: &Thresholds) -> MetricReading {
    let s = &update.system;
    match metric {
        Metric::CpuTotal => percent_reading(f64::from(s.cpu.total), thresholds),
        Metric::CpuPerCore => {
            let cores = &s.cpu.per_core;
            let v = if cores.is_empty() {
                s.cpu.total
            } else {
                cores.iter().sum::<f32>() / cores.len() as f32
            };
            percent_reading(f64::from(v), thresholds)
        }
        Metric::Memory => percent_reading(f64::from(s.memory_percent()), thresholds),
        Metric::Swap if s.memory.swap_total > 0 => {
            percent_reading(s.memory.swap_used as f64 / s.memory.swap_total as f64 * 100.0, thresholds)
        }
        Metric::Swap => unavailable("No swap is configured."),
        Metric::MemoryPressure => {
            let (value, word) = match update.pressure {
                MemoryPressure::Normal => (0.0, "Normal"),
                MemoryPressure::Elevated => (60.0, "Elevated"),
                MemoryPressure::Critical => (100.0, "Critical"),
            };
            MetricReading {
                value: Some(value),
                text: word.into(),
                unit: String::new(),
                percent: None,
                tone: thresholds.tone(Some(value)),
                reason: None,
            }
        }
        Metric::DiskActivity => match s.disk_io.busy.value() {
            Some(busy) => percent_reading(f64::from(*busy), thresholds),
            None => unavailable("The disk's busy time is not reported here."),
        },
        Metric::DiskFree => match s.disks.first().filter(|d| d.total > 0) {
            Some(d) => percent_reading(d.free as f64 / d.total as f64 * 100.0, thresholds),
            None => unavailable("No drive was reported."),
        },
        Metric::NetworkDown => rate_reading(s.network_totals().0, thresholds),
        Metric::NetworkUp => rate_reading(s.network_totals().1, thresholds),
        Metric::GpuLoad => match s.gpu.value() {
            Some(g) => percent_reading(f64::from(g.utilization), thresholds),
            None => unavailable("No GPU load is reported here."),
        },
        Metric::Temperature => match s.hottest() {
            Some(t) => MetricReading {
                value: Some(f64::from(t.celsius)),
                text: format!("{:.0}", t.celsius),
                unit: "°C".into(),
                percent: None,
                tone: thresholds.tone(Some(f64::from(t.celsius))),
                reason: None,
            },
            None => unavailable("No temperature sensor was reported."),
        },
        Metric::Fan => match s.fans.value().and_then(|f| f.first()) {
            Some(f) => MetricReading {
                value: Some(f64::from(f.rpm)),
                text: f.rpm.to_string(),
                unit: "rpm".into(),
                percent: None,
                tone: thresholds.tone(Some(f64::from(f.rpm))),
                reason: None,
            },
            None => unavailable("No fan was reported."),
        },
        Metric::Battery => match s.battery.value() {
            Some(b) => percent_reading(f64::from(b.charge), thresholds),
            None => unavailable("No battery was reported."),
        },
        Metric::Process(name) => {
            let cpu = update
                .top_cpu
                .iter()
                .find(|g| g.name.eq_ignore_ascii_case(name))
                .map_or(0.0, |g| g.cpu_read());
            percent_reading(f64::from(cpu), thresholds)
        }
        Metric::PerfCounter(_) => unavailable("Counters are read separately."),
    }
}

/// Gives each counter reading the tone of the dashboard widget that shows it, against that widget's own thresholds (the first
/// widget showing the counter, when several do). A counter no dashboard widget shows keeps the default tone.
pub fn with_dashboard_thresholds(
    readings: Vec<crate::counters::CounterReading>,
    layout: &[paddock_widgets::Placement<super::DashboardWidget, paddock_widgets::Cells>],
) -> Vec<crate::counters::CounterReading> {
    let key = |p: &str| {
        p.parse::<crate::counters::CounterPath>()
            .map_or_else(|_| p.to_string(), |c| c.to_string())
    };
    readings
        .into_iter()
        .map(|r| {
            let own = key(&r.path);
            match layout
                .iter()
                .find(|p| matches!(&p.widget.metric, Metric::PerfCounter(path) if key(path) == own))
            {
                Some(p) => r.with_thresholds(&p.widget.thresholds),
                None => r,
            }
        })
        .collect()
}

/// Reads every query from the last live update, in order; before the first update every reading says it is waiting. At most
/// [`MAX_QUERIES`] are read, the rest are left out.
pub fn read_all(queries: &[ReadingQuery], update: Option<&LiveUpdate>) -> Vec<MetricReading> {
    queries
        .iter()
        .take(MAX_QUERIES)
        .map(|q| match update {
            Some(u) => read_metric(&q.metric, u, &q.thresholds),
            None => unavailable("Waiting for the first reading."),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grouping::AppGroup;
    use crate::metrics::fixtures::sample;
    use crate::metrics::Reading;
    use crate::process::Measured;

    fn update() -> LiveUpdate {
        LiveUpdate {
            system: sample(1),
            pressure: MemoryPressure::Elevated,
            top_cpu: vec![AppGroup {
                id: "c".into(),
                name: "Chrome".into(),
                exe: None,
                pids: vec![],
                cpu: Measured::Value(33.0),
                memory: Measured::Value(1),
                disk_bps: Measured::Value(0.0),
                partial: false,
            }],
            top_memory: vec![],
            overhead: Default::default(),
            processes: None,
            groups: None,
            other_users: None,
        }
    }

    fn none() -> Thresholds {
        Thresholds::default()
    }

    #[test]
    fn percent_metrics_read_with_a_scale_and_a_tone_from_the_thresholds() {
        let mut u = update();
        u.system.cpu.total = 91.0;
        let hot = Thresholds {
            warning: Some(80.0),
            critical: Some(90.0),
        };
        let r = read_metric(&Metric::CpuTotal, &u, &hot);
        assert_eq!(
            (r.value, r.text.as_str(), r.unit.as_str(), r.percent),
            (Some(91.0), "91", "%", Some(91.0))
        );
        assert_eq!(r.tone, ValueTone::Bad);
        assert_eq!(read_metric(&Metric::CpuTotal, &u, &none()).tone, ValueTone::Ok);
        u.system.cpu.total = 85.0;
        assert_eq!(read_metric(&Metric::CpuTotal, &u, &hot).tone, ValueTone::Warn);
    }

    #[test]
    fn memory_swap_disk_free_and_the_process_read_as_percentages() {
        let u = update();
        assert_eq!(read_metric(&Metric::Memory, &u, &none()).percent, Some(50.0));
        assert_eq!(read_metric(&Metric::Swap, &u, &none()).percent, Some(0.0));
        assert_eq!(read_metric(&Metric::DiskFree, &u, &none()).percent, Some(40.0));
        assert_eq!(read_metric(&Metric::Process("chrome".into()), &u, &none()).value, Some(33.0));
        assert_eq!(read_metric(&Metric::Process("gone".into()), &u, &none()).value, Some(0.0));
        let mut no_swap = update();
        no_swap.system.memory.swap_total = 0;
        assert_eq!(
            read_metric(&Metric::Swap, &no_swap, &none()).reason.as_deref(),
            Some("No swap is configured.")
        );
    }

    #[test]
    fn per_core_is_the_average_of_the_cores_and_the_total_without_cores() {
        let mut u = update();
        u.system.cpu.per_core = vec![10.0, 30.0];
        u.system.cpu.total = 99.0;
        assert_eq!(read_metric(&Metric::CpuPerCore, &u, &none()).value, Some(20.0));
        u.system.cpu.per_core.clear();
        assert_eq!(read_metric(&Metric::CpuPerCore, &u, &none()).value, Some(99.0));
    }

    #[test]
    fn rates_use_decimal_units_and_have_no_percent() {
        let mut u = update();
        u.system.network = vec![crate::metrics::NetworkSample {
            name: "en0".into(),
            rx_bps: 5_000_000.0,
            tx_bps: 1_500.0,
        }];
        let down = read_metric(&Metric::NetworkDown, &u, &none());
        assert_eq!((down.text.as_str(), down.unit.as_str(), down.percent), ("5.0", "MB/s", None));
        let up = read_metric(&Metric::NetworkUp, &u, &none());
        assert_eq!((up.text.as_str(), up.unit.as_str()), ("1.5", "KB/s"));
        u.system.network.clear();
        let idle = read_metric(&Metric::NetworkDown, &u, &none());
        assert_eq!((idle.text.as_str(), idle.unit.as_str(), idle.value), ("0", "B/s", Some(0.0)));
        u.system.network = vec![crate::metrics::NetworkSample {
            name: "x".into(),
            rx_bps: f64::NAN,
            tx_bps: 3e9,
        }];
        assert_eq!(read_metric(&Metric::NetworkDown, &u, &none()).value, Some(0.0));
        assert_eq!(read_metric(&Metric::NetworkUp, &u, &none()).unit, "GB/s");
    }

    #[test]
    fn pressure_is_a_word_with_a_value_for_the_tone() {
        let u = update();
        let hot = Thresholds {
            warning: Some(50.0),
            critical: Some(90.0),
        };
        let r = read_metric(&Metric::MemoryPressure, &u, &hot);
        assert_eq!(
            (r.text.as_str(), r.value, r.percent, r.tone),
            ("Elevated", Some(60.0), None, ValueTone::Warn)
        );
    }

    #[test]
    fn temperature_and_fan_read_when_the_sensors_report_and_say_why_when_they_do_not() {
        let mut u = update();
        u.system.temperatures = Reading::Available(vec![crate::metrics::Temperature {
            label: "cpu".into(),
            celsius: 81.4,
            critical: None,
        }]);
        let gaming = Thresholds {
            warning: Some(80.0),
            critical: Some(92.0),
        };
        let t = read_metric(&Metric::Temperature, &u, &gaming);
        assert_eq!((t.text.as_str(), t.unit.as_str(), t.tone), ("81", "°C", ValueTone::Warn));
        u.system.temperatures = Reading::Unavailable("Needs a vendor driver".into());
        assert!(read_metric(&Metric::Temperature, &u, &gaming).reason.is_some());
        u.system.fans = Reading::Available(vec![crate::metrics::Fan {
            label: "f".into(),
            rpm: 1200,
        }]);
        assert_eq!(read_metric(&Metric::Fan, &u, &none()).unit, "rpm");
        u.system.fans = Reading::Unavailable("none".into());
        assert_eq!(read_metric(&Metric::Fan, &u, &none()).value, None);
        u.system.battery = Reading::Unavailable("No battery".into());
        let b = read_metric(&Metric::Battery, &u, &none());
        assert_eq!((b.value, b.text.as_str(), b.tone), (None, "—", ValueTone::Ok));
    }

    #[test]
    fn a_counter_is_not_in_the_live_update_and_says_so() {
        let r = read_metric(&Metric::PerfCounter("\\A\\B".into()), &update(), &none());
        assert_eq!(r.value, None);
        assert!(r.reason.is_some());
    }

    #[test]
    fn a_dashboard_counter_widget_tones_its_counter_with_its_own_thresholds() {
        use crate::counters::{CounterReading, CounterUnit};
        use crate::widgets::{DashboardKind, DashboardWidget};
        let reading = |path: &str| CounterReading::new(path.into(), CounterUnit::Percent, Reading::Available(60.0));
        let mut w = DashboardWidget::new(
            DashboardKind::Graph,
            Metric::PerfCounter(r"\Processor(_Total)\% Processor Time".into()),
        );
        w.thresholds = Thresholds {
            warning: Some(50.0),
            critical: Some(80.0),
        };
        let layout = paddock_widgets::assign_ids([(
            paddock_widgets::Cells {
                rect: paddock_widgets::Rect::new(0, 0, 6, 6),
            },
            w,
        )]);
        let out = with_dashboard_thresholds(
            vec![reading(r"\Processor(_Total)\% Processor Time"), reading(r"\Other\Counter")],
            &layout,
        );
        assert_eq!(out[0].tone, ValueTone::Warn, "the widget's thresholds");
        assert_eq!(out[1].tone, ValueTone::Ok, "no widget: the default thresholds");
    }

    #[test]
    fn before_the_first_update_every_reading_waits_and_the_batch_is_bounded() {
        let q = ReadingQuery {
            metric: Metric::CpuTotal,
            thresholds: none(),
        };
        let waiting = read_all(std::slice::from_ref(&q), None);
        assert_eq!(waiting[0].reason.as_deref(), Some("Waiting for the first reading."));
        let many = vec![q; MAX_QUERIES + 5];
        assert_eq!(read_all(&many, Some(&update())).len(), MAX_QUERIES);
    }
}
