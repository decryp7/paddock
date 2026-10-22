//! What the tray icon shows, as pure functions: the user's metrics and style become RGBA pixels and a tooltip from the
//! live updates (`dyno_core::tray` draws; this picks the numbers). The shell hands the pixels to the OS at most once a
//! second.

use dyno_core::protocol::LiveUpdate;
use dyno_core::settings::{Tray, TrayMetric, TrayStyle};
use dyno_core::tray::{self, Icon};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many recent values the graph style keeps (more than the widest icon).
const HISTORY: usize = 64;
/// Network traffic is drawn against the busiest second seen, but never against less than this (bytes per second).
const NET_FLOOR: f64 = 1_000_000.0;

/// Lets an event through at most once per `min` interval.
#[derive(Debug)]
pub struct Throttle {
    min: Duration,
    last: Option<Instant>,
}

impl Throttle {
    /// At most one per `min`.
    pub fn new(min: Duration) -> Throttle {
        Throttle { min, last: None }
    }

    /// Whether an event at `now` may pass (and records it if so).
    pub fn allow(&mut self, now: Instant) -> bool {
        if self.last.is_some_and(|l| now.saturating_duration_since(l) < self.min) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

/// An icon with its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The pixels.
    pub icon: Icon,
    /// The tooltip.
    pub tooltip: String,
    /// Whether any metric is at the high-load colour (the static fallback picks its variant by this).
    pub high: bool,
}

/// Draws the tray icon from live updates.
#[derive(Debug)]
pub struct TrayModel {
    settings: Tray,
    history: VecDeque<f32>,
    net_peak: f64,
    size: u32,
}

impl TrayModel {
    /// A model drawing `size`-pixel icons.
    pub fn new(settings: Tray, size: u32) -> TrayModel {
        TrayModel {
            settings,
            history: VecDeque::new(),
            net_peak: NET_FLOOR,
            size,
        }
    }

    /// The user's tray settings changed.
    pub fn set_settings(&mut self, settings: Tray) {
        if settings != self.settings {
            self.history.clear();
            self.settings = settings;
        }
    }

    /// The numbers of the configured metrics (0–100) in `u`. A metric the PC cannot measure is left out, so the icon
    /// never shows a made-up zero; when none can be measured the CPU stands in.
    pub fn values(&mut self, u: &LiveUpdate) -> Vec<(TrayMetric, f32)> {
        let (rx, tx) = u.system.network_totals();
        self.net_peak = (self.net_peak * 0.995).max(rx + tx).max(NET_FLOOR);
        let one = |m: TrayMetric| -> Option<f32> {
            match m {
                TrayMetric::Cpu => Some(u.system.cpu.total),
                TrayMetric::Memory => Some(u.system.memory_percent()),
                TrayMetric::Disk => u.system.disk_io.busy.value().copied(),
                TrayMetric::Network => Some((((rx + tx) / self.net_peak) * 100.0) as f32),
                TrayMetric::Gpu => u.system.gpu.value().map(|g| g.utilization),
            }
        };
        let mut out: Vec<(TrayMetric, f32)> = self
            .settings
            .metrics
            .iter()
            .filter_map(|m| one(*m).map(|v| (*m, v.clamp(0.0, 100.0))))
            .collect();
        if out.is_empty() {
            out.push((TrayMetric::Cpu, u.system.cpu.total.clamp(0.0, 100.0)));
        }
        out
    }

    /// The icon and tooltip for an update.
    pub fn frame(&mut self, u: &LiveUpdate, dark_taskbar: bool) -> Frame {
        let values = self.values(u);
        if let Some((_, v)) = values.first() {
            self.history.push_back(*v);
            while self.history.len() > HISTORY {
                self.history.pop_front();
            }
        }
        let graph: Vec<f32> = self.history.iter().copied().collect();
        let style: TrayStyle = self.settings.style;
        Frame {
            icon: tray::render(&values, &graph, style, self.size, dark_taskbar),
            tooltip: tray::tooltip(&values),
            high: values.iter().any(|(_, v)| *v >= tray::HIGH_LOAD),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::metrics::{NetworkSample, Reading};
    use dyno_core::protocol::{LiveUpdate, Overhead};
    use dyno_core::rules::ProfileId;
    use dyno_core::time::Timestamp;
    use dyno_platform::fake::quiet_sample;

    fn update(cpu: f32) -> LiveUpdate {
        let mut system = quiet_sample(Timestamp(0));
        system.network = vec![NetworkSample {
            name: "en0".into(),
            rx_bps: 0.0,
            tx_bps: 0.0,
        }];
        system.cpu.total = cpu;
        LiveUpdate {
            system,
            pressure: dyno_core::metrics::MemoryPressure::Normal,
            top_cpu: vec![],
            top_memory: vec![],
            overhead: Overhead::default(),
            profile: ProfileId::Normal,
            rules_paused: false,
            active_actions: vec![],
            processes: None,
            groups: None,
        }
    }

    fn settings(metrics: &[TrayMetric], style: TrayStyle) -> Tray {
        Tray {
            metrics: metrics.to_vec(),
            style,
        }
    }

    #[test]
    fn the_throttle_lets_one_event_through_per_interval() {
        let mut t = Throttle::new(Duration::from_secs(1));
        let t0 = Instant::now();
        assert!(t.allow(t0));
        assert!(!t.allow(t0 + Duration::from_millis(900)));
        assert!(t.allow(t0 + Duration::from_millis(1000)));
        assert!(!t.allow(t0 + Duration::from_millis(1500)));
    }

    #[test]
    fn the_icon_follows_the_users_metrics_and_flags_high_load() {
        let mut m = TrayModel::new(settings(&[TrayMetric::Cpu, TrayMetric::Memory], TrayStyle::Bars), 32);
        let f = m.frame(&update(95.0), true);
        assert_eq!(f.icon.size, 32);
        assert_eq!(f.icon.rgba.len(), 32 * 32 * 4);
        assert!(f.high);
        assert!(f.tooltip.starts_with("Dyno — CPU 95%"), "{}", f.tooltip);
        assert!(f.tooltip.contains("Memory"));
        assert!(!m.frame(&update(10.0), true).high);
        // A different style or metrics gives different pixels.
        let bars = m.frame(&update(50.0), true).icon;
        m.set_settings(settings(&[TrayMetric::Cpu], TrayStyle::Graph));
        assert_ne!(m.frame(&update(50.0), true).icon, bars);
    }

    #[test]
    fn an_unmeasurable_metric_is_left_out_and_cpu_stands_in_when_none_is() {
        let mut u = update(40.0);
        u.system.gpu = Reading::Unavailable("no GPU".into());
        let mut m = TrayModel::new(settings(&[TrayMetric::Gpu, TrayMetric::Cpu], TrayStyle::Bars), 32);
        assert_eq!(m.values(&u), vec![(TrayMetric::Cpu, 40.0)]);
        m.set_settings(settings(&[TrayMetric::Gpu, TrayMetric::Disk], TrayStyle::Bars));
        assert_eq!(m.values(&u), vec![(TrayMetric::Cpu, 40.0)]);
    }

    #[test]
    fn values_are_clamped_and_network_is_relative_to_the_busiest_second() {
        let mut u = update(250.0);
        u.system.network[0].rx_bps = 5_000_000.0;
        u.system.network[0].tx_bps = 0.0;
        let mut m = TrayModel::new(settings(&[TrayMetric::Cpu, TrayMetric::Network], TrayStyle::Bars), 32);
        let v = m.values(&u);
        assert_eq!(v[0], (TrayMetric::Cpu, 100.0));
        assert!(v[1].1 > 90.0, "the busiest second so far is ~100 %: {v:?}");
        u.system.network[0].rx_bps = 2_500_000.0;
        assert!((m.values(&u)[1].1 - 50.0).abs() < 2.0);
    }
}
