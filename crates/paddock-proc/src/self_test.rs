//! The `--self-test` every sidecar answers (rule 23, plan X34). CI starts each sidecar from the built app with this flag: it
//! must load (every library found, no `Different Team IDs`), run its checks, touch nothing of the user's and exit 0.
//!
//! ```no_run
//! // The first thing `main` does, before any state is opened:
//! fn early_exit() {
//!     if let Some(code) = paddock_proc::self_test::handle("my-worker", env!("CARGO_PKG_VERSION"), &[
//!         paddock_proc::self_test::Check::new("engine", || Ok(())),
//!     ]) {
//!         std::process::exit(code);
//!     }
//!     // ... the real program
//! }
//! ```
//!
//! The report is one line per check (`ok engine` / `FAIL engine: reason`) after a header line with the name and version.
//! A check that panics fails with its message instead of killing the report.

use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// The flag that asks for the self-test (the first argument).
pub const FLAG: &str = "--self-test";

/// One thing the sidecar proves it can do (open its library, parse a built-in table). It must not touch user data.
pub struct Check {
    name: &'static str,
    run: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
}

impl Check {
    /// A named check.
    pub fn new(
        name: &'static str,
        run: impl Fn() -> Result<(), String> + Send + Sync + 'static,
    ) -> Check {
        Check {
            name,
            run: Box::new(run),
        }
    }
}

/// Whether the arguments (without the program name) ask for the self-test.
pub fn requested<S: AsRef<str>>(args: &[S]) -> bool {
    args.first().is_some_and(|a| a.as_ref() == FLAG)
}

/// Runs the checks, writes the report to `out` and returns the exit code: 0 when every check passed, 1 otherwise. The
/// executable itself is always checked first (it can name its own path), so a sidecar with no checks of its own still
/// proves that it starts.
pub fn run(name: &str, version: &str, checks: &[Check], out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "{name} {version} self-test");
    let mut failed = 0;
    let own = Check::new("executable", || {
        std::env::current_exe()
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    for check in std::iter::once(&own).chain(checks) {
        let outcome = catch_unwind(AssertUnwindSafe(|| (check.run)()));
        match outcome {
            Ok(Ok(())) => {
                let _ = writeln!(out, "ok {}", check.name);
            }
            Ok(Err(reason)) => {
                failed += 1;
                let _ = writeln!(out, "FAIL {}: {reason}", check.name);
            }
            Err(panic) => {
                failed += 1;
                let reason = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "panicked".to_string());
                let _ = writeln!(out, "FAIL {}: panicked: {reason}", check.name);
            }
        }
    }
    i32::from(failed > 0)
}

/// For `main`: when the process was started with the flag, runs the checks, prints the report to standard output and
/// returns the exit code; otherwise `None`.
pub fn handle(name: &str, version: &str, checks: &[Check]) -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    requested(&args).then(|| run(name, version, checks, &mut std::io::stdout()))
}
