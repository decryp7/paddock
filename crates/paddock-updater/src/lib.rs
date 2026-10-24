//! Updates for the Paddock apps: the update state machine, manifest and signature check and the install hand-off, over `paddock-download`.
//!
//! ADR 0012 §3; plan S11. Taken from Debrief's `updater/`; replaces the apps' own `updater.rs` copies.
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
