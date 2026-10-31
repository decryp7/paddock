//! Widget layouts for the Paddock apps (family ADR 0035).
//!
//! A pure crate: no I/O, no Tauri, no app types. It owns everything about a layout that is not what a widget shows:
//!
//! - [`Catalogue`]: an app's closed set of widget kinds for one [`Surface`] (a dashboard grid, a popup, a menu bar), with
//!   limits, defaults and built-in presets, as data. [`check_catalogue`] is the app's one test of it.
//! - [`Layout`], [`Placement`], [`Preset`]: the stored layout, ids parsed once ([`PlacementId`]).
//! - [`read`] and [`Stored`]: versioned, lenient reading that never fails the settings file. A damaged, newer or oversized
//!   layout gives the defaults and one [`Note`]; each invalid or unknown entry is dropped on its own with a count.
//! - [`Edit`] and [`apply`]: add, remove, move, resize, replace, presets, reset and one level of undo, each a pure
//!   function that returns a validated layout or a [`WidgetError`].
//! - [`LayoutView`], [`CatalogueInfo`]: what the chooser draws and limits itself with.
//! - [`WidgetState`], [`PlacementState`]: the ready, empty and error envelope every surface renders alike.
//!
//! What a widget shows (values, thresholds, domain rules) stays in the app. Every wire type derives `ts_rs::TS`; the
//! generated file is `ui/projects/paddock-ui/widgets/generated/widgets.ts`.

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
pub use error::{Note, NoteCode, WidgetError};
pub use id::{MAX_ID, PlacementId};
pub use layout::{Layout, Placement, Preset, assign_ids};
pub use read::{MAX_NAME, Read, Stored, read};
pub use state::{PlacementState, WidgetAction, WidgetState};
pub use view::{
    CatalogueInfo, KindInfo, LayoutView, SurfaceInfo, builtin_presets, catalogue_info, view,
};
