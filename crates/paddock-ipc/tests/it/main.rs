//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod common;

mod basics;
mod body;
mod child;
mod clocked;
mod edges;
mod frame;
mod link;
mod peer;
mod system;
