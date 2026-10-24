//! The command table for the Paddock apps: one macro for the kind, window classes, request size limit and async-only rule of every command, the guard hook and generated TypeScript command names.
//!
//! ADR 0012 §3; plan S1. Taken from Debrief's `command_table!` and `commands.rs` (Debrief ADR 0016).
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
