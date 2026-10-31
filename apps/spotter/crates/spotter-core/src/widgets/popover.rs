//! The tray popup's catalogue: which widgets it offers, their spans, limits and the defaults.

use super::SpotterSurface;
use paddock_widgets::{assign_ids, Catalogue, KindSpec, Placement, Refresh, Surface, Widget};
use serde::{Deserialize, Serialize};

/// A widget of the tray popup. None has options yet; the variants are the kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum PopoverWidget {
    /// The watched sites and the state of each.
    WatchedSites,
    /// The newest releases found.
    RecentChanges,
    /// The watch that is checked next, and when.
    NextCheck,
    /// The daily hours when notifications wait.
    QuietHours,
    /// The watches whose last check failed, each with Retry.
    Failures,
    /// Whether notifications are on, paused or off.
    Notifications,
}

/// The kinds of [`PopoverWidget`] (the same names).
pub type PopoverKind = PopoverWidget;

impl Widget for PopoverWidget {
    type Kind = PopoverKind;

    fn kind(&self) -> PopoverKind {
        *self
    }

    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/// The popup's catalogue (ADR 0035).
pub struct PopoverCatalogue;

/// Icons are registered names in the UI (`sp:` is Spotter's set, `pd:` the family's).
const KINDS: &[KindSpec<PopoverKind>] = &[
    KindSpec {
        kind: PopoverWidget::WatchedSites,
        title: "Watched sites",
        summary: "Your watches and how each one is doing",
        icon: "sp:watches",
        spans: &[2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverWidget::RecentChanges,
        title: "Recent changes",
        summary: "The newest releases Spotter found",
        icon: "sp:history",
        spans: &[2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverWidget::NextCheck,
        title: "Next check",
        summary: "Which watch is checked next, and when",
        icon: "sp:clock",
        spans: &[1, 2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverWidget::QuietHours,
        title: "Quiet hours",
        summary: "When notifications wait",
        icon: "sp:moon",
        spans: &[1, 2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverWidget::Failures,
        title: "Failures",
        summary: "Watches that could not be checked, with Retry",
        icon: "sp:alert",
        spans: &[2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverWidget::Notifications,
        title: "Notifications",
        summary: "Whether notifications are on, paused or off",
        icon: "sp:bell",
        spans: &[1, 2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
];

impl Catalogue for PopoverCatalogue {
    type SurfaceId = SpotterSurface;
    type Widget = PopoverWidget;
    const SURFACE_ID: SpotterSurface = SpotterSurface::Popover;
    const SURFACE: Surface = Surface {
        noun: "popup",
        schema_version: 1,
        columns: 2,
        max_placements: 6,
        min_placements: 0,
        max_user_presets: 0,
        max_preset_placements: 6,
        max_bytes: super::MAX_BYTES,
    };

    fn kinds() -> &'static [KindSpec<PopoverKind>] {
        KINDS
    }

    fn defaults() -> Vec<Placement<PopoverWidget>> {
        assign_ids([
            (2, PopoverWidget::RecentChanges),
            (2, PopoverWidget::WatchedSites),
            (2, PopoverWidget::Failures),
            (1, PopoverWidget::NextCheck),
            (1, PopoverWidget::Notifications),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::{check_catalogue, read, Edit, Stored};

    #[test]
    fn the_catalogue_is_consistent() {
        check_catalogue::<PopoverCatalogue>().unwrap();
    }

    #[test]
    fn the_defaults_lead_with_what_changed_and_what_is_wrong() {
        let kinds: Vec<PopoverKind> = PopoverCatalogue::defaults().iter().map(|p| p.widget.kind()).collect();
        assert_eq!(
            kinds,
            [
                PopoverWidget::RecentChanges,
                PopoverWidget::WatchedSites,
                PopoverWidget::Failures,
                PopoverWidget::NextCheck,
                PopoverWidget::Notifications
            ]
        );
    }

    #[test]
    fn every_kind_is_placed_at_most_once() {
        assert!(PopoverCatalogue::kinds().iter().all(|k| k.max_instances == 1));
        assert_eq!(PopoverCatalogue::kinds().len(), 6);
    }

    #[test]
    fn an_unknown_kind_in_the_file_is_dropped_with_a_note() {
        let value = serde_json::json!({"version": 1, "placements": [
            {"id": "failures", "span": 2, "widget": "failures"},
            {"id": "weather", "span": 2, "widget": "weather"},
        ], "presets": []});
        let r = read::<PopoverCatalogue>(&value);
        assert_eq!(r.layout.placements.len(), 1);
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn the_popup_holds_six_and_a_kind_is_placed_once() {
        let mut s = Stored::<PopoverCatalogue>::default();
        let again = s.edit(Edit::Add {
            widget: PopoverWidget::Failures,
            span: None,
            at: None,
        });
        assert!(again.is_err(), "Failures is already shown");
        s.edit(Edit::Add {
            widget: PopoverWidget::QuietHours,
            span: None,
            at: None,
        })
        .unwrap();
        assert_eq!(s.layout().placements.len(), 6);
    }

    #[test]
    fn the_wire_shape_is_a_camel_case_string() {
        assert_eq!(serde_json::to_value(PopoverWidget::RecentChanges).unwrap(), "recentChanges");
        let back: PopoverWidget = serde_json::from_value(serde_json::json!("quietHours")).unwrap();
        assert_eq!(back, PopoverWidget::QuietHours);
    }
}
