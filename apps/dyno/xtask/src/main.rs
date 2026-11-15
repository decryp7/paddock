//! Repository chores for Dyno developers, run as `cargo xtask <task>` (alias in `.cargo/config.toml`; the Debrief pattern).
//!
//! - `linux-units [machine]`: the benchmark unit tests (ADR 0030 D5) on a Linux machine with a systemd user manager.
//! - `bench-smoke <folder>`: the mechanics check of the benchmark runners (ADR 0030 section 5).
//! - `verify-tool-pin <file> <file.asc> [key.asc]`: checks a downloaded tool against the publisher's OpenPGP signature.
//!
//! Each task's logic is a pure function over text or arguments with unit tests; the thin edge starts programs through
//! `paddock-proc` (bounded, the whole tree ended at the limit).

mod bench_smoke;
mod linux_units;
mod tool_pin;

use paddock_proc::{command, run_bounded, Limits};
use std::process::ExitCode;
use std::time::Duration;

/// What a program printed and how it ended.
pub(crate) struct Ran {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `program args` bounded by `timeout`, in `dir` and with `env` added (or removed, `None`).
pub(crate) fn run(program: &str, args: &[&str], env: &[(&str, Option<&str>)], timeout: Duration) -> Result<Ran, String> {
    let mut cmd = command(program);
    cmd.args(args);
    for (k, v) in env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    let out = run_bounded(cmd, Limits::new(timeout)).map_err(|e| format!("could not run `{program}`: {e}"))?;
    Ok(Ran {
        success: out.success(),
        stdout: out.stdout_text(),
        stderr: out.stderr_text(),
    })
}

const USAGE: &str = "usage: cargo xtask <task>\n\ntasks:\n  linux-units [machine]                      run the benchmark unit tests on a Linux machine with a systemd user manager (default paddock-gate-arm)\n  bench-smoke <folder>                       check the benchmark runners' mechanics (folder holding dyno-bench and dyno-bench-gpu)\n  verify-tool-pin <file> <file.asc> [key]    check a downloaded tool's OpenPGP signature and print the pack lines\n  help                                       show this message";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("linux-units") => linux_units::run_task(&args[1..]),
        Some("bench-smoke") => bench_smoke::run_task(&args[1..]),
        Some("verify-tool-pin") => tool_pin::run_task(&args[1..]),
        Some("help" | "-h" | "--help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown task `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
