//! What the chooser needs to draw and limit itself: the catalogue as data, with the layout.

use serde::Serialize;
use ts_rs::TS;

use crate::catalogue::{Catalogue, Refresh, Widget};
use crate::check;
use crate::error::Note;
use crate::layout::{Layout, Preset};

/// A surface's grid and limits as the UI sees them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceInfo {
    /// The surface's name.
    pub id: String,
    /// Columns of the grid.
    pub columns: u8,
    /// Most placements.
    pub max_placements: u16,
    /// Fewest placements.
    pub min_placements: u16,
    /// Most saved presets.
    pub max_user_presets: u8,
    /// Most placements in a preset.
    pub max_preset_placements: u16,
}

/// One catalogue entry as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct KindInfo<K> {
    /// The kind.
    pub kind: K,
    /// Sentence-case title.
    pub title: String,
    /// One line for the chooser.
    pub summary: String,
    /// Registered icon name.
    pub icon: String,
    /// Allowed spans, the first is the default.
    pub spans: Vec<u8>,
    /// How many may be placed.
    pub max_instances: u8,
    /// How it refreshes.
    pub refresh: Refresh,
}

/// The catalogue as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueInfo<K> {
    /// The surface.
    pub surface: SurfaceInfo,
    /// The kinds, in chooser order.
    pub kinds: Vec<KindInfo<K>>,
}

/// A layout with everything the chooser needs: no limit, title or preset is hard-coded in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LayoutView<W, K> {
    /// The layout.
    pub layout: Layout<W>,
    /// The catalogue.
    pub catalogue: CatalogueInfo<K>,
    /// The catalogue's own presets (the person's are in the layout).
    pub builtin_presets: Vec<Preset<W>>,
    /// What was wrong when the layout was read, to show once.
    pub notes: Vec<Note>,
    /// Whether an undo is available.
    pub can_undo: bool,
}

/// The catalogue as data.
pub fn catalogue_info<C: Catalogue>() -> CatalogueInfo<<C::Widget as Widget>::Kind> {
    let s = &C::SURFACE;
    CatalogueInfo {
        surface: SurfaceInfo {
            id: s.id.into(),
            columns: s.columns,
            max_placements: s.max_placements,
            min_placements: s.min_placements,
            max_user_presets: s.max_user_presets,
            max_preset_placements: s.max_preset_placements,
        },
        kinds: C::kinds()
            .iter()
            .map(|k| KindInfo {
                kind: k.kind,
                title: k.title.into(),
                summary: k.summary.into(),
                icon: k.icon.into(),
                spans: k.spans.to_vec(),
                max_instances: k.max_instances,
                refresh: k.refresh,
            })
            .collect(),
    }
}

/// The catalogue's presets, marked built-in, with invalid placements dropped.
pub fn builtin_presets<C: Catalogue>() -> Vec<Preset<C::Widget>> {
    C::presets()
        .into_iter()
        .map(|mut p| {
            p.builtin = true;
            p.placements = check::keep_valid::<C>(
                p.placements,
                C::SURFACE.max_preset_placements as usize,
                &mut 0,
            );
            p
        })
        .collect()
}

/// A layout with its catalogue and notes, for the chooser.
pub fn view<C: Catalogue>(
    layout: Layout<C::Widget>,
    notes: Vec<Note>,
) -> LayoutView<C::Widget, <C::Widget as Widget>::Kind> {
    LayoutView {
        can_undo: layout.can_undo(),
        layout,
        catalogue: catalogue_info::<C>(),
        builtin_presets: builtin_presets::<C>(),
        notes,
    }
}
