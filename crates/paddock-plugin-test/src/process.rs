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

/// Builds the test program `name` with the `cargo` that runs the tests; whether it succeeded.
pub fn build_with_cargo(name: &str) -> bool {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    std::process::Command::new(cargo)
        .args([
            "build",
            "--quiet",
            "-p",
            "paddock-plugin-test",
            "--bin",
            name,
        ])
        .status()
        .is_ok_and(|s| s.success())
}
