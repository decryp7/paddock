//! Placements, presets and the stored layout.

use serde::{Deserialize, Serialize};

use crate::catalogue::{Catalogue, Widget};
use crate::check;
use crate::error::{Counted, WidgetError};
use crate::id::{PlacementId, PresetId, slug, unique_id};

ts! {
/// One widget on a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement<W> {
    /// Unique within its layout.
    pub id: PlacementId,
    /// Columns it spans; one of its kind's spans.
    pub span: u8,
    /// The widget and its options.
    pub widget: W,
}

/// A named layout the person can switch to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset<W> {
    /// Unique among the built-in and saved presets of a surface.
    pub id: PresetId,
    /// The name to show, 1 to 64 characters.
    pub name: String,
    /// The app's own: rebuilt on every read, cannot be edited or deleted.
    pub builtin: bool,
    /// Its placements.
    pub placements: Vec<Placement<W>>,
}

/// The stored layout of one surface. Built-in presets are not stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(serialize = "W: Serialize", deserialize = "W: Deserialize<'de>"))]
pub struct Layout<W> {
    /// The surface's `schema_version` when written.
    pub version: u32,
    /// The placements, in order.
    pub placements: Vec<Placement<W>>,
    /// The person's own presets only.
    pub presets: Vec<Preset<W>>,
    /// One level of undo: the placements before the last remove, preset or reset. In memory only; a placement edit
    /// clears it.
    #[serde(skip)]
    #[cfg_attr(any(test, feature = "ts"), ts(skip))]
    pub previous: Option<Vec<Placement<W>>>,
}
}

impl<W: Widget> Layout<W> {
    /// A layout of this surface's version with no undo level. Not checked: [`Layout::validate`] does that.
    pub fn new<C: Catalogue<Widget = W>>(
        placements: Vec<Placement<W>>,
        presets: Vec<Preset<W>>,
    ) -> Layout<W> {
        Layout {
            version: C::SURFACE.schema_version,
            placements,
            presets,
            previous: None,
        }
    }

    /// The catalogue's defaults (invalid defaults are dropped; [`crate::check_catalogue`] reports them).
    pub fn defaults<C: Catalogue<Widget = W>>() -> Layout<W> {
        let placements = check::keep_valid::<C>(
            C::defaults(),
            C::SURFACE.max_placements as usize,
            Counted::Placements,
            &mut 0,
        );
        Layout::new::<C>(placements, Vec::new())
    }

    /// Whether [`crate::Edit::Undo`] has something to restore.
    pub fn can_undo(&self) -> bool {
        self.previous.is_some()
    }

    /// Checks the whole layout against the catalogue and the surface's limits; the first problem is returned.
    pub fn validate<C: Catalogue<Widget = W>>(&self) -> Result<(), WidgetError> {
        let s = &C::SURFACE;
        check::check_all::<C>(
            &self.placements,
            s.max_placements as usize,
            Counted::Placements,
        )?;
        if self.placements.len() < s.min_placements as usize {
            return Err(WidgetError::TooFew {
                min: s.min_placements,
            });
        }
        if self.presets.len() > s.max_user_presets as usize {
            return Err(WidgetError::TooMany {
                counted: Counted::UserPresets,
                max: s.max_user_presets.into(),
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
        }
        Ok(())
    }
}

/// Placements for `(span, widget)` pairs with ids derived from the kinds ("disk-space", "disk-space-2"). For a
/// catalogue's `defaults()` and `presets()`, so the app writes no ids by hand.
pub fn assign_ids<W: Widget>(items: impl IntoIterator<Item = (u8, W)>) -> Vec<Placement<W>> {
    let mut out: Vec<Placement<W>> = Vec::new();
    for (span, widget) in items {
        let id = derive_id(&out, &widget);
        out.push(Placement { id, span, widget });
    }
    out
}

/// A free id for `widget` among `existing`, from its kind's wire name.
pub(crate) fn derive_id<W: Widget>(existing: &[Placement<W>], widget: &W) -> PlacementId {
    let name = serde_json::to_value(widget.kind())
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    PlacementId(unique_id(&slug(&name, "widget"), |c| {
        existing.iter().any(|p| p.id.as_str() == c)
    }))
}

/// A free preset id for `name` among `taken`.
pub(crate) fn derive_preset_id(name: &str, taken: impl Fn(&str) -> bool) -> PresetId {
    PresetId(unique_id(&slug(name, "layout"), taken))
}
