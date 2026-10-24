//! Settings for the Paddock apps: the section schema (sections, typed fields, validation, owners) and the versioned settings file that falls back to defaults when it is missing or corrupt.
//!
//! Rule 21, ADR 0012 §3; plan S10. Taken from `debrief_core::settings`, Debrief's `config_store.rs` and `settings.rs`.
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
