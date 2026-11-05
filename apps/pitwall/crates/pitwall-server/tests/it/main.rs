//! The server's integration tests, one binary: each module is one area, all over the shared fake llama-server in `common`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod caching;
mod common;
mod discovery;
mod e2e;
mod engine_pins;
mod jobs;
mod llama_e2e;
mod presets;
mod privacy;
mod slots;
mod updates;
mod usage;
