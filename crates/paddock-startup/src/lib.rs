//! The start-up contract of the Paddock apps: the pure watchdog and its ticker, `window_ready`, boot-error forwarding, the UI checkpoint and the native ask dialog.
//!
//! ADR 0012 §3; plan S7. Taken from Debrief's `startup_watchdog.rs` (Debrief ADR 0058).
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
