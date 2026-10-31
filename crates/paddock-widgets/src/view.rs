//! What the chooser needs to draw and limit itself: the catalogue as data, with the layout.

use serde::{Deserialize, Serialize};

use crate::catalogue::{Catalogue, Refresh, Widget};
use crate::check;
use crate::error::{Counted, Note};
use crate::layout::{Layout, Preset};
use crate::read::Stored;

ts! {
/// The surface as the chooser needs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceInfo<S> {
    /// The app's surface.
    pub id: S,
    /// What the surface is called in messages ("popup").
    pub noun: String,
    /// Columns of the grid.
    pub columns: u8,
    /// Most placements.
    pub max_placements: u16,
    /// Fewest placements.
    pub min_placements: u16,
    /// Most saved presets.
    pub max_user_presets: u8,
}

/// One kind as the chooser needs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// The catalogue of one surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueInfo<S, K> {
    /// The surface.
    pub surface: SurfaceInfo<S>,
    /// The kinds, in chooser order.
    pub kinds: Vec<KindInfo<K>>,
}

/// Everything the chooser draws: the layout, its catalogue, the notes and whether Undo is offered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutView<S, W, K> {
    /// The layout.
    pub layout: Layout<W>,
    /// The catalogue.
    pub catalogue: CatalogueInfo<S, K>,
    /// The catalogue's own presets (the person's are in the layout).
    pub builtin_presets: Vec<Preset<W>>,
    /// What was wrong when the layout was read, to show once.
    pub notes: Vec<Note>,
    /// True right after an undoable edit.
    pub can_undo: bool,
    /// False while a newer version's layout is kept (`NoteCode::Newer`): the chooser is read-only.
    pub editable: bool,
}
}

/// The [`LayoutView`] of a catalogue `C`.
pub type ViewOf<C> = LayoutView<
    <C as Catalogue>::SurfaceId,
    <C as Catalogue>::Widget,
    <<C as Catalogue>::Widget as Widget>::Kind,
>;

/// The catalogue as data.
pub fn info<C: Catalogue>() -> CatalogueInfo<C::SurfaceId, <C::Widget as Widget>::Kind> {
    let s = &C::SURFACE;
    CatalogueInfo {
        surface: SurfaceInfo {
            id: C::SURFACE_ID,
            noun: s.noun.into(),
            columns: s.columns,
            max_placements: s.max_placements,
            min_placements: s.min_placements,
            max_user_presets: s.max_user_presets,
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
                Counted::PresetPlacements,
                &mut 0,
            );
            p
        })
        .collect()
}

/// A stored layout with its catalogue, built-in presets and notes, for the chooser.
pub fn view<C: Catalogue>(
    stored: &Stored<C>,
) -> LayoutView<C::SurfaceId, C::Widget, <C::Widget as Widget>::Kind> {
    LayoutView {
        can_undo: stored.read.layout.can_undo(),
        layout: stored.read.layout.clone(),
        catalogue: info::<C>(),
        builtin_presets: builtin_presets::<C>(),
        notes: stored.read.notes.clone(),
        editable: stored.editable(),
    }
}
