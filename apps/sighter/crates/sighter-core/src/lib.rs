//! Sighter's pure core (SG1). No I/O, no clock, no randomness: capture, input, models and the OS sit behind the ports in
//! [`ports`]. The design review's P1s live here as types:
//!
//! * one [`Outcome`] enum that keeps infrastructure errors apart from product failures,
//! * [`CalibratedScore`] is the only score that can be fused; a [`RawScore`] must pass its layer's [`Calibrator`],
//! * the margin rule ([`decide`]): *found*, *ambiguous* or *not found*, so two look-alikes never produce a click.

pub mod decision;
pub mod frame;
pub mod geometry;
pub mod outcome;
pub mod ports;
pub mod score;

pub use decision::{Resolution, Thresholds, ThresholdsError, decide, fuse};
pub use frame::{Frame, FrameError, MAX_DIM, MAX_PIXELS};
pub use geometry::{Point, Rect};
pub use outcome::Outcome;
pub use score::{CalibratedScore, Calibrator, Candidate, FusedCandidate, Layer, Probability, RawScore, ScoreError};
