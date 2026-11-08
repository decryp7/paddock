//! The closed catalogue an app implements for each surface: its widget type, kinds, limits, defaults and presets.

use std::fmt::Debug;
use std::hash::Hash;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::Counted;
use crate::layout::{Placement, Preset};
use crate::place::Place;

/// The grid and limits of the surface a catalogue fills (ADR 0035 §6). Its identity is the app's `SurfaceId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    /// What the surface is called in messages: "popup", "dashboard", "menu bar".
    pub noun: &'static str,
    /// Bumped by the app on a breaking change of its widget type; a newer stored layout is kept unread (ADR 0035 §1).
    pub schema_version: u32,
    /// Columns of the grid: popup 2, dashboard 12, menu bar 1.
    pub columns: u8,
    /// Most placements.
    pub max_placements: u16,
    /// Fewest placements; 0 allows an empty surface, which the UI designs.
    pub min_placements: u16,
    /// Most user presets (0: the surface has none).
    pub max_user_presets: u8,
    /// Most placements in one user preset.
    pub max_preset_placements: u16,
    /// Largest serialised layout read, in bytes.
    pub max_bytes: usize,
}

ts! {
/// How a kind gets fresh values; drives the engine's refresh and its stale marking, never a UI timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Refresh {
    /// Every live tick (the app's sampler).
    Live,
    /// Every `secs` seconds, from the engine's cache.
    Every {
        /// The period.
        secs: u16,
    },
    /// When the source reports a change.
    OnChange,
}
}

/// One catalogue entry, as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindSpec<K: 'static, R: 'static = &'static [u8]> {
    /// The kind.
    pub kind: K,
    /// Sentence case: "CPU and memory".
    pub title: &'static str,
    /// One line for the chooser: "Load and use now, with the last minute".
    pub summary: &'static str,
    /// The app's registered icon name.
    pub icon: &'static str,
    /// The sizes it allows: spans for a flow surface (columns, the first is the default, never empty), a
    /// [`crate::SizeRule`] for a grid surface.
    pub size: R,
    /// How many may be placed at once (1 for most).
    pub max_instances: u8,
    /// How it refreshes.
    pub refresh: Refresh,
}

/// The app's widget value: a closed enum whose variants carry the widget's options.
pub trait Widget: Clone + Debug + PartialEq + Serialize + DeserializeOwned + 'static {
    /// The widget's kind, a fieldless enum serialised as a string.
    type Kind: Copy + Eq + Hash + Debug + Serialize + DeserializeOwned + 'static;

    /// The widget's kind.
    fn kind(&self) -> Self::Kind;

    /// Whether the widget's own options are valid; the message says what to change.
    fn validate(&self) -> Result<(), String>;

    /// Ranges clamped and text trimmed (a valid widget is also an in-range one).
    fn clamped(self) -> Self {
        self
    }
}

/// An app's catalogue for one surface.
pub trait Catalogue: 'static {
    /// The app's surfaces (Dyno: dashboard, menu bar, popover), one enum per app.
    type SurfaceId: Copy + Eq + Hash + Debug + Serialize + DeserializeOwned + 'static;

    /// The widget type.
    type Widget: Widget;

    /// How placements sit on this surface: [`crate::Span`] (flow) or [`crate::Cells`] (grid).
    type Place: Place;

    /// Widths of the surface's container, from which the arrangement has fewer columns than the design one (grid
    /// surfaces only; widest first). Empty: the design arrangement at every width.
    const BREAKPOINTS: &'static [Breakpoint] = &[];

    /// This catalogue's surface.
    const SURFACE_ID: Self::SurfaceId;

    /// Its grid and limits.
    const SURFACE: Surface;

    /// Every kind, in the order the chooser lists them.
    fn kinds() -> &'static [Kind<Self>];

    /// Brings a stored layout of an older `version` (below the surface's `schema_version`) up to the current one, as
    /// JSON, before it is read; a grid surface uses [`crate::spans_to_cells`]. The default returns the value as it is
    /// (read leniently, as before). An `Err` takes the damaged path: the defaults are shown and the old value is kept
    /// until the first edit.
    fn upgrade(_from: u32, value: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(value)
    }

    /// The layout of a new or reset surface.
    fn defaults() -> Vec<Placement<Self::Widget, Self::Place>>;

    /// The built-in presets (`builtin` is set on read, whatever they say).
    fn presets() -> Vec<Preset<Self::Widget, Self::Place>> {
        Vec::new()
    }
}

/// A catalogue entry of `C`.
pub type Kind<C> =
    KindSpec<<<C as Catalogue>::Widget as Widget>::Kind, <<C as Catalogue>::Place as Place>::Rule>;

/// The arrangement of a grid surface from a container width: from `min` px wide, `columns` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breakpoint {
    /// The least container width, in px, for this arrangement.
    pub min: u16,
    /// Its columns; the first (widest) breakpoint is the design arrangement and has the surface's columns.
    pub columns: u8,
}

/// The catalogue entry of `kind`.
pub(crate) fn spec_of<C: Catalogue>(kind: <C::Widget as Widget>::Kind) -> Option<&'static Kind<C>> {
    C::kinds().iter().find(|s| s.kind == kind)
}

/// Checks a catalogue once, in the app's own test: spans in the grid, titles filled, defaults and presets valid. The
/// message names the first problem. Reading and editing drop what is invalid, so a broken catalogue would otherwise
/// show only as missing widgets.
pub fn check_catalogue<C: Catalogue>() -> Result<(), String> {
    let s = &C::SURFACE;
    if s.noun.is_empty() {
        return Err("the surface needs a noun".into());
    }
    if s.schema_version == 0 {
        return Err("the schema version starts at 1".into());
    }
    if s.columns == 0 || s.max_bytes == 0 {
        return Err("the surface needs columns and a size limit".into());
    }
    if s.min_placements > s.max_placements {
        return Err("the fewest placements is above the most".into());
    }
    if s.max_preset_placements > s.max_placements {
        return Err("max_preset_placements is above max_placements".into());
    }
    if let Some(first) = C::BREAKPOINTS.first() {
        let ok = first.columns == s.columns
            && C::BREAKPOINTS
                .windows(2)
                .all(|w| w[0].min > w[1].min && w[0].columns >= w[1].columns)
            && C::BREAKPOINTS
                .last()
                .is_some_and(|l| l.min == 0 && l.columns >= 1);
        if !ok {
            return Err("breakpoints go from the widest (the surface's columns) to min 0, with fewer columns each".into());
        }
    }
    if C::kinds().is_empty() {
        return Err("the catalogue has no kinds".into());
    }
    for (i, k) in C::kinds().iter().enumerate() {
        if C::kinds()[..i].iter().any(|o| o.kind == k.kind) {
            return Err(format!("kind {:?} is listed twice", k.kind));
        }
        if k.title.is_empty() || k.summary.is_empty() || k.icon.is_empty() {
            return Err(format!(
                "kind {:?} needs a title, a summary and an icon",
                k.kind
            ));
        }
        if !<C::Place as Place>::rule_valid(&k.size, s.columns) {
            return Err(format!(
                "kind {:?} needs a size rule inside the {} columns (spans from 1; a grid kind's ranges not empty, its default inside)",
                k.kind, s.columns
            ));
        }
        if k.max_instances == 0 {
            return Err(format!(
                "kind {:?} can never be placed (max_instances is 0)",
                k.kind
            ));
        }
    }
    let defaults = C::defaults();
    let kept = crate::check::keep_valid::<C>(
        defaults.clone(),
        s.max_placements as usize,
        Counted::Placements,
        &mut 0,
    );
    if kept != defaults || defaults.len() < s.min_placements as usize {
        return Err("the defaults are not a valid layout".into());
    }
    let presets = C::presets();
    for (i, p) in presets.iter().enumerate() {
        if presets[..i].iter().any(|o| o.id == p.id) {
            return Err(format!("preset {} is listed twice", p.id));
        }
        if p.name.trim().is_empty() {
            return Err(format!("preset {} needs a name", p.id));
        }
        let kept = crate::check::keep_valid::<C>(
            p.placements.clone(),
            s.max_preset_placements as usize,
            Counted::PresetPlacements,
            &mut 0,
        );
        if kept != p.placements || p.placements.len() < s.min_placements as usize {
            return Err(format!("preset {} is not a valid layout", p.id));
        }
    }
    Ok(())
}
