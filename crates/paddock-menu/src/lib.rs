//! Menus for the Paddock apps: the menu model, its validation and limits, the macOS native menu with the standard items, diffing and routing to the focused window.
//!
//! ADR 0012 §3; plan S13. Taken from Debrief's `menu.rs`.
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
