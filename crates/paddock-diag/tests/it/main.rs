//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`. `tests/webview.rs` stays a
//! binary of its own: it installs the process-wide log.

mod report;
