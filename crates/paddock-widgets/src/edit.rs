//! Edits as commands: each is a pure function from a layout to a validated layout.

use serde::{Deserialize, Serialize};

use crate::catalogue::{Catalogue, Widget, spec_of};
use crate::check;
use crate::error::{Counted, WidgetError};
use crate::id::{PlacementId, PresetId};
use crate::layout::{Layout, Placement, Preset, derive_id, derive_preset_id};
use crate::read::{MAX_NAME, clean_name};
use crate::view::builtin_presets;

ts! {
/// A change to a layout. Each is applied by the engine and validated as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Edit<W> {
    /// Adds a widget (its id derived from its kind and unique, e.g. `disk-space-2`) at `at`, or at the end.
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
    /// Moves a placement to index `to` (clamped to the list).
    Move {
        /// The placement.
        id: PlacementId,
        /// The new position.
        to: u16,
    },
    /// Changes a placement's span.
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
    /// Restores the placements before the last undoable edit.
    Undo,
}
}

fn position(layout: &[Placement<impl Widget>], id: &PlacementId) -> Result<usize, WidgetError> {
    layout
        .iter()
        .position(|p| &p.id == id)
        .ok_or_else(|| WidgetError::UnknownPlacement(id.to_string()))
}

/// Applies `edit`: the new layout, or why not. The input is untouched, and the result is checked as a whole
/// ([`Layout::validate`]) after every edit, so no edit leaves a layout that breaks a limit.
///
/// Undo: an edit that changes the placements keeps the old ones as the single undo level when it is destructive
/// (`Remove`, `ApplyPreset`, `ResetToDefaults`) and clears the level otherwise; `SavePreset` and `DeletePreset` leave
/// the placements, and the level, alone. `Undo` restores the level and clears it.
pub fn apply<C: Catalogue>(
    layout: &Layout<C::Widget>,
    edit: Edit<C::Widget>,
) -> Result<Layout<C::Widget>, WidgetError> {
    let s = &C::SURFACE;
    let mut next = layout.clone();
    let max = s.max_placements as usize;
    match edit {
        Edit::Add { widget, span, at } => {
            let widget = widget.clamped();
            let Some(spec) = spec_of::<C>(widget.kind()) else {
                return Err(WidgetError::Invalid(
                    "This widget is not available here.".into(),
                ));
            };
            let Some(span) = span.or_else(|| spec.spans.first().copied()) else {
                return Err(WidgetError::Invalid(
                    "This widget has no width to be placed with.".into(),
                ));
            };
            let placement = Placement {
                id: derive_id(&layout.placements, &widget),
                span,
                widget,
            };
            check::check_one::<C>(&layout.placements, &placement, max, Counted::Placements)?;
            let index = at.map_or(next.placements.len(), |a| {
                usize::from(a).min(next.placements.len())
            });
            next.placements.insert(index, placement);
            next.previous = None;
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
            next.previous = Some(layout.placements.clone());
        }
        Edit::Move { id, to } => {
            let from = position(&layout.placements, &id)?;
            let placement = next.placements.remove(from);
            let to = usize::from(to).min(next.placements.len());
            next.placements.insert(to, placement);
            next.previous = None;
        }
        Edit::Resize { id, span } => {
            let index = position(&layout.placements, &id)?;
            next.placements[index].span = span;
            check_edited::<C>(&next.placements, index)?;
            next.previous = None;
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
            next.previous = None;
        }
        Edit::ApplyPreset { id } => {
            let found = builtin_presets::<C>()
                .into_iter()
                .chain(layout.presets.iter().cloned())
                .find(|p| p.id == id)
                .ok_or_else(|| WidgetError::UnknownPreset(id.to_string()))?;
            check::check_all::<C>(&found.placements, max, Counted::Placements)?;
            next.placements = found.placements;
            next.previous = Some(layout.placements.clone());
        }
        Edit::SavePreset { name } => {
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
            next.previous = Some(layout.placements.clone());
        }
        Edit::Undo => {
            next.placements = next.previous.take().ok_or(WidgetError::NothingToUndo)?;
        }
    }
    next.validate::<C>()?;
    Ok(next)
}

/// Checks the edited placement at `index` against the others (a resize or replace must not break a limit).
fn check_edited<C: Catalogue>(
    placements: &[Placement<C::Widget>],
    index: usize,
) -> Result<(), WidgetError> {
    let mut others = placements.to_vec();
    let edited = others.remove(index);
    check::check_one::<C>(&others, &edited, usize::MAX, Counted::Placements)
}
