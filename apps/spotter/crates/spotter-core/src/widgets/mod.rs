//! Spotter's widget layouts on the shared `paddock-widgets` crate (family ADR 0035): the closed catalogue of the tray
//! popup and the types of what its widgets show. Layout mechanics (limits, lenient reading, edits, undo) are the shared
//! crate's; this module owns what is Spotter's: which widgets exist and what they show.
//!
//! - [`popover`]: the popup's widgets ([`PopoverCatalogue`]).
//! - [`values`]: what each widget shows ([`PopoverValue`]) and the actions it offers ([`WidgetActionId`]).
//!
//! The values themselves are computed by `spotter-engine` (`widgets`), from the watches the engine already holds.

pub mod popover;
pub mod values;

#[cfg(test)]
mod generated;

pub use popover::{PopoverCatalogue, PopoverKind, PopoverWidget};
pub use values::{
    ChangeRow, FailureKind, FailureRow, NotificationMode, PopoverPlacementState, PopoverState, PopoverValue, PopoverValues, SiteRow,
    SiteState, WidgetActionId,
};

use serde::{Deserialize, Serialize};

/// The popup's layout as the chooser draws it.
pub type PopoverView = paddock_widgets::LayoutView<SpotterSurface, PopoverWidget, PopoverKind>;

/// Spotter's surfaces, one catalogue each (the shared crate's `SurfaceId`). Only the tray popup so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum SpotterSurface {
    /// The tray popup.
    Popover,
}

/// The largest layout read, in bytes.
pub const MAX_BYTES: usize = 64 * 1024;
