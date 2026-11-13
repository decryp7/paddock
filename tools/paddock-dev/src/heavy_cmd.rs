//! `dev heavy -- <command...>`: run a command through the machine-wide heavy slot (ER-24), found by the one resolver the push gate and
//! `./dev gen check` use ([`crate::pre_push::exec::heavy_command_for`]: `PADDOCK_HEAVY`, `heavy.sh` or `heavy` on PATH, else the
//! `heavy` built from tools/heavy). When none can be found it is an error with the next step: never an unthrottled run.

use crate::pre_push::exec::{BUDGET, heavy_command_for, on_path};
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Arguments of `dev heavy`.
#[derive(Args, Debug)]
pub struct HeavyArgs {
    /// The command and its arguments (after `--`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub command: Vec<String>,
}

/// The full command line to run, or why there is no slot to run it through.
pub fn plan(
    command: &[String],
    env: &HashMap<String, String>,
    has: &dyn Fn(&str) -> bool,
    root: &Path,
    built: &OnceLock<Result<PathBuf, String>>,
) -> Result<Vec<String>, String> {
    let prefix = heavy_command_for(root, env, has, built, BUDGET).map_err(|why| {
        format!("dev heavy: no machine-wide heavy slot, so `{}` was not started: {why}. Next: put `heavy` or `heavy.sh` on PATH, set PADDOCK_HEAVY, or fix the build of tools/heavy", command[0])
    })?;
    Ok(prefix.into_iter().chain(command.iter().cloned()).collect())
}

/// `heavy`, against the real machine.
pub fn command(a: &HeavyArgs) -> Result<Report, CliError> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?;
    // The tree being committed in (a hook runs in an app's folder): heavy is built from this tree's tools/heavy.
    let root = crate::worktree::git::Git::at(&cwd)
        .text(&["rev-parse", "--show-toplevel"])
        .map(|t| PathBuf::from(t.trim()))
        .map_err(|e| CliError::failure(format!("dev heavy: {e}")))?;
    let built = OnceLock::new();
    let line = plan(&a.command, &env, &|p| on_path(&env, p), &root, &built).map_err(CliError::failure)?;
    let (program, args) = line.split_first().ok_or_else(|| CliError::failure("dev heavy: no command"))?;
    let status = paddock_proc::command(program)
        .args(args)
        .status()
        .map_err(|e| CliError::failure(format!("dev heavy: cannot start {program}: {e}")))?;
    if status.success() {
        return Ok(Report::new(json!({ "ok": true }), ""));
    }
    // The wrapped command's own status is ours: the exit code, or 128 plus the signal on Unix.
    let code = status.code().unwrap_or_else(|| signal_code(&status));
    eprintln!(
        "::error::dev heavy: `{}` ended with {status}. Next: fix what it printed above and commit again.",
        a.command[0]
    );
    std::process::exit(code);
}

#[cfg(unix)]
fn signal_code(status: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map_or(1, |s| 128 + s)
}

#[cfg(not(unix))]
fn signal_code(_: &std::process::ExitStatus) -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> Vec<String> {
        ["cargo", "clippy"].map(String::from).to_vec()
    }

    #[test]
    fn no_slot_tool_is_an_error_naming_the_command_and_the_next_step_never_a_plain_run() {
        let built = OnceLock::new();
        built.set(Err("the build of heavy failed".into())).unwrap();
        let err = plan(&cmd(), &HashMap::new(), &|_| false, Path::new("/nonexistent"), &built).unwrap_err();
        assert!(
            err.contains("`cargo` was not started") && err.contains("the build of heavy failed") && err.contains("Next:"),
            "{err}"
        );
    }

    #[test]
    fn paddock_heavy_and_a_tool_on_path_are_put_in_front_of_the_command() {
        let built = OnceLock::new();
        let env = HashMap::from([("PADDOCK_HEAVY".to_string(), "/x/heavy.sh".to_string())]);
        assert_eq!(
            plan(&cmd(), &env, &|_| false, Path::new("/nonexistent"), &built).unwrap(),
            ["/x/heavy.sh", "cargo", "clippy"]
        );
        let line = plan(&cmd(), &HashMap::new(), &|p| p == "heavy.sh", Path::new("/nonexistent"), &built).unwrap();
        assert_eq!(line, ["heavy.sh", "cargo", "clippy"]);
    }

    #[test]
    fn the_override_off_without_the_test_variable_is_an_error() {
        let built = OnceLock::new();
        let env = HashMap::from([("PADDOCK_HEAVY".to_string(), "off".to_string())]);
        assert!(plan(&cmd(), &env, &|_| false, Path::new("/nonexistent"), &built).is_err());
    }
}
