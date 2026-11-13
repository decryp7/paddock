//! The programs cargo built for the test that is running: found from `current_exe()` in both build-folder layouts.
//!
//! A test program runs from `<profile>/deps/` (cargo before 1.100) or `<profile>/build/<package>/<hash>/out/` (cargo
//! 1.100, which made `build.build-dir-new-layout` always on); a binary or an example of the workspace is in `<profile>/`
//! and `<profile>/examples/` in both. Never take `current_exe().parent().parent()`: it is right in one layout and wrong
//! in the other (Debrief's `debrief-plugins` lookup, CI run 8028; Pitcrew's fake runner, run 8027). Use these, which
//! refuse any other shape loudly. Cargo documents the build-folder layout as not a stable interface: a binary of the test's own package is found with
//! `env!("CARGO_BIN_EXE_<name>")`, never with these (cargo then owns the path);
//! these are for a binary of another package or an example, and for production code that has to cope with a test
//! program, `paddock_env::cargo_layout`, which holds the same shapes.

use std::path::{Path, PathBuf};

pub use paddock_env::cargo_layout::{profile_dir_of_dir, profile_dir_of_exe};

/// The profile folder (`<target>/<profile>`) the test program `exe` was built into, in both layouts.
///
/// # Errors
///
/// When `exe` is in neither shape: the message says what was expected, so the test fails with the cause instead of looking
/// for its neighbour in a wrong folder.
pub fn profile_dir_for(exe: &Path) -> Result<PathBuf, String> {
    profile_dir_of_exe(exe).ok_or_else(|| {
        format!(
            "{} is not in a cargo build folder: expected <profile>/deps/<test> or <profile>/build/<package>/<hash>/out/<test>",
            exe.display()
        )
    })
}

/// [`profile_dir_for`] the running test program.
///
/// # Panics
///
/// When `current_exe()` fails or is in neither layout.
#[must_use]
pub fn profile_dir() -> PathBuf {
    let exe = std::env::current_exe()
        .unwrap_or_else(|e| panic!("the test program's own path is unknown: {e}"));
    profile_dir_for(&exe).unwrap_or_else(|e| panic!("{e}"))
}

/// `<profile>/<name>` (with the platform's executable suffix): a binary the workspace build made. It need not exist; the
/// caller decides whether absent is fine or a failure.
///
/// # Panics
///
/// As [`profile_dir`].
#[must_use]
pub fn sibling_bin(name: &str) -> PathBuf {
    profile_dir().join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// `<profile>/examples/<name>` (with the platform's executable suffix): an example the workspace test build made.
///
/// # Panics
///
/// As [`profile_dir`].
#[must_use]
pub fn example_bin(name: &str) -> PathBuf {
    profile_dir()
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}
