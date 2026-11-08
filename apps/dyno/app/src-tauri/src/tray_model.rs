//! What the tray / menu-bar items show, as pure functions: each item's metric and glyph become RGBA pixels and a tooltip
//! from the live updates (`dyno_core::tray` draws; this picks the numbers). The shell hands the pixels to the OS at most
//! twice a second and only for items whose picture changed.
//!
//! The items come from the menu bar layout the agent keeps ([`items_from_settings`]); `DYNO_TRAY_ITEMS` (see [`parse_items`]) overrides
//! them for development and measurements. A small always-visible Windows mini-bar window is a documented stub (off by
//! default; not built yet).

use crate::tray_look::{mac_kind, px_kind, render_mac, render_px, AlertGate, AlertPolicy, Look, Sample};
use dyno_core::protocol::LiveUpdate;
use dyno_core::tray::text::TextRaster;
use dyno_core::tray::{menu_bar, pixel};
use dyno_core::widgets::menu_bar::MAX_ITEMS;
use dyno_core::widgets::{read_metric, MenuBarItem, Metric, MetricReading, NumberFormat, Thresholds};
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
    /// Whether a number draws on one line (otherwise stacked).
    pub line: bool,
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
            line: false,
        }
    }
}

/// The items for the menu bar layout (the agent's, `settings.menu_bar`).
pub fn items_from_settings(placements: &[Placement<MenuBarItem>]) -> Vec<ItemSpec> {
    placements
        .iter()
        .map(|p| {
            let metric = p.widget.metric().clone();
            let line = matches!(
                &p.widget,
                MenuBarItem::Number {
                    format: NumberFormat::Line,
                    ..
                }
            );
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
                line,
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
            Some("network") => Metric::NetworkBoth,
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

/// What one drawn frame costs the native icon. The picture is set in ONE call when it changed (a second call renders the
/// status item twice and flickers on macOS, where `set_icon` and then `set_icon_as_template` did exactly that) and not at
/// all when only the tooltip changed (each set redraws the item; an idle Dyno's window-server cost was mostly these redraws,
/// docs/performance.md 2026-11-03), the tooltip is sent only when its text changed, and the saved position is refreshed on
/// the first draw and every 30th.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaintOps {
    /// Calls that set the picture: 1 when it changed, else 0.
    pub image_sets: u8,
    /// Whether the tooltip is sent.
    pub tooltip: bool,
    /// Whether the macOS position copy is refreshed.
    pub save_position: bool,
}

/// The [`PaintOps`] of the `count`th draw (1-based) of an item whose tooltip was `previous`; `picture_changed` says whether
/// its picture differs from the one last set (true for the first draw).
pub fn paint_ops(previous: &str, tooltip: &str, picture_changed: bool, count: u64) -> PaintOps {
    PaintOps {
        image_sets: u8::from(picture_changed),
        tooltip: previous != tooltip,
        save_position: crate::tray_identity::should_save_position(count),
    }
}

/// The longest tooltip in UTF-16 units that every platform shows whole: the Windows notification area keeps 127 (its `szTip` holds 128
/// with the terminating zero); macOS and the status-notifier hosts have no such limit but a long tip is unreadable anyway.
pub const TOOLTIP_MAX_UNITS: usize = 127;

/// What an item's tooltip says about its metric.
#[derive(Debug, Clone, PartialEq)]
pub enum TipState {
    /// The reading, as drawn on the icon (for example "34%").
    Value(String),
    /// The reading while its alarm (or fallback limit) is active.
    Alert(String),
    /// The metric cannot be measured on this computer.
    Unavailable,
    /// The agent is not running, so there is no reading.
    AgentAway,
    /// The per-core bars: the total and the busiest core, in percent.
    Cores {
        /// The CPU total.
        total: f32,
        /// The busiest core.
        busiest: f32,
    },
}

/// The tooltip of one tray item: the metric's name first (the app name is the menu's, not the tip's), then the reading and, when its
/// alarm is active, the state in words (below 24 px alerts on different metrics differ only by their order). Cut to
/// [`TOOLTIP_MAX_UNITS`] with an ellipsis. Pure: the same inputs give the same text, so a changed tip is a changed reading.
pub fn item_tooltip(name: &str, state: &TipState) -> String {
    let text = match state {
        TipState::Value(v) => format!("{name} {v}"),
        TipState::Alert(v) => format!("{name} {v} (alert: above its limit)"),
        TipState::Unavailable => format!("{name} is not available on this computer"),
        TipState::AgentAway => format!("{name}: the agent is not running"),
        TipState::Cores { total, busiest } => format!("{name} {total:.0}% · busiest core {busiest:.0}%"),
    };
    fit_units(&text, TOOLTIP_MAX_UNITS)
}

/// `text` cut to at most `max` UTF-16 units, ending in an ellipsis when it was cut (never inside a character).
fn fit_units(text: &str, max: usize) -> String {
    if text.encode_utf16().count() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut units = 0;
    for c in text.chars() {
        units += c.len_utf16();
        if units > max - 1 {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// A picture in device pixels: straight RGBA bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Picture {
    /// Row-major RGBA bytes (`width * height * 4`).
    pub rgba: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Whether a menu bar item shows one app's numbers, which come from the top lists: the shell then asks for the summary
/// level even with no window open, so the process table keeps the user's pace for that item.
pub fn shows_apps(specs: &[ItemSpec]) -> bool {
    specs.iter().any(|s| matches!(s.metric, Metric::Process(_)))
}

/// An item's pictures with its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The pictures and the item's fixed size (see [`Look`]).
    pub look: Look,
    /// The tooltip.
    pub tooltip: String,
}

/// Which family of renderers draws the items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS: template images at 1x and @2x, text through the text port.
    Mac,
    /// Windows and Linux: whole-pixel icons.
    Pixels,
}

/// What the items are drawn with: the renderer family, the icon size where it matters, and the text port.
#[derive(Clone)]
pub struct Env {
    /// The renderer family.
    pub platform: Platform,
    /// The icon size in pixels for [`Platform::Pixels`] (16, 20, 24, 32; Linux 22).
    pub px: u32,
    /// The text port ([`Platform::Mac`]).
    pub text: std::sync::Arc<dyn TextRaster + Send + Sync>,
}

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("platform", &self.platform)
            .field("px", &self.px)
            .finish()
    }
}

impl Env {
    /// This machine's: CoreText on macOS, the taskbar's size on Windows, 22 px on Linux.
    pub fn native() -> Env {
        #[cfg(target_os = "macos")]
        {
            Env {
                platform: Platform::Mac,
                px: 18,
                text: std::sync::Arc::new(crate::tray_text::CoreText::new()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Env {
                platform: Platform::Pixels,
                px: crate::tray::native_icon_px(),
                text: std::sync::Arc::new(dyno_core::tray::text::BoxText),
            }
        }
    }
}

#[derive(Debug, Default)]
struct ItemState {
    history: VecDeque<f32>,
    alert: AlertGate,
    last: Option<Frame>,
}

/// Draws the tray items from live updates.
#[derive(Debug)]
pub struct TrayItems {
    specs: Vec<ItemSpec>,
    state: Vec<ItemState>,
    net_peak: f64,
    env: Env,
    /// The last frame of each item with what it was drawn from: an item whose inputs did not change is not rendered again.
    memo: std::cell::RefCell<std::collections::HashMap<String, (MemoKey, Frame)>>,
    policy: AlertPolicy,
    /// The metrics the agent has an open alarm for now.
    alarmed: Vec<Metric>,
}

/// A measurement: where it sits on the 0-100 drawing scale, and how to say it.
struct Measured {
    scale: f32,
    raw: f64,
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
        Metric::NetworkBoth => "Network".into(),
        Metric::GpuLoad => "GPU".into(),
        Metric::Temperature => "Temperature".into(),
        Metric::Fan => "Fan".into(),
        Metric::Battery => "Battery".into(),
        Metric::Process(n) => n.clone(),
        Metric::PerfCounter(p) => p.clone(),
    }
}

impl TrayItems {
    /// Items drawn with `env`.
    pub fn new(specs: Vec<ItemSpec>, env: Env) -> TrayItems {
        let state = specs.iter().map(|_| ItemState::default()).collect();
        TrayItems {
            specs,
            state,
            net_peak: NET_FLOOR,
            env,
            memo: Default::default(),
            policy: AlertPolicy::default(),
            alarmed: Vec::new(),
        }
    }

    /// Which alarm rules cover which metrics, and the fallback level (the settings changed).
    pub fn set_policy(&mut self, policy: AlertPolicy) {
        self.policy = policy;
    }

    /// The metrics with an open alarm (an alarm changed).
    pub fn set_alarmed(&mut self, alarmed: Vec<Metric>) {
        self.alarmed = alarmed;
    }

    /// The icon size changed (the taskbar's DPI): every item is drawn again at the next update. Returns whether it changed.
    pub fn set_px(&mut self, px: u32) -> bool {
        if self.env.px == px {
            return false;
        }
        self.env.px = px;
        self.memo.borrow_mut().clear();
        for st in &mut self.state {
            st.last = None;
        }
        true
    }

    /// The frame of every item as unavailable (the agent is away), and forgets what was drawn so the live pictures return.
    pub fn unavailable_frames(&mut self, dark: bool) -> Vec<(String, Frame)> {
        let out = (0..self.specs.len())
            .map(|i| {
                let spec = self.specs[i].clone();
                let name = self.name(&spec);
                (
                    spec.id.clone(),
                    self.frame(
                        &spec,
                        &Sample::default(),
                        Visual::Unavailable,
                        dark,
                        item_tooltip(&name, &TipState::AgentAway),
                    ),
                )
            })
            .collect();
        for st in &mut self.state {
            st.last = None;
        }
        out
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
        self.memo.borrow_mut().clear();
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
            // The combined item scales by its busier direction against the busiest second seen in either.
            (None, Metric::NetworkBoth) => {
                let (rx, tx) = u.system.network_totals();
                ((rx.max(tx) / self.net_peak) * 100.0).clamp(0.0, 100.0) as f32
            }
            (None, Metric::MemoryPressure) => match u.pressure {
                dyno_core::metrics::MemoryPressure::Normal => 20.0,
                dyno_core::metrics::MemoryPressure::Elevated => 60.0,
                dyno_core::metrics::MemoryPressure::Critical => 95.0,
            },
            (None, _) => (value as f32).clamp(0.0, 100.0),
        };
        let words = |r: MetricReading| match r.unit.as_str() {
            "" => r.text,
            "%" => format!("{}%", r.text),
            unit => format!("{} {unit}", r.text),
        };
        let text = if *m == Metric::NetworkBoth {
            // The item draws both rates, so its words give both, not the total the engine reads for other consumers.
            let t = Thresholds::default();
            format!(
                "{} up, {} down",
                words(read_metric(&Metric::NetworkUp, u, &t)),
                words(read_metric(&Metric::NetworkDown, u, &t))
            )
        } else {
            words(r)
        };
        Some(Measured { scale, raw: value, text })
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

    fn name(&self, spec: &ItemSpec) -> String {
        if spec.label.is_empty() {
            metric_name(&spec.metric)
        } else {
            spec.label.clone()
        }
    }

    /// The pictures of one item in one visual state.
    fn frame(&self, spec: &ItemSpec, s: &Sample, visual: Visual, dark: bool, tooltip: String) -> Frame {
        let key = MemoKey {
            sample: s.clone(),
            visual,
            dark,
            tooltip: tooltip.clone(),
        };
        if let Some((k, f)) = self.memo.borrow().get(&spec.id) {
            if *k == key {
                return f.clone();
            }
        }
        let frame = self.render(spec, s, visual, dark, tooltip);
        self.memo.borrow_mut().insert(spec.id.clone(), (key, frame.clone()));
        frame
    }

    fn render(&self, spec: &ItemSpec, s: &Sample, visual: Visual, dark: bool, tooltip: String) -> Frame {
        let look = match self.env.platform {
            Platform::Mac => {
                let st = match visual {
                    Visual::Normal => menu_bar::State::Normal,
                    Visual::Alert => menu_bar::State::Alert,
                    Visual::Unavailable => menu_bar::State::Unavailable,
                };
                render_mac(&mac_kind(spec, s.cores.len().max(1)), s, st, &*self.env.text)
            }
            Platform::Pixels => {
                let st = match visual {
                    Visual::Normal => pixel::State::Normal,
                    Visual::Alert => pixel::State::Alert,
                    Visual::Unavailable => pixel::State::Unavailable,
                };
                render_px(&px_kind(spec), s, st, self.env.px, dark)
            }
        };
        Frame { look, tooltip }
    }

    fn draw(&mut self, i: usize, u: &LiveUpdate, dark: bool) -> Frame {
        let spec = self.specs[i].clone();
        let name = self.name(&spec);
        let (down, up) = u.system.network_totals();
        let cores = u.system.cpu.per_core.clone();
        let now = u.system.at.0;
        if spec.glyph == Glyph::CoreBars {
            let busiest = cores.iter().copied().fold(0.0f32, f32::max);
            let s = Sample {
                value: Some(u.system.cpu.total),
                cores,
                ..Sample::default()
            };
            let tooltip = item_tooltip(
                &name,
                &TipState::Cores {
                    total: u.system.cpu.total,
                    busiest,
                },
            );
            return self.frame(&spec, &s, Visual::Normal, dark, tooltip);
        }
        let network = matches!(spec.metric, Metric::NetworkUp | Metric::NetworkDown | Metric::NetworkBoth);
        let Some(m) = self.measure(u, &spec.metric) else {
            self.state[i].alert = AlertGate::default();
            let tooltip = item_tooltip(&name, &TipState::Unavailable);
            return self.frame(&spec, &Sample::default(), Visual::Unavailable, dark, tooltip);
        };
        let history = &mut self.state[i].history;
        history.push_back(m.scale);
        while history.len() > HISTORY {
            history.pop_front();
        }
        // Degrees are drawn as degrees; everything else on the 0 to 100 scale.
        let shown = if spec.metric == Metric::Temperature {
            m.raw as f32
        } else {
            m.scale
        };
        let s = Sample {
            value: Some(shown),
            // Only the graphs draw the history; for the others it must not make the sample (and so the memo key) change every tick.
            history: if matches!(spec.glyph, Glyph::Graph | Glyph::GraphNumber) {
                history.iter().copied().collect()
            } else {
                Vec::new()
            },
            // Only the core bars draw the cores (they return above) and only the network items draw up and down: for the
            // others these change every tick on a real machine and would make the memo key miss every time.
            cores: Vec::new(),
            up: if network { up } else { 0.0 },
            down: if network { down } else { 0.0 },
        };
        let alert = if self.policy.covers(&spec.metric) {
            // An alarm rule covers it: the alert is the alarm, with the alarm's own hold and hysteresis.
            self.state[i].alert = AlertGate::default();
            self.alarmed.contains(&spec.metric)
        } else {
            let level = self.policy.fallback_level(&spec.metric);
            self.state[i].alert.update(Some(shown), level, now)
        };
        let text = if spec.unit.is_empty() {
            m.text
        } else {
            format!("{:.0}{}", m.scale, spec.unit)
        };
        // Below 24 px alerts on different metrics differ only by their order, so the tooltip says it in words.
        let tooltip = if alert {
            item_tooltip(&name, &TipState::Alert(text))
        } else {
            item_tooltip(&name, &TipState::Value(text))
        };
        self.frame(&spec, &s, if alert { Visual::Alert } else { Visual::Normal }, dark, tooltip)
    }
}

/// What a frame was rendered from (the item's spec and the icon size are fixed by the memo's lifetime).
#[derive(Debug, Clone, PartialEq)]
struct MemoKey {
    sample: Sample,
    visual: Visual,
    dark: bool,
    tooltip: String,
}

/// How an item is drawn now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visual {
    Normal,
    Alert,
    Unavailable,
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
    use paddock_widgets::{assign_spans, Catalogue};

    fn mac_env() -> Env {
        Env {
            platform: Platform::Mac,
            px: 18,
            text: std::sync::Arc::new(dyno_core::tray::text::BoxText),
        }
    }

    fn win_env(px: u32) -> Env {
        Env {
            platform: Platform::Pixels,
            px,
            text: std::sync::Arc::new(dyno_core::tray::text::BoxText),
        }
    }

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
            via: Default::default(),
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
        assert_eq!(default.len(), 4, "the design's default set");
        assert_eq!((default[0].glyph, &default[0].metric), (Glyph::Number, &Metric::CpuTotal));
        assert_eq!(default[0].id, "number");
        assert_eq!(default[1].id, "number-2");
        let s = assign_spans([
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
                    format: NumberFormat::Line,
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
        assert!(items[1].line && !items[0].line, "only the item set to one line draws on one line");
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
        let s = assign_spans(metrics.iter().cloned().map(|metric| (1, MenuBarItem::Ring { metric })));
        let specs = items_from_settings(&s);
        assert_eq!(specs.len(), 8);
        let ids: std::collections::HashSet<&str> = specs.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids.len(), 8, "every item has its own id");
        let mut items = TrayItems::new(specs, mac_env());
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
        let mut items = TrayItems::new(specs, mac_env());
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
        let first = paint_ops("", "Dyno CPU 5%", true, 1);
        assert_eq!((first.image_sets, first.tooltip, first.save_position), (1, true, true));
        let same = paint_ops("Dyno CPU 5%", "Dyno CPU 5%", true, 2);
        assert_eq!((same.image_sets, same.tooltip, same.save_position), (1, false, false));
        assert!(paint_ops("Dyno CPU 5%", "Dyno CPU 6%", true, 3).tooltip);
        // Only the tooltip changed: the picture is not set again (each set redraws the status item).
        let tip_only = paint_ops("Dyno CPU 5%", "Dyno CPU 6%", false, 4);
        assert_eq!((tip_only.image_sets, tip_only.tooltip), (0, true));
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
            let mut items = TrayItems::new(vec![ItemSpec::new("cpu-1", Metric::CpuTotal, glyph)], mac_env());
            let mut shapes = std::collections::HashSet::new();
            for v in [0.0, 1.0, 9.0, 10.0, 55.0, 99.0, 100.0] {
                for (_, f) in items.frames(&update(v), true) {
                    shapes.insert((
                        f.look.width,
                        f.look.height,
                        f.look.pictures.iter().map(|p| p.rgba.len()).sum::<usize>(),
                    ));
                }
            }
            assert_eq!(shapes.len(), 1, "{glyph:?} changed size: {shapes:?}");
        }
    }

    #[test]
    fn an_unchanged_value_draws_nothing() {
        let mut items = TrayItems::new(vec![ItemSpec::new("cpu-1", Metric::CpuTotal, Glyph::Ring)], mac_env());
        assert_eq!(items.frames(&update(40.0), true).len(), 1);
        assert!(
            items.frames(&update(40.0), true).is_empty(),
            "no redraw when the picture did not change"
        );
    }

    fn at(mut u: LiveUpdate, ms: i64) -> LiveUpdate {
        u.system.at = Timestamp(ms);
        u
    }

    fn frame_of(m: &mut TrayItems, u: &LiveUpdate) -> Option<Frame> {
        m.frames(u, true).pop().map(|(_, f)| f)
    }

    #[test]
    fn a_metric_an_alarm_rule_covers_alerts_only_with_an_open_alarm() {
        let mut m = TrayItems::new(vec![item("c", Metric::CpuTotal, Glyph::Number)], win_env(32));
        m.set_policy(AlertPolicy {
            covered: vec![Metric::CpuTotal],
            fallback_percent: 90.0,
            enabled: true,
        });
        let calm = frame_of(&mut m, &at(update(95.0), 0)).unwrap();
        // 95 % for 10 s, but the rule has not raised an alarm: no alert, whatever the fallback level says.
        assert_eq!(frame_of(&mut m, &at(update(95.0), 10_000)), None, "unchanged picture");
        m.set_alarmed(vec![Metric::CpuTotal]);
        let alarmed = frame_of(&mut m, &at(update(95.0), 11_000)).unwrap();
        assert_ne!(alarmed.look.pictures, calm.look.pictures, "the open alarm shows as an alert");
        m.set_alarmed(vec![]);
        let cleared = frame_of(&mut m, &at(update(95.0), 12_000)).unwrap();
        assert_eq!(cleared.look.pictures, calm.look.pictures);
    }

    #[test]
    fn a_load_no_rule_covers_uses_the_settings_level_after_three_seconds() {
        let mut m = TrayItems::new(vec![item("c", Metric::CpuTotal, Glyph::Number)], win_env(32));
        m.set_policy(AlertPolicy {
            covered: vec![],
            fallback_percent: 80.0,
            enabled: true,
        });
        let first = frame_of(&mut m, &at(update(85.0), 0)).unwrap();
        assert_eq!(frame_of(&mut m, &at(update(85.0), 2_999)), None, "held for less than three seconds");
        let later = frame_of(&mut m, &at(update(85.0), 3_000)).unwrap();
        assert_ne!(
            later.look.pictures, first.look.pictures,
            "alert after three seconds at the Settings level"
        );
        // 76 is inside the five-point band below 80: still alerting; 74 clears it.
        assert_eq!(
            frame_of(&mut m, &at(update(76.0), 4_000)).map(|f| f.look.pictures == later.look.pictures),
            Some(false),
            "the number changed"
        );
    }

    #[test]
    fn an_alert_is_named_in_the_tooltip_in_words_with_the_metric() {
        let mut m = TrayItems::new(
            vec![
                item("c", Metric::CpuTotal, Glyph::Number),
                item("m", Metric::MemoryPressure, Glyph::Number),
            ],
            win_env(16),
        );
        m.set_policy(AlertPolicy {
            covered: vec![Metric::CpuTotal],
            fallback_percent: 90.0,
            enabled: true,
        });
        m.set_alarmed(vec![Metric::CpuTotal]);
        let f = m.frames(&update(95.0), true);
        let cpu = &f.iter().find(|(id, _)| id == "c").unwrap().1.tooltip;
        assert!(cpu.contains("CPU") && cpu.contains("alert"), "{cpu}");
        let mem = &f.iter().find(|(id, _)| id == "m").unwrap().1.tooltip;
        assert!(!mem.contains("alert"), "{mem}");
    }

    #[test]
    fn the_pictures_follow_the_taskbar_theme() {
        let mut m = TrayItems::new(vec![item("c", Metric::CpuTotal, Glyph::Number)], win_env(32));
        let dark = m.frames(&update(42.0), true);
        let light = m.frames(&update(42.0), false);
        assert_eq!(light.len(), 1, "a theme change redraws the item");
        assert_ne!(dark[0].1.look.pictures, light[0].1.look.pictures);
    }

    /// A text port that counts the masks it is asked for.
    struct Counting(std::sync::atomic::AtomicUsize);

    impl TextRaster for Counting {
        fn advance(&self, t: &str, s: &dyno_core::tray::text::TextStyle) -> f32 {
            dyno_core::tray::text::BoxText.advance(t, s)
        }
        fn cap_height(&self, s: &dyno_core::tray::text::TextStyle) -> f32 {
            dyno_core::tray::text::BoxText.cap_height(s)
        }
        fn mask(&self, t: &str, s: &dyno_core::tray::text::TextStyle, scale: f32) -> dyno_core::tray::text::TextMask {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            dyno_core::tray::text::BoxText.mask(t, s, scale)
        }
    }

    #[test]
    fn an_item_whose_inputs_did_not_change_is_not_rendered_again() {
        let counting = std::sync::Arc::new(Counting(Default::default()));
        let env = Env {
            platform: Platform::Mac,
            px: 18,
            text: counting.clone(),
        };
        let specs: Vec<ItemSpec> = (0..8).map(|i| item(&format!("n{i}"), Metric::CpuTotal, Glyph::Number)).collect();
        let mut m = TrayItems::new(specs, env);
        for _ in 0..3 {
            m.frames(&update(40.0), true);
        }
        let masks = counting.0.load(std::sync::atomic::Ordering::SeqCst);
        assert!(masks > 0, "the first draws rendered text");
        assert!(m.frames(&update(40.0), true).is_empty());
        assert_eq!(
            counting.0.load(std::sync::atomic::Ordering::SeqCst),
            masks,
            "no text was rasterised for unchanged items"
        );
        assert_eq!(m.frames(&update(41.0), true).len(), 8, "a changed value draws again");
        assert!(counting.0.load(std::sync::atomic::Ordering::SeqCst) > masks);
    }

    // On a real machine the per-core loads and the network rates change every tick; a Number, Ring or Meter item does not draw
    // them, so they must not be in its memo key (they were: every such item rendered again each second).
    #[test]
    fn cores_and_network_rates_do_not_redraw_items_that_do_not_show_them() {
        let counting = std::sync::Arc::new(Counting(Default::default()));
        let env = Env {
            platform: Platform::Mac,
            px: 18,
            text: counting.clone(),
        };
        let specs = vec![
            item("n", Metric::CpuTotal, Glyph::Number),
            item("r", Metric::Memory, Glyph::Ring),
            item("m", Metric::Swap, Glyph::Meter),
        ];
        let mut m = TrayItems::new(specs, env);
        let tick = |k: f32| {
            let mut u = update(40.0);
            u.system.cpu.per_core = vec![k, 100.0 - k, k / 2.0, 3.0];
            u.system.network[0].rx_bps = f64::from(k) * 1_000.0;
            u.system.network[0].tx_bps = f64::from(k) * 500.0;
            u
        };
        m.frames(&tick(10.0), true);
        m.frames(&tick(10.0), true);
        let masks = counting.0.load(std::sync::atomic::Ordering::SeqCst);
        for k in [20.0, 30.0, 70.0] {
            assert!(m.frames(&tick(k), true).is_empty(), "nothing these items show changed");
        }
        assert_eq!(
            counting.0.load(std::sync::atomic::Ordering::SeqCst),
            masks,
            "no item rendered again"
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
        let mut m = TrayItems::new(specs, win_env(32));
        let first = m.frames(&update(40.0), true);
        assert_eq!(first.len(), all.len());
        assert!(first
            .iter()
            .all(|(_, f)| f.look.width == 32 && f.look.height == 32 && f.look.pictures.len() == 1 && f.tooltip.starts_with("CPU")));
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
        let mut m = TrayItems::new(vec![item("c", Metric::CpuPerCore, Glyph::CoreBars)], win_env(32));
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
            win_env(32),
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
            win_env(32),
        );
        let f = m.frames(&u, true);
        assert_eq!(f[0].1.tooltip, "CPU 100%");
        assert_eq!(f[1].1.tooltip, "Download 5.0 MB/s");
        assert!(f[2].1.tooltip.contains("Upload 1.0 MB/s"));
        assert!(f[3].1.tooltip.contains("Normal"));
        assert!(f[4].1.tooltip.contains("Swap 0%"), "{}", f[4].1.tooltip);
    }

    #[test]
    fn the_combined_network_item_says_both_rates_and_follows_either() {
        let mut u = update(250.0);
        u.system.network[0].rx_bps = 5_000_000.0;
        u.system.network[0].tx_bps = 1_000_000.0;
        let mut m = TrayItems::new(vec![item("n", Metric::NetworkBoth, Glyph::Number)], win_env(32));
        let first = m.frames(&u, true);
        assert!(
            first[0].1.tooltip.contains("Network 1.0 MB/s up, 5.0 MB/s down"),
            "{}",
            first[0].1.tooltip
        );
        u.system.network[0].tx_bps = 3_000_000.0;
        assert_eq!(m.frames(&u, true).len(), 1, "an upload change redraws the item");
        u.system.network[0].rx_bps = 2_000_000.0;
        assert_eq!(m.frames(&u, true).len(), 1, "a download change redraws the item");
    }

    #[test]
    fn the_legacy_network_setting_that_showed_both_maps_to_the_combined_metric() {
        let items = parse_items("network:number");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].metric, Metric::NetworkBoth);
    }

    // Windows keeps 127 UTF-16 units of a tip (128 with the terminating zero); a longer name must be cut by us, visibly, not
    // by the shell. Failing first: the tooltip was the raw text, 200+ units for a long process name.
    #[test]
    fn a_tooltip_fits_the_windows_limit_and_still_names_the_metric() {
        let long = "x".repeat(300);
        let mut m = TrayItems::new(vec![item("p", Metric::Process(long), Glyph::Number)], win_env(32));
        let tip = m.frames(&update(10.0), true)[0].1.tooltip.clone();
        assert!(
            tip.encode_utf16().count() <= TOOLTIP_MAX_UNITS,
            "{} units",
            tip.encode_utf16().count()
        );
        assert!(tip.starts_with("xxx") && tip.ends_with('…'), "{tip}");
    }

    #[test]
    fn the_tooltip_builder_names_the_metric_its_value_and_the_alarm() {
        assert_eq!(item_tooltip("CPU", &TipState::Value("34%".into())), "CPU 34%");
        assert_eq!(
            item_tooltip("Disk C:", &TipState::Alert("81%".into())),
            "Disk C: 81% (alert: above its limit)"
        );
        assert_eq!(item_tooltip("GPU", &TipState::Unavailable), "GPU is not available on this computer");
        assert_eq!(item_tooltip("CPU", &TipState::AgentAway), "CPU: the agent is not running");
        assert_eq!(
            item_tooltip(
                "CPU",
                &TipState::Cores {
                    total: 34.4,
                    busiest: 95.2
                }
            ),
            "CPU 34% · busiest core 95%"
        );
    }

    // The users own label is the name in every state (the icon is often too small to draw it, so the tooltip is where it shows).
    #[test]
    fn the_users_label_is_the_name_in_every_tooltip_state() {
        let mut spec = item("t", Metric::GpuLoad, Glyph::Number);
        spec.label = "Hot".into();
        let mut m = TrayItems::new(vec![spec.clone()], win_env(32));
        assert!(m.unavailable_frames(true)[0].1.tooltip.starts_with("Hot"), "agent away");
        let mut u = update(40.0);
        u.system.gpu = Reading::Unavailable("x".into());
        assert!(m.frames(&u, true)[0].1.tooltip.starts_with("Hot is not available"));
        let mut m = TrayItems::new(
            vec![ItemSpec {
                metric: Metric::CpuTotal,
                ..spec
            }],
            win_env(32),
        );
        m.set_policy(AlertPolicy {
            covered: vec![],
            fallback_percent: 50.0,
            enabled: true,
        });
        let tips: Vec<String> = (0..5)
            .map(|i| {
                m.frames(&at(update(95.0), i * 1000), true)
                    .pop()
                    .map(|(_, f)| f.tooltip)
                    .unwrap_or_default()
            })
            .collect();
        assert!(tips.iter().any(|t| t.starts_with("Hot ") && t.contains("alert")), "{tips:?}");
    }

    #[test]
    fn a_number_uses_its_label_and_unit() {
        let mut spec = item("t", Metric::CpuTotal, Glyph::Number);
        spec.label = "Proc".into();
        spec.unit = "pct".into();
        let mut m = TrayItems::new(vec![spec], win_env(32));
        assert_eq!(m.frames(&update(42.0), true)[0].1.tooltip, "Proc 42pct");
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
        let mut m = TrayItems::new(vec![item("p", Metric::Process("chrome".into()), Glyph::Number)], win_env(32));
        assert!(m.frames(&u, true)[0].1.tooltip.contains("chrome 33%"));
    }

    #[test]
    fn an_item_that_shows_one_app_needs_the_top_lists() {
        assert!(!shows_apps(&[item("c", Metric::CpuTotal, Glyph::Number)]));
        assert!(shows_apps(&[
            item("c", Metric::CpuTotal, Glyph::Number),
            item("p", Metric::Process("chrome".into()), Glyph::Number)
        ]));
    }

    #[test]
    fn changing_the_items_resets_them() {
        let mut m = TrayItems::new(vec![item("a", Metric::CpuTotal, Glyph::Ring)], win_env(32));
        m.frames(&update(10.0), true);
        assert!(!m.set_specs(m.specs().to_vec()), "same items: no change");
        assert!(m.set_specs(vec![item("b", Metric::CpuTotal, Glyph::Ring)]));
        assert_eq!(m.frames(&update(10.0), true)[0].0, "b");
    }
}
