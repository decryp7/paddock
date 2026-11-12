//! The server's integration tests, one binary (conformance C37): each file is a module.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod audio_parts;
mod caching;
mod cancel;
mod common;
mod discovery;
mod e2e;
mod engine_pins;
mod jobs;
mod llama_e2e;
mod logging;
mod playground_chat;
mod presets;
mod privacy;
mod shipped_binary;
mod slots;
mod updates;
mod usage;
