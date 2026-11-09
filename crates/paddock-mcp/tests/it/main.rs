//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod common;

mod behaviour;
mod contract;
mod endings;
mod control;
mod key_file;
mod listener;
mod never_panics;
mod transports;
