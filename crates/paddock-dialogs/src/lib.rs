//! Dialogs as OS windows for the Paddock apps: the dialog contract schema and `allows(dialog, command)`, pure placement on the parent's monitor, window creation by kind, fit/grow and bounds.
//!
//! ADR 0009, ADR 0012 §3; plan S5. Taken from Debrief's `dialog_contracts.rs`, `dialog_placement.rs` and `dialog_windows.rs`.
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
