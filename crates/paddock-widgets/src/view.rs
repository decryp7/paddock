//! What the chooser needs to draw and limit itself: the catalogue as data, with the layout.

use serde::{Deserialize, Serialize};

use crate::catalogue::{Catalogue, Refresh, Widget};
use crate::check;
use crate::error::{Counted, Note};
use crate::grid::{self, Block, Dir, GridGesture, Item, Moves, Rect};
use crate::id::PlacementId;
use crate::layout::{Layout, Preset};
use crate::place::{KindSize, Place};
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
    /// How it may be sized: spans (flow) or widths and heights (grid).
    pub size: KindSize,
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

/// What an arrangement edit did to one placement, for the screen-reader announcement (ADR 0044 §5): the engine's own
/// answer, so the UI never derives it. "Memory moved right, after CPU. Position 6 of 15."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The placement.
    pub id: PlacementId,
    /// Its place in reading order, counted from 1.
    pub position: u16,
    /// How many placements there are.
    pub of: u16,
    /// The placement before it in reading order, if any.
    pub after: Option<PlacementId>,
    /// The placement after it in reading order, if any.
    pub before: Option<PlacementId>,
    /// Its width now, in columns.
    pub w: u8,
    /// Its height now, in rows.
    pub h: u8,
    /// Set when a resize left it at the kind's largest or smallest size in that direction.
    pub limit: Option<Block>,
}

/// A placement's rectangle in one arrangement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placed {
    /// The placement.
    pub id: PlacementId,
    /// Its cells.
    pub rect: Rect,
}

/// The arrangement a grid surface shows from a container width on (ADR 0044 §3). The first is the design arrangement,
/// the stored layout; narrower ones are derived and never stored. Reading order is the design's in every one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arrangement {
    /// The least container width, in px, for this arrangement.
    pub min_width: u16,
    /// Its columns.
    pub columns: u8,
    /// Every placement, in reading order.
    pub placements: Vec<Placed>,
}

/// Which gestures would change the arrangement for one placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementMoves {
    /// The placement.
    pub id: PlacementId,
    /// Its moves.
    pub moves: Moves,
}

/// Everything the chooser draws: the layout, its catalogue, the notes and whether Undo is offered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutView<S, W, P, K> {
    /// The layout.
    pub layout: Layout<W, P>,
    /// Counts up with every applied edit and every reload; an arrange edit names the revision it was computed on.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
    pub revision: u64,
    /// The arrangement for each breakpoint, widest first (grid surfaces; empty on flow surfaces).
    pub arrangements: Vec<Arrangement>,
    /// Which gestures work for each placement (grid surfaces; empty on flow surfaces).
    pub moves: Vec<PlacementMoves>,
    /// True when an undone edit can be redone.
    pub can_redo: bool,
    /// What the last arrangement edit did to its placement (also in a previewed view); `None` after any other edit.
    pub outcome: Option<Outcome>,
    /// The catalogue.
    pub catalogue: CatalogueInfo<S, K>,
    /// The catalogue's own presets (the person's are in the layout).
    pub builtin_presets: Vec<Preset<W, P>>,
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
    <C as Catalogue>::Place,
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
                size: <C::Place as Place>::size_info(&k.size),
                max_instances: k.max_instances,
                refresh: k.refresh,
            })
            .collect(),
    }
}

/// The catalogue's presets, marked built-in, with invalid placements dropped.
pub fn builtin_presets<C: Catalogue>() -> Vec<Preset<C::Widget, C::Place>> {
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

/// The layout as grid items, or `None` on a flow surface.
fn grid_items<C: Catalogue>(layout: &Layout<C::Widget, C::Place>) -> Option<Vec<Item>> {
    layout
        .placements
        .iter()
        .map(|p| {
            let spec = crate::catalogue::spec_of::<C>(p.widget.kind())?;
            Some(Item {
                id: p.id.clone(),
                rect: p.place.rect()?,
                rule: <C::Place as Place>::size_rule(&spec.size)?,
            })
        })
        .collect()
}

fn placed(items: &[Item]) -> Vec<Placed> {
    items
        .iter()
        .map(|i| Placed {
            id: i.id.clone(),
            rect: i.rect,
        })
        .collect()
}

/// The arrangement of each breakpoint: the stored layout at the widest, the derived ones below.
fn arrangements<C: Catalogue>(items: &[Item]) -> Vec<Arrangement> {
    let design = C::SURFACE.columns;
    let sorted = grid::reading_order(items.to_vec());
    if C::BREAKPOINTS.is_empty() {
        return vec![Arrangement {
            min_width: 0,
            columns: design,
            placements: placed(&sorted),
        }];
    }
    C::BREAKPOINTS
        .iter()
        .filter_map(|bp| {
            let at = if bp.columns == design {
                Ok(sorted.clone())
            } else {
                grid::derive(&sorted, design, bp.columns)
            };
            at.ok().map(|a| Arrangement {
                min_width: bp.min,
                columns: bp.columns,
                placements: placed(&a),
            })
        })
        .collect()
}

/// A stored layout with its catalogue, built-in presets and notes, for the chooser.
pub fn view<C: Catalogue>(stored: &Stored<C>) -> ViewOf<C> {
    let layout = &stored.read.layout;
    let items = grid_items::<C>(layout);
    let (arrangements, moves) = match &items {
        Some(items) => (
            arrangements::<C>(items),
            items
                .iter()
                .zip(grid::moves_all(items, C::SURFACE.columns))
                .map(|(i, moves)| PlacementMoves {
                    id: i.id.clone(),
                    moves,
                })
                .collect(),
        ),
        None => (Vec::new(), Vec::new()),
    };
    LayoutView {
        can_undo: layout.can_undo(),
        can_redo: layout.can_redo(),
        revision: stored.revision(),
        outcome: stored.outcome.clone(),
        arrangements,
        moves,
        layout: layout.clone(),
        catalogue: info::<C>(),
        builtin_presets: builtin_presets::<C>(),
        notes: stored.read.notes.clone(),
        editable: stored.editable(),
    }
}

/// What `gesture` just did to its placement in `layout` (already arranged): its position in reading order, its
/// neighbours, its size and whether a resize ended at a limit of the kind's rule.
pub(crate) fn outcome_of<C: Catalogue>(
    layout: &Layout<C::Widget, C::Place>,
    gesture: &GridGesture,
) -> Option<Outcome> {
    let id = gesture.id();
    let index = layout.placements.iter().position(|p| &p.id == id)?;
    let placed = &layout.placements[index];
    let rect = placed.place.rect()?;
    let limit = match gesture {
        GridGesture::Grow { dir, .. } => {
            let rule = <C::Place as Place>::size_rule(
                &crate::catalogue::spec_of::<C>(placed.widget.kind())?.size,
            )?;
            match dir {
                Dir::Right => rule.widths.up(rect.w).is_none().then_some(Block::Largest),
                Dir::Down => rule.heights.up(rect.h).is_none().then_some(Block::Largest),
                Dir::Left => rule
                    .widths
                    .down(rect.w)
                    .is_none()
                    .then_some(Block::Smallest),
                Dir::Up => rule
                    .heights
                    .down(rect.h)
                    .is_none()
                    .then_some(Block::Smallest),
            }
        }
        _ => None,
    };
    Some(Outcome {
        id: id.clone(),
        position: u16::try_from(index + 1).unwrap_or(u16::MAX),
        of: u16::try_from(layout.placements.len()).unwrap_or(u16::MAX),
        after: index
            .checked_sub(1)
            .map(|i| layout.placements[i].id.clone()),
        before: layout.placements.get(index + 1).map(|p| p.id.clone()),
        w: rect.w,
        h: rect.h,
        limit,
    })
}
