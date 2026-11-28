//! Integration tests of the throttle core. Every wait is bounded: the fake world never sleeps.
#![allow(clippy::expect_used, clippy::unwrap_used)]
mod admission;
mod filelock;
mod helpers;
mod logging;
mod ported;
mod priority;
mod props;
mod share;
mod slots;
mod status;
