#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The integration tests of this crate are modules of this one binary (conformance C37). They start the real worker program,
//! built with the `test-hooks` feature (files with a magic suffix make it abort, exit, spin, block or overflow its stack).

mod crashes;
mod index_run;
mod measure;
mod support;
