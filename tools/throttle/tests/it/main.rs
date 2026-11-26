//! Integration tests of throttle. Every wait is bounded: the fake world never sleeps, and real processes run under a deadline.
#![allow(clippy::expect_used, clippy::unwrap_used)]
mod admission;
mod capture;
mod filelock;
mod helpers;
mod logging;
mod places;
mod ported;
mod priority;
mod procutil;
mod props;
mod real;
mod share;
mod slots;
