//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod common;

mod eta;
mod hash;
mod model_and_cancel;
mod parallel;
mod pool;
mod properties;
mod reporter_and_jobs;
mod schedule;
mod tauri;
mod time;
mod walk;
