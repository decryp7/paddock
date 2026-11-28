//! The parts of the test kit that reach outside the test process: ending or blocking this process on purpose
//! ([`Real`], what `paddock-plugin-fixture` does), and running `cargo` to build a test program. They only run in
//! processes whose coverage data is never written (ended by force) or when a program is missing, so this file is the
//! one thing besides the programs' `main`s that the coverage gate leaves out.

use crate::fixture::Danger;

/// The dangerous operations for real.
pub struct Real;

impl Danger for Real {
    fn crash(&self) {
        std::process::exit(86);
    }

    fn hang(&self) {
        loop {
            std::thread::park();
        }
    }

    fn quit_soon(&self) {
        // The reply goes out first; then the process ends by itself, with no call in flight.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::process::exit(7);
        });
    }

    fn exit(&self, code: i32) {
        std::process::exit(code);
    }
}

/// The `cargo build` of the test program `name`, as [`build_with_cargo`] starts it. The build folder is the one the running test
/// program lives in (`profile_dir`'s parent): cargo exports none of the parent's command-line configuration (`--config
/// build.target-dir=...` of a `./dev` or agent run) to a test, so without it the child builds into the workspace's own `target/`
/// and the test then finds no program next to itself. Configuration in files (the CI build folder's `.cargo/ci.toml`) the child
/// reads itself: its working folder is the test's package folder.
pub fn build_command(
    cargo: &std::ffi::OsStr,
    name: &str,
    profile_dir: &std::path::Path,
) -> std::process::Command {
    // A developer's own build in the same terminal: no creation flags, so its output and Ctrl+C reach it.
    let mut cmd = paddock_env::command::attached_command(cargo);
    cmd.args([
        "build",
        "--quiet",
        "-p",
        "paddock-plugin-test",
        "--bin",
        name,
    ]);
    if let Some(target) = profile_dir.parent() {
        // As configuration, never `CARGO_TARGET_DIR`: sccache puts every `CARGO_*` variable into its cache key (AGENTS.md, "Build
        // folders"). A TOML literal string cannot hold a quote, so such a path (never a build folder of ours) goes by the variable.
        let dir = target.to_string_lossy();
        if dir.contains('\'') {
            cmd.env("CARGO_TARGET_DIR", target);
        } else {
            cmd.args(["--config", &format!("build.target-dir='{dir}'")]);
        }
    }
    cmd
}

/// Builds the test program `name` with the `cargo` that runs the tests, into the build folder of the running test program
/// (`profile_dir`); whether it succeeded.
pub fn build_with_cargo(name: &str, profile_dir: &std::path::Path) -> bool {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    build_command(&cargo, name, profile_dir)
        .status()
        .is_ok_and(|s| s.success())
}
