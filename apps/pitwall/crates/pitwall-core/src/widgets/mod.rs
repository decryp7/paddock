//! Pitwall's widget layouts on the shared `paddock-widgets` crate (family ADR 0035): the closed catalogue of the tray popup,
//! what each widget shows and the rules behind it. Layout mechanics (limits, lenient reading, edits, undo) are the shared
//! crate's; this module owns what is Pitwall's: which widgets exist, which facts they read and which state, tone and wording
//! flag each one gets. Everything is pure: the app feeds [`Facts`] from the live feed and the clock, the UI only draws.
//!
//! - [`popover`]: the popup's widgets ([`PopoverCatalogue`]) and the rows each draws.
//! - [`values`]: what each widget shows ([`PopoverValue`]) and the actions it offers ([`WidgetActionId`]).
//! - [`facts`]: the accumulator the live feed writes ([`Facts`]).
//! - [`build`]: facts and layout to the values, the header status and the surface notice.

pub mod build;
pub mod facts;
pub mod popover;
pub mod values;

#[cfg(test)]
mod generated;

pub use build::{popover_header, popover_values, state};
pub use facts::{ErrorFact, Facts, UpdatesFact};
pub use popover::{
    loading_shape, rows_of, LoadingRows, PopoverCatalogue, PopoverKind, PopoverWidget, DOWNLOAD_ROWS, ERROR_ROWS, MODEL_ROWS,
};
pub use values::{
    DownloadRow, DownloadRowState, EngineHealth, ErrorRow, MemoryLevel, MeterTone, ModelRow, ModelRowState, Page, PopoverHeader,
    PopoverPlacementState, PopoverState, PopoverStatus, PopoverValue, PopoverValues, ServerState, ServingToggle, VramReading,
    WidgetActionId,
};

use serde::{Deserialize, Serialize};

/// The popup's layout as the chooser draws it.
pub type PopoverView = paddock_widgets::LayoutView<PitwallSurface, PopoverWidget, paddock_widgets::Span, PopoverKind>;

/// Pitwall's surfaces, one catalogue each (the shared crate's `SurfaceId`). Only the tray popup so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum PitwallSurface {
    /// The tray popup.
    Popover,
}

/// The largest layout read, in bytes.
pub const MAX_BYTES: usize = 64 * 1024;
