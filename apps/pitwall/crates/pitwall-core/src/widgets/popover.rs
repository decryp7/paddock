//! The tray popup's catalogue: which widgets it offers, their spans, limits and the defaults. Each kind answers part of
//! "is my local AI working, and what is it doing".

use super::PitwallSurface;
use paddock_widgets::{assign_spans, Catalogue, KindSpec, Placement, Refresh, Surface, Widget};
use serde::{Deserialize, Serialize};

/// A widget of the tray popup. None has options yet; the variants are the kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum PopoverWidget {
    /// Whether the server is up and serving, on which port, and whether its engines are healthy.
    Server,
    /// Chats running now, the queue and tokens per second.
    Activity,
    /// Memory and video memory in use.
    Memory,
    /// The models in memory, each with what it holds.
    LoadedModels,
    /// Model and engine downloads in progress.
    Downloads,
    /// Model updates on offer.
    Updates,
    /// The latest errors.
    RecentErrors,
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

/// Icons are registered names in the UI (`pw:` is Pitwall's set).
const KINDS: &[KindSpec<PopoverKind>] = &[
    KindSpec {
        kind: PopoverWidget::Server,
        title: "Server",
        summary: "Whether the server is serving, and its engines are healthy",
        icon: "pw:server",
        size: &[1, 2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverWidget::Activity,
        title: "Activity",
        summary: "Chats running now, the queue and tokens per second",
        icon: "pw:bolt",
        size: &[1, 2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverWidget::Memory,
        title: "Memory",
        summary: "RAM and video memory in use, and the pressure",
        icon: "pw:memory",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverWidget::LoadedModels,
        title: "Loaded models",
        summary: "The models in memory and what each one holds",
        icon: "pw:models",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverWidget::Downloads,
        title: "Downloads",
        summary: "Model and engine downloads in progress",
        icon: "pw:download",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::Live,
    },
    KindSpec {
        kind: PopoverWidget::Updates,
        title: "Updates",
        summary: "Newer versions of the models you downloaded",
        icon: "pw:sparkles",
        size: &[1, 2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
    KindSpec {
        kind: PopoverWidget::RecentErrors,
        title: "Recent errors",
        summary: "What failed in the last day",
        icon: "pw:alert",
        size: &[2],
        max_instances: 1,
        refresh: Refresh::OnChange,
    },
];

/// Rows Loaded models lists.
pub const MODEL_ROWS: usize = 2;
/// Rows Downloads lists when none is running (the paused and failed ones).
pub const DOWNLOAD_ROWS: usize = 2;
/// Rows Recent errors lists.
pub const ERROR_ROWS: usize = 2;

/// How many rows a widget draws when it has values: what its loading twin draws too, so loading and ready are the same
/// height. The one source of the row limits (the engine truncates to these; the UI reads them from the engine).
pub const fn rows_of(kind: PopoverWidget) -> usize {
    match kind {
        PopoverWidget::LoadedModels => MODEL_ROWS,
        PopoverWidget::Downloads => DOWNLOAD_ROWS,
        PopoverWidget::RecentErrors => ERROR_ROWS,
        PopoverWidget::Server | PopoverWidget::Activity | PopoverWidget::Memory | PopoverWidget::Updates => 0,
    }
}

/// The rows of one widget kind in its loading twin (see [`rows_of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LoadingRows {
    /// The widget kind.
    pub kind: PopoverWidget,
    /// How many placeholder rows its loading twin draws.
    pub rows: u32,
}

/// The loading shape of every kind, in catalogue order.
pub fn loading_shape() -> Vec<LoadingRows> {
    PopoverCatalogue::kinds()
        .iter()
        .map(|k| LoadingRows {
            kind: k.kind,
            rows: u32::try_from(rows_of(k.kind)).unwrap_or(u32::MAX),
        })
        .collect()
}

impl Catalogue for PopoverCatalogue {
    type SurfaceId = PitwallSurface;
    type Widget = PopoverWidget;
    type Place = paddock_widgets::Span;
    const SURFACE_ID: PitwallSurface = PitwallSurface::Popover;
    const SURFACE: Surface = Surface {
        noun: "popup",
        schema_version: 1,
        columns: 2,
        max_placements: 7,
        min_placements: 0,
        max_user_presets: 0,
        max_preset_placements: 7,
        max_bytes: super::MAX_BYTES,
    };

    fn kinds() -> &'static [KindSpec<PopoverKind>] {
        KINDS
    }

    fn defaults() -> Vec<Placement<PopoverWidget>> {
        assign_spans([
            (1, PopoverWidget::Server),
            (1, PopoverWidget::Activity),
            (2, PopoverWidget::Memory),
            (2, PopoverWidget::LoadedModels),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_widgets::{check_catalogue, read, Edit, Stored};

    #[test]
    fn the_loading_shape_has_every_kind_with_the_literal_row_limits() {
        let shape: Vec<(PopoverWidget, u32)> = loading_shape().iter().map(|l| (l.kind, l.rows)).collect();
        assert_eq!(shape.len(), 7);
        let rows = |k| shape.iter().find(|(w, _)| *w == k).unwrap().1;
        assert_eq!(
            [
                rows(PopoverWidget::LoadedModels),
                rows(PopoverWidget::Downloads),
                rows(PopoverWidget::RecentErrors)
            ],
            [2, 2, 2]
        );
        assert_eq!(
            [
                rows(PopoverWidget::Server),
                rows(PopoverWidget::Activity),
                rows(PopoverWidget::Memory),
                rows(PopoverWidget::Updates)
            ],
            [0, 0, 0, 0]
        );
    }

    #[test]
    fn the_catalogue_is_consistent() {
        check_catalogue::<PopoverCatalogue>().unwrap();
    }

    #[test]
    fn the_defaults_answer_is_it_working_what_is_it_doing_how_much_memory_and_what_is_loaded() {
        let placed: Vec<(u8, PopoverKind)> = PopoverCatalogue::defaults().iter().map(|p| (p.place.span, p.widget.kind())).collect();
        assert_eq!(
            placed,
            [
                (1, PopoverWidget::Server),
                (1, PopoverWidget::Activity),
                (2, PopoverWidget::Memory),
                (2, PopoverWidget::LoadedModels)
            ]
        );
    }

    #[test]
    fn every_kind_is_placed_at_most_once_and_the_popup_holds_all_seven() {
        assert!(PopoverCatalogue::kinds().iter().all(|k| k.max_instances == 1));
        assert_eq!(PopoverCatalogue::kinds().len(), 7);
        let mut s = Stored::<PopoverCatalogue>::default();
        let again = s.edit(Edit::Add {
            widget: PopoverWidget::Memory,
            span: None,
            at: None,
        });
        assert!(again.is_err(), "Memory is already shown");
        for widget in [PopoverWidget::Downloads, PopoverWidget::Updates, PopoverWidget::RecentErrors] {
            s.edit(Edit::Add {
                widget,
                span: None,
                at: None,
            })
            .unwrap();
        }
        assert_eq!(s.layout().placements.len(), 7);
    }

    #[test]
    fn an_unknown_kind_in_the_file_is_dropped_with_a_note() {
        let value = serde_json::json!({"version": 1, "placements": [
            {"id": "memory", "span": 2, "widget": "memory"},
            {"id": "offline", "span": 2, "widget": "offline"},
        ], "presets": []});
        let r = read::<PopoverCatalogue>(&value);
        assert_eq!(r.layout.placements.len(), 1);
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn the_wire_shape_is_a_camel_case_string() {
        assert_eq!(serde_json::to_value(PopoverWidget::LoadedModels).unwrap(), "loadedModels");
        let back: PopoverWidget = serde_json::from_value(serde_json::json!("recentErrors")).unwrap();
        assert_eq!(back, PopoverWidget::RecentErrors);
    }
}
