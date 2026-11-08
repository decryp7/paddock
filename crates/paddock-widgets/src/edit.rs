//! Edits as commands: each is a pure function from a layout to a validated layout.

use serde::{Deserialize, Serialize};

use crate::catalogue::{Catalogue, Widget, spec_of};
use crate::check;
use crate::error::{Counted, WidgetError};
use crate::grid::{self, GridError, GridGesture, Item};
use crate::id::{PlacementId, PresetId};
use crate::layout::{Layout, Placement, Preset, derive_id, derive_preset_id};
use crate::place::Place;
use crate::read::{MAX_NAME, clean_name};
use crate::view::builtin_presets;

ts! {
/// A change to a layout. Each is applied by the engine and validated as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Edit<W> {
    /// Adds a widget (its id derived from its kind and unique, e.g. `disk-space-2`) at `at`, or at the end. On a grid
    /// surface it takes its default size at the first free place and `span` and `at` are not used.
    Add {
        /// The widget.
        widget: W,
        /// Its span; the kind's first by default.
        span: Option<u8>,
        /// Its position (clamped into range).
        at: Option<u16>,
    },
    /// Removes a placement (undoable).
    Remove {
        /// The placement.
        id: PlacementId,
    },
    /// Moves a placement to index `to` (clamped to the list; flow surfaces).
    Move {
        /// The placement.
        id: PlacementId,
        /// The new position.
        to: u16,
    },
    /// Changes a placement's span (flow surfaces).
    Resize {
        /// The placement.
        id: PlacementId,
        /// The new span.
        span: u8,
    },
    /// Changes a placement's options; the kind stays.
    Replace {
        /// The placement.
        id: PlacementId,
        /// The new widget, of the same kind.
        widget: W,
    },
    /// Replaces the placements with a preset's (undoable).
    ApplyPreset {
        /// The preset.
        id: PresetId,
    },
    /// Saves the placements as a new user preset.
    SavePreset {
        /// The name, 1 to 64 characters.
        name: String,
    },
    /// Deletes a user preset.
    DeletePreset {
        /// The preset.
        id: PresetId,
    },
    /// Back to the catalogue's defaults (undoable).
    ResetToDefaults,
    /// Arranges a placement on a grid surface: a drop, a resize or a key (ADR 0044 §1a).
    Arrange {
        /// The gesture.
        gesture: GridGesture,
    },
    /// Restores the placements before the last edit (up to 20 steps).
    Undo,
    /// Applies the last undone edit again.
    Redo,
}
}

fn position<W: Widget, P>(
    layout: &[Placement<W, P>],
    id: &PlacementId,
) -> Result<usize, WidgetError> {
    layout
        .iter()
        .position(|p| &p.id == id)
        .ok_or_else(|| WidgetError::UnknownPlacement(id.to_string()))
}

/// The placements as grid items, or `None` on a flow surface.
fn items<C: Catalogue>(placements: &[Placement<C::Widget, C::Place>]) -> Option<Vec<Item>> {
    placements
        .iter()
        .map(|p| {
            let rect = p.place.rect()?;
            let spec = spec_of::<C>(p.widget.kind())?;
            let rule = <C::Place as Place>::size_rule(&spec.size)?;
            Some(Item {
                id: p.id.clone(),
                rect,
                rule,
            })
        })
        .collect()
}

/// Writes the arranged items back: the same placements, in the items' order, with their new rectangles.
fn write_back<C: Catalogue>(
    placements: &mut Vec<Placement<C::Widget, C::Place>>,
    arranged: &[Item],
) -> Result<(), WidgetError> {
    let mut out = Vec::with_capacity(arranged.len());
    for item in arranged {
        let mut p = placements
            .iter()
            .find(|p| p.id == item.id)
            .cloned()
            .ok_or_else(|| WidgetError::UnknownPlacement(item.id.to_string()))?;
        p.place = <C::Place as Place>::from_rect(item.rect)
            .ok_or_else(|| WidgetError::Invalid("This surface has no grid.".into()))?;
        out.push(p);
    }
    *placements = out;
    Ok(())
}

fn grid_error(e: GridError) -> WidgetError {
    match e {
        GridError::Blocked(b) => WidgetError::Blocked(b),
        e @ (GridError::Unknown | GridError::NoRoom | GridError::Invalid) => {
            WidgetError::Invalid(e.to_string())
        }
    }
}

/// Gravity on a grid surface; flow surfaces keep their order. The result is in reading order.
fn settle<C: Catalogue>(
    placements: &mut Vec<Placement<C::Widget, C::Place>>,
) -> Result<(), WidgetError> {
    if let Some(items) = items::<C>(placements) {
        let compact = grid::compact(items).map_err(grid_error)?;
        write_back::<C>(placements, &compact)?;
    }
    Ok(())
}

/// Applies `edit`: the new layout, or why not. The input is untouched, and the result is checked as a whole
/// ([`Layout::validate`]) after every edit, so no edit leaves a layout that breaks a limit.
///
/// Undo: every edit that changes the placements keeps the old ones as an undo step (at most
/// [`crate::UNDO_DEPTH`]) and clears redo; `SavePreset` and `DeletePreset` leave the placements and the steps alone.
/// `Undo` and `Redo` move the placements between the two stacks. On a grid surface the placements are in reading order
/// after every edit.
pub fn apply<C: Catalogue>(
    layout: &Layout<C::Widget, C::Place>,
    edit: Edit<C::Widget>,
) -> Result<Layout<C::Widget, C::Place>, WidgetError> {
    let s = &C::SURFACE;
    let mut next = layout.clone();
    let max = s.max_placements as usize;
    let before = layout.placements.clone();
    // Whether the edit changed the placements and so is an undo step.
    let mut step = true;
    match edit {
        Edit::Add { widget, span, at } => {
            let widget = widget.clamped();
            let Some(spec) = spec_of::<C>(widget.kind()) else {
                return Err(WidgetError::Invalid(
                    "This widget is not available here.".into(),
                ));
            };
            let initial = <C::Place as Place>::initial(&spec.size).ok_or_else(|| {
                WidgetError::Invalid("This widget has no size to be placed with.".into())
            })?;
            let place = match (span, initial.span()) {
                (Some(span), Some(_)) => initial.with_span(span).unwrap_or(initial),
                (Some(_), None) => {
                    return Err(WidgetError::Invalid(
                        "A grid widget has a size, not a span.".into(),
                    ));
                }
                (None, _) => initial,
            };
            let mut placement = Placement {
                id: derive_id(&layout.placements, &widget),
                place,
                widget,
            };
            if let Some(rect) = placement.place.rect() {
                let taken = items::<C>(&layout.placements).unwrap_or_default();
                let free = grid::first_free(&taken, (0, 0), rect.w, rect.h, s.columns)
                    .map_err(grid_error)?;
                placement.place = <C::Place as Place>::from_rect(free).unwrap_or(placement.place);
            }
            check::check_one::<C>(&layout.placements, &placement, max, Counted::Placements)?;
            let index = at.map_or(next.placements.len(), |a| {
                usize::from(a).min(next.placements.len())
            });
            next.placements.insert(index, placement);
            settle::<C>(&mut next.placements)?;
        }
        Edit::Remove { id } => {
            let index = position(&layout.placements, &id)?;
            if layout.placements.len() <= s.min_placements as usize {
                return Err(WidgetError::TooFew {
                    min: s.min_placements,
                    noun: s.noun.into(),
                });
            }
            next.placements.remove(index);
            settle::<C>(&mut next.placements)?;
        }
        Edit::Move { id, to } => {
            let from = position(&layout.placements, &id)?;
            if layout.placements[from].place.span().is_none() {
                return Err(WidgetError::Invalid(
                    "Arrange widgets on this surface by dragging.".into(),
                ));
            }
            let placement = next.placements.remove(from);
            let to = usize::from(to).min(next.placements.len());
            next.placements.insert(to, placement);
        }
        Edit::Resize { id, span } => {
            let index = position(&layout.placements, &id)?;
            let Some(place) = layout.placements[index].place.with_span(span) else {
                return Err(WidgetError::Invalid(
                    "Resize widgets on this surface by dragging.".into(),
                ));
            };
            next.placements[index].place = place;
            check_edited::<C>(&next.placements, index)?;
        }
        Edit::Replace { id, widget } => {
            let index = position(&layout.placements, &id)?;
            let widget = widget.clamped();
            if widget.kind() != layout.placements[index].widget.kind() {
                return Err(WidgetError::Invalid(
                    "A widget keeps its kind; remove it and add another to change the kind.".into(),
                ));
            }
            next.placements[index].widget = widget;
            check_edited::<C>(&next.placements, index)?;
        }
        Edit::Arrange { gesture } => {
            let items = items::<C>(&layout.placements)
                .ok_or_else(|| WidgetError::Invalid("This surface has no grid.".into()))?;
            position(&layout.placements, gesture.id())?;
            let arranged = grid::arrange(&items, s.columns, &gesture).map_err(grid_error)?;
            write_back::<C>(&mut next.placements, &arranged)?;
        }
        Edit::ApplyPreset { id } => {
            let found = builtin_presets::<C>()
                .into_iter()
                .chain(layout.presets.iter().cloned())
                .find(|p| p.id == id)
                .ok_or_else(|| WidgetError::UnknownPreset(id.to_string()))?;
            check::check_all::<C>(&found.placements, max, Counted::Placements)?;
            next.placements = found.placements;
            settle::<C>(&mut next.placements)?;
        }
        Edit::SavePreset { name } => {
            step = false;
            let name = clean_name(&name);
            if name.is_empty() {
                return Err(WidgetError::Invalid(format!(
                    "Give the preset a name of 1 to {MAX_NAME} characters."
                )));
            }
            if layout.presets.len() >= s.max_user_presets as usize {
                return Err(WidgetError::TooMany {
                    counted: Counted::UserPresets,
                    max: s.max_user_presets.into(),
                    noun: s.noun.into(),
                });
            }
            if layout.placements.len() > s.max_preset_placements as usize {
                return Err(WidgetError::TooMany {
                    counted: Counted::PresetPlacements,
                    max: s.max_preset_placements,
                    noun: s.noun.into(),
                });
            }
            let builtin = builtin_presets::<C>();
            let id = derive_preset_id(&name, |c| {
                builtin
                    .iter()
                    .chain(&layout.presets)
                    .any(|p| p.id.as_str() == c)
            });
            next.presets.push(Preset {
                id,
                name,
                builtin: false,
                placements: layout.placements.clone(),
            });
        }
        Edit::DeletePreset { id } => {
            step = false;
            if builtin_presets::<C>().iter().any(|p| p.id == id) {
                return Err(WidgetError::BuiltinPreset);
            }
            let index = layout
                .presets
                .iter()
                .position(|p| p.id == id)
                .ok_or_else(|| WidgetError::UnknownPreset(id.to_string()))?;
            next.presets.remove(index);
        }
        Edit::ResetToDefaults => {
            next.placements = Layout::defaults::<C>().placements;
            settle::<C>(&mut next.placements)?;
        }
        Edit::Undo => {
            step = false;
            next.placements = next.undo.pop().ok_or(WidgetError::NothingToUndo)?;
            next.redo.push(before.clone());
        }
        Edit::Redo => {
            step = false;
            next.placements = next.redo.pop().ok_or(WidgetError::NothingToRedo)?;
            next.undo.push(before.clone());
        }
    }
    if step && next.placements != before {
        next.push_undo(before);
    }
    next.validate::<C>()?;
    Ok(next)
}

/// Checks the edited placement at `index` against the others (a resize or replace must not break a limit).
fn check_edited<C: Catalogue>(
    placements: &[Placement<C::Widget, C::Place>],
    index: usize,
) -> Result<(), WidgetError> {
    let mut others = placements.to_vec();
    let edited = others.remove(index);
    check::check_one::<C>(&others, &edited, usize::MAX, Counted::Placements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Block;

    #[test]
    fn every_grid_error_has_a_message_for_the_person() {
        assert_eq!(
            grid_error(GridError::Blocked(Block::Largest)),
            WidgetError::Blocked(Block::Largest)
        );
        for e in [GridError::Unknown, GridError::NoRoom, GridError::Invalid] {
            assert_eq!(grid_error(e), WidgetError::Invalid(e.to_string()));
        }
    }
}
