//! The dashboard's catalogue: the Overview's widgets, their metric, options and thresholds, and the four built-in presets.

use super::metric::Metric;
use super::DynoSurface;
use paddock_widgets::{
    assign_spans, Breakpoint, Catalogue, Cells, Kind, KindSpec, Placement, Preset, PresetId, Refresh, Size, SizeRule, Steps, Surface,
    Widget,
};
use serde::{Deserialize, Serialize};

/// When a value turns amber and red.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Thresholds {
    /// Warning level, in the metric's unit.
    pub warning: Option<f32>,
    /// Critical level.
    pub critical: Option<f32>,
}

impl Thresholds {
    fn validate(&self) -> Result<(), String> {
        if [self.warning, self.critical].iter().flatten().any(|v| !v.is_finite()) {
            return Err("Thresholds must be numbers.".into());
        }
        match (self.warning, self.critical) {
            (Some(w), Some(c)) if w >= c => Err("The warning threshold must be below the critical threshold.".into()),
            _ => Ok(()),
        }
    }

    /// `ok` when no threshold is crossed (or there is no value), `warn` at the warning level, `bad` at the critical one.
    /// This is the one place a reading becomes a tone (it replaces the UI's `thresholdTone`).
    pub fn tone(&self, value: Option<f64>) -> ValueTone {
        let Some(v) = value else { return ValueTone::Ok };
        if self.critical.is_some_and(|c| v >= f64::from(c)) {
            ValueTone::Bad
        } else if self.warning.is_some_and(|w| v >= f64::from(w)) {
            ValueTone::Warn
        } else {
            ValueTone::Ok
        }
    }
}

/// How serious a reading is, from its thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ValueTone {
    /// No threshold is crossed.
    Ok,
    /// Past the warning level.
    Warn,
    /// Past the critical level.
    Bad,
}

/// Per-widget display options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
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

fn is_hex_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// What a dashboard widget draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum DashboardKind {
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
    /// The plain-language reading of what is slowing the computer down (the "Why slow" summary).
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

impl DashboardKind {
    fn name(self) -> &'static str {
        match self {
            DashboardKind::Ring => "ring",
            DashboardKind::Heatmap => "heat map",
            DashboardKind::TopProcesses => "top processes",
            // The other kinds show any metric, so they never appear in an error.
            _ => "widget",
        }
    }

    /// Whether the kind can show `metric` (the one place this rule lives; the chooser asks the engine).
    pub fn accepts(self, metric: &Metric) -> bool {
        match self {
            DashboardKind::Heatmap => *metric == Metric::CpuPerCore,
            DashboardKind::Ring => metric.is_percent(),
            DashboardKind::TopProcesses => matches!(
                metric,
                Metric::CpuTotal | Metric::Memory | Metric::DiskActivity | Metric::NetworkUp | Metric::NetworkDown | Metric::GpuLoad
            ),
            _ => true,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            DashboardKind::Heatmap => "per-core CPU",
            DashboardKind::Ring => "a percentage such as CPU, memory, disk free or battery",
            _ => "CPU, memory, disk, network or GPU",
        }
    }
}

/// One dashboard widget: what it draws, what it measures and how it looks. Its place and width are the placement's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DashboardWidget {
    /// What it draws.
    pub kind: DashboardKind,
    /// What it measures.
    pub metric: Metric,
    /// Display options.
    #[serde(default)]
    pub options: WidgetOptions,
    /// Colour thresholds.
    #[serde(default)]
    pub thresholds: Thresholds,
}

impl DashboardWidget {
    /// A widget with default options and thresholds.
    pub fn new(kind: DashboardKind, metric: Metric) -> DashboardWidget {
        DashboardWidget {
            kind,
            metric,
            options: WidgetOptions::default(),
            thresholds: Thresholds::default(),
        }
    }

    fn hot(mut self, warning: f32, critical: f32) -> DashboardWidget {
        self.thresholds = Thresholds {
            warning: Some(warning),
            critical: Some(critical),
        };
        self
    }
}

impl Widget for DashboardWidget {
    type Kind = DashboardKind;

    fn kind(&self) -> DashboardKind {
        self.kind
    }

    fn validate(&self) -> Result<(), String> {
        self.metric.validate()?;
        if !self.kind.accepts(&self.metric) {
            return Err(format!(
                "A {} widget cannot show {}; choose {}.",
                self.kind.name(),
                self.metric.label(),
                self.kind.hint()
            ));
        }
        self.thresholds.validate()?;
        match &self.options.accent {
            Some(a) if !is_hex_colour(a) => Err("The accent colour must look like #1a2b3c.".into()),
            _ => Ok(()),
        }
    }

    fn clamped(mut self) -> DashboardWidget {
        self.options.range_secs = self.options.range_secs.clamp(30, 3_600);
        self
    }
}

/// Columns of the 12-column board: small, medium, large (the widths the 1.0 dashboard used; the defaults are built from them).
pub const SMALL: u8 = 3;
/// Medium (the default width of most widgets).
pub const MEDIUM: u8 = 6;
/// Large.
pub const LARGE: u8 = 12;

/// A kind's sizes in cells: its default (`w` by `h`), the widths and heights it allows (ADR 0044 WP-C1's table).
const fn rule(w: u8, h: u8, widths: (u8, u8), heights: (u8, u8)) -> SizeRule {
    SizeRule {
        widths: Steps::Range {
            min: widths.0,
            max: widths.1,
        },
        heights: Steps::Range {
            min: heights.0,
            max: heights.1,
        },
        default: Size { w, h },
    }
}

const fn spec(
    kind: DashboardKind,
    title: &'static str,
    summary: &'static str,
    icon: &'static str,
    size: SizeRule,
    refresh: Refresh,
) -> KindSpec<DashboardKind, SizeRule> {
    KindSpec {
        kind,
        title,
        summary,
        icon,
        size,
        max_instances: 32,
        refresh,
    }
}

const KINDS: &[KindSpec<DashboardKind, SizeRule>] = &[
    spec(
        DashboardKind::Graph,
        "Graph",
        "A line over the last minutes.",
        "graph",
        rule(3, 4, (2, 12), (4, 8)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::BigNumber,
        "Big number",
        "One number, large.",
        "number",
        rule(3, 3, (2, 4), (3, 4)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::Bars,
        "Bars",
        "A level against its limit.",
        "bars",
        rule(3, 3, (2, 6), (3, 4)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::Ring,
        "Ring",
        "A percentage as a ring.",
        "ring",
        rule(3, 4, (2, 4), (4, 6)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::Heatmap,
        "Per-core heatmap",
        "One cell per core.",
        "heatmap",
        rule(6, 3, (4, 12), (3, 5)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::TopProcesses,
        "Top processes",
        "The apps using the most.",
        "apps",
        rule(6, 6, (3, 12), (5, 12)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::Issues,
        "Issues",
        "Unusual patterns found.",
        "issue",
        rule(3, 6, (3, 6), (4, 12)),
        Refresh::OnChange,
    ),
    spec(
        DashboardKind::Health,
        "Health",
        "The health score.",
        "health",
        rule(6, 9, (3, 12), (5, 14)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::Cpu,
        "CPU",
        "Load, per-core view and details.",
        "cpu",
        rule(6, 6, (4, 12), (5, 10)),
        Refresh::Live,
    ),
    spec(
        DashboardKind::Diagnosis,
        "Why slow",
        "What is slowing the computer, in words.",
        "diagnosis",
        rule(6, 9, (4, 12), (6, 14)),
        Refresh::Every { secs: 10 },
    ),
    spec(
        DashboardKind::Events,
        "Recent events",
        "Alarms and what Dyno did.",
        "events",
        rule(6, 7, (4, 12), (4, 14)),
        Refresh::OnChange,
    ),
    spec(
        DashboardKind::History,
        "History",
        "A chart with events marked.",
        "history",
        rule(12, 8, (6, 12), (5, 14)),
        Refresh::OnChange,
    ),
    spec(
        DashboardKind::Consumers,
        "Biggest consumers",
        "Biggest consumers over a day.",
        "consumers",
        rule(6, 7, (4, 12), (5, 12)),
        Refresh::Every { secs: 60 },
    ),
    spec(
        DashboardKind::DiskTrend,
        "Disk space trend",
        "When the disks will be full.",
        "disk",
        rule(12, 6, (6, 12), (4, 10)),
        Refresh::Every { secs: 60 },
    ),
];

/// The dashboard's catalogue (ADR 0012, ADR 0035).
pub struct DashboardCatalogue;

/// The built-in presets' ids, in display order.
pub const PRESET_IDS: [&str; 4] = ["overview", "developer", "gaming", "battery"];

/// Placements for `(width, widget)` rows as the 1.0 dashboard laid them out: left to right, a new row when the next
/// widget does not fit, each row as tall as its tallest default. The built-in layouts are the 1.0 ones, upgraded.
fn cells(items: impl IntoIterator<Item = (u8, DashboardWidget)>) -> Vec<Placement<DashboardWidget, Cells>> {
    let spans = assign_spans(items);
    let v1 = serde_json::json!({ "version": 1, "placements": serde_json::to_value(&spans).unwrap_or_default() });
    paddock_widgets::spans_to_cells::<DashboardCatalogue>(v1)
        .ok()
        .and_then(|v| v.get("placements").cloned())
        .and_then(|p| serde_json::from_value(p).ok())
        .unwrap_or_default()
}

fn w(kind: DashboardKind, metric: Metric) -> DashboardWidget {
    DashboardWidget::new(kind, metric)
}

fn overview() -> Vec<Placement<DashboardWidget, Cells>> {
    use DashboardKind as K;
    use Metric as M;
    // What the fixed dashboard used to show, as widgets: the verdict first, then the live tiles, the busiest apps,
    // what happened and the history.
    cells([
        (MEDIUM, w(K::Health, M::CpuTotal)),
        (MEDIUM, w(K::Diagnosis, M::CpuTotal)),
        (SMALL, w(K::Issues, M::CpuTotal)),
        (MEDIUM, w(K::Cpu, M::CpuTotal)),
        (SMALL, w(K::Graph, M::Memory)),
        (SMALL, w(K::Graph, M::DiskActivity)),
        (SMALL, w(K::Graph, M::NetworkDown)),
        (SMALL, w(K::Graph, M::GpuLoad)),
        (SMALL, w(K::Ring, M::Battery)),
        (MEDIUM, w(K::TopProcesses, M::CpuTotal)),
        (MEDIUM, w(K::TopProcesses, M::Memory)),
        (MEDIUM, w(K::Events, M::CpuTotal)),
        (MEDIUM, w(K::Consumers, M::CpuTotal)),
        (LARGE, w(K::History, M::CpuTotal)),
        (LARGE, w(K::DiskTrend, M::DiskFree)),
    ])
}

fn developer() -> Vec<Placement<DashboardWidget, Cells>> {
    use DashboardKind as K;
    use Metric as M;
    cells([
        (LARGE, w(K::Heatmap, M::CpuPerCore)),
        (MEDIUM, w(K::Graph, M::CpuTotal)),
        (MEDIUM, w(K::Graph, M::Memory)),
        (SMALL, w(K::BigNumber, M::Swap)),
        (MEDIUM, w(K::Graph, M::DiskActivity)),
        (SMALL, w(K::Ring, M::DiskFree)),
        (LARGE, w(K::TopProcesses, M::CpuTotal)),
    ])
}

fn gaming() -> Vec<Placement<DashboardWidget, Cells>> {
    use DashboardKind as K;
    use Metric as M;
    cells([
        (LARGE, w(K::Graph, M::GpuLoad)),
        (MEDIUM, w(K::Graph, M::CpuTotal)),
        (SMALL, w(K::BigNumber, M::Temperature).hot(80.0, 92.0)),
        (SMALL, w(K::BigNumber, M::Fan)),
        (SMALL, w(K::Ring, M::Memory)),
        (MEDIUM, w(K::Graph, M::NetworkDown)),
        (MEDIUM, w(K::TopProcesses, M::CpuTotal)),
    ])
}

fn battery() -> Vec<Placement<DashboardWidget, Cells>> {
    use DashboardKind as K;
    use Metric as M;
    cells([
        (MEDIUM, w(K::Ring, M::Battery)),
        (MEDIUM, w(K::Graph, M::CpuTotal)),
        (SMALL, w(K::BigNumber, M::Temperature).hot(80.0, 92.0)),
        (LARGE, w(K::TopProcesses, M::CpuTotal)),
        (MEDIUM, w(K::Issues, M::CpuTotal)),
    ])
}

impl Catalogue for DashboardCatalogue {
    type SurfaceId = DynoSurface;
    type Widget = DashboardWidget;
    type Place = Cells;
    const SURFACE_ID: DynoSurface = DynoSurface::Dashboard;
    const SURFACE: Surface = Surface {
        noun: "dashboard",
        schema_version: 2,
        columns: 12,
        max_placements: 32,
        // An empty dashboard shows nothing to read; the Overview is back instead.
        min_placements: 1,
        max_user_presets: 8,
        max_preset_placements: 24,
        max_bytes: super::MAX_BYTES,
    };

    /// The dashboard's arrangements by its own width (not the window's): 12 columns from 840 px, 6 from 560, else one.
    const BREAKPOINTS: &'static [Breakpoint] = &[
        Breakpoint { min: 840, columns: 12 },
        Breakpoint { min: 560, columns: 6 },
        Breakpoint { min: 0, columns: 1 },
    ];

    fn kinds() -> &'static [Kind<Self>] {
        KINDS
    }

    /// Dyno 1.0 stored spans; they become cells as they looked (ADR 0044 §4).
    fn upgrade(_from: u32, value: serde_json::Value) -> Result<serde_json::Value, String> {
        paddock_widgets::spans_to_cells::<Self>(value)
    }

    fn defaults() -> Vec<Placement<DashboardWidget, Cells>> {
        overview()
    }

    fn presets() -> Vec<Preset<DashboardWidget, Cells>> {
        let build = |id: &str, name: &str, placements| Preset {
            id: PresetId::new(id).unwrap_or_else(|_| unreachable!("the built-in preset ids are valid")),
            name: name.into(),
            builtin: true,
            placements,
        };
        vec![
            build(PRESET_IDS[0], "Overview", overview()),
            build(PRESET_IDS[1], "Developer", developer()),
            build(PRESET_IDS[2], "Gaming", gaming()),
            build(PRESET_IDS[3], "Battery", battery()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::{apply, check_catalogue, read, Edit, Layout, Stored, WidgetError};

    #[test]
    fn the_catalogue_is_consistent_and_the_presets_are_valid() {
        check_catalogue::<DashboardCatalogue>().unwrap();
        let ids: Vec<String> = DashboardCatalogue::presets().iter().map(|p| p.id.to_string()).collect();
        assert_eq!(ids, PRESET_IDS);
    }

    #[test]
    fn the_overview_recreates_the_old_fixed_dashboard() {
        let ids: Vec<String> = DashboardCatalogue::defaults().iter().map(|p| p.id.to_string()).collect();
        for needed in [
            "health",
            "diagnosis",
            "issues",
            "cpu",
            "top-processes",
            "events",
            "consumers",
            "history",
            "disk-trend",
        ] {
            assert!(ids.iter().any(|i| i == needed), "missing {needed} in {ids:?}");
        }
        assert_eq!(DashboardCatalogue::defaults().len(), 15);
    }

    #[test]
    fn the_shipped_1_0_default_upgrades_to_the_overview_and_each_preset_is_valid_cells() {
        // Dyno 1.0.0's default dashboard, as it was stored: spans only.
        let spans = [6, 6, 3, 6, 3, 3, 3, 3, 3, 6, 6, 6, 6, 12, 12];
        let defaults = DashboardCatalogue::defaults();
        assert_eq!(defaults.len(), spans.len());
        let stored: Vec<serde_json::Value> = defaults
            .iter()
            .zip(spans)
            .map(|(p, span)| serde_json::json!({ "id": p.id, "span": span, "widget": p.widget }))
            .collect();
        let r = read::<DashboardCatalogue>(&serde_json::json!({ "version": 1, "placements": stored, "presets": [] }));
        assert_eq!(r.upgraded, Some(1));
        assert!(r.notes.is_empty(), "{:?}", r.notes);
        assert_eq!(r.layout.placements, defaults, "the 1.0 default arrives looking as it did");
        // Row 1 is 6 + 6 (Health, Why slow: 9 rows); row 2: Issues 3, CPU 6, Graph 3 (CPU/Issues: 6 rows).
        assert_eq!(defaults[0].place.rect, paddock_widgets::Rect::new(0, 0, 6, 9));
        assert_eq!(defaults[2].place.rect, paddock_widgets::Rect::new(0, 9, 3, 6));
        assert_eq!(defaults[14].place.rect.w, 12);
        for p in DashboardCatalogue::presets() {
            assert!(!p.placements.is_empty(), "{}", p.id);
        }
    }

    #[test]
    fn a_widget_must_be_able_to_show_its_metric() {
        let ring = DashboardWidget::new(DashboardKind::Ring, Metric::NetworkDown);
        assert!(ring.validate().unwrap_err().contains("ring widget cannot show"));
        assert!(DashboardWidget::new(DashboardKind::Heatmap, Metric::CpuPerCore).validate().is_ok());
        assert!(DashboardWidget::new(DashboardKind::Heatmap, Metric::CpuTotal).validate().is_err());
        assert!(DashboardWidget::new(DashboardKind::TopProcesses, Metric::Fan).validate().is_err());
        assert!(DashboardWidget::new(DashboardKind::Graph, Metric::Process(" ".into()))
            .validate()
            .is_err());
        let mut w = DashboardWidget::new(DashboardKind::Graph, Metric::CpuTotal);
        w.options.accent = Some("red".into());
        assert!(w.validate().unwrap_err().contains("#1a2b3c"));
        w.options.accent = None;
        w.thresholds = Thresholds {
            warning: Some(90.0),
            critical: Some(80.0),
        };
        assert!(w.validate().unwrap_err().contains("below the critical"));
        w.thresholds = Thresholds {
            warning: Some(f32::NAN),
            critical: None,
        };
        assert!(w.validate().unwrap_err().contains("numbers"));
    }

    #[test]
    fn thresholds_decide_the_tone() {
        let t = Thresholds {
            warning: Some(70.0),
            critical: Some(90.0),
        };
        assert_eq!(t.tone(None), ValueTone::Ok);
        assert_eq!(t.tone(Some(69.9)), ValueTone::Ok);
        assert_eq!(t.tone(Some(70.0)), ValueTone::Warn);
        assert_eq!(t.tone(Some(95.0)), ValueTone::Bad);
        assert_eq!(Thresholds::default().tone(Some(1e9)), ValueTone::Ok);
        let only_critical = Thresholds {
            warning: None,
            critical: Some(10.0),
        };
        assert_eq!(only_critical.tone(Some(10.0)), ValueTone::Bad);
    }

    #[test]
    fn the_range_is_clamped_when_added() {
        let mut s = Stored::<DashboardCatalogue>::default();
        let mut wide = DashboardWidget::new(DashboardKind::Graph, Metric::Memory);
        wide.options.range_secs = 99_999;
        s.edit(Edit::Add {
            widget: wide,
            span: None,
            at: None,
        })
        .unwrap();
        let last = s.layout().placements.last().unwrap();
        assert_eq!(last.widget.options.range_secs, 3_600);
        assert_eq!(
            last.place.rect.size(),
            Size { w: 3, h: 4 },
            "a new graph has its kind's default size"
        );
    }

    #[test]
    fn applying_a_preset_replaces_the_widgets_and_can_be_undone() {
        let mut s = Stored::<DashboardCatalogue>::default();
        s.edit(Edit::ApplyPreset {
            id: PresetId::new("gaming").unwrap(),
        })
        .unwrap();
        assert_eq!(s.layout().placements.len(), 7);
        assert!(s.layout().can_undo());
        s.edit(Edit::Undo).unwrap();
        assert_eq!(s.layout().placements.len(), 15);
        assert_eq!(
            apply::<DashboardCatalogue>(
                s.layout(),
                Edit::ApplyPreset {
                    id: PresetId::new("nope").unwrap()
                }
            ),
            Err(WidgetError::UnknownPreset("nope".into()))
        );
    }

    #[test]
    fn the_dashboard_holds_thirty_two_widgets() {
        let placements = (0..32u16)
            .map(|n| Placement {
                id: paddock_widgets::PlacementId::new(&format!("graph-{n}")).unwrap(),
                place: Cells {
                    rect: paddock_widgets::Rect::new(u8::try_from(n % 4 * 3).unwrap(), n / 4 * 4, 3, 4),
                },
                widget: DashboardWidget::new(DashboardKind::Graph, Metric::Memory),
            })
            .collect();
        let layout: Layout<DashboardWidget, Cells> = Layout::new(2, placements, Vec::new());
        let added = apply::<DashboardCatalogue>(
            &layout,
            Edit::Add {
                widget: DashboardWidget::new(DashboardKind::Graph, Metric::Memory),
                span: None,
                at: None,
            },
        );
        assert!(matches!(added, Err(WidgetError::TooMany { max: 32, .. })), "{added:?}");
    }

    #[test]
    fn an_old_style_file_with_an_invalid_widget_drops_only_that_widget() {
        let value = serde_json::json!({"version": 1, "placements": [
            {"id": "a", "span": 6, "widget": {"kind": "graph", "metric": "memory"}},
            {"id": "b", "span": 6, "widget": {"kind": "ring", "metric": "networkDown"}},
        ], "presets": []});
        let r = read::<DashboardCatalogue>(&value);
        assert_eq!(r.layout.placements.len(), 1);
        assert_eq!(r.notes.len(), 1);
    }
}
