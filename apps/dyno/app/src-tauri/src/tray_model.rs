//! What the tray / menu-bar items show, as pure functions: each item's metric and glyph become RGBA pixels and a tooltip
//! from the live updates (`dyno_core::tray` draws; this picks the numbers). The shell hands the pixels to the OS at most
//! twice a second and only for items whose picture changed.
//!
//! The items come from the menu bar layout the agent keeps ([`items_from_settings`]); `DYNO_TRAY_ITEMS` (see [`parse_items`]) overrides
//! them for development and measurements. A small always-visible Windows mini-bar window is a documented stub (off by
//! default; not built yet).

use dyno_core::protocol::LiveUpdate;
use dyno_core::tray::{self, Icon, IconMetric, IconStyle};
use dyno_core::widgets::menu_bar::MAX_ITEMS;
use dyno_core::widgets::{read_metric, MenuBarItem, Metric, Thresholds};
use paddock_widgets::Placement;
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

/// One menu-bar / tray item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpec {
    /// Unique; `[a-z0-9-]{1,32}` for its own popover (the tray id suffix, window label and route).
    pub id: String,
    /// What it measures.
    pub metric: Metric,
    /// How it draws.
    pub glyph: Glyph,
    /// The label a number shows in the tooltip (empty: the metric's name).
    pub label: String,
    /// The unit a number shows in the tooltip (empty: the metric's own).
    pub unit: String,
}

impl ItemSpec {
    /// A spec with the default label and unit.
    pub fn new(id: &str, metric: Metric, glyph: Glyph) -> ItemSpec {
        ItemSpec {
            id: id.into(),
            metric,
            glyph,
            label: String::new(),
            unit: String::new(),
        }
    }
}

/// The items for the menu bar layout (the agent's, `settings.menu_bar`).
pub fn items_from_settings(placements: &[Placement<MenuBarItem>]) -> Vec<ItemSpec> {
    placements
        .iter()
        .map(|p| {
            let metric = p.widget.metric().clone();
            let (glyph, label, unit) = match &p.widget {
                MenuBarItem::LineGraph { .. } => (Glyph::Graph, String::new(), String::new()),
                MenuBarItem::VerticalBars { .. } if metric == Metric::CpuPerCore => (Glyph::CoreBars, String::new(), String::new()),
                MenuBarItem::VerticalBars { .. } => (Glyph::Bars, String::new(), String::new()),
                MenuBarItem::HorizontalMeter { .. } => (Glyph::Meter, String::new(), String::new()),
                MenuBarItem::Ring { .. } => (Glyph::Ring, String::new(), String::new()),
                MenuBarItem::Number { unit, label, .. } => (Glyph::Number, label.clone(), unit.clone()),
                MenuBarItem::GraphAndNumber { .. } => (Glyph::GraphNumber, String::new(), String::new()),
            };
            ItemSpec {
                id: p.id.to_string(),
                metric,
                glyph,
                label,
                unit,
            }
        })
        .collect()
}

/// Items from `metric:glyph` entries separated by commas, e.g. `cpu:ring,memory:number,cpu:cores` (metrics
/// `cpu memory swap disk network gpu temp battery`; glyphs `bars graph meter ring number graphnumber cores`). Anything
/// unrecognised is skipped; at most [`MAX_ITEMS`] items.
pub fn parse_items(text: &str) -> Vec<ItemSpec> {
    let mut out: Vec<ItemSpec> = Vec::new();
    for entry in text.split(',').take(usize::from(MAX_ITEMS)) {
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
        out.push(ItemSpec::new(&format!("{}-{}", parts[0], out.len() + 1), metric, glyph));
    }
    out
}

/// What changing the icons from `current` to `wanted` takes: only items that are gone or differ are removed, only items that are new
/// or differ are made, so an unchanged item keeps its native icon (and with it its place in the bar).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncPlan {
    /// Ids to remove, sorted.
    pub remove: Vec<String>,
    /// Specs to make, in `wanted` order.
    pub create: Vec<ItemSpec>,
}

/// The [`SyncPlan`] from the icons now there to the wanted specs.
pub fn plan_sync(current: &std::collections::HashMap<String, ItemSpec>, wanted: &[ItemSpec]) -> SyncPlan {
    let mut remove: Vec<String> = current
        .iter()
        .filter(|(id, old)| wanted.iter().find(|w| &w.id == *id).is_none_or(|w| w != *old))
        .map(|(id, _)| id.clone())
        .collect();
    remove.sort();
    let create = wanted.iter().filter(|w| current.get(&w.id) != Some(*w)).cloned().collect();
    SyncPlan { remove, create }
}

/// What one drawn frame costs the native icon. The picture is set in ONE call whatever else changed (a second call renders the
/// status item twice and flickers on macOS, where `set_icon` and then `set_icon_as_template` did exactly that), the tooltip is
/// sent only when its text changed, and the saved position is refreshed on the first draw and every 30th.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaintOps {
    /// Calls that set the picture (always 1).
    pub image_sets: u8,
    /// Whether the tooltip is sent.
    pub tooltip: bool,
    /// Whether the macOS position copy is refreshed.
    pub save_position: bool,
}

/// The [`PaintOps`] of the `count`th draw (1-based) of an item whose tooltip was `previous`.
pub fn paint_ops(previous: &str, tooltip: &str, count: u64) -> PaintOps {
    PaintOps {
        image_sets: 1,
        tooltip: previous != tooltip,
        save_position: crate::tray_identity::should_save_position(count),
    }
}

/// What a render function gets: the value on the glyph's 0..100 scale, the history (oldest first), the pixel height of the bar
/// and whether the bar is dark.
pub struct RenderCtx<'a> {
    /// The value, 0 to 100.
    pub value: f32,
    /// The recent values, oldest first.
    pub graph: &'a [f32],
    /// The bar's picture height in pixels.
    pub size: u32,
    /// Whether the taskbar or menu bar is dark.
    pub dark: bool,
}

/// A picture and the fixed width its status item keeps. One function per item kind returns this, so a new look replaces a function
/// here and nothing else (the mechanics around it are the same for every look).
pub struct Rendered {
    /// The pixels.
    pub icon: Icon,
    /// The width in pixels (today the picture is square, so equal to its size).
    pub width: u32,
}

impl Rendered {
    fn of(icon: Icon) -> Rendered {
        Rendered { width: icon.size, icon }
    }
}

fn render_bars(c: &RenderCtx) -> Rendered {
    Rendered::of(tray::render(&[(IconMetric::Cpu, c.value)], &[], IconStyle::Bars, c.size, c.dark))
}

fn render_graph(c: &RenderCtx) -> Rendered {
    Rendered::of(tray::render(
        &[(IconMetric::Cpu, c.value)],
        c.graph,
        IconStyle::Graph,
        c.size,
        c.dark,
    ))
}

fn render_meter(c: &RenderCtx) -> Rendered {
    Rendered::of(tray::render_meter(c.value, c.size, c.dark))
}

fn render_ring(c: &RenderCtx) -> Rendered {
    Rendered::of(tray::render_ring(c.value, c.size, c.dark))
}

fn render_number(c: &RenderCtx) -> Rendered {
    Rendered::of(tray::render_number(c.value, c.size, c.dark))
}

fn render_graph_number(c: &RenderCtx) -> Rendered {
    Rendered::of(tray::render_graph_number(c.value, c.graph, c.size, c.dark))
}

fn render_core_bars(cores: &[f32], size: u32, dark: bool) -> Rendered {
    Rendered::of(tray::render_core_bars(cores, size, dark))
}

/// The picture of an item whose metric cannot be measured here.
fn render_unavailable(size: u32, dark: bool) -> Rendered {
    Rendered::of(tray::render_meter(0.0, size, dark))
}

/// The render function of a glyph (per-core bars take the cores, not a single value: see [`render_core_bars`]).
pub fn render_glyph(glyph: Glyph, c: &RenderCtx) -> Rendered {
    match glyph {
        Glyph::Bars => render_bars(c),
        Glyph::Graph => render_graph(c),
        Glyph::Meter => render_meter(c),
        Glyph::Ring => render_ring(c),
        Glyph::Number => render_number(c),
        Glyph::GraphNumber => render_graph_number(c),
        Glyph::CoreBars => render_unavailable(c.size, c.dark),
    }
}

/// An icon with its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The pixels.
    pub icon: Icon,
    /// The width in pixels the status item keeps whatever the value (its length never follows the text).
    pub width: u32,
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

    /// The reading of `m` in `u` on the 0 to 100 drawing scale; `None` when the PC cannot measure it. The number comes from
    /// the engine's one reader ([`read_metric`]); only the scale of the rates (against the busiest second seen) and of the
    /// words and degrees is the icon's own.
    fn measure(&self, u: &LiveUpdate, m: &Metric) -> Option<Measured> {
        // A fan speed and a counter have no scale to draw.
        if matches!(m, Metric::Fan | Metric::PerfCounter(_)) {
            return None;
        }
        let r = read_metric(m, u, &Thresholds::default());
        let value = r.value?;
        let scale = match (r.percent, m) {
            (Some(p), _) => p,
            (None, Metric::NetworkUp | Metric::NetworkDown) => ((value / self.net_peak) * 100.0).clamp(0.0, 100.0) as f32,
            (None, Metric::MemoryPressure) => match u.pressure {
                dyno_core::metrics::MemoryPressure::Normal => 20.0,
                dyno_core::metrics::MemoryPressure::Elevated => 60.0,
                dyno_core::metrics::MemoryPressure::Critical => 95.0,
            },
            (None, _) => (value as f32).clamp(0.0, 100.0),
        };
        let text = match r.unit.as_str() {
            "" => r.text,
            "%" => format!("{}%", r.text),
            unit => format!("{} {unit}", r.text),
        };
        Some(Measured { scale, text })
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
            let r = render_core_bars(cores, size, dark);
            return Frame {
                icon: r.icon,
                width: r.width,
                tooltip: format!("Dyno — {name} {:.0}% · busiest core {busiest:.0}%", u.system.cpu.total),
            };
        }
        let Some(m) = self.measure(u, &spec.metric) else {
            let r = render_unavailable(size, dark);
            return Frame {
                icon: r.icon,
                width: r.width,
                tooltip: format!("Dyno — {name} is not available on this computer"),
            };
        };
        let history = &mut self.state[i].history;
        history.push_back(m.scale);
        while history.len() > HISTORY {
            history.pop_front();
        }
        let graph: Vec<f32> = history.iter().copied().collect();
        let v = m.scale;
        let rendered = render_glyph(
            spec.glyph,
            &RenderCtx {
                value: v,
                graph: &graph,
                size,
                dark,
            },
        );
        let text = if spec.unit.is_empty() {
            m.text
        } else {
            format!("{:.0}{}", m.scale, spec.unit)
        };
        Frame {
            icon: rendered.icon,
            width: rendered.width,
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
    use dyno_core::widgets::MenuBarCatalogue;
    use dyno_platform::fake::quiet_sample;
    use paddock_widgets::{assign_ids, Catalogue};

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
        ItemSpec::new(id, metric, glyph)
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
        let default = items_from_settings(&MenuBarCatalogue::defaults());
        assert_eq!(default.len(), 2);
        assert_eq!((default[0].glyph, &default[0].metric), (Glyph::Bars, &Metric::CpuTotal));
        assert_eq!(default[0].id, "vertical-bars");
        assert_eq!(default[1].id, "vertical-bars-2");
        let s = assign_ids([
            (
                1,
                MenuBarItem::VerticalBars {
                    metric: Metric::CpuPerCore,
                },
            ),
            (
                1,
                MenuBarItem::Number {
                    metric: Metric::Temperature,
                    unit: "C".into(),
                    label: "Hot".into(),
                },
            ),
            (1, MenuBarItem::GraphAndNumber { metric: Metric::Memory }),
            (1, MenuBarItem::LineGraph { metric: Metric::Swap }),
            (1, MenuBarItem::HorizontalMeter { metric: Metric::Battery }),
            (1, MenuBarItem::Ring { metric: Metric::GpuLoad }),
        ]);
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
        assert_eq!((items[1].label.as_str(), items[1].unit.as_str()), ("Hot", "C"));
    }

    #[test]
    fn more_than_four_items_each_get_their_own_spec_and_frame() {
        let metrics = [
            Metric::CpuTotal,
            Metric::Memory,
            Metric::Swap,
            Metric::DiskActivity,
            Metric::NetworkDown,
            Metric::GpuLoad,
            Metric::Temperature,
            Metric::Battery,
        ];
        let s = assign_ids(metrics.iter().cloned().map(|metric| (1, MenuBarItem::Ring { metric })));
        let specs = items_from_settings(&s);
        assert_eq!(specs.len(), 8);
        let ids: std::collections::HashSet<&str> = specs.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids.len(), 8, "every item has its own id");
        let mut items = TrayItems::new(specs, 18);
        assert_eq!(items.frames(&update(10.0), true).len(), 8, "every item draws");
        let text = (0..usize::from(MAX_ITEMS) + 5).map(|_| "cpu:ring").collect::<Vec<_>>().join(",");
        assert_eq!(parse_items(&text).len(), usize::from(MAX_ITEMS));
    }

    /// ER-20 measurement, not a check: `cargo test -p dyno-app --lib measure_32 -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement that prints the drawing time of 32 graph items; it asserts nothing about time"]
    fn measure_32_graph_items() {
        let specs: Vec<ItemSpec> = (0..32)
            .map(|n| ItemSpec::new(&format!("cpu-{n}"), Metric::CpuTotal, Glyph::Graph))
            .collect();
        let mut items = TrayItems::new(specs, 18);
        let start = Instant::now();
        let mut drawn = 0;
        for tick in 0..200 {
            drawn += items.frames(&update((tick % 100) as f32), true).len();
        }
        let per_tick = start.elapsed() / 200;
        println!("32 graph items: {drawn} frames in 200 ticks, {per_tick:?} per tick (all items changing every tick)");
    }

    #[test]
    fn a_frame_sets_the_picture_once_and_the_tooltip_only_when_it_changed() {
        let first = paint_ops("", "Dyno CPU 5%", 1);
        assert_eq!((first.image_sets, first.tooltip, first.save_position), (1, true, true));
        let same = paint_ops("Dyno CPU 5%", "Dyno CPU 5%", 2);
        assert_eq!((same.image_sets, same.tooltip, same.save_position), (1, false, false));
        assert!(paint_ops("Dyno CPU 5%", "Dyno CPU 6%", 3).tooltip);
    }

    #[test]
    fn an_items_picture_keeps_its_size_whatever_the_value() {
        // A picture that changes size changes the status item's width and moves its neighbours: the flicker's third suspect.
        for glyph in [
            Glyph::Bars,
            Glyph::Graph,
            Glyph::Meter,
            Glyph::Ring,
            Glyph::Number,
            Glyph::GraphNumber,
            Glyph::CoreBars,
        ] {
            let mut items = TrayItems::new(vec![ItemSpec::new("cpu-1", Metric::CpuTotal, glyph)], 18);
            let mut shapes = std::collections::HashSet::new();
            for v in [0.0, 1.0, 9.0, 10.0, 55.0, 99.0, 100.0] {
                for (_, f) in items.frames(&update(v), true) {
                    shapes.insert((f.icon.size, f.icon.rgba.len(), f.width));
                }
            }
            assert_eq!(shapes.len(), 1, "{glyph:?} changed size: {shapes:?}");
        }
    }

    #[test]
    fn an_unchanged_value_draws_nothing() {
        let mut items = TrayItems::new(vec![ItemSpec::new("cpu-1", Metric::CpuTotal, Glyph::Ring)], 18);
        assert_eq!(items.frames(&update(40.0), true).len(), 1);
        assert!(
            items.frames(&update(40.0), true).is_empty(),
            "no redraw when the picture did not change"
        );
    }

    #[test]
    fn only_changed_items_are_removed_and_made() {
        use std::collections::HashMap;
        let a = ItemSpec::new("cpu-1", Metric::CpuTotal, Glyph::Ring);
        let b = ItemSpec::new("memory-2", Metric::Memory, Glyph::Number);
        let c = ItemSpec::new("swap-3", Metric::Swap, Glyph::Bars);
        let current: HashMap<String, ItemSpec> = [&a, &b, &c].iter().map(|s| (s.id.clone(), (*s).clone())).collect();
        assert_eq!(
            plan_sync(&current, &[a.clone(), b.clone(), c.clone()]),
            SyncPlan::default(),
            "nothing changed"
        );
        let b2 = ItemSpec::new("memory-2", Metric::Memory, Glyph::Graph);
        let d = ItemSpec::new("gpu-4", Metric::GpuLoad, Glyph::Ring);
        let plan = plan_sync(&current, &[a.clone(), b2.clone(), d.clone()]);
        assert_eq!(plan.remove, ["memory-2", "swap-3"]);
        assert_eq!(plan.create, [b2, d]);
        assert_eq!(plan_sync(&HashMap::new(), std::slice::from_ref(&a)).create, [a]);
    }

    #[test]
    fn items_parse_from_text_and_junk_is_skipped() {
        let items = parse_items("cpu:ring, memory:number, gpu:nope, disk:cores, cpu:cores, bogus:ring");
        let got: Vec<(&str, Glyph)> = items.iter().map(|i| (i.id.as_str(), i.glyph)).collect();
        assert_eq!(
            got,
            [("cpu-1", Glyph::Ring), ("memory-2", Glyph::Number), ("cpu-3", Glyph::CoreBars)]
        );
        assert!(items.iter().all(|i| i.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')));
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
        assert!(f[3].1.tooltip.contains("Normal"));
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
