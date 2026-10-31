//! Dyno's widget layouts on the shared `paddock-widgets` crate (family ADR 0035): the closed catalogues of its three
//! surfaces, and the value builders of the tray popup. Layout mechanics (limits, lenient reading, edits, undo) are the
//! shared crate's; this module owns what is Dyno's: which widgets exist, their options and what they show.
//!
//! - [`popover`]: the tray popup's widgets ([`PopoverCatalogue`]).
//! - [`dashboard`]: the Overview's widgets and presets ([`DashboardCatalogue`]).
//! - [`menu_bar`]: the menu-bar items ([`MenuBarCatalogue`]).
//! - [`popover_values`]: the popup's widget values, one pure state function per kind.
//! - [`metric`]: what a widget measures.

pub mod choices;
pub mod dashboard;
pub mod menu_bar;
pub mod metric;
pub mod popover;
pub mod popover_values;
pub mod reading;
pub mod stored_wire;
pub mod wire;

#[cfg(test)]
mod generated;

pub use choices::{widget_choices, DashboardKindChoices, MenuBarKindChoices, MetricChoice, PopoverKindChoices, WidgetChoices};
pub use dashboard::{DashboardCatalogue, DashboardKind, DashboardWidget, Thresholds, WidgetOptions};
pub use menu_bar::{MenuBarCatalogue, MenuBarItem, MenuBarKind};
pub use metric::Metric;
pub use popover::{PopoverCatalogue, PopoverKind, PopoverWidget, VolumeChoice};
pub use reading::{read_all, read_metric, with_dashboard_thresholds, MetricReading, ReadingQuery};
pub use wire::{edit_layout, layout_view, JsonOnLink, SurfaceEdit, SurfaceView};

use serde::{Deserialize, Serialize};

/// Dyno's surfaces, one catalogue each (the shared crate's `SurfaceId`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum DynoSurface {
    /// The tray popup.
    Popover,
    /// The Overview.
    Dashboard,
    /// The menu-bar / notification-area items.
    MenuBar,
}

/// The largest layout read, in bytes (all three surfaces).
pub const MAX_BYTES: usize = 128 * 1024;
