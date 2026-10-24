//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

#[cfg(feature = "tauri")]
mod adapter;
mod channel;
mod common;
mod hub;
mod runner;
