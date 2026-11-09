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

// The ending harness and the endings of this crate's endpoints (ADR 0072) need the `testing` feature, so they are compiled and
// run only with it: `cargo test -p paddock-ipc --features testing` (CI runs every crate with `--all-features`). Without the
// feature the module is absent and `cargo test` says nothing about it: run it with the feature before pushing a change here.
// (Not `required-features`: that would drop this whole binary when the feature is off.)
#[cfg(feature = "testing")]
mod endings;
