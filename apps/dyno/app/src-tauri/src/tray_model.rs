//! What the tray / menu-bar items show, as pure functions: each item's metric and glyph become RGBA pixels and a tooltip
//! from the live updates (`dyno_core::tray` draws; this picks the numbers). The shell hands the pixels to the OS at most
//! twice a second and only for items whose picture changed.
//!
//! The item list is a thin adapter for now: [`items_from_settings`] turns the existing `settings.tray` into one combined
//! item, and `DYNO_TRAY_ITEMS` (see [`parse_items`]) gives several independent ones for development and measurements.
//! When `dyno_core::widgets` lands, only [`items_from_settings`] changes. A small always-visible Windows mini-bar window
//! is a documented stub (off by default; not built yet).

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

/// How an item draws itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// One vertical bar per metric (up to three).
    Bars,
    /// A line graph of the first metric.
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
    /// Unique, `[a-z0-9-]{1,32}` (the tray id suffix, the window label and the route).
    pub id: String,
    /// The metrics (one for every glyph but [`Glyph::Bars`], which takes up to three).
    pub metrics: Vec<TrayMetric>,
    /// How it draws.
    pub glyph: Glyph,
    /// What a click opens.
    pub opens: Opens,
}

/// The items for the user's tray settings: one combined item, as before.
pub fn items_from_settings(t: &Tray) -> Vec<ItemSpec> {
    vec![ItemSpec {
        id: "main".into(),
        metrics: t.metrics.iter().take(3).copied().collect(),
        glyph: match t.style {
            TrayStyle::Bars => Glyph::Bars,
            TrayStyle::Graph => Glyph::Graph,
        },
        opens: Opens::Main,
    }]
}

/// Items from `metric:glyph[:own]` entries separated by commas, e.g. `cpu:ring,memory:number,cpu:cores:own`
/// (metrics `cpu memory disk network gpu`; glyphs `bars graph meter ring number graphnumber cores`). Anything
/// unrecognised is skipped; at most 8 items.
pub fn parse_items(text: &str) -> Vec<ItemSpec> {
    let mut out: Vec<ItemSpec> = Vec::new();
    for entry in text.split(',').take(8) {
        let parts: Vec<&str> = entry.trim().split(':').collect();
        let metric = match parts.first().copied() {
            Some("cpu") => TrayMetric::Cpu,
            Some("memory") => TrayMetric::Memory,
            Some("disk") => TrayMetric::Disk,
            Some("network") => TrayMetric::Network,
            Some("gpu") => TrayMetric::Gpu,
            _ => continue,
        };
        let glyph = match parts.get(1).copied() {
            Some("bars") => Glyph::Bars,
            Some("graph") => Glyph::Graph,
            Some("meter") => Glyph::Meter,
            Some("ring") => Glyph::Ring,
            Some("number") => Glyph::Number,
            Some("graphnumber") => Glyph::GraphNumber,
            Some("cores") if metric == TrayMetric::Cpu => Glyph::CoreBars,
            _ => continue,
        };
        out.push(ItemSpec {
            id: format!("{}-{}", parts[0], out.len() + 1),
            metrics: vec![metric],
            glyph,
            opens: if parts.get(2) == Some(&"own") { Opens::Own } else { Opens::Main },
        });
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

fn metric_name(m: TrayMetric) -> &'static str {
    match m {
        TrayMetric::Cpu => "CPU",
        TrayMetric::Memory => "Memory",
        TrayMetric::Disk => "Disk",
        TrayMetric::Network => "Network",
        TrayMetric::Gpu => "GPU",
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

    /// The number (0-100) of `m` in `u`; `None` when the PC cannot measure it.
    fn value(&self, u: &LiveUpdate, m: TrayMetric) -> Option<f32> {
        let (rx, tx) = u.system.network_totals();
        let v = match m {
            TrayMetric::Cpu => Some(u.system.cpu.total),
            TrayMetric::Memory => Some(u.system.memory_percent()),
            TrayMetric::Disk => u.system.disk_io.busy.value().copied(),
            TrayMetric::Network => Some((((rx + tx) / self.net_peak) * 100.0) as f32),
            TrayMetric::Gpu => u.system.gpu.value().map(|g| g.utilization),
        };
        v.map(|v| v.clamp(0.0, 100.0))
    }

    /// The pictures that changed since the last call, as `(item id, frame)`. A metric the PC cannot measure is left
    /// out of a combined item (CPU stands in when none is) and shows an empty glyph with an honest tooltip alone.
    pub fn frames(&mut self, u: &LiveUpdate, dark_taskbar: bool) -> Vec<(String, Frame)> {
        let (rx, tx) = u.system.network_totals();
        self.net_peak = (self.net_peak * 0.995).max(rx + tx).max(NET_FLOOR);
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
        let values: Vec<(TrayMetric, f32)> = spec.metrics.iter().filter_map(|m| self.value(u, *m).map(|v| (*m, v))).collect();
        let first = spec.metrics.first().copied().unwrap_or(TrayMetric::Cpu);
        let value = values.first().map_or(0.0, |(_, v)| *v);
        let history = &mut self.state[i].history;
        history.push_back(value);
        while history.len() > HISTORY {
            history.pop_front();
        }
        let graph: Vec<f32> = history.iter().copied().collect();
        if spec.glyph == Glyph::CoreBars {
            let cores = &u.system.cpu.per_core;
            let busiest = cores.iter().copied().fold(0.0f32, f32::max);
            return Frame {
                icon: tray::render_core_bars(cores, size, dark),
                tooltip: format!("Dyno — CPU {:.0}% · busiest core {busiest:.0}%", u.system.cpu.total),
            };
        }
        if values.is_empty() && spec.glyph != Glyph::Bars {
            return Frame {
                icon: tray::render_meter(0.0, size, dark),
                tooltip: format!("Dyno — {} is not available on this PC", metric_name(first)),
            };
        }
        let combined: Vec<(TrayMetric, f32)> = if values.is_empty() {
            vec![(TrayMetric::Cpu, u.system.cpu.total.clamp(0.0, 100.0))]
        } else {
            values
        };
        let icon = match spec.glyph {
            Glyph::Bars => tray::render(&combined, &[], TrayStyle::Bars, size, dark),
            Glyph::Graph => tray::render(&combined, &graph, TrayStyle::Graph, size, dark),
            Glyph::Meter => tray::render_meter(value, size, dark),
            Glyph::Ring => tray::render_ring(value, size, dark),
            Glyph::Number => tray::render_number(value, size, dark),
            Glyph::GraphNumber => tray::render_graph_number(value, &graph, size, dark),
            Glyph::CoreBars => tray::render_meter(0.0, size, dark), // handled above
        };
        Frame {
            icon,
            tooltip: tray::tooltip(&combined),
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
            profile: ProfileId::Normal,
            rules_paused: false,
            active_actions: vec![],
            processes: None,
            groups: None,
        }
    }

    fn item(id: &str, metrics: &[TrayMetric], glyph: Glyph) -> ItemSpec {
        ItemSpec {
            id: id.into(),
            metrics: metrics.to_vec(),
            glyph,
            opens: Opens::Main,
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
    fn settings_become_one_combined_item_as_before() {
        let items = items_from_settings(&Tray {
            metrics: vec![TrayMetric::Cpu, TrayMetric::Memory],
            style: TrayStyle::Graph,
        });
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].glyph, items[0].metrics.len(), items[0].opens),
            (Glyph::Graph, 2, Opens::Main)
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
            .map(|(i, g)| item(&format!("i{i}"), &[TrayMetric::Cpu], *g))
            .collect();
        let mut m = TrayItems::new(specs, 32);
        let first = m.frames(&update(40.0), true);
        assert_eq!(first.len(), all.len());
        assert!(first.iter().all(|(_, f)| f.icon.size == 32 && f.tooltip.starts_with("Dyno")));
        // The same numbers again: nothing to repaint (the graph glyphs move with history, so only they may repeat).
        let again = m.frames(&update(40.0), true);
        assert!(
            again.iter().all(|(id, _)| id == "i1" || id == "i5"),
            "{:?}",
            again.iter().map(|(i, _)| i).collect::<Vec<_>>()
        );
        let moved = m.frames(&update(80.0), true);
        assert!(moved.len() >= 5);
    }

    #[test]
    fn per_core_bars_follow_the_cores_and_name_the_busiest() {
        let mut u = update(10.0);
        u.system.cpu.per_core = vec![5.0, 95.0, 10.0, 0.0];
        let mut m = TrayItems::new(vec![item("c", &[TrayMetric::Cpu], Glyph::CoreBars)], 32);
        let f = m.frames(&u, true).remove(0).1;
        assert!(f.tooltip.contains("busiest core 95%"), "{}", f.tooltip);
    }

    #[test]
    fn a_combined_item_leaves_out_what_cannot_be_measured_and_a_lone_one_says_so() {
        let u = update(40.0);
        let mut m = TrayItems::new(
            vec![
                item("a", &[TrayMetric::Gpu, TrayMetric::Cpu], Glyph::Bars),
                item("b", &[TrayMetric::Gpu], Glyph::Ring),
                item("c", &[TrayMetric::Gpu], Glyph::Bars),
            ],
            32,
        );
        let f = m.frames(&u, true);
        assert_eq!(f[0].1.tooltip, "Dyno — CPU 40%");
        assert!(f[1].1.tooltip.contains("GPU is not available"));
        assert_eq!(f[2].1.tooltip, "Dyno — CPU 40%", "nothing measurable: the CPU stands in");
        let mut u = u;
        u.system.gpu = Reading::Unavailable("x".into());
        assert!(!m.set_specs(m.specs().to_vec()), "same items: no change");
    }

    #[test]
    fn values_are_clamped_and_network_is_relative_to_the_busiest_second() {
        let mut u = update(250.0);
        u.system.network[0].rx_bps = 5_000_000.0;
        let mut m = TrayItems::new(
            vec![
                item("c", &[TrayMetric::Cpu], Glyph::Number),
                item("n", &[TrayMetric::Network], Glyph::Number),
            ],
            32,
        );
        let f = m.frames(&u, true);
        assert!(f[0].1.tooltip.contains("CPU 100%"));
        assert!(f[1].1.tooltip.contains("Network 100%"), "{}", f[1].1.tooltip);
        u.system.network[0].rx_bps = 2_500_000.0;
        let f = m.frames(&u, true);
        assert!(f.iter().any(|(id, fr)| id == "n" && fr.tooltip.contains("Network 50%")), "{f:?}");
    }

    #[test]
    fn changing_the_items_resets_them() {
        let mut m = TrayItems::new(vec![item("a", &[TrayMetric::Cpu], Glyph::Ring)], 32);
        m.frames(&update(10.0), true);
        assert!(m.set_specs(vec![item("b", &[TrayMetric::Cpu], Glyph::Ring)]));
        assert_eq!(m.frames(&update(10.0), true)[0].0, "b");
    }
}
