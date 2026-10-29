//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`. `shell_demo` was `demo.rs`
//! (it holds a `demo` module of its own).

mod plugin;
mod shell_demo;
mod start;
