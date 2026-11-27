//! `dev ci dyno-linux-units`: Dyno's release step 2c in CI (user decision 2026-11-26: no VM or container on the developer's Mac, so
//! the OrbStack machine this check used to drive is gone). The benchmark-unit tests (ADR 0030 D5) run on the Linux runner in the
//! `linux-units` job of `dyno-release.yml`. They need a systemd user manager and are `#[ignore]`d in a plain `cargo test`. Where the
//! job's container has none, the check is NOT MEASURED (exit 6), never a pass (ER-33): the run says what the manager reported and
//! what to change. Ported behaviour for behaviour from the shell script of the branch chore/dyno-linux-units-on-ci (f36010990), which never landed:
//!
//! 1. `XDG_RUNTIME_DIR` is the caller's, or `/run/user/<uid>` when it is unset or empty.
//! 2. `systemctl --user is-system-running` gives the manager's state (a failure to run it, a non-zero exit and any stderr are not an
//!    error here: the state is then empty and said as `none`). The line `linux-units: user manager: <state>` is printed first.
//! 3. `running` and `degraded` are a manager. Any other state ends with one `::error::` line (the state quoted, `nothing` when empty)
//!    and exit 6.
//! 4. In `apps/dyno`, `cargo test -p dyno-agent --test bench_units -- --ignored --test-threads=1`, its output streamed. It failing
//!    ends with one `::error::` line and exit 1 (as the script did, not 5); passing prints `linux-units: passed` and exits 0.
//!
//! New against the script: the cargo run has a time limit (ER-32), a hang detector with a wide margin and no measurement yet:
//! the job has never run on the runner (UNVERIFIED), so its first run is the measurement.

use crate::ci_tool::{NETWORK_ENV, Output, ToolCall};
use clap::Args;
use paddock_cli::{CliError, Exit, Report};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The hang detector of the cargo run: one hour. Not a speed limit and not measured (the job has no green run on the runner yet,
/// ER-12): the tests of `bench_units.rs` each wait for a few seconds, so a run that takes an hour is a hang.
const CARGO_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Arguments of `dev ci dyno-linux-units`.
#[derive(Args, Debug)]
pub struct DynoLinuxUnitsArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// What the step needs from the machine.
pub trait Machine {
    /// The caller's user id (`id -u`).
    fn uid(&self) -> String;
    /// `systemctl --user is-system-running`'s standard output, trimmed; empty when it could not run or printed nothing.
    fn manager_state(&self, runtime_dir: &str) -> String;
    /// Runs the benchmark-unit tests in `dir`, their output streamed; `Ok(passed)`, `Err` when cargo could not start.
    fn bench_tests(&self, dir: &Path, runtime_dir: &str) -> Result<bool, String>;
}

/// `XDG_RUNTIME_DIR` as the caller has it, else `/run/user/<uid>` (the shell's `${XDG_RUNTIME_DIR:-/run/user/$(id -u)}`).
pub fn runtime_dir(env: &HashMap<String, String>, uid: impl FnOnce() -> String) -> String {
    match env.get("XDG_RUNTIME_DIR") {
        Some(v) if !v.is_empty() => v.clone(),
        _ => format!("/run/user/{}", uid()),
    }
}

/// Whether `is-system-running` names a usable user manager.
pub fn has_manager(state: &str) -> bool {
    matches!(state, "running" | "degraded")
}

/// The line that ends a run without a manager.
pub fn not_measured_line(state: &str) -> String {
    let said = if state.is_empty() { "nothing" } else { state };
    format!(
        "::error::linux-units: no systemd user manager here (is-system-running said '{said}'): the benchmark unit tests are NOT MEASURED. Next step: run this job on a runner whose container has systemd as init and a user session (docs/linux.md), or record the first green run that moves the job from report-only to blocking."
    )
}

/// The line that ends a run whose tests failed.
pub const FAILED_LINE: &str = "::error::linux-units: the benchmark unit tests failed: read the output above; the Linux runner must not ship units until they pass (docs/linux.md)";

/// The step against `machine`: progress lines to `out` as they happen; the exit code and the last line in the report.
pub fn run(machine: &dyn Machine, env: &HashMap<String, String>, root: &Path, out: &mut dyn Write) -> Report {
    let dir = runtime_dir(env, || machine.uid());
    let state = machine.manager_state(&dir);
    let _ = writeln!(out, "linux-units: user manager: {}", if state.is_empty() { "none" } else { &state });
    let report = |exit: Exit, outcome: &str, text: String| Report::new(json!({"outcome": outcome, "manager": state}), text).with_exit(exit);
    if !has_manager(&state) {
        return report(Exit::NotMeasured, "not measured", not_measured_line(&state));
    }
    match machine.bench_tests(&root.join("apps/dyno"), &dir) {
        Ok(true) => report(Exit::Ok, "passed", "linux-units: passed".to_string()),
        Ok(false) => report(Exit::Failure, "failed", FAILED_LINE.to_string()),
        Err(why) => report(
            Exit::Failure,
            "failed",
            format!("{FAILED_LINE}\n::error::linux-units: cargo could not run: {why}"),
        ),
    }
}

/// The real machine.
struct Real {
    env: HashMap<String, String>,
}

impl Real {
    fn tool(&self, program: &str) -> ToolCall {
        ToolCall::new(program)
            .pass(NETWORK_ENV.iter().copied())
            .pass(["DBUS_SESSION_BUS_ADDRESS"])
    }
}

impl Machine for Real {
    fn uid(&self) -> String {
        self.tool("id")
            .args(["-u"])
            .timeout(Duration::from_secs(30))
            .run(&self.env, &mut std::io::sink())
            .map(|r| r.stdout.trim().to_string())
            .unwrap_or_default()
    }

    fn manager_state(&self, runtime_dir: &str) -> String {
        self.tool("systemctl")
            .args(["--user", "is-system-running"])
            .env("XDG_RUNTIME_DIR", runtime_dir)
            .timeout(Duration::from_secs(60))
            .run(&self.env, &mut std::io::sink())
            .map(|r| r.stdout.trim().to_string())
            .unwrap_or_default()
    }

    fn bench_tests(&self, dir: &Path, runtime_dir: &str) -> Result<bool, String> {
        let call = self
            .tool("cargo")
            .args([
                "test",
                "-p",
                "dyno-agent",
                "--test",
                "bench_units",
                "--",
                "--ignored",
                "--test-threads=1",
            ])
            .pass(crate::child_env::PASS.iter().copied())
            .pass(["RUST_*", "LD_LIBRARY_PATH"])
            .env("XDG_RUNTIME_DIR", runtime_dir)
            .cwd(dir)
            .timeout(CARGO_TIMEOUT)
            .output(Output::Stream);
        call.run(&self.env, &mut std::io::stdout()).map(|r| r.success)
    }
}

/// `dev ci dyno-linux-units`, against the real machine.
pub fn command(a: &DynoLinuxUnitsArgs) -> Result<Report, CliError> {
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let root = match &a.root {
        Some(r) => r.clone(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?;
            crate::worktree::git::Git::at(&cwd)
                .text(&["rev-parse", "--show-toplevel"])
                .map(|t| PathBuf::from(t.trim()))
                .map_err(|e| CliError::failure(format!("dyno-linux-units: {e}")))?
        }
    };
    Ok(run(&Real { env: env.clone() }, &env, &root, &mut std::io::stdout()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Fake {
        state: &'static str,
        passes: Result<bool, &'static str>,
        cargo_calls: Cell<u32>,
        seen: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(state: &'static str, passes: Result<bool, &'static str>) -> Fake {
            Fake {
                state,
                passes,
                cargo_calls: Cell::new(0),
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl Machine for Fake {
        fn uid(&self) -> String {
            "1234".into()
        }
        fn manager_state(&self, runtime_dir: &str) -> String {
            self.seen.borrow_mut().push(runtime_dir.to_string());
            self.state.to_string()
        }
        fn bench_tests(&self, dir: &Path, runtime_dir: &str) -> Result<bool, String> {
            self.cargo_calls.set(self.cargo_calls.get() + 1);
            self.seen.borrow_mut().push(format!("{} {runtime_dir}", dir.display()));
            self.passes.map_err(str::to_string)
        }
    }

    fn go(f: &Fake, env: &[(&str, &str)]) -> (Report, String) {
        let env: HashMap<String, String> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut out = Vec::new();
        let r = run(f, &env, Path::new("repo"), &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_running_or_degraded_manager_runs_the_tests_in_apps_dyno_and_a_pass_exits_0() {
        for state in ["running", "degraded"] {
            let f = Fake::new(state, Ok(true));
            let (r, out) = go(&f, &[("XDG_RUNTIME_DIR", "/run/user/7")]);
            assert_eq!(r.exit, Exit::Ok);
            assert_eq!(out, format!("linux-units: user manager: {state}\n"));
            assert_eq!(r.text, "linux-units: passed");
            assert_eq!(
                f.seen.borrow().last().unwrap(),
                &format!("{} /run/user/7", Path::new("repo").join("apps/dyno").display())
            );
        }
    }

    /// Positive control (ER-33): hide the manager and the step is exit 6 with the error line, and cargo never ran: not a pass.
    #[test]
    fn no_manager_is_not_measured_exit_6_and_the_tests_do_not_run() {
        for state in ["", "offline", "stopping", "starting", "maintenance", "initializing", "unknown"] {
            let f = Fake::new(state, Ok(true));
            let (r, out) = go(&f, &[]);
            assert_eq!(r.exit, Exit::NotMeasured, "{state:?}");
            assert_eq!(f.cargo_calls.get(), 0, "{state:?}");
            assert!(
                out.contains(&format!("user manager: {}", if state.is_empty() { "none" } else { state })),
                "{out}"
            );
            assert!(
                r.text.starts_with("::error::linux-units: no systemd user manager here"),
                "{}",
                r.text
            );
            assert!(
                r.text
                    .contains(&format!("said '{}'", if state.is_empty() { "nothing" } else { state })),
                "{}",
                r.text
            );
            assert!(r.text.contains("NOT MEASURED") && r.text.contains("Next step"), "{}", r.text);
        }
    }

    #[test]
    fn failing_tests_exit_1_with_one_error_line_and_a_cargo_that_cannot_start_is_a_failure_too() {
        let f = Fake::new("running", Ok(false));
        let (r, _) = go(&f, &[]);
        assert_eq!(r.exit, Exit::Failure);
        assert_eq!(r.text, FAILED_LINE);
        let f = Fake::new("running", Err("cargo is not installed on this runner (or not on PATH)."));
        let (r, _) = go(&f, &[]);
        assert_eq!(r.exit, Exit::Failure);
        assert!(r.text.contains("cargo could not run"), "{}", r.text);
    }

    #[test]
    fn the_runtime_dir_is_the_callers_else_run_user_uid_and_an_empty_value_counts_as_unset() {
        let env = |v: Option<&str>| -> HashMap<String, String> {
            v.map(|v| ("XDG_RUNTIME_DIR".to_string(), v.to_string())).into_iter().collect()
        };
        assert_eq!(runtime_dir(&env(Some("/x")), || unreachable!()), "/x");
        assert_eq!(runtime_dir(&env(Some("")), || "42".into()), "/run/user/42");
        assert_eq!(runtime_dir(&env(None), || "42".into()), "/run/user/42");
    }
}
