//! The closed catalogue an app implements for each surface: its widget type, kinds, limits, defaults and presets.

use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;
use serde::de::DeserializeOwned;
use ts_rs::TS;

use crate::layout::{Placement, Preset};

/// The surface a catalogue fills: its grid and its limits (ADR 0035 §1 and §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    /// The surface's name on the wire: "popover", "dashboard", "menuBar".
    pub id: &'static str,
    /// Bumped by the app on a breaking change of its widget type; a layout of a higher version is not read.
    pub schema_version: u32,
    /// Columns of the surface's grid (popover 2, dashboard 12, menu bar 1).
    pub columns: u8,
    /// Most placements.
    pub max_placements: u16,
    /// Fewest placements (0: an empty layout is allowed and designed).
    pub min_placements: u16,
    /// Most placements saved by the user as presets (0: the surface has none).
    pub max_user_presets: u8,
    /// Most placements in one preset.
    pub max_preset_placements: u16,
    /// The largest serialised layout read.
    pub max_bytes: usize,
}

/// How a kind gets fresh values (drives the engine's refresh and the stale flag; never a UI timer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum Refresh {
    /// With the live tick.
    Live,
    /// Every `secs` seconds.
    Every {
        /// The period.
        secs: u16,
    },
    /// When the underlying state changes.
    OnChange,
}

/// One catalogue entry, as data.
#[derive(Debug, Clone, Copy)]
pub struct KindSpec<K: 'static> {
    /// The kind.
    pub kind: K,
    /// Sentence case: "CPU and memory".
    pub title: &'static str,
    /// One line for the chooser: "Load and use now, with the last minute".
    pub summary: &'static str,
    /// The app's registered icon name.
    pub icon: &'static str,
    /// Allowed spans (columns of the grid), the first is the default. Never empty.
    pub spans: &'static [u8],
    /// How many may be placed at once (1 for most).
    pub max_instances: u8,
    /// How it refreshes.
    pub refresh: Refresh,
}

/// The app's widget value: a closed enum whose variants carry the widget's options.
pub trait Widget: Clone + Debug + PartialEq + Serialize + DeserializeOwned + TS {
    /// The widget's kind, a fieldless enum serialised as a string.
    type Kind: Copy + Eq + Hash + Debug + Serialize + DeserializeOwned + TS + 'static;

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
    /// The widget type.
    type Widget: Widget;

    /// The surface's grid and limits.
    const SURFACE: Surface;

    /// Every kind, in the order the chooser lists them.
    fn kinds() -> &'static [KindSpec<<Self::Widget as Widget>::Kind>];

    /// The layout of a new or reset surface.
    fn defaults() -> Vec<Placement<Self::Widget>>;

    /// The built-in presets (`builtin` is set on read, whatever they say).
    fn presets() -> Vec<Preset<Self::Widget>> {
        Vec::new()
    }
}

/// The catalogue entry of `kind`.
pub(crate) fn spec_of<C: Catalogue>(
    kind: <C::Widget as Widget>::Kind,
) -> Option<&'static KindSpec<<C::Widget as Widget>::Kind>> {
    C::kinds().iter().find(|s| s.kind == kind)
}

/// Checks a catalogue once, in the app's own test: spans in the grid, titles filled, defaults and presets valid. The
/// message names the first problem. Reading and editing drop what is invalid, so a broken catalogue would otherwise
/// show only as missing widgets.
pub fn check_catalogue<C: Catalogue>() -> Result<(), String> {
    let s = &C::SURFACE;
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
        if k.spans.is_empty() || k.spans.iter().any(|&sp| sp == 0 || sp > s.columns) {
            return Err(format!(
                "kind {:?} needs spans between 1 and {}",
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
    let kept = crate::check::keep_valid::<C>(defaults.clone(), s.max_placements as usize, &mut 0);
    if kept != defaults || defaults.len() < s.min_placements as usize {
        return Err("the defaults are not a valid layout".into());
    }
    for p in C::presets() {
        let kept = crate::check::keep_valid::<C>(
            p.placements.clone(),
            s.max_preset_placements as usize,
            &mut 0,
        );
        if kept != p.placements {
            return Err(format!("preset {} is not a valid layout", p.id));
        }
    }
    Ok(())
}
