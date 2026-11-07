//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`. `tests/tauri.rs`
//! stays a binary of its own: it needs the `tauri` feature.

mod call_ids;
mod memory_store;
mod never_panics;
mod phases;
mod pitwall;
mod store;
mod support;
mod tool_path;
mod turns;
