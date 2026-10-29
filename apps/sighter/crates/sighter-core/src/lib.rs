//! Sighter's pure core (wave 0): geometry and the coordinate chain, the detection model, the tile planner and the
//! benchmark harness. Everything here is a pure function over typed values; capture, model runtimes and the OS stay in
//! adapters that do not exist yet.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod chain;
pub mod detect;
pub mod geometry;
pub mod tiling;
