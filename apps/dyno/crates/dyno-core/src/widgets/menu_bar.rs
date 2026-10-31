//! The menu-bar items' catalogue: one tray icon per item, drawn live from one metric in one style.

use super::metric::Metric;
use super::DynoSurface;
use paddock_widgets::{assign_ids, Catalogue, KindSpec, Placement, Refresh, Surface, Widget};
use serde::{Deserialize, Serialize};

/// Longest unit of a number item ("%", "°C"), in characters.
pub const MAX_UNIT: usize = 8;
/// Longest label of a number item, in characters.
pub const MAX_LABEL: usize = 16;

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
            MenuBarItem::Number { metric, unit, label } => MenuBarItem::Number {
                metric,
                unit: clean_text(&unit, MAX_UNIT),
                label: clean_text(&label, MAX_LABEL),
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
        spans: &[1],
        max_instances: 4,
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
    const SURFACE_ID: DynoSurface = DynoSurface::MenuBar;
    const SURFACE: Surface = Surface {
        schema_version: 1,
        columns: 1,
        max_placements: 4,
        min_placements: 1,
        max_user_presets: 0,
        max_preset_placements: 4,
        max_bytes: super::MAX_BYTES,
    };

    fn kinds() -> &'static [KindSpec<MenuBarKind>] {
        KINDS
    }

    fn defaults() -> Vec<Placement<MenuBarItem>> {
        assign_ids([
            (1, MenuBarItem::VerticalBars { metric: Metric::CpuTotal }),
            (1, MenuBarItem::VerticalBars { metric: Metric::Memory }),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::{check_catalogue, read, Edit, Stored, WidgetError};

    #[test]
    fn the_catalogue_is_consistent_and_the_defaults_are_cpu_and_memory_bars() {
        check_catalogue::<MenuBarCatalogue>().unwrap();
        let d = MenuBarCatalogue::defaults();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].widget.metric(), &Metric::CpuTotal);
        assert_eq!(d[1].widget.metric(), &Metric::Memory);
    }

    #[test]
    fn a_number_item_is_cleaned_and_a_bad_metric_is_refused() {
        let item = MenuBarItem::Number {
            metric: Metric::Memory,
            unit: "%\n%%%%%%%%%%%".into(),
            label: "L".repeat(40),
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
    fn four_items_at_most_and_at_least_one() {
        let mut s = Stored::<MenuBarCatalogue>::default();
        for _ in 0..2 {
            s.edit(Edit::Add {
                widget: MenuBarItem::Ring { metric: Metric::GpuLoad },
                span: None,
                at: None,
            })
            .unwrap();
        }
        let e = s.edit(Edit::Add {
            widget: MenuBarItem::Ring { metric: Metric::GpuLoad },
            span: None,
            at: None,
        });
        assert!(matches!(e, Err(WidgetError::TooMany { max: 4, .. })), "{e:?}");
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
