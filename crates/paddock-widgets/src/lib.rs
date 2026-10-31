//! Widget layouts for the Paddock apps (family ADR 0035): what a surface (a tray popup, a dashboard grid, a menu bar)
//! shows, in which order and how wide, as data the engine stores and validates and the UI only draws.
//!
//! A pure crate: no I/O, no Tauri, no app types. Domain rules (what a widget shows, its thresholds) stay in the app.
//!
//! - [`Catalogue`]: an app's closed set of widget kinds for one [`Surface`], with limits, defaults and built-in presets,
//!   as data. [`check_catalogue`] is the app's one test of it.
//! - [`Layout`], [`Placement`], [`Preset`]: the stored layout, ids parsed once ([`PlacementId`], [`PresetId`]).
//! - [`read`] and [`Stored`]: versioned, lenient reading that never fails the settings file. A damaged or oversized
//!   layout gives the defaults and one [`Note`]; a newer version's layout is kept unchanged and not edited over; each
//!   invalid or unknown entry is dropped on its own with a count.
//! - [`Edit`] and [`apply`]: add, remove, move, resize, replace, presets, reset and one level of undo, each a pure
//!   function that returns a validated layout or a [`WidgetError`].
//! - [`info`], [`view`], [`LayoutView`]: what the chooser draws and limits itself with.
//! - [`WidgetState`], [`PlacementState`], [`SurfaceValues`]: the ready, working, empty and error envelope.
//!
//! Wire types derive `ts_rs::TS` under the `ts` feature (and in tests); the generated file is
//! `ui/projects/paddock-ui/widgets/generated/widgets.ts`.

/// Items that are wire types: they derive `ts_rs::TS` in tests and with the `ts` feature.
macro_rules! ts {
    ($($item:item)*) => { $( #[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))] $item )* };
}

mod catalogue;
mod check;
mod edit;
mod error;
#[cfg(test)]
mod generated;
mod id;
mod layout;
mod read;
mod state;
mod view;

pub use catalogue::{Catalogue, KindSpec, Refresh, Surface, Widget, check_catalogue};
pub use edit::{Edit, apply};
pub use error::{Counted, Note, NoteCode, WidgetError};
pub use id::{BadId, MAX_ID, PlacementId, PresetId};
pub use layout::{Layout, Placement, Preset, assign_ids};
pub use read::{MAX_NAME, Read, Stored, read};
pub use state::{
    PlacementState, Progress, SurfaceNotice, SurfaceValues, Tone, WidgetAction, WidgetState,
};
pub use view::{
    CatalogueInfo, KindInfo, LayoutView, SurfaceInfo, ViewOf, builtin_presets, info, view,
};
