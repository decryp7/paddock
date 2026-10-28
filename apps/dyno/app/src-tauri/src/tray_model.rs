//! What the tray / menu-bar items show, as pure functions: each item's metric and glyph become RGBA pixels and a tooltip
//! from the live updates (`dyno_core::tray` draws; this picks the numbers). The shell hands the pixels to the OS at most
//! twice a second and only for items whose picture changed.
//!
//! The items come from `settings.tray_items` ([`items_from_settings`]); `DYNO_TRAY_ITEMS` (see [`parse_items`]) overrides
//! them for development and measurements. A small always-visible Windows mini-bar window is a documented stub (off by
//! default; not built yet).

use dyno_core::protocol::LiveUpdate;
use dyno_core::settings::{TrayMetric, TrayStyle};
use dyno_core::tray::{self, Icon};
use dyno_core::widgets::{Metric, TrayItemStyle, TrayItems as TrayItemSettings, TrayOpens};
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

/// How an item draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// A vertical bar.
    Bars,
    /// A line graph.
    Graph,
    /// A horizontal meter.
    Meter,
    /// A ring.
    Ring,
    /// The value as a number.
    Number,
    /// The number over a small graph.
    GraphNumber,
    /// One thin column per CPU core.
    CoreBars,
}

/// What clicking an item opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    /// The shared popover (`#/tray`).
    Main,
    /// The item's own focused popover (`#/popover/<id>`).
    Own,
}

/// One menu-bar / tray item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpec {
    /// Unique; `[a-z0-9-]{1,32}` for its own popover (the tray id suffix, window label and route).
    pub id: String,
    /// What it measures.
    pub metric: Metric,
    /// How it draws.
    pub glyph: Glyph,
    /// What a click opens.
    pub opens: Opens,
    /// The label a number shows in the tooltip (empty: the metric's name).
    pub label: String,
    /// The unit a number shows in the tooltip (empty: the metric's own).
    pub unit: String,
}

impl ItemSpec {
    /// A spec with the default label and unit.
    pub fn new(id: &str, metric: Metric, glyph: Glyph, opens: Opens) -> ItemSpec {
        ItemSpec {
            id: id.into(),
            metric,
            glyph,
            opens,
            label: String::new(),
            unit: String::new(),
        }
    }
}

/// The items for the user's settings.
pub fn items_from_settings(s: &TrayItemSettings) -> Vec<ItemSpec> {
    s.items
        .iter()
        .map(|i| {
            let (glyph, label, unit) = match &i.style {
                TrayItemStyle::LineGraph => (Glyph::Graph, String::new(), String::new()),
                TrayItemStyle::VerticalBars if i.metric == Metric::CpuPerCore => (Glyph::CoreBars, String::new(), String::new()),
                TrayItemStyle::VerticalBars => (Glyph::Bars, String::new(), String::new()),
                TrayItemStyle::HorizontalMeter => (Glyph::Meter, String::new(), String::new()),
                TrayItemStyle::Ring => (Glyph::Ring, String::new(), String::new()),
                TrayItemStyle::Number { unit, label } => (Glyph::Number, label.clone(), unit.clone()),
                TrayItemStyle::GraphAndNumber => (Glyph::GraphNumber, String::new(), String::new()),
            };
            ItemSpec {
                id: i.id.clone(),
                metric: i.metric.clone(),
                glyph,
                opens: match i.opens {
                    TrayOpens::OwnPopover => Opens::Own,
                    TrayOpens::MainPopover => Opens::Main,
                },
                label,
                unit,
            }
        })
        .collect()
}

/// Items from `metric:glyph[:own]` entries separated by commas, e.g. `cpu:ring,memory:number:own,cpu:cores` (metrics
/// `cpu memory swap disk network gpu temp battery`; glyphs `bars graph meter ring number graphnumber cores`). Anything
/// unrecognised is skipped; at most 8 items.
pub fn parse_items(text: &str) -> Vec<ItemSpec> {
    let mut out: Vec<ItemSpec> = Vec::new();
    for entry in text.split(',').take(8) {
        let parts: Vec<&str> = entry.trim().split(':').collect();
        let metric = match parts.first().copied() {
            Some("cpu") => Metric::CpuTotal,
            Some("memory") => Metric::Memory,
            Some("swap") => Metric::Swap,
            Some("disk") => Metric::DiskActivity,
            Some("network") => Metric::NetworkDown,
            Some("gpu") => Metric::GpuLoad,
            Some("temp") => Metric::Temperature,
            Some("battery") => Metric::Battery,
            _ => continue,
        };
        let glyph = match parts.get(1).copied() {
            Some("bars") => Glyph::Bars,
            Some("graph") => Glyph::Graph,
            Some("meter") => Glyph::Meter,
            Some("ring") => Glyph::Ring,
            Some("number") => Glyph::Number,
            Some("graphnumber") => Glyph::GraphNumber,
            Some("cores") if metric == Metric::CpuTotal => Glyph::CoreBars,
            _ => continue,
        };
        let opens = if parts.get(2) == Some(&"own") { Opens::Own } else { Opens::Main };
        out.push(ItemSpec::new(&format!("{}-{}", parts[0], out.len() + 1), metric, glyph, opens));
    }
    out
}

/// An icon with its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The pixels.
    pub icon: Icon,
    /// The tooltip.
    pub tooltip: String,
}

#[derive(Debug, Default)]
struct ItemState {
    history: VecDeque<f32>,
    last: Option<Frame>,
}

/// Draws the tray items from live updates.
#[derive(Debug)]
pub struct TrayItems {
    specs: Vec<ItemSpec>,
    state: Vec<ItemState>,
    net_peak: f64,
    size: u32,
}

/// A measurement: where it sits on the 0-100 drawing scale, and how to say it.
struct Measured {
    scale: f32,
    text: String,
}

/// The metric's name.
pub fn metric_name(m: &Metric) -> String {
    match m {
        Metric::CpuTotal | Metric::CpuPerCore => "CPU".into(),
        Metric::Memory => "Memory".into(),
        Metric::MemoryPressure => "Memory pressure".into(),
        Metric::Swap => "Swap".into(),
        Metric::DiskActivity => "Disk".into(),
        Metric::DiskFree => "Free space".into(),
        Metric::NetworkUp => "Upload".into(),
        Metric::NetworkDown => "Download".into(),
        Metric::GpuLoad => "GPU".into(),
        Metric::Temperature => "Temperature".into(),
        Metric::Fan => "Fan".into(),
        Metric::Battery => "Battery".into(),
        Metric::Process(n) => n.clone(),
        Metric::PerfCounter(p) => p.clone(),
    }
}

fn percent(v: f32) -> Measured {
    let v = v.clamp(0.0, 100.0);
    Measured {
        scale: v,
        text: format!("{v:.0}%"),
    }
}

impl TrayItems {
    /// Items drawn at `size` pixels.
    pub fn new(specs: Vec<ItemSpec>, size: u32) -> TrayItems {
        let state = specs.iter().map(|_| ItemState::default()).collect();
        TrayItems {
            specs,
            state,
            net_peak: NET_FLOOR,
            size,
        }
    }

    /// The items.
    pub fn specs(&self) -> &[ItemSpec] {
        &self.specs
    }

    /// Replaces the items; returns whether they changed.
    pub fn set_specs(&mut self, specs: Vec<ItemSpec>) -> bool {
        if specs == self.specs {
            return false;
        }
        self.state = specs.iter().map(|_| ItemState::default()).collect();
        self.specs = specs;
        true
    }

    /// The reading of `m` in `u`; `None` when the PC cannot measure it.
    fn measure(&self, u: &LiveUpdate, m: &Metric) -> Option<Measured> {
        let s = &u.system;
        let rate = |bps: f64| Measured {
            scale: ((bps / self.net_peak) * 100.0).clamp(0.0, 100.0) as f32,
            text: format!("{:.1} MB/s", bps / 1_000_000.0),
        };
        match m {
            Metric::CpuTotal | Metric::CpuPerCore => Some(percent(s.cpu.total)),
            Metric::Memory => Some(percent(s.memory_percent())),
            Metric::MemoryPressure => {
                let (scale, text) = match u.pressure {
                    dyno_core::metrics::MemoryPressure::Normal => (20.0, "normal"),
                    dyno_core::metrics::MemoryPressure::Elevated => (60.0, "elevated"),
                    dyno_core::metrics::MemoryPressure::Critical => (95.0, "critical"),
                };
                Some(Measured { scale, text: text.into() })
            }
            Metric::Swap => (s.memory.swap_total > 0).then(|| percent(s.memory.swap_used as f32 / s.memory.swap_total as f32 * 100.0)),
            Metric::DiskActivity => s.disk_io.busy.value().map(|v| percent(*v)),
            Metric::DiskFree => s
                .disks
                .first()
                .filter(|d| d.total > 0)
                .map(|d| percent(d.free as f32 / d.total as f32 * 100.0)),
            Metric::NetworkUp => Some(rate(s.network_totals().1)),
            Metric::NetworkDown => Some(rate(s.network_totals().0)),
            Metric::GpuLoad => s.gpu.value().map(|g| percent(g.utilization)),
            Metric::Temperature => s.hottest().map(|t| Measured {
                scale: t.celsius.clamp(0.0, 100.0),
                text: format!("{:.0} °C", t.celsius),
            }),
            Metric::Battery => s.battery.value().map(|b| percent(b.charge)),
            Metric::Process(name) => Some(percent(
                u.top_cpu
                    .iter()
                    .find(|g| g.name.eq_ignore_ascii_case(name))
                    .map_or(0.0, |g| g.cpu_read()),
            )),
            Metric::Fan | Metric::PerfCounter(_) => None,
        }
    }

    /// The pictures that changed since the last call, as `(item id, frame)`.
    pub fn frames(&mut self, u: &LiveUpdate, dark_taskbar: bool) -> Vec<(String, Frame)> {
        let (rx, tx) = u.system.network_totals();
        self.net_peak = (self.net_peak * 0.995).max(rx).max(tx).max(NET_FLOOR);
        let mut changed = Vec::new();
        for i in 0..self.specs.len() {
            let frame = self.draw(i, u, dark_taskbar);
            if self.state[i].last.as_ref() != Some(&frame) {
                self.state[i].last = Some(frame.clone());
                changed.push((self.specs[i].id.clone(), frame));
            }
        }
        changed
    }

    fn draw(&mut self, i: usize, u: &LiveUpdate, dark: bool) -> Frame {
        let spec = self.specs[i].clone();
        let size = self.size;
        let name = if spec.label.is_empty() {
            metric_name(&spec.metric)
        } else {
            spec.label.clone()
        };
        if spec.glyph == Glyph::CoreBars {
            let cores = &u.system.cpu.per_core;
            let busiest = cores.iter().copied().fold(0.0f32, f32::max);
            return Frame {
                icon: tray::render_core_bars(cores, size, dark),
                tooltip: format!("Dyno — {name} {:.0}% · busiest core {busiest:.0}%", u.system.cpu.total),
            };
        }
        let Some(m) = self.measure(u, &spec.metric) else {
            return Frame {
                icon: tray::render_meter(0.0, size, dark),
                tooltip: format!("Dyno — {name} is not available on this PC"),
            };
        };
        let history = &mut self.state[i].history;
        history.push_back(m.scale);
        while history.len() > HISTORY {
            history.pop_front();
        }
        let graph: Vec<f32> = history.iter().copied().collect();
        let v = m.scale;
        let icon = match spec.glyph {
            Glyph::Bars => tray::render(&[(TrayMetric::Cpu, v)], &[], TrayStyle::Bars, size, dark),
            Glyph::Graph => tray::render(&[(TrayMetric::Cpu, v)], &graph, TrayStyle::Graph, size, dark),
            Glyph::Meter => tray::render_meter(v, size, dark),
            Glyph::Ring => tray::render_ring(v, size, dark),
            Glyph::Number => tray::render_number(v, size, dark),
            Glyph::GraphNumber => tray::render_graph_number(v, &graph, size, dark),
            Glyph::CoreBars => tray::render_meter(0.0, size, dark), // handled above
        };
        let text = if spec.unit.is_empty() {
            m.text
        } else {
            format!("{:.0}{}", m.scale, spec.unit)
        };
        Frame {
            icon,
            tooltip: format!("Dyno — {name} {text}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::grouping::AppGroup;
    use dyno_core::metrics::{NetworkSample, Reading};
    use dyno_core::protocol::{LiveUpdate, Overhead};
    use dyno_core::time::Timestamp;
    use dyno_core::widgets::TrayItem;
    use dyno_platform::fake::quiet_sample;

    fn update(cpu: f32) -> LiveUpdate {
        let mut system = quiet_sample(Timestamp(0));
        system.cpu.total = cpu;
        system.network = vec![NetworkSample {
            name: "en0".into(),
            rx_bps: 0.0,
            tx_bps: 0.0,
        }];
        LiveUpdate {
            system,
            pressure: dyno_core::metrics::MemoryPressure::Normal,
            top_cpu: vec![],
            top_memory: vec![],
            overhead: Overhead::default(),
            processes: None,
            groups: None,
            other_users: None,
        }
    }

    fn item(id: &str, metric: Metric, glyph: Glyph) -> ItemSpec {
        ItemSpec::new(id, metric, glyph, Opens::Main)
    }

    fn setting(id: &str, metric: Metric, style: TrayItemStyle, opens: TrayOpens) -> TrayItem {
        TrayItem {
            id: id.into(),
            metric,
            style,
            opens,
        }
    }

    #[test]
    fn the_throttle_lets_one_event_through_per_interval() {
        let mut t = Throttle::new(Duration::from_millis(500));
        let t0 = Instant::now();
        assert!(t.allow(t0));
        assert!(!t.allow(t0 + Duration::from_millis(400)));
        assert!(t.allow(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn the_settings_model_maps_to_items() {
        let default = items_from_settings(&TrayItemSettings::default());
        assert_eq!(default.len(), 2);
        assert_eq!(
            (default[0].glyph, default[0].opens, &default[0].metric),
            (Glyph::Bars, Opens::Own, &Metric::CpuTotal)
        );
        let s = TrayItemSettings {
            version: 1,
            items: vec![
                setting("a", Metric::CpuPerCore, TrayItemStyle::VerticalBars, TrayOpens::MainPopover),
                setting(
                    "b",
                    Metric::Temperature,
                    TrayItemStyle::Number {
                        unit: "C".into(),
                        label: "Hot".into(),
                    },
                    TrayOpens::OwnPopover,
                ),
                setting("c", Metric::Memory, TrayItemStyle::GraphAndNumber, TrayOpens::OwnPopover),
                setting("d", Metric::Swap, TrayItemStyle::LineGraph, TrayOpens::OwnPopover),
                setting("e", Metric::Battery, TrayItemStyle::HorizontalMeter, TrayOpens::OwnPopover),
                setting("f", Metric::GpuLoad, TrayItemStyle::Ring, TrayOpens::OwnPopover),
            ],
        };
        let items = items_from_settings(&s);
        let got: Vec<Glyph> = items.iter().map(|i| i.glyph).collect();
        assert_eq!(
            got,
            [
                Glyph::CoreBars,
                Glyph::Number,
                Glyph::GraphNumber,
                Glyph::Graph,
                Glyph::Meter,
                Glyph::Ring
            ]
        );
        assert_eq!(
            (items[0].opens, items[1].label.as_str(), items[1].unit.as_str()),
            (Opens::Main, "Hot", "C")
        );
    }

    #[test]
    fn items_parse_from_text_and_junk_is_skipped() {
        let items = parse_items("cpu:ring, memory:number:own, gpu:nope, disk:cores, cpu:cores, bogus:ring");
        let got: Vec<(&str, Glyph, Opens)> = items.iter().map(|i| (i.id.as_str(), i.glyph, i.opens)).collect();
        assert_eq!(
            got,
            [
                ("cpu-1", Glyph::Ring, Opens::Main),
                ("memory-2", Glyph::Number, Opens::Own),
                ("cpu-3", Glyph::CoreBars, Opens::Main)
            ]
        );
        assert!(items.iter().all(|i| crate::popover::valid_item_id(&i.id)));
        assert_eq!(parse_items(&"cpu:ring,".repeat(20)).len(), 8, "at most eight items");
        assert!(parse_items("").is_empty());
    }

    #[test]
    fn every_glyph_draws_and_only_changed_pictures_are_returned() {
        let all = [
            Glyph::Bars,
            Glyph::Graph,
            Glyph::Meter,
            Glyph::Ring,
            Glyph::Number,
            Glyph::GraphNumber,
            Glyph::CoreBars,
        ];
        let specs: Vec<ItemSpec> = all
            .iter()
            .enumerate()
            .map(|(i, g)| item(&format!("i{i}"), Metric::CpuTotal, *g))
            .collect();
        let mut m = TrayItems::new(specs, 32);
        let first = m.frames(&update(40.0), true);
        assert_eq!(first.len(), all.len());
        assert!(first.iter().all(|(_, f)| f.icon.size == 32 && f.tooltip.starts_with("Dyno")));
        // Only the graph glyphs move with their history when the number stays the same.
        let again = m.frames(&update(40.0), true);
        assert!(
            again.iter().all(|(id, _)| id == "i1" || id == "i5"),
            "{:?}",
            again.iter().map(|(i, _)| i).collect::<Vec<_>>()
        );
        assert!(m.frames(&update(80.0), true).len() >= 5);
    }

    #[test]
    fn per_core_bars_follow_the_cores_and_name_the_busiest() {
        let mut u = update(10.0);
        u.system.cpu.per_core = vec![5.0, 95.0, 10.0, 0.0];
        let mut m = TrayItems::new(vec![item("c", Metric::CpuPerCore, Glyph::CoreBars)], 32);
        let f = m.frames(&u, true).remove(0).1;
        assert!(f.tooltip.contains("busiest core 95%"), "{}", f.tooltip);
    }

    #[test]
    fn what_cannot_be_measured_says_so_instead_of_drawing_zero() {
        let mut u = update(40.0);
        u.system.gpu = Reading::Unavailable("x".into());
        let mut m = TrayItems::new(
            vec![
                item("g", Metric::GpuLoad, Glyph::Ring),
                item("f", Metric::Fan, Glyph::Number),
                item("p", Metric::PerfCounter("x".into()), Glyph::Number),
                item("t", Metric::Temperature, Glyph::Number),
            ],
            32,
        );
        let f = m.frames(&u, true);
        assert!(f.iter().all(|(_, fr)| fr.tooltip.contains("not available")), "{f:?}");
    }

    #[test]
    fn readings_are_clamped_named_and_network_is_relative_to_the_busiest_second() {
        let mut u = update(250.0);
        u.system.network[0].rx_bps = 5_000_000.0;
        u.system.network[0].tx_bps = 1_000_000.0;
        let mut m = TrayItems::new(
            vec![
                item("c", Metric::CpuTotal, Glyph::Number),
                item("d", Metric::NetworkDown, Glyph::Number),
                item("u", Metric::NetworkUp, Glyph::Number),
                item("m", Metric::MemoryPressure, Glyph::Meter),
                item("s", Metric::Swap, Glyph::Meter),
            ],
            32,
        );
        let f = m.frames(&u, true);
        assert_eq!(f[0].1.tooltip, "Dyno — CPU 100%");
        assert_eq!(f[1].1.tooltip, "Dyno — Download 5.0 MB/s");
        assert!(f[2].1.tooltip.contains("Upload 1.0 MB/s"));
        assert!(f[3].1.tooltip.contains("normal"));
        assert!(f[4].1.tooltip.contains("Swap 0%"), "{}", f[4].1.tooltip);
    }

    #[test]
    fn a_number_uses_its_label_and_unit() {
        let mut spec = item("t", Metric::CpuTotal, Glyph::Number);
        spec.label = "Proc".into();
        spec.unit = "pct".into();
        let mut m = TrayItems::new(vec![spec], 32);
        assert_eq!(m.frames(&update(42.0), true)[0].1.tooltip, "Dyno — Proc 42pct");
    }

    #[test]
    fn one_apps_cpu_comes_from_the_top_list() {
        let mut u = update(10.0);
        u.top_cpu = vec![AppGroup {
            id: "chrome".into(),
            name: "Chrome".into(),
            exe: None,
            pids: vec![],
            cpu: dyno_core::process::Measured::Value(33.0),
            memory: dyno_core::process::Measured::Value(0),
            disk_bps: dyno_core::process::Measured::Value(0.0),
            partial: false,
        }];
        let mut m = TrayItems::new(vec![item("p", Metric::Process("chrome".into()), Glyph::Number)], 32);
        assert!(m.frames(&u, true)[0].1.tooltip.contains("chrome 33%"));
    }

    #[test]
    fn changing_the_items_resets_them() {
        let mut m = TrayItems::new(vec![item("a", Metric::CpuTotal, Glyph::Ring)], 32);
        m.frames(&update(10.0), true);
        assert!(!m.set_specs(m.specs().to_vec()), "same items: no change");
        assert!(m.set_specs(vec![item("b", Metric::CpuTotal, Glyph::Ring)]));
        assert_eq!(m.frames(&update(10.0), true)[0].0, "b");
    }
}
