//! The data-driven widget model: the dashboard layout (what the Overview shows) and the tray items (what the menu bar
//! / system tray shows). Both are versioned, parsed once at the edge (`parse`) and normalised so a damaged, oversized
//! or newer file can never leave the UI with something it cannot draw. Pure: no I/O.

use serde::{Deserialize, Serialize};

/// The schema version of [`DashboardLayout`] and [`TrayItems`].
pub const SCHEMA_VERSION: u32 = 1;
/// The largest layout or tray file read.
pub const MAX_BYTES: usize = 128 * 1024;
/// Most widgets on the dashboard.
pub const MAX_WIDGETS: usize = 32;
/// Most widgets in a saved preset.
pub const MAX_PRESET_WIDGETS: usize = 24;
/// Most user presets.
pub const MAX_CUSTOM_PRESETS: usize = 8;
/// Most tray items.
pub const MAX_TRAY_ITEMS: usize = 4;
/// Longest id, name and metric argument, in characters.
pub const MAX_TEXT: usize = 64;
/// Longest tray number unit, in characters.
pub const MAX_UNIT: usize = 8;
/// Longest tray number label, in characters.
pub const MAX_LABEL: usize = 16;

/// Why a widget, layout or tray item is not valid (the message says what to change).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WidgetError {
    /// An id is empty or too long.
    #[error("Give every widget an id of 1 to {MAX_TEXT} characters.")]
    BadId,
    /// Two entries share an id.
    #[error("Two items share the id \"{0}\"; ids must be unique.")]
    DuplicateId(String),
    /// A widget kind cannot show a metric.
    #[error("A {kind} widget cannot show {metric}; choose {hint}.")]
    Incompatible {
        /// The widget kind.
        kind: &'static str,
        /// The metric.
        metric: String,
        /// What would work.
        hint: &'static str,
    },
    /// A metric's argument (process name, counter path) is empty or too long.
    #[error("A process name or counter path must be 1 to {MAX_TEXT} characters.")]
    BadMetricArgument,
    /// Thresholds are not in order.
    #[error("The warning threshold must be below the critical threshold.")]
    ThresholdOrder,
    /// A threshold is not a number.
    #[error("Thresholds must be numbers.")]
    ThresholdNotFinite,
    /// The accent colour is not `#rrggbb`.
    #[error("The accent colour must look like #1a2b3c.")]
    BadAccent,
    /// Too many entries.
    #[error("At most {max} {what} fit; remove some.")]
    TooMany {
        /// What there are too many of.
        what: &'static str,
        /// The limit.
        max: usize,
    },
}

/// What a widget draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WidgetKind {
    /// A history line graph.
    Graph,
    /// One large number.
    BigNumber,
    /// Horizontal bars (one per core for per-core CPU).
    Bars,
    /// A ring gauge.
    Ring,
    /// A heat map over time (per-core load).
    Heatmap,
    /// The busiest apps.
    TopProcesses,
    /// Current problems and alarms.
    Issues,
    /// The health score.
    Health,
    /// The CPU card: load, history, the per-core view and a link to the CPU details.
    Cpu,
    /// The plain-language reading of what is slowing the PC down (the "Why slow" summary).
    Diagnosis,
    /// What happened lately: alarms, rule actions and Dyno's own changes.
    Events,
    /// The history chart with events marked.
    History,
    /// The biggest consumers over the last day.
    Consumers,
    /// How fast the disks are filling and when they will be full.
    DiskTrend,
}

impl WidgetKind {
    fn name(self) -> &'static str {
        match self {
            WidgetKind::Ring => "ring",
            WidgetKind::Heatmap => "heat map",
            WidgetKind::TopProcesses => "top processes",
            // The other kinds show any metric, so they never appear in an error.
            _ => "widget",
        }
    }

    /// Whether the kind can show `metric`.
    pub fn accepts(self, metric: &Metric) -> bool {
        match self {
            WidgetKind::Heatmap => *metric == Metric::CpuPerCore,
            WidgetKind::Ring => metric.is_percent(),
            WidgetKind::TopProcesses => matches!(
                metric,
                Metric::CpuTotal | Metric::Memory | Metric::DiskActivity | Metric::NetworkUp | Metric::NetworkDown | Metric::GpuLoad
            ),
            WidgetKind::Graph
            | WidgetKind::BigNumber
            | WidgetKind::Bars
            | WidgetKind::Issues
            | WidgetKind::Health
            | WidgetKind::Cpu
            | WidgetKind::Diagnosis
            | WidgetKind::Events
            | WidgetKind::History
            | WidgetKind::Consumers
            | WidgetKind::DiskTrend => true,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            WidgetKind::Heatmap => "per-core CPU",
            WidgetKind::Ring => "a percentage such as CPU, memory, disk free or battery",
            _ => "CPU, memory, disk, network or GPU",
        }
    }
}

/// What a widget measures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    /// Total CPU load.
    CpuTotal,
    /// Load per core.
    CpuPerCore,
    /// Memory in use.
    Memory,
    /// Memory pressure.
    MemoryPressure,
    /// Swap / page file in use.
    Swap,
    /// Disk activity.
    DiskActivity,
    /// Free space on the system disk.
    DiskFree,
    /// Network upload.
    NetworkUp,
    /// Network download.
    NetworkDown,
    /// GPU load.
    GpuLoad,
    /// The hottest temperature sensor.
    Temperature,
    /// Fan speed.
    Fan,
    /// Battery charge.
    Battery,
    /// One app's CPU, by process name.
    Process(String),
    /// A Windows performance counter, by path.
    PerfCounter(String),
}

impl Metric {
    /// Whether the value is a 0-100 percentage (what a ring or a meter can show).
    pub fn is_percent(&self) -> bool {
        matches!(
            self,
            Metric::CpuTotal
                | Metric::Memory
                | Metric::MemoryPressure
                | Metric::Swap
                | Metric::DiskActivity
                | Metric::DiskFree
                | Metric::GpuLoad
                | Metric::Battery
                | Metric::Process(_)
        )
    }

    fn label(&self) -> String {
        match self {
            Metric::Process(n) => format!("process {n}"),
            Metric::PerfCounter(p) => format!("counter {p}"),
            other => format!("{other:?}"),
        }
    }

    fn validate(&self) -> Result<(), WidgetError> {
        match self {
            Metric::Process(s) | Metric::PerfCounter(s) if s.trim().is_empty() || s.chars().count() > MAX_TEXT => {
                Err(WidgetError::BadMetricArgument)
            }
            _ => Ok(()),
        }
    }
}

/// How big a widget is on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WidgetSize {
    /// Small.
    S,
    /// Medium.
    #[default]
    M,
    /// Large.
    L,
}

/// When a value turns amber and red.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Thresholds {
    /// Warning level, in the metric's unit.
    pub warning: Option<f32>,
    /// Critical level.
    pub critical: Option<f32>,
}

impl Thresholds {
    fn validate(&self) -> Result<(), WidgetError> {
        if [self.warning, self.critical].iter().flatten().any(|v| !v.is_finite()) {
            return Err(WidgetError::ThresholdNotFinite);
        }
        match (self.warning, self.critical) {
            (Some(w), Some(c)) if w >= c => Err(WidgetError::ThresholdOrder),
            _ => Ok(()),
        }
    }
}

/// Per-widget display options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WidgetOptions {
    /// How much history a graph or heat map shows, seconds (30-3600).
    pub range_secs: u32,
    /// Draw the metric's name.
    pub show_label: bool,
    /// Fill under graph lines.
    pub fill: bool,
    /// An accent colour (`#rrggbb`), or the theme's default.
    pub accent: Option<String>,
}

impl Default for WidgetOptions {
    fn default() -> Self {
        WidgetOptions {
            range_secs: 300,
            show_label: true,
            fill: true,
            accent: None,
        }
    }
}

impl WidgetOptions {
    fn validate(&self) -> Result<(), WidgetError> {
        match &self.accent {
            Some(a) if !is_hex_colour(a) => Err(WidgetError::BadAccent),
            _ => Ok(()),
        }
    }
}

fn is_hex_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// One dashboard widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    /// Unique id within the layout.
    pub id: String,
    /// What it draws.
    pub kind: WidgetKind,
    /// What it measures.
    pub metric: Metric,
    /// Display options.
    #[serde(default)]
    pub options: WidgetOptions,
    /// Size on the grid.
    #[serde(default)]
    pub size: WidgetSize,
    /// Colour thresholds.
    #[serde(default)]
    pub thresholds: Thresholds,
}

fn id_ok(id: &str) -> bool {
    !id.trim().is_empty() && id.chars().count() <= MAX_TEXT
}

impl Widget {
    /// A widget with default options and thresholds.
    pub fn new(id: &str, kind: WidgetKind, metric: Metric, size: WidgetSize) -> Widget {
        Widget {
            id: id.into(),
            kind,
            metric,
            options: WidgetOptions::default(),
            size,
            thresholds: Thresholds::default(),
        }
    }

    /// Checks the widget, with a message that says what to change.
    pub fn validate(&self) -> Result<(), WidgetError> {
        if !id_ok(&self.id) {
            return Err(WidgetError::BadId);
        }
        self.metric.validate()?;
        if !self.kind.accepts(&self.metric) {
            return Err(WidgetError::Incompatible {
                kind: self.kind.name(),
                metric: self.metric.label(),
                hint: self.kind.hint(),
            });
        }
        self.thresholds.validate()?;
        self.options.validate()
    }

    fn clamped(mut self) -> Widget {
        self.options.range_secs = self.options.range_secs.clamp(30, 3_600);
        self
    }
}

/// A named set of widgets the user can switch to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedPreset {
    /// Unique id ("overview", or the user's).
    pub id: String,
    /// Name to show.
    pub name: String,
    /// Dyno's own preset: regenerated on every load, cannot be edited or removed.
    pub builtin: bool,
    /// Its widgets.
    pub widgets: Vec<Widget>,
}

/// Dyno's built-in presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    /// The everyday view.
    Overview,
    /// CPU, memory, disk and the busiest processes.
    Developer,
    /// GPU, temperature and fans.
    Gaming,
    /// Battery, power-hungry apps and heat.
    Battery,
}

impl Preset {
    /// All built-in presets, in display order.
    pub const ALL: [Preset; 4] = [Preset::Overview, Preset::Developer, Preset::Gaming, Preset::Battery];

    /// The id used in [`NamedPreset::id`].
    pub fn id(self) -> &'static str {
        match self {
            Preset::Overview => "overview",
            Preset::Developer => "developer",
            Preset::Gaming => "gaming",
            Preset::Battery => "battery",
        }
    }

    /// The widgets of the preset.
    pub fn widgets(self) -> Vec<Widget> {
        use Metric as M;
        use WidgetKind as K;
        use WidgetSize::{L, M as Md, S};
        let w = |id: &str, k, m, s| Widget::new(id, k, m, s);
        let hot = |mut x: Widget, warn: f32, crit: f32| {
            x.thresholds = Thresholds {
                warning: Some(warn),
                critical: Some(crit),
            };
            x
        };
        match self {
            // What the fixed dashboard used to show, as widgets: the verdict first, then the live tiles, the busiest apps,
            // what happened and the history.
            Preset::Overview => vec![
                w("health", K::Health, M::CpuTotal, S),
                w("diagnosis", K::Diagnosis, M::CpuTotal, Md),
                w("issues", K::Issues, M::CpuTotal, S),
                w("cpu", K::Cpu, M::CpuTotal, Md),
                w("memory", K::Graph, M::Memory, S),
                w("disk", K::Graph, M::DiskActivity, S),
                w("network", K::Graph, M::NetworkDown, S),
                w("gpu", K::Graph, M::GpuLoad, S),
                w("battery", K::Ring, M::Battery, S),
                hot(w("temp", K::BigNumber, M::Temperature, S), 80.0, 92.0),
                w("top-cpu", K::TopProcesses, M::CpuTotal, Md),
                w("top-memory", K::TopProcesses, M::Memory, Md),
                w("events", K::Events, M::CpuTotal, Md),
                w("consumers", K::Consumers, M::CpuTotal, Md),
                w("history", K::History, M::CpuTotal, L),
                w("disk-trend", K::DiskTrend, M::DiskFree, L),
            ],
            Preset::Developer => vec![
                w("cpu-cores", K::Heatmap, M::CpuPerCore, L),
                w("cpu", K::Graph, M::CpuTotal, Md),
                w("memory", K::Graph, M::Memory, Md),
                w("swap", K::BigNumber, M::Swap, S),
                w("disk", K::Graph, M::DiskActivity, Md),
                w("disk-free", K::Ring, M::DiskFree, S),
                w("top", K::TopProcesses, M::CpuTotal, L),
            ],
            Preset::Gaming => vec![
                w("gpu", K::Graph, M::GpuLoad, L),
                w("cpu", K::Graph, M::CpuTotal, Md),
                hot(w("temp", K::BigNumber, M::Temperature, S), 80.0, 92.0),
                w("fan", K::BigNumber, M::Fan, S),
                w("memory", K::Ring, M::Memory, S),
                w("network", K::Graph, M::NetworkDown, Md),
                w("top", K::TopProcesses, M::CpuTotal, Md),
            ],
            Preset::Battery => vec![
                w("battery", K::Ring, M::Battery, Md),
                w("cpu", K::Graph, M::CpuTotal, Md),
                hot(w("temp", K::BigNumber, M::Temperature, S), 80.0, 92.0),
                w("top", K::TopProcesses, M::CpuTotal, L),
                w("issues", K::Issues, M::CpuTotal, Md),
            ],
        }
    }

    /// The preset as a [`NamedPreset`].
    pub fn named(self) -> NamedPreset {
        let name = match self {
            Preset::Overview => "Overview",
            Preset::Developer => "Developer",
            Preset::Gaming => "Gaming",
            Preset::Battery => "Battery",
        };
        NamedPreset {
            id: self.id().into(),
            name: name.into(),
            builtin: true,
            widgets: self.widgets(),
        }
    }
}

/// The dashboard: the widgets shown now and the presets to switch between.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DashboardLayout {
    /// Schema version.
    pub version: u32,
    /// The widgets, in display order.
    pub widgets: Vec<Widget>,
    /// Built-in presets first, then the user's own.
    pub presets: Vec<NamedPreset>,
}

impl Default for DashboardLayout {
    fn default() -> Self {
        DashboardLayout::from_preset(Preset::Overview)
    }
}

/// Validates a list of widgets (ids unique, each valid).
fn validate_widgets(widgets: &[Widget], max: usize) -> Result<(), WidgetError> {
    if widgets.len() > max {
        return Err(WidgetError::TooMany { what: "widgets", max });
    }
    let mut seen: Vec<&str> = Vec::new();
    for w in widgets {
        w.validate()?;
        if seen.contains(&w.id.as_str()) {
            return Err(WidgetError::DuplicateId(w.id.clone()));
        }
        seen.push(&w.id);
    }
    Ok(())
}

/// Keeps the valid, unique widgets (up to `max`), counting what was dropped.
fn keep_valid(widgets: Vec<Widget>, max: usize, dropped: &mut usize) -> Vec<Widget> {
    let mut out: Vec<Widget> = Vec::new();
    for w in widgets {
        if out.len() >= max || w.validate().is_err() || out.iter().any(|o| o.id == w.id) {
            *dropped += 1;
        } else {
            out.push(w.clamped());
        }
    }
    out
}

impl DashboardLayout {
    /// A layout showing `preset`, with the built-in presets available.
    pub fn from_preset(preset: Preset) -> DashboardLayout {
        DashboardLayout {
            version: SCHEMA_VERSION,
            widgets: preset.widgets(),
            presets: Preset::ALL.iter().map(|p| p.named()).collect(),
        }
    }

    /// Checks the layout as the user edited it (what the engine runs before saving).
    pub fn validate(&self) -> Result<(), WidgetError> {
        validate_widgets(&self.widgets, MAX_WIDGETS)?;
        let custom: Vec<&NamedPreset> = self.presets.iter().filter(|p| !p.builtin).collect();
        if custom.len() > MAX_CUSTOM_PRESETS {
            return Err(WidgetError::TooMany {
                what: "custom presets",
                max: MAX_CUSTOM_PRESETS,
            });
        }
        let mut ids: Vec<&str> = Vec::new();
        for p in &custom {
            if !id_ok(&p.id) {
                return Err(WidgetError::BadId);
            }
            if ids.contains(&p.id.as_str()) || Preset::ALL.iter().any(|b| b.id() == p.id) {
                return Err(WidgetError::DuplicateId(p.id.clone()));
            }
            ids.push(&p.id);
            validate_widgets(&p.widgets, MAX_PRESET_WIDGETS)?;
        }
        Ok(())
    }

    /// The layout with invalid or surplus parts dropped and the built-in presets regenerated. Returns how many
    /// widgets and presets were dropped.
    pub fn normalised_counting(self) -> (DashboardLayout, usize) {
        let mut dropped = 0;
        let widgets = keep_valid(self.widgets, MAX_WIDGETS, &mut dropped);
        let mut presets: Vec<NamedPreset> = Preset::ALL.iter().map(|p| p.named()).collect();
        let mut custom = 0;
        for mut p in self.presets.into_iter().filter(|p| !p.builtin) {
            let clash = presets.iter().any(|o| o.id == p.id);
            if !id_ok(&p.id) || clash || custom >= MAX_CUSTOM_PRESETS {
                dropped += 1;
                continue;
            }
            p.name = p.name.chars().take(MAX_TEXT).collect();
            p.widgets = keep_valid(p.widgets, MAX_PRESET_WIDGETS, &mut dropped);
            custom += 1;
            presets.push(p);
        }
        let widgets = if widgets.is_empty() { Preset::Overview.widgets() } else { widgets };
        (
            DashboardLayout {
                version: SCHEMA_VERSION,
                widgets,
                presets,
            },
            dropped,
        )
    }

    /// The layout with invalid or surplus parts dropped.
    pub fn normalised(self) -> DashboardLayout {
        self.normalised_counting().0
    }

    /// Replaces the widgets with the preset `id`'s.
    pub fn apply_preset(&mut self, id: &str) -> bool {
        match self.presets.iter().find(|p| p.id == id) {
            Some(p) => {
                self.widgets = p.widgets.clone();
                true
            }
            None => false,
        }
    }
}

/// Reads a dashboard layout file: damaged, oversized or newer files fall back to the default with a warning.
pub fn parse(text: &str) -> (DashboardLayout, Option<String>) {
    if text.len() > MAX_BYTES {
        return (
            DashboardLayout::default(),
            Some("The dashboard layout is too large; using the default.".into()),
        );
    }
    match serde_json::from_str::<DashboardLayout>(text) {
        Ok(l) if l.version > SCHEMA_VERSION => (
            DashboardLayout::default(),
            Some(format!(
                "The dashboard was saved by a newer Dyno (version {}); using the default.",
                l.version
            )),
        ),
        Ok(l) => {
            let (n, dropped) = l.normalised_counting();
            let warning = (dropped > 0).then(|| format!("{dropped} dashboard item(s) were not valid and were removed."));
            (n, warning)
        }
        Err(e) => (
            DashboardLayout::default(),
            Some(format!("The dashboard layout is damaged ({e}); using the default.")),
        ),
    }
}

/// How a tray item draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TrayItemStyle {
    /// A small history line.
    LineGraph,
    /// Vertical bars.
    VerticalBars,
    /// A horizontal meter.
    HorizontalMeter,
    /// A ring.
    Ring,
    /// The value as text.
    Number {
        /// The unit after the value ("%", "°C").
        unit: String,
        /// A short label before it ("CPU").
        label: String,
    },
    /// A line with the number beside it.
    GraphAndNumber,
}

/// What clicking a tray item opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TrayOpens {
    /// A popover about this metric.
    #[default]
    OwnPopover,
    /// The main popover.
    MainPopover,
}

/// One tray item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayItem {
    /// Unique id.
    pub id: String,
    /// What it shows.
    pub metric: Metric,
    /// How it draws.
    pub style: TrayItemStyle,
    /// What a click opens.
    pub opens: TrayOpens,
}

fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl TrayItem {
    fn validate(&self) -> Result<(), WidgetError> {
        if !id_ok(&self.id) {
            return Err(WidgetError::BadId);
        }
        self.metric.validate()
    }

    fn clamped(mut self) -> TrayItem {
        if let TrayItemStyle::Number { unit, label } = &mut self.style {
            *unit = clean_text(unit, MAX_UNIT);
            *label = clean_text(label, MAX_LABEL);
        }
        self
    }
}

/// The tray items, in order (the user can reorder them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrayItems {
    /// Schema version.
    pub version: u32,
    /// The items, left to right.
    pub items: Vec<TrayItem>,
}

impl Default for TrayItems {
    fn default() -> Self {
        TrayItems {
            version: SCHEMA_VERSION,
            items: vec![
                TrayItem {
                    id: "cpu".into(),
                    metric: Metric::CpuTotal,
                    style: TrayItemStyle::VerticalBars,
                    opens: TrayOpens::OwnPopover,
                },
                TrayItem {
                    id: "memory".into(),
                    metric: Metric::Memory,
                    style: TrayItemStyle::VerticalBars,
                    opens: TrayOpens::OwnPopover,
                },
            ],
        }
    }
}

impl TrayItems {
    /// Checks the items as the user edited them.
    pub fn validate(&self) -> Result<(), WidgetError> {
        if self.items.len() > MAX_TRAY_ITEMS {
            return Err(WidgetError::TooMany {
                what: "tray items",
                max: MAX_TRAY_ITEMS,
            });
        }
        let mut seen: Vec<&str> = Vec::new();
        for i in &self.items {
            i.validate()?;
            if seen.contains(&i.id.as_str()) {
                return Err(WidgetError::DuplicateId(i.id.clone()));
            }
            seen.push(&i.id);
        }
        Ok(())
    }

    /// Valid, unique items up to the maximum, counting what was dropped; the default when none remain.
    pub fn normalised_counting(self) -> (TrayItems, usize) {
        let mut dropped = 0;
        let mut items: Vec<TrayItem> = Vec::new();
        for i in self.items {
            if items.len() >= MAX_TRAY_ITEMS || i.validate().is_err() || items.iter().any(|o| o.id == i.id) {
                dropped += 1;
            } else {
                items.push(i.clamped());
            }
        }
        if items.is_empty() {
            return (TrayItems::default(), dropped);
        }
        (
            TrayItems {
                version: SCHEMA_VERSION,
                items,
            },
            dropped,
        )
    }

    /// Valid, unique items up to the maximum.
    pub fn normalised(self) -> TrayItems {
        self.normalised_counting().0
    }

    /// Moves the item at `from` to position `to` (both clamped into range); false when `from` is out of range.
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() {
            return false;
        }
        let item = self.items.remove(from);
        let to = to.min(self.items.len());
        self.items.insert(to, item);
        true
    }
}

/// Reads a tray items file, like [`parse`] for the dashboard.
pub fn parse_tray(text: &str) -> (TrayItems, Option<String>) {
    if text.len() > MAX_BYTES {
        return (
            TrayItems::default(),
            Some("The tray items file is too large; using the default.".into()),
        );
    }
    match serde_json::from_str::<TrayItems>(text) {
        Ok(t) if t.version > SCHEMA_VERSION => (
            TrayItems::default(),
            Some(format!(
                "The tray items were saved by a newer Dyno (version {}); using the default.",
                t.version
            )),
        ),
        Ok(t) => {
            let (n, dropped) = t.normalised_counting();
            (
                n,
                (dropped > 0).then(|| format!("{dropped} tray item(s) were not valid and were removed.")),
            )
        }
        Err(e) => (
            TrayItems::default(),
            Some(format!("The tray items are damaged ({e}); using the default.")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(id: &str) -> Widget {
        Widget::new(id, WidgetKind::Graph, Metric::CpuTotal, WidgetSize::M)
    }

    #[test]
    fn builtin_presets_are_valid() {
        for p in Preset::ALL {
            validate_widgets(&p.widgets(), MAX_PRESET_WIDGETS).unwrap_or_else(|e| panic!("{p:?}: {e}"));
            assert!(p.named().builtin);
        }
        let ids: Vec<&str> = Preset::ALL.iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["overview", "developer", "gaming", "battery"]);
        let d = DashboardLayout::default();
        d.validate().unwrap();
        assert_eq!(d.presets.len(), 4);
        assert_eq!(d.widgets, Preset::Overview.widgets());
        assert_eq!(TrayItems::default().validate(), Ok(()));
    }

    #[test]
    fn the_overview_recreates_the_old_fixed_dashboard() {
        let ids: Vec<String> = Preset::Overview.widgets().into_iter().map(|w| w.id).collect();
        for needed in ["health", "diagnosis", "issues", "cpu", "memory", "disk", "network", "gpu", "battery", "top-cpu", "top-memory", "events", "history"] {
            assert!(ids.iter().any(|i| i == needed), "the overview lacks {needed}");
        }
        for kind in ["cpu", "diagnosis", "events", "history", "consumers", "diskTrend"] {
            let k: WidgetKind = serde_json::from_value(serde_json::json!(kind)).unwrap();
            assert!(k.accepts(&Metric::CpuTotal));
        }
    }

    #[test]
    fn widgets_are_validated_with_clear_messages() {
        assert_eq!(widget("").validate(), Err(WidgetError::BadId));
        assert_eq!(widget(&"x".repeat(65)).validate(), Err(WidgetError::BadId));
        let mut w = widget("a");
        w.kind = WidgetKind::Heatmap;
        let e = w.validate().unwrap_err();
        assert!(e.to_string().contains("heat map") && e.to_string().contains("per-core CPU"), "{e}");
        w.kind = WidgetKind::Ring;
        w.metric = Metric::NetworkUp;
        assert!(w.validate().unwrap_err().to_string().contains("percentage"));
        w.kind = WidgetKind::TopProcesses;
        w.metric = Metric::Fan;
        assert!(w.validate().is_err());
        w.metric = Metric::Process(" ".into());
        assert_eq!(w.validate(), Err(WidgetError::BadMetricArgument));
        w.metric = Metric::PerfCounter("x".repeat(65));
        assert_eq!(w.validate(), Err(WidgetError::BadMetricArgument));
        let mut t = widget("t");
        t.thresholds = Thresholds {
            warning: Some(90.0),
            critical: Some(80.0),
        };
        assert_eq!(t.validate(), Err(WidgetError::ThresholdOrder));
        t.thresholds.warning = Some(f32::NAN);
        assert_eq!(t.validate(), Err(WidgetError::ThresholdNotFinite));
        t.thresholds = Thresholds::default();
        t.options.accent = Some("red".into());
        assert_eq!(t.validate(), Err(WidgetError::BadAccent));
        t.options.accent = Some("#1A2b3c".into());
        assert_eq!(t.validate(), Ok(()));
        let mut c = widget("c");
        c.kind = WidgetKind::Heatmap;
        c.metric = Metric::PerfCounter("\\Memory\\Pages/sec".into());
        assert!(c.validate().unwrap_err().to_string().contains("counter \\Memory"));
        c.metric = Metric::Fan;
        assert!(c.validate().unwrap_err().to_string().contains("Fan"));
        assert!(WidgetError::DuplicateId("a".into()).to_string().contains("\"a\""));
        assert!(WidgetError::TooMany { what: "widgets", max: 3 }.to_string().contains("3"));
        let ok = Widget::new("p", WidgetKind::Ring, Metric::Process("chrome".into()), WidgetSize::S);
        assert_eq!(ok.validate(), Ok(()));
        assert!(Metric::PerfCounter("\\Processor(_Total)\\% Processor Time".into())
            .validate()
            .is_ok());
    }

    #[test]
    fn layout_validation_and_normalisation() {
        let mut l = DashboardLayout::default();
        l.widgets = vec![widget("a"), widget("a")];
        assert_eq!(l.validate(), Err(WidgetError::DuplicateId("a".into())));
        l.widgets = (0..MAX_WIDGETS + 1).map(|i| widget(&format!("w{i}"))).collect();
        assert!(matches!(l.validate(), Err(WidgetError::TooMany { .. })));
        let (n, dropped) = l.clone().normalised_counting();
        assert_eq!((n.widgets.len(), dropped), (MAX_WIDGETS, 1));
        l.widgets = vec![widget("ok")];
        l.presets.push(NamedPreset {
            id: "mine".into(),
            name: "n".repeat(100),
            builtin: false,
            widgets: vec![widget("a"), widget("a"), widget("")],
        });
        l.validate().unwrap_err();
        let (n, dropped) = l.clone().normalised_counting();
        assert_eq!(dropped, 2);
        let mine = n.presets.last().unwrap();
        assert_eq!(
            (mine.id.as_str(), mine.widgets.len(), mine.name.chars().count()),
            ("mine", 1, MAX_TEXT)
        );
        assert_eq!(n.presets.len(), 5);
        // A custom preset may not shadow a built-in id, and builtins are regenerated even if edited.
        let mut l = DashboardLayout::default();
        l.presets[0].widgets.clear();
        l.presets.push(NamedPreset {
            id: "gaming".into(),
            name: "x".into(),
            builtin: false,
            widgets: vec![],
        });
        assert_eq!(l.validate(), Err(WidgetError::DuplicateId("gaming".into())));
        let (n, dropped) = l.normalised_counting();
        assert_eq!((dropped, n.presets.len()), (1, 4));
        assert!(!n.presets[0].widgets.is_empty());
        // Empty widgets fall back to the Overview.
        let mut e = DashboardLayout::default();
        e.widgets.clear();
        assert_eq!(e.normalised().widgets, Preset::Overview.widgets());
        // Too many custom presets, bad ids, duplicates.
        let mk = |id: &str| NamedPreset {
            id: id.into(),
            name: id.into(),
            builtin: false,
            widgets: vec![],
        };
        let mut many = DashboardLayout::default();
        many.presets.extend((0..MAX_CUSTOM_PRESETS + 2).map(|i| mk(&format!("p{i}"))));
        assert!(matches!(many.validate(), Err(WidgetError::TooMany { .. })));
        assert_eq!(many.clone().normalised_counting().1, 2);
        let mut bad = DashboardLayout::default();
        bad.presets.extend([mk(""), mk("a"), mk("a")]);
        assert_eq!(bad.validate(), Err(WidgetError::BadId));
        bad.presets.remove(4);
        assert_eq!(bad.validate(), Err(WidgetError::DuplicateId("a".into())));
        let mut big = DashboardLayout::default();
        let mut p = mk("big");
        p.widgets = (0..MAX_PRESET_WIDGETS + 1).map(|i| widget(&format!("w{i}"))).collect();
        big.presets.push(p);
        assert!(matches!(big.validate(), Err(WidgetError::TooMany { .. })));
    }

    #[test]
    fn apply_preset_and_range_clamp() {
        let mut l = DashboardLayout::default();
        assert!(l.apply_preset("gaming"));
        assert_eq!(l.widgets, Preset::Gaming.widgets());
        assert!(!l.apply_preset("nope"));
        let mut w = widget("r");
        w.options.range_secs = 1;
        assert_eq!(w.clone().clamped().options.range_secs, 30);
        w.options.range_secs = 999_999;
        assert_eq!(w.clamped().options.range_secs, 3_600);
    }

    #[test]
    fn parse_tolerates_damage() {
        let (l, w) = parse("{not json");
        assert_eq!(l, DashboardLayout::default());
        assert!(w.unwrap().contains("damaged"));
        assert!(parse(r#"{"version":99}"#).1.unwrap().contains("newer"));
        assert!(parse(&" ".repeat(MAX_BYTES + 1)).1.unwrap().contains("too large"));
        let (l, w) = parse("{}");
        assert!(w.is_none());
        assert_eq!(l, DashboardLayout::default());
        let text = r#"{"widgets":[{"id":"a","kind":"ring","metric":"battery"},{"id":"b","kind":"heatmap","metric":"memory"},
            {"id":"c","kind":"graph","metric":{"process":"chrome"},"size":"l","options":{"rangeSecs":5},"futureKey":1}]}"#;
        let (l, w) = parse(text);
        assert!(w.unwrap().contains("1 dashboard item"));
        assert_eq!(l.widgets.len(), 2);
        assert_eq!(l.widgets[1].metric, Metric::Process("chrome".into()));
        assert_eq!(l.widgets[1].size, WidgetSize::L);
        assert_eq!(l.widgets[1].options.range_secs, 30);
        for junk in [
            "",
            "null",
            "[]",
            "\"x\"",
            "{\"widgets\":5}",
            "{\"widgets\":[{}]}",
            "{\"widgets\":[null]}",
        ] {
            let (l, _) = parse(junk);
            assert!(l.validate().is_ok(), "{junk}");
        }
    }

    #[test]
    fn tray_items_normalise_reorder_and_parse() {
        let item = |id: &str| TrayItem {
            id: id.into(),
            metric: Metric::GpuLoad,
            style: TrayItemStyle::Number {
                unit: "%\n%%%%%%%%%%".into(),
                label: "L".repeat(40),
            },
            opens: TrayOpens::MainPopover,
        };
        let mut t = TrayItems {
            version: 1,
            items: vec![item("a"), item("a"), item(""), item("b"), item("c"), item("d"), item("e")],
        };
        assert!(matches!(t.validate(), Err(WidgetError::TooMany { .. })));
        let (n, dropped) = t.clone().normalised_counting();
        assert_eq!((n.items.len(), dropped), (4, 3));
        let TrayItemStyle::Number { unit, label } = &n.items[0].style else {
            unreachable!("the items were built with Number styles")
        };
        assert_eq!((unit.chars().count(), label.chars().count()), (MAX_UNIT, MAX_LABEL));
        assert!(!unit.contains('\n'));
        t.items.truncate(2);
        assert_eq!(t.validate(), Err(WidgetError::DuplicateId("a".into())));
        t.items = vec![item("")];
        assert_eq!(t.validate(), Err(WidgetError::BadId));
        let mut ok = TrayItems::default();
        ok.items.push(item("z"));
        assert!(ok.reorder(2, 0));
        assert_eq!(ok.items[0].id, "z");
        assert!(ok.reorder(0, 99));
        assert_eq!(ok.items[2].id, "z");
        assert!(!ok.reorder(9, 0));
        assert_eq!(TrayItems { version: 1, items: vec![] }.normalised(), TrayItems::default());
        let (_, w) = parse_tray("{bad");
        assert!(w.unwrap().contains("damaged"));
        assert!(parse_tray(r#"{"version":7}"#).1.unwrap().contains("newer"));
        assert!(parse_tray(&" ".repeat(MAX_BYTES + 1)).1.unwrap().contains("too large"));
        assert_eq!(parse_tray("{}"), (TrayItems::default(), None));
        let (t, w) = parse_tray(
            r#"{"items":[{"id":"a","metric":"cpuTotal","style":"lineGraph","opens":"mainPopover"},{"id":"","metric":"fan","style":"ring","opens":"ownPopover"}]}"#,
        );
        assert_eq!((t.items.len(), w.is_some()), (1, true));
        let mut bad = TrayItems::default();
        bad.items[0].metric = Metric::PerfCounter(String::new());
        assert_eq!(bad.validate(), Err(WidgetError::BadMetricArgument));
    }

    #[test]
    fn serde_shapes_and_round_trips() {
        let j = serde_json::to_value(Widget::new("w", WidgetKind::BigNumber, Metric::Process("x".into()), WidgetSize::S)).unwrap();
        assert_eq!(j["kind"], "bigNumber");
        assert_eq!(j["metric"], serde_json::json!({"process": "x"}));
        assert_eq!(j["size"], "s");
        assert_eq!(j["options"]["rangeSecs"], 300);
        let n = serde_json::to_value(TrayItemStyle::Number {
            unit: "%".into(),
            label: "CPU".into(),
        })
        .unwrap();
        assert_eq!(n, serde_json::json!({"number": {"unit": "%", "label": "CPU"}}));
        assert_eq!(serde_json::to_value(TrayItemStyle::GraphAndNumber).unwrap(), "graphAndNumber");
        let d = DashboardLayout::default();
        assert_eq!(
            serde_json::from_str::<DashboardLayout>(&serde_json::to_string(&d).unwrap()).unwrap(),
            d
        );
        let t = TrayItems::default();
        assert_eq!(postcard::from_bytes::<TrayItems>(&postcard::to_stdvec(&t).unwrap()).unwrap(), t);
        assert_eq!(
            postcard::from_bytes::<DashboardLayout>(&postcard::to_stdvec(&d).unwrap()).unwrap(),
            d
        );
        assert!(Metric::Battery.is_percent() && !Metric::Fan.is_percent());
    }
}
