//! Dialogs as OS windows for the Paddock apps: the dialog contract schema and `allows(dialog, command)`, pure placement on the parent's monitor, window creation by kind, fit/grow and bounds.
//!
//! ADR 0009, ADR 0012 §3; plan S5. Taken from Debrief's `dialog_contracts.rs`, `dialog_placement.rs` and
//! `dialog_windows.rs` (Debrief ADRs 0050 and 0054), without Debrief's dialog ids, labels, events or error type.
//!
//! - [`table`]: the contracts as static tables ([`dialogs!`], ADR 0026 §3): every dialog declares a [`Shape`] (kind and
//!   modality as one typed value), a width step, instancing, required choice and the commands its window may call, named
//!   by the `dialog_callable` markers of the command tables, so a wrong command does not compile.
//! - [`contract`]: the run-time contracts built from the tables ([`Contracts::from_tables`]), the kind table
//!   ([`Kind::policy`]), and [`Grant`], the [`paddock_commands::DialogGrant`] the command guard asks for every call
//!   from a dialog window.
//! - [`ts`]: the generated TypeScript (the kind table, and each owner's `dialog-contracts.ts`).
//! - [`placement`]: where a dialog opens, as pure geometry in physical px: on the parent's monitor (never the primary
//!   display as such), a remembered offset from the parent only when the whole dialog stays on that monitor's work
//!   area, shrunk to fit, mixed DPI and negative coordinates included.
//! - [`window`]: the wire types (`DialogRequest`, `DialogStart`, `Measure`, ...), request validation, geometry by kind,
//!   the window configuration (no title-bar buttons; owned or independent per OS; created unseen), the show mechanism
//!   per OS ([`show_mechanism`]) and the fit rules (`plan_fit`: the cap, the minimum content, a viewer only grows).
//! - [`registry`]: [`Dialogs`], the open dialog windows by label (`d-<host>-<n>`), with the close-request rules
//!   (forwarded as Close/Cancel, a required choice kept unless its page does not answer).
//!
//! The core is pure Rust with no Tauri dependency; the adapter (`tauri`: window creation, the four commands, the window
//! events) lives behind the `tauri` feature.
//!
//! ```
//! use paddock_commands::DialogGrant;
//! use std::sync::Arc;
//!
//! mod app {
//!     paddock_commands::command_table! { pub static COMMANDS; Query [MAIN | DIALOG] => app::app_status, }
//! }
//! paddock_dialogs::dialogs! {
//!     static DIALOGS;
//!     "about" => Viewer { commands: &[app::dialog_callable::app_status] },
//! }
//! let dialogs = paddock_dialogs::Dialogs::from_tables(&[&DIALOGS]).unwrap();
//! let about = dialogs.contracts().get("about").unwrap();
//! let label = dialogs.reserve(paddock_dialogs::OpenDialog::new("main", "main", "about", about)).unwrap();
//! assert_eq!(label, "d-main-1");
//! let grant = dialogs.grant_of(&label).unwrap();
//! assert!(grant.allows("app_status") && grant.allows("dialog_close") && !grant.allows("app_quit"));
//! ```

pub mod contract;
pub mod edge;
pub mod expect;
pub mod placement;
pub mod registry;
pub mod size;
pub mod table;
pub mod ts;
pub mod window;

pub use contract::{
    BASE_COMMANDS, Contract, ContractError, Contracts, Grant, Kind, KindPolicy, Modality,
    Remembers, Shrink, Size, SizeMode,
};
pub use paddock_commands::DialogCallable;
pub use placement::{Bounds, Monitor, Placement, Rect, Remembered, ScreenPosition};
pub use registry::{CloseDecision, Dialogs, OpenDialog, SHOW_DEADLINE, Scheduler, ThreadScheduler};
pub use table::{DialogTable, Entry, Shape, Width};
pub use window::{
    CLOSE_REQUESTED_EVENT, CLOSED_EVENT, DialogClosed, DialogError, DialogErrorCode, DialogInfo,
    DialogReadyWire, DialogRemeasureEvent, DialogRequest, DialogResult, DialogShown,
    DialogSizeModeEvent, DialogStart, INPUTS_EVENT, Measure, Parenting, REMEASURE_EVENT,
    RESULT_EVENT, SHOWN_EVENT, SIZE_MODE_EVENT, ShowMechanism, SizeModeName, Theme, WindowConfig,
    show_mechanism,
};

#[cfg(feature = "tauri")]
pub mod tauri;
#[cfg(feature = "probe")]
pub mod probe;

paddock_commands::command_table! {
    /// The dialog layer's commands (`paddock_dialogs::tauri::invoke_handler` serves them; the shell plugin folds them
    /// into its table). [`BASE_COMMANDS`] are the ones every dialog contract includes.
    pub static COMMANDS;
    /// Opens a dialog window for the calling window (a dialog may open a dialog of its own).
    Mutation [MAIN | DIALOG] limit 9 * paddock_commands::MIB => tauri::dialog_open,
    /// The calling dialog's start data.
    Query [DIALOG] => tauri::dialog_take_input,
    /// Closes the calling dialog with its result.
    Mutation [DIALOG] limit 2 * paddock_commands::MIB => tauri::dialog_close,
    /// The calling dialog's page measured itself: sizes its window, makes the minimum content the OS minimum, shows it.
    Quiet [DIALOG] => tauri::dialog_ready,
    /// Refits the calling dialog to a new measure.
    Quiet [DIALOG] => tauri::dialog_measure,
}

#[cfg(test)]
mod generated;
