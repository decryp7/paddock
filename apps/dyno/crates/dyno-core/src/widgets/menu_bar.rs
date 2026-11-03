//! The menu-bar items' catalogue: one tray icon per item, drawn live from one metric in one style.

use super::metric::Metric;
use super::DynoSurface;
use paddock_widgets::{assign_spans, Catalogue, KindSpec, Placement, Refresh, Surface, Widget};
use serde::{Deserialize, Serialize};

/// Most menu-bar items (one tray icon each). Every metric can be its own item, in several styles; 32 is the bound the dashboard
/// uses and the longest list the menu bar or taskbar can sensibly hold, and keeps the native icons (and the 32-character ids
/// their OS identities derive from) bounded. Raised from 4 in Dyno 1.1 (ADR 0028).
pub const MAX_ITEMS: u8 = 32;
/// Longest unit of a number item ("%", "°C"), in characters.
pub const MAX_UNIT: usize = 8;
/// Longest label of a number item, in characters.
pub const MAX_LABEL: usize = 16;

/// How a number item draws: the label over the value (the narrowest), or label and value on one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum NumberFormat {
    /// The label over the value.
    #[default]
    Stacked,
    /// "CPU 23%" on one line.
    Line,
}

/// One menu-bar item; the variant is how it draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MenuBarItem {
    /// A small history line.
    LineGraph {
        /// What it shows.
        metric: Metric,
    },
    /// Vertical bars.
    VerticalBars {
        /// What it shows.
        metric: Metric,
    },
    /// A horizontal meter.
    HorizontalMeter {
        /// What it shows.
        metric: Metric,
    },
    /// A ring.
    Ring {
        /// What it shows.
        metric: Metric,
    },
    /// The value as text.
    Number {
        /// What it shows.
        metric: Metric,
        /// The unit after the value ("%", "°C").
        unit: String,
        /// A short label before it ("CPU").
        label: String,
        /// Stacked (the default) or on one line. Absent in older saved settings: stacked.
        #[serde(default)]
        format: NumberFormat,
    },
    /// A line with the number beside it.
    GraphAndNumber {
        /// What it shows.
        metric: Metric,
    },
}

/// The kinds of [`MenuBarItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum MenuBarKind {
    /// [`MenuBarItem::LineGraph`].
    LineGraph,
    /// [`MenuBarItem::VerticalBars`].
    VerticalBars,
    /// [`MenuBarItem::HorizontalMeter`].
    HorizontalMeter,
    /// [`MenuBarItem::Ring`].
    Ring,
    /// [`MenuBarItem::Number`].
    Number,
    /// [`MenuBarItem::GraphAndNumber`].
    GraphAndNumber,
}

impl MenuBarItem {
    /// What the item shows.
    pub fn metric(&self) -> &Metric {
        match self {
            MenuBarItem::LineGraph { metric }
            | MenuBarItem::VerticalBars { metric }
            | MenuBarItem::HorizontalMeter { metric }
            | MenuBarItem::Ring { metric }
            | MenuBarItem::Number { metric, .. }
            | MenuBarItem::GraphAndNumber { metric } => metric,
        }
    }
}

fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl Widget for MenuBarItem {
    type Kind = MenuBarKind;

    fn kind(&self) -> MenuBarKind {
        match self {
            MenuBarItem::LineGraph { .. } => MenuBarKind::LineGraph,
            MenuBarItem::VerticalBars { .. } => MenuBarKind::VerticalBars,
            MenuBarItem::HorizontalMeter { .. } => MenuBarKind::HorizontalMeter,
            MenuBarItem::Ring { .. } => MenuBarKind::Ring,
            MenuBarItem::Number { .. } => MenuBarKind::Number,
            MenuBarItem::GraphAndNumber { .. } => MenuBarKind::GraphAndNumber,
        }
    }

    fn validate(&self) -> Result<(), String> {
        self.metric().validate()
    }

    fn clamped(self) -> MenuBarItem {
        match self {
            MenuBarItem::Number {
                metric,
                unit,
                label,
                format,
            } => MenuBarItem::Number {
                metric,
                unit: clean_text(&unit, MAX_UNIT),
                label: clean_text(&label, MAX_LABEL),
                format,
            },
            other => other,
        }
    }
}

/// The menu bar's catalogue.
pub struct MenuBarCatalogue;

const fn spec(kind: MenuBarKind, title: &'static str, summary: &'static str, icon: &'static str) -> KindSpec<MenuBarKind> {
    KindSpec {
        kind,
        title,
        summary,
        icon,
        size: &[1],
        max_instances: MAX_ITEMS,
        refresh: Refresh::Live,
    }
}

const KINDS: &[KindSpec<MenuBarKind>] = &[
    spec(MenuBarKind::LineGraph, "Line graph", "A small history line", "graph"),
    spec(MenuBarKind::VerticalBars, "Bars", "Vertical bars", "bars"),
    spec(MenuBarKind::HorizontalMeter, "Meter", "A horizontal meter", "meter"),
    spec(MenuBarKind::Ring, "Ring", "A percentage as a ring", "ring"),
    spec(MenuBarKind::Number, "Number", "The value as text", "number"),
    spec(
        MenuBarKind::GraphAndNumber,
        "Graph and number",
        "A line with the number beside it",
        "graph",
    ),
];

impl Catalogue for MenuBarCatalogue {
    type SurfaceId = DynoSurface;
    type Widget = MenuBarItem;
    type Place = paddock_widgets::Span;
    const SURFACE_ID: DynoSurface = DynoSurface::MenuBar;
    const SURFACE: Surface = Surface {
        noun: "menu bar",
        schema_version: 1,
        columns: 1,
        max_placements: MAX_ITEMS as u16,
        min_placements: 1,
        max_user_presets: 0,
        max_preset_placements: MAX_ITEMS as u16,
        max_bytes: super::MAX_BYTES,
    };

    fn kinds() -> &'static [KindSpec<MenuBarKind>] {
        KINDS
    }

    fn defaults() -> Vec<Placement<MenuBarItem>> {
        // The design's default set (docs/design/dyno-tray/README.md §2): four stacked numbers, about 200 pt in a macOS menu bar.
        let number = |metric| MenuBarItem::Number {
            metric,
            unit: String::new(),
            label: String::new(),
            format: NumberFormat::Stacked,
        };
        assign_spans([
            (1, number(Metric::CpuTotal)),
            (1, number(Metric::Memory)),
            (1, number(Metric::NetworkDown)),
            (1, number(Metric::Temperature)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::{check_catalogue, read, Edit, Stored, WidgetError};

    #[test]
    fn the_catalogue_is_consistent_and_the_defaults_are_four_numbers() {
        check_catalogue::<MenuBarCatalogue>().unwrap();
        let d = MenuBarCatalogue::defaults();
        let metrics: Vec<&Metric> = d.iter().map(|p| p.widget.metric()).collect();
        assert_eq!(
            metrics,
            [&Metric::CpuTotal, &Metric::Memory, &Metric::NetworkDown, &Metric::Temperature]
        );
        assert!(
            d.iter().all(|p| matches!(p.widget, MenuBarItem::Number { .. })),
            "numbers draw stacked"
        );
    }

    #[test]
    fn a_number_item_is_cleaned_and_a_bad_metric_is_refused() {
        let item = MenuBarItem::Number {
            metric: Metric::Memory,
            unit: "%\n%%%%%%%%%%%".into(),
            label: "L".repeat(40),
            format: NumberFormat::Stacked,
        };
        let MenuBarItem::Number { unit, label, .. } = item.clamped() else {
            unreachable!()
        };
        assert_eq!(unit.chars().count(), MAX_UNIT);
        assert!(!unit.contains('\n'));
        assert_eq!(label.chars().count(), MAX_LABEL);
        assert!(MenuBarItem::Ring {
            metric: Metric::Process("".into())
        }
        .validate()
        .is_err());
    }

    #[test]
    fn up_to_the_cap_in_items_and_at_least_one() {
        let mut s = Stored::<MenuBarCatalogue>::default();
        let add = |s: &mut Stored<MenuBarCatalogue>| {
            s.edit(Edit::Add {
                widget: MenuBarItem::Ring { metric: Metric::GpuLoad },
                span: None,
                at: None,
            })
        };
        while s.layout().placements.len() < usize::from(MAX_ITEMS) {
            add(&mut s).unwrap();
        }
        assert_eq!(s.layout().placements.len(), usize::from(MAX_ITEMS), "the cap is reachable");
        assert!(s.layout().placements.len() > 4, "more than the old cap of four");
        let e = add(&mut s);
        assert!(matches!(e, Err(WidgetError::TooMany { max: 32, .. })), "{e:?}");
        let ids: Vec<String> = s.layout().placements.iter().map(|p| p.id.to_string()).collect();
        for id in ids.iter().skip(1) {
            s.edit(Edit::Remove {
                id: paddock_widgets::PlacementId::new(id).unwrap(),
            })
            .unwrap();
        }
        let last = s.layout().placements[0].id.clone();
        assert!(matches!(s.edit(Edit::Remove { id: last }), Err(WidgetError::TooFew { .. })));
    }

    #[test]
    fn the_old_tray_items_keys_are_not_read() {
        // Dyno 1.0 is unreleased: `trayItems` and `tray` are dropped outright, not migrated.
        let r = read::<MenuBarCatalogue>(&serde_json::json!({"version": 1, "items": [{"id": "cpu"}]}));
        assert_eq!(r.layout.placements, MenuBarCatalogue::defaults());
    }
}
