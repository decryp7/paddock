//! Dialogs as OS windows for the Paddock apps: the dialog contract schema and `allows(dialog, command)`, pure placement on the parent's monitor, window creation by kind, fit/grow and bounds.
//!
//! ADR 0009, ADR 0012 §3; plan S5. Taken from Debrief's `dialog_contracts.rs`, `dialog_placement.rs` and
//! `dialog_windows.rs` (Debrief ADRs 0050 and 0054), without Debrief's dialog ids, labels, events or error type.
//!
//! - [`contract`]: every dialog declares a [`Kind`] (decision, form, viewer, tool) and a contract (modality, required
//!   choice, instancing, minimum size, the commands its window may call). [`Contracts::parse`] reads the app's generated
//!   contract file once and refuses one that contradicts itself; [`Grant`] is the [`paddock_commands::DialogGrant`] the
//!   command guard asks for every call from a dialog window.
//! - [`placement`]: where a dialog opens, as pure geometry in physical px: on the parent's monitor (never the primary
//!   display as such), a remembered offset from the parent only when the whole dialog stays on that monitor's work
//!   area, shrunk to fit, mixed DPI and negative coordinates included.
//! - [`window`]: the wire types (`DialogRequest`, `DialogStart`, ...), request validation, geometry by kind, the window
//!   configuration (no title-bar buttons; owned or independent per OS) and the fit-to-content rules (a viewer only
//!   grows).
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
//! let contracts = paddock_dialogs::Contracts::parse(r#"{
//!     "version": 1,
//!     "dialogs": { "about": { "kind": "viewer", "modality": "modeless", "commands": ["app_status"] } }
//! }"#).unwrap();
//! let dialogs = paddock_dialogs::Dialogs::new(Arc::new(contracts));
//! let about = dialogs.contracts().get("about").unwrap();
//! let label = dialogs.reserve(paddock_dialogs::OpenDialog::new("main", "main", "about", about)).unwrap();
//! assert_eq!(label, "d-main-1");
//! let grant = dialogs.grant_of(&label).unwrap();
//! assert!(grant.allows("app_status") && grant.allows("dialog_close") && !grant.allows("app_quit"));
//! ```

pub mod contract;
pub mod placement;
pub mod registry;
pub mod window;

pub use contract::{
    BASE_COMMANDS, Contract, ContractError, ContractFile, Contracts, Grant, Kind, KindPolicy,
    Modality, Size,
};
pub use placement::{Bounds, Monitor, Placement, Rect, Remembered, ScreenPosition};
pub use registry::{CloseDecision, Dialogs, OpenDialog};
pub use window::{
    CLOSE_REQUESTED_EVENT, CLOSED_EVENT, DialogClosed, DialogError, DialogErrorCode, DialogInfo,
    DialogRequest, DialogResult, DialogStart, Parenting, RESULT_EVENT, WindowConfig,
};

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
