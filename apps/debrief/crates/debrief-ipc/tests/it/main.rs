//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod child_log;
mod endpoint_exit;
mod exit_once;
mod launch;
mod properties;

/// The fake child's first start (a fresh binary the OS has not run yet) is paid here, before a test starts it with a bound it
/// cannot remove (a child that exits at once must be seen exiting within the OS's publication window). One definition for
/// every module of this binary; the kit memoises it per file, so only the first call runs the child.
pub(crate) fn warm() {
    paddock_testkit_start::warm_first_start(std::path::Path::new(env!("CARGO_BIN_EXE_debrief-ipc-fake-child")), &["--version"]);
}
