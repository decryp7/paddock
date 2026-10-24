//! Service health for the Paddock apps: service state, cause and repair action, the registry and its snapshot, and the cause-key notification centre with versioned persistence.
//!
//! ADR 0010, ADR 0012 §3; plan S3. Taken from Debrief's `health/model.rs`, `health/registry.rs` and `notification_centre/`.
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
