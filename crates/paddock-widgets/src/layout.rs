//! Placements, presets and the stored layout.

use serde::{Deserialize, Serialize};

use crate::catalogue::{Catalogue, Widget};
use crate::check;
use crate::error::{Counted, WidgetError};
use crate::id::{PlacementId, PresetId, slug, unique_id};
use crate::place::Place;

ts! {
/// One widget on a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement<W, P = crate::place::Span> {
    /// Unique within its layout.
    pub id: PlacementId,
    /// Where it sits: a span on a flow surface, `x`, `y`, `w`, `h` on a grid surface.
    #[serde(flatten)]
    #[cfg_attr(any(test, feature = "ts"), ts(flatten))]
    pub place: P,
    /// The widget and its options.
    pub widget: W,
}

/// A named layout the person can switch to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset<W, P = crate::place::Span> {
    /// Unique among the built-in and saved presets of a surface.
    pub id: PresetId,
    /// The name to show, 1 to 64 characters.
    pub name: String,
    /// The app's own: rebuilt on every read, cannot be edited or deleted.
    pub builtin: bool,
    /// Its placements.
    pub placements: Vec<Placement<W, P>>,
}

/// The stored layout of one surface. Built-in presets are not stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(serialize = "W: Serialize, P: Serialize", deserialize = "W: Deserialize<'de>, P: Deserialize<'de>"))]
pub struct Layout<W, P = crate::place::Span> {
    /// The surface's `schema_version` when written.
    pub version: u32,
    /// The placements, in order.
    pub placements: Vec<Placement<W, P>>,
    /// The person's own presets only.
    pub presets: Vec<Preset<W, P>>,
    /// Up to [`UNDO_DEPTH`] earlier placement lists, the latest last. In memory only.
    #[serde(skip)]
    #[cfg_attr(any(test, feature = "ts"), ts(skip))]
    pub(crate) undo: Vec<Vec<Placement<W, P>>>,
    /// Placement lists undone and not yet replaced by a new edit, the next to redo last. In memory only.
    #[serde(skip)]
    #[cfg_attr(any(test, feature = "ts"), ts(skip))]
    pub(crate) redo: Vec<Vec<Placement<W, P>>>,
}
}

/// Layouts are equal when version, placements and presets are; the in-memory undo and redo steps are history, not content.
impl<W: PartialEq, P: PartialEq> PartialEq for Layout<W, P> {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.placements == other.placements
            && self.presets == other.presets
    }
}

/// How many edits can be undone.
pub const UNDO_DEPTH: usize = 20;

impl<W, P> Layout<W, P> {
    /// A layout with nothing to undo. Not checked: [`Layout::validate`] does that.
    pub fn new(version: u32, placements: Vec<Placement<W, P>>, presets: Vec<Preset<W, P>>) -> Self {
        Layout {
            version,
            placements,
            presets,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Whether there is an edit to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is an undone edit to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Keeps the current placements as the next undo step (the oldest beyond [`UNDO_DEPTH`] is dropped) and clears redo.
    pub(crate) fn push_undo(&mut self, before: Vec<Placement<W, P>>) {
        self.undo.push(before);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

impl<W: Widget, P: Place> Layout<W, P> {
    /// The catalogue's defaults (invalid defaults are dropped; [`crate::check_catalogue`] reports them).
    pub fn defaults<C: Catalogue<Widget = W, Place = P>>() -> Layout<W, P> {
        let placements = check::keep_valid::<C>(
            C::defaults(),
            C::SURFACE.max_placements as usize,
            Counted::Placements,
            &mut 0,
        );
        Layout::new(C::SURFACE.schema_version, placements, Vec::new())
    }

    /// Checks the whole layout against the catalogue and the surface's limits; the first problem is returned.
    pub fn validate<C: Catalogue<Widget = W, Place = P>>(&self) -> Result<(), WidgetError> {
        let s = &C::SURFACE;
        check::check_all::<C>(
            &self.placements,
            s.max_placements as usize,
            Counted::Placements,
        )?;
        if self.placements.len() < s.min_placements as usize {
            return Err(WidgetError::TooFew {
                min: s.min_placements,
                noun: s.noun.into(),
            });
        }
        if self.presets.len() > s.max_user_presets as usize {
            return Err(WidgetError::TooMany {
                counted: Counted::UserPresets,
                max: s.max_user_presets.into(),
                noun: s.noun.into(),
            });
        }
        let mut seen: Vec<PresetId> = crate::view::builtin_presets::<C>()
            .into_iter()
            .map(|p| p.id)
            .collect();
        for p in &self.presets {
            if seen.contains(&p.id) {
                return Err(WidgetError::DuplicateId(p.id.to_string()));
            }
            seen.push(p.id.clone());
            check::check_all::<C>(
                &p.placements,
                s.max_preset_placements as usize,
                Counted::PresetPlacements,
            )?;
            if p.placements.len() < s.min_placements as usize {
                return Err(WidgetError::TooFew {
                    min: s.min_placements,
                    noun: s.noun.into(),
                });
            }
        }
        Ok(())
    }
}

/// Placements for `(place, widget)` pairs with ids derived from the kinds ("disk-space", "disk-space-2"). For a
/// catalogue's `defaults()` and `presets()`, so the app writes no ids by hand. The place is a [`crate::Span`] or a
/// [`crate::Cells`]; for the common flow case, pass a span with `.into()` or use [`assign_spans`].
pub fn assign_ids<W: Widget, P: Place>(
    items: impl IntoIterator<Item = (P, W)>,
) -> Vec<Placement<W, P>> {
    let mut out: Vec<Placement<W, P>> = Vec::new();
    for (place, widget) in items {
        let id = derive_id(&out, &widget);
        out.push(Placement { id, place, widget });
    }
    out
}

/// [`assign_ids`] for a flow surface, from `(span, widget)` pairs.
pub fn assign_spans<W: Widget>(
    items: impl IntoIterator<Item = (u8, W)>,
) -> Vec<Placement<W, crate::place::Span>> {
    assign_ids(
        items
            .into_iter()
            .map(|(span, w)| (crate::place::Span { span }, w)),
    )
}

/// A free id for `widget` among `existing`, from its kind's wire name.
pub(crate) fn derive_id<W: Widget, P>(existing: &[Placement<W, P>], widget: &W) -> PlacementId {
    let name = serde_json::to_value(widget.kind())
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    PlacementId(
        unique_id(&slug(&name, "widget"), |c| {
            existing.iter().any(|p| p.id.as_str() == c)
        })
        .into(),
    )
}

/// A free preset id for `name` among `taken`.
pub(crate) fn derive_preset_id(name: &str, taken: impl Fn(&str) -> bool) -> PresetId {
    PresetId(unique_id(&slug(name, "layout"), taken).into())
}
