//! Edits as commands: each is a pure function from a layout to a validated layout.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalogue::{Catalogue, Widget, spec_of};
use crate::check;
use crate::error::WidgetError;
use crate::id::{PlacementId, slug, unique_id};
use crate::layout::{Layout, Placement, Preset, derive_id};
use crate::read::{MAX_NAME, clean_name};
use crate::view::builtin_presets;

/// One change to a layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum Edit<W> {
    /// Adds a widget; its id is derived from the kind ("disk-space-2"). The span defaults to the kind's first, the
    /// position to the end.
    Add {
        /// The widget.
        widget: W,
        /// Its span.
        #[serde(default)]
        #[ts(optional = nullable)]
        span: Option<u8>,
        /// Its position (clamped into range).
        #[serde(default)]
        #[ts(optional = nullable)]
        at: Option<u16>,
    },
    /// Removes a placement (undoable).
    Remove {
        /// The placement.
        id: PlacementId,
    },
    /// Moves a placement to a position (clamped into range).
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
    /// Changes a widget's options; the kind stays.
    Replace {
        /// The placement.
        id: PlacementId,
        /// The new widget, of the same kind.
        widget: W,
    },
    /// Replaces the placements with a built-in or saved preset's (undoable).
    ApplyPreset {
        /// The preset.
        id: PlacementId,
    },
    /// Saves the placements as a new preset of the person's.
    SavePreset {
        /// The name, 1 to 64 characters.
        name: String,
    },
    /// Deletes a saved preset.
    DeletePreset {
        /// The preset.
        id: PlacementId,
    },
    /// Replaces the placements with the catalogue's defaults (undoable).
    ResetToDefaults,
    /// Restores the placements before the last remove, preset or reset.
    Undo,
}

fn position(layout: &[Placement<impl Widget>], id: &PlacementId) -> Result<usize, WidgetError> {
    layout
        .iter()
        .position(|p| &p.id == id)
        .ok_or_else(|| WidgetError::UnknownPlacement(id.to_string()))
}

/// Applies `edit`: the new layout, or why not. The input is untouched.
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
            let placement = Placement {
                id: derive_id(&layout.placements, &widget),
                span: span.unwrap_or(spec.spans[0]),
                widget,
            };
            check::check_one::<C>(&layout.placements, &placement, max)?;
            let index = at.map_or(next.placements.len(), |a| {
                usize::from(a).min(next.placements.len())
            });
            next.placements.insert(index, placement);
            next.previous = None;
        }
        Edit::Remove { id } => {
            let index = position(&layout.placements, &id)?;
            if layout.placements.len() <= s.min_placements as usize {
                return Err(WidgetError::Invalid(format!(
                    "Keep at least {} widget(s) in this layout.",
                    s.min_placements
                )));
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
                .ok_or_else(|| WidgetError::NotAPreset(id.to_string()))?;
            check::check_all::<C>(&found.placements, max)?;
            next.placements = found.placements;
            next.previous = Some(layout.placements.clone());
        }
        Edit::SavePreset { name } => {
            let name = clean_name(&name);
            if name.is_empty() {
                return Err(WidgetError::Invalid(format!(
                    "Give the layout a name of 1 to {MAX_NAME} characters."
                )));
            }
            if layout.presets.len() >= s.max_user_presets as usize {
                return Err(WidgetError::TooMany {
                    what: "saved layouts".into(),
                    max: s.max_user_presets.into(),
                });
            }
            if layout.placements.len() > s.max_preset_placements as usize {
                return Err(WidgetError::TooMany {
                    what: "widgets in a saved layout".into(),
                    max: s.max_preset_placements,
                });
            }
            let builtin = builtin_presets::<C>();
            let id = unique_id(&slug(&name, "layout"), |c| {
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
                .ok_or_else(|| WidgetError::NotAPreset(id.to_string()))?;
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
    Ok(next)
}

/// Checks the edited placement at `index` against the others (a resize or replace must not break a limit).
fn check_edited<C: Catalogue>(
    placements: &[Placement<C::Widget>],
    index: usize,
) -> Result<(), WidgetError> {
    let mut others = placements.to_vec();
    let edited = others.remove(index);
    check::check_one::<C>(&others, &edited, usize::MAX)
}
