//! Sighter's pure core: geometry and the coordinate chain, the detection model, the tile planner, calibrated scores and
//! fusion with the margin rule, the result enum, the ports and the benchmark harness. Everything here is a pure function
//! over typed values; capture, model runtimes and the OS stay in adapters that do not exist yet.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod bench;
pub mod chain;
pub mod change;
pub mod decision;
pub mod detect;
pub mod frame;
pub mod geometry;
pub mod hud;
pub mod outcome;
pub mod ports;
pub mod score;
pub mod tiling;
