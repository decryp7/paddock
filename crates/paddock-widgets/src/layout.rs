//! Placements, presets and the stored layout.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::catalogue::{Catalogue, Widget};
use crate::check;
use crate::error::WidgetError;
use crate::id::{PlacementId, slug, unique_id};

/// One widget on a surface: an id, a span (columns of the grid) and the app's widget value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Placement<W> {
    /// Unique within its layout.
    pub id: PlacementId,
    /// Columns of the surface's grid; one the kind allows.
    pub span: u8,
    /// The widget and its options.
    pub widget: W,
}

/// A named set of placements the person can switch to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Preset<W> {
    /// Unique among the built-in and saved presets of a surface.
    pub id: PlacementId,
    /// The name to show, 1 to 64 characters.
    pub name: String,
    /// The catalogue's own preset: never stored, cannot be deleted.
    #[serde(default)]
    pub builtin: bool,
    /// Its placements.
    pub placements: Vec<Placement<W>>,
}

/// The stored layout. Built-in presets are not stored (they come from the catalogue on every read).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Layout<W> {
    /// The surface's `schema_version` when written.
    pub version: u32,
    /// The placements, in order.
    pub placements: Vec<Placement<W>>,
    /// The person's own presets.
    pub presets: Vec<Preset<W>>,
    /// One level of undo, in memory until the next edit that changes the placements.
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) previous: Option<Vec<Placement<W>>>,
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
        let placements =
            check::keep_valid::<C>(C::defaults(), C::SURFACE.max_placements as usize, &mut 0);
        Layout::new::<C>(placements, Vec::new())
    }

    /// Whether [`crate::Edit::Undo`] has something to restore.
    pub fn can_undo(&self) -> bool {
        self.previous.is_some()
    }

    /// Checks the whole layout against the catalogue and the surface's limits; the first problem is returned.
    pub fn validate<C: Catalogue<Widget = W>>(&self) -> Result<(), WidgetError> {
        let s = &C::SURFACE;
        check::check_all::<C>(&self.placements, s.max_placements as usize)?;
        if self.placements.len() < s.min_placements as usize {
            return Err(WidgetError::Invalid(format!(
                "Keep at least {} widget(s) in this layout.",
                s.min_placements
            )));
        }
        if self.presets.len() > s.max_user_presets as usize {
            return Err(WidgetError::TooMany {
                what: "saved layouts".into(),
                max: s.max_user_presets.into(),
            });
        }
        let builtin = crate::view::builtin_presets::<C>();
        let mut seen: Vec<&PlacementId> = builtin.iter().map(|p| &p.id).collect();
        for p in &self.presets {
            if seen.contains(&&p.id) {
                return Err(WidgetError::DuplicateId(p.id.to_string()));
            }
            seen.push(&p.id);
            check::check_all::<C>(&p.placements, s.max_preset_placements as usize)?;
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
    unique_id(&slug(&name, "widget"), |c| {
        existing.iter().any(|p| p.id.as_str() == c)
    })
}
