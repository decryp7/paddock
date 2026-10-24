//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`. `tests/installed.rs` stays a
//! binary of its own: it installs the process-wide subscriber, `log` logger and panic hook.

mod never_panics;
mod scoped;
