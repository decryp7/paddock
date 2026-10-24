//! Running `cargo`: the one place this tool starts a process (through `paddock-proc`, which opens no console window on
//! Windows). Behind a trait so the commands' plans are tested without building anything.

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;

/// Runs `cargo` with arguments.
pub trait Cargo {
    /// Runs `cargo <args>` in `dir`; cargo's own messages reach the user on standard error. `Err` is what to tell the user.
    fn run(&self, args: &[String], dir: &Path) -> Result<(), String>;
}

/// The real `cargo`: `$CARGO` (set when this tool runs as `cargo paddock-plugin`), else the one on `PATH`.
pub struct ProcCargo {
    program: OsString,
}

impl ProcCargo {
    /// The `cargo` that started this tool, or the one on `PATH`.
    pub fn from_env() -> ProcCargo {
        ProcCargo::with_program(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
    }

    /// A specific `cargo` program.
    pub fn with_program(program: impl Into<OsString>) -> ProcCargo {
        ProcCargo {
            program: program.into(),
        }
    }
}

impl Cargo for ProcCargo {
    fn run(&self, args: &[String], dir: &Path) -> Result<(), String> {
        let status = paddock_proc::command(&self.program)
            .args(args)
            .current_dir(dir)
            // Standard output is for the result (one JSON object in JSON mode); cargo build writes its messages to stderr.
            .stdout(Stdio::null())
            .status()
            .map_err(|e| {
                format!(
                    "cargo could not be started ({e}); install Rust from https://rustup.rs, or put cargo on PATH"
                )
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "cargo {} failed ({status}); fix the errors above",
                args.first().map_or("", String::as_str)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_cargo_says_how_to_get_one() {
        let dir = tempfile::tempdir().unwrap();
        let err = ProcCargo::with_program(dir.path().join("no-such-cargo"))
            .run(&["build".into()], dir.path())
            .unwrap_err();
        assert!(
            err.contains("could not be started") && err.contains("rustup"),
            "{err}"
        );
    }

    #[test]
    fn cargo_is_taken_from_the_environment_and_a_failure_names_the_command() {
        let dir = tempfile::tempdir().unwrap();
        // No Cargo.toml here: the real cargo refuses `build` and the error names it.
        let err = ProcCargo::from_env()
            .run(&["build".into(), "--offline".into()], dir.path())
            .unwrap_err();
        assert!(err.starts_with("cargo build failed"), "{err}");
    }
}
