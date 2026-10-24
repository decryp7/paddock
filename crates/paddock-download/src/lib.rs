//! Downloads for the Paddock apps: segmented Range downloads with per-host caps, 429 and Retry-After back-off, resume, and one in-order SHA-256 while writing, reported as job phases.
//!
//! Rule 15, ADR 0012 §3; plan S8. Built on `paddock-jobs` (`HashingWriter`).
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in [`tauri`](crate::tauri) behind the `tauri` feature.
//! Scaffold only (plan S0): the content arrives with its work package.

#[cfg(feature = "tauri")]
pub mod tauri;
