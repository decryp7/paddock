//! Integration tests of the `throttle` binary: real processes under a deadline, through the program's own command line.
//! The core (the fake world: admission, slots, priority, logging) is tested in `crates/paddock-throttle/tests/it`.
#![allow(clippy::expect_used, clippy::unwrap_used)]
mod capture;
mod filelock;
mod places;
mod procutil;
mod real;
mod shim;
