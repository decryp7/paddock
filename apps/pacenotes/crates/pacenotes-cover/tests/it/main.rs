#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The integration tests of this crate are modules of this one binary (conformance C37). The reports are invented, in the shape of
//! each tool's output; no real project's data is used.

mod blend;
mod hostile;
mod json;
mod lcov;
mod review;
mod support;
mod xml;
