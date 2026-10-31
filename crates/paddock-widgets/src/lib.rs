//! Widget layouts for the Paddock apps (family ADR 0035): what a surface (a tray popup, a dashboard grid, a menu bar)
//! shows, in which order and how wide, as data the engine stores and validates and the UI only draws.
//!
//! **Type skeleton.** This file fixes the public types and traits of ADR 0035 §2 so that the shared UI (WP-B) and Dyno
//! (WP-C1) can build against them. The behaviour comes with WP-A, with these signatures:
//!
//! ```text
//! pub fn read<C: Catalogue>(value: &serde_json::Value) -> Read<C>;              // never fails; notes say what was wrong
//! pub fn apply<C: Catalogue>(layout: &Layout<C::Widget>, edit: Edit<C::Widget>) -> Result<Layout<C::Widget>, WidgetError>;
//! pub fn info<C: Catalogue>() -> CatalogueInfo<C::SurfaceId, <C::Widget as Widget>::Kind>;
//! pub fn view<C: Catalogue>(stored: &Stored<C>) -> LayoutView<C::SurfaceId, C::Widget, <C::Widget as Widget>::Kind>;
//! impl<C: Catalogue> Serialize / Deserialize for Stored<C>  // Deserialize goes through read::<C> and never errors
//! ```
//!
//! Pure: no I/O, no Tauri, no app types. Domain rules (what a widget shows, its thresholds) stay in the app.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::hash::Hash;

/// Longest placement id, preset id and preset name, in characters.
pub const MAX_ID: usize = 64;

macro_rules! ts {
    ($($item:item)*) => { $( #[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))] $item )* };
}

ts! {
/// A placement's id: 1 to [`MAX_ID`] characters of `[a-z0-9-]`, unique within its layout. Parsed once at the edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(any(test, feature = "ts"), ts(type = "string"))]
pub struct PlacementId(String);

/// A user preset's id, same rules as [`PlacementId`]; never the id of a built-in preset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(any(test, feature = "ts"), ts(type = "string"))]
pub struct PresetId(String);
}

/// Why a string is not an id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("An id is 1 to {MAX_ID} characters of a-z, 0-9 and -.")]
pub struct BadId;

fn check_id(s: &str) -> Result<(), BadId> {
    let ok = !s.is_empty()
        && s.chars().count() <= MAX_ID
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok { Ok(()) } else { Err(BadId) }
}

macro_rules! id_conversions {
    ($t:ident) => {
        impl TryFrom<String> for $t {
            type Error = BadId;
            fn try_from(s: String) -> Result<Self, BadId> {
                check_id(&s).map(|()| $t(s))
            }
        }
        impl From<$t> for String {
            fn from(id: $t) -> String {
                id.0
            }
        }
        impl Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                $t::try_from(String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
        impl $t {
            /// The id as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
id_conversions!(PlacementId);
id_conversions!(PresetId);

/// The grid and limits of the surface a catalogue fills (ADR 0035 §6). Its identity is the app's `SurfaceId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
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
    Every { secs: u16 },
    /// When the source reports a change.
    OnChange,
}
}

/// One catalogue entry, as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindSpec<K: 'static> {
    /// The kind.
    pub kind: K,
    /// Sentence case, e.g. "CPU and memory".
    pub title: &'static str,
    /// One line for the chooser.
    pub summary: &'static str,
    /// The app's registered icon name.
    pub icon: &'static str,
    /// Allowed spans in columns; the first is the default.
    pub spans: &'static [u8],
    /// How many placements of this kind a layout may hold.
    pub max_instances: u8,
    /// How it refreshes.
    pub refresh: Refresh,
}

/// The app's widget value: a closed enum whose variants carry the widget's options.
pub trait Widget: Clone + PartialEq + Debug + Serialize + DeserializeOwned + 'static {
    /// The kind of a widget, without its options.
    type Kind: Copy + Eq + Hash + Debug + Serialize + DeserializeOwned + 'static;
    /// Its kind.
    fn kind(&self) -> Self::Kind;
    /// Whether its options are valid; the message says what to change.
    fn validate(&self) -> Result<(), String>;
    /// Ranges clamped and text trimmed, so a valid widget is also an in-range one.
    fn clamped(self) -> Self {
        self
    }
}

/// One app's catalogue for one surface.
pub trait Catalogue: 'static {
    /// The app's surfaces (Dyno: dashboard, menu bar, popover), one enum per app.
    type SurfaceId: Copy + Eq + Hash + Debug + Serialize + DeserializeOwned + 'static;
    /// The widget type.
    type Widget: Widget;
    /// This catalogue's surface.
    const SURFACE_ID: Self::SurfaceId;
    /// Its grid and limits.
    const SURFACE: Surface;
    /// Every kind, in the chooser's order.
    fn kinds() -> &'static [KindSpec<<Self::Widget as Widget>::Kind>];
    /// The layout a new or damaged file gets.
    fn defaults() -> Vec<Placement<Self::Widget>>;
    /// Built-in presets (never stored; rebuilt on every read).
    fn presets() -> Vec<Preset<Self::Widget>> {
        Vec::new()
    }
}

ts! {
/// One widget on a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement<W> {
    pub id: PlacementId,
    /// Columns it spans; one of its kind's spans.
    pub span: u8,
    pub widget: W,
}

/// A named layout the person can switch to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset<W> {
    pub id: PresetId,
    pub name: String,
    /// The app's own: rebuilt on every read, cannot be edited or deleted.
    pub builtin: bool,
    pub placements: Vec<Placement<W>>,
}

/// The stored layout of one surface. Built-in presets are not stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(serialize = "W: Serialize", deserialize = "W: Deserialize<'de>"))]
pub struct Layout<W> {
    pub version: u32,
    pub placements: Vec<Placement<W>>,
    /// The person's own presets only.
    pub presets: Vec<Preset<W>>,
    /// One level of undo: the placements before the last remove, preset or reset. In memory only; the next edit
    /// replaces or clears it.
    #[serde(skip)]
    #[cfg_attr(any(test, feature = "ts"), ts(skip))]
    previous: Option<Vec<Placement<W>>>,
}

/// What was wrong with a stored layout (shown once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NoteCode {
    /// Not a layout at all (not an object, wrong shape): the defaults are in use.
    Damaged,
    /// Larger than the surface's `max_bytes`: the defaults are in use.
    TooLarge,
    /// Saved by a newer version: shown as the defaults, kept unchanged and not saved over (edits are refused).
    Newer { version: u32 },
    /// Entries that were invalid, unknown or over a limit, each dropped on its own.
    Dropped { count: u32 },
}

/// A note with its message for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub code: NoteCode,
    pub message: String,
}

/// A change to a layout. Each is applied by the engine and validated as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Edit<W> {
    /// Adds a widget (its id derived from its kind and unique, e.g. `disk-space-2`) at `at`, or at the end.
    Add { widget: W, span: Option<u8>, at: Option<u16> },
    /// Removes a placement (undoable).
    Remove { id: PlacementId },
    /// Moves a placement to index `to` (clamped to the list).
    Move { id: PlacementId, to: u16 },
    /// Changes a placement's span.
    Resize { id: PlacementId, span: u8 },
    /// Changes a placement's options; the kind stays.
    Replace { id: PlacementId, widget: W },
    /// Replaces the placements with a preset's (undoable).
    ApplyPreset { id: PresetId },
    /// Saves the placements as a new user preset.
    SavePreset { name: String },
    /// Deletes a user preset.
    DeletePreset { id: PresetId },
    /// Back to the catalogue's defaults (undoable).
    ResetToDefaults,
    /// Restores the placements before the last undoable edit.
    Undo,
}

/// What a limit counts (typed, so the UI and the messages never parse text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Counted {
    Placements,
    UserPresets,
    PresetPlacements,
}

/// The surface as the chooser needs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceInfo<S> {
    pub id: S,
    pub columns: u8,
    pub max_placements: u16,
    pub min_placements: u16,
    pub max_user_presets: u8,
}

/// One kind as the chooser needs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindInfo<K> {
    pub kind: K,
    pub title: String,
    pub summary: String,
    pub icon: String,
    pub spans: Vec<u8>,
    pub max_instances: u8,
    pub refresh: Refresh,
}

/// The catalogue of one surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueInfo<S, K> {
    pub surface: SurfaceInfo<S>,
    pub kinds: Vec<KindInfo<K>>,
}

/// Everything the chooser draws: the layout, its catalogue, the notes and whether Undo is offered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutView<S, W, K> {
    pub layout: Layout<W>,
    pub catalogue: CatalogueInfo<S, K>,
    /// The catalogue's built-in presets (never stored).
    pub builtin_presets: Vec<Preset<W>>,
    pub notes: Vec<Note>,
    /// True right after an undoable edit; the next edit sets it again or clears it (the undo toast follows it).
    pub can_undo: bool,
    /// False while a newer version's layout is kept (`NoteCode::Newer`): the chooser is read-only.
    pub editable: bool,
}

/// A next step the app offers (its own closed action enum `A`, e.g. "open the alarms page", "retry the backup check").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetAction<A> {
    pub action: A,
    /// Verb plus object, sentence case: "Pin counters", "Retry".
    pub label: String,
}

/// How far a job behind a widget has come (ER-18: a growing count when the total is unknown, never a static 0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Progress {
    Count {
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        done: u64,
        unit: String,
    },
    Fraction {
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        done: u64,
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        total: u64,
        unit: String,
    },
}

/// One widget's value as the engine reports it. Loading is not on the wire: it is the UI's state before the first value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum WidgetState<V, A> {
    /// A value, read at `as_of` (ms since the epoch); `stale_since` is set when the source has not refreshed it in time
    /// (the UI says "Updated 4 min ago" and dims nothing else).
    Ready {
        value: V,
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        as_of: i64,
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number | null"))]
        stale_since: Option<i64>,
    },
    /// Work is running for this widget (a check, a job): phase-labelled progress, never timed out by the UI (ER-19);
    /// `last` is the previous value, still shown under the progress when there is one.
    Working { title: String, phase: String, progress: Progress, last: Option<V>, cancel: Option<WidgetAction<A>> },
    /// Nothing to show; `title` starts with what the person gains, `next` is the next step.
    Empty { title: String, detail: String, next: Option<WidgetAction<A>> },
    /// The source failed. A busy source is `Working` or a stale `Ready`, never an error.
    Error { title: String, cause: Option<String>, next: Option<WidgetAction<A>> },
}

/// A widget's state, by placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementState<V, A> {
    pub id: PlacementId,
    pub state: WidgetState<V, A>,
}

/// How serious a surface notice is (always drawn with its icon and word).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    Info,
    Warning,
    Danger,
}

/// One message for the whole surface instead of one per widget (e.g. Dyno's "Agent stopped" with Restart agent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceNotice<A> {
    pub tone: Tone,
    pub title: String,
    pub detail: String,
    pub action: Option<WidgetAction<A>>,
}

/// All values of a surface at once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceValues<V, A> {
    pub notice: Option<SurfaceNotice<A>>,
    pub placements: Vec<PlacementState<V, A>>,
}
}

impl<W> Layout<W> {
    /// A layout with nothing to undo.
    pub fn new(version: u32, placements: Vec<Placement<W>>, presets: Vec<Preset<W>>) -> Self {
        Layout {
            version,
            placements,
            presets,
            previous: None,
        }
    }

    /// Whether an undoable edit was the last one (remove, apply preset, reset).
    pub fn can_undo(&self) -> bool {
        self.previous.is_some()
    }
}

/// What reading a stored layout found.
#[derive(Debug, Clone, PartialEq)]
pub struct Read<C: Catalogue> {
    /// The layout to use.
    pub layout: Layout<C::Widget>,
    /// What was wrong, if anything.
    pub notes: Vec<Note>,
    /// A newer version's value, kept as read and written back unchanged (`NoteCode::Newer`).
    pub unread: Option<serde_json::Value>,
}

/// A settings field holding one surface's layout that never fails its settings file:
/// `#[serde(default)] popover: Stored<PopoverCatalogue>`. WP-A implements `Default`, `Serialize` and a lenient
/// `Deserialize` through `read::<C>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored<C: Catalogue> {
    pub read: Read<C>,
}

/// Why an edit was refused (the message says what to do).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
pub enum WidgetError {
    #[error("No widget has the id \"{0}\".")]
    UnknownPlacement(String),
    #[error("No preset has the id \"{0}\".")]
    UnknownPreset(String),
    #[error("Two items share the id \"{0}\"; ids must be unique.")]
    DuplicateId(String),
    #[error("At most {max} fit; remove one first.")]
    TooMany { counted: Counted, max: u16 },
    #[error("At most {max} of this widget fit; remove one first.")]
    KindLimit { max: u8 },
    #[error("This widget can be {allowed:?} columns wide.")]
    SpanNotAllowed { allowed: Vec<u8> },
    #[error("At least {min} widgets are needed here.")]
    TooFew { min: u16 },
    #[error("{0}")]
    Invalid(String),
    #[error("Built-in presets cannot be changed or deleted.")]
    BuiltinPreset,
    #[error("There is nothing to undo.")]
    NothingToUndo,
    #[error(
        "This layout was saved by a newer version of the app and is kept unchanged; update the app to edit it."
    )]
    NewerLayout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_types_have_typescript() {
        use ts_rs::TS;
        let decls = [
            PlacementId::decl(),
            Placement::<ts_rs::Dummy>::decl(),
            Layout::<ts_rs::Dummy>::decl(),
            Edit::<ts_rs::Dummy>::decl(),
            WidgetState::<ts_rs::Dummy, ts_rs::Dummy>::decl(),
            LayoutView::<ts_rs::Dummy, ts_rs::Dummy, ts_rs::Dummy>::decl(),
            SurfaceValues::<ts_rs::Dummy, ts_rs::Dummy>::decl(),
        ];
        assert!(decls[4].contains("\"state\": \"ready\""), "{}", decls[4]);
        assert!(!decls[2].contains("previous"), "{}", decls[2]);
        assert!(!decls.join("").contains("bigint"));
        if std::env::var_os("PADDOCK_PRINT_TS").is_some() {
            println!("{}", decls.join("\n"));
        }
    }

    #[test]
    fn ids_are_parsed_once() {
        assert!(PlacementId::try_from("disk-space-2".to_string()).is_ok());
        for bad in ["", "Disk", "a b", &"x".repeat(MAX_ID + 1)] {
            assert_eq!(PlacementId::try_from(bad.to_string()), Err(BadId));
        }
        let id: PresetId = serde_json::from_str("\"travel\"").unwrap();
        assert_eq!(id.as_str(), "travel");
        assert!(serde_json::from_str::<PresetId>("\"No\"").is_err());
    }
}
