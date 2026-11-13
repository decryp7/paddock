//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod boot_volume;
mod canonical_tempdir;
mod cargo_layout;
mod env;
mod golden;
mod hostile;
mod limits;
mod net;
mod runner;
mod scratch;
