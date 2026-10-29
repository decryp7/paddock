//! The pure core of the debug HUD (design review 2026-10, section 0 (b)): one stream of typed trace events, a bounded
//! ring and subscriber fan-out, the sidecar track format for run videos (the HUD is never burnt into frames), secret
//! masking, and the overlay item model (a border and a confidence band per detection).
//!
//! Nothing here draws. Overlays, the live view and the local overlay window need the shared UI and wait.

pub mod mask;
pub mod trace;
