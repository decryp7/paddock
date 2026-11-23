//! Integration tests of heavy. Every wait is bounded: the fake world never sleeps, and real processes run under a deadline.
#![allow(clippy::expect_used, clippy::unwrap_used)]
mod admission;
mod helpers;
mod logging;
mod ported;
mod priority;
mod procutil;
mod props;
mod real;
mod slots;
