//! Windows for the Paddock apps: window state, single instance, deep links, title bar and materials, and the tray popover window.
//!
//! ADR 0006, ADR 0012 §3; plan S12. Taken from Debrief's `native_feel.rs`, `instance.rs` and `tray.rs` and the apps' `window.rs`/`tray.rs`.
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
