//! `dev throttle -- <command...>`: run a command through the machine-wide heavy slot (ER-24), found by the one resolver the push gate and
//! `./dev gen check` use ([`crate::pre_push::exec::throttle_command_for`]: `PADDOCK_THROTTLE` or `heavy` on PATH, else the
//! `heavy` built from tools/throttle). When none can be found it is an error with the next step: never an unthrottled run.

use crate::pre_push::exec::{on_path, throttle_command_for};
use crate::pre_push::heavy_tool::BUILD_LIMIT;
use crate::tool_cache::Held;
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Arguments of `dev throttle`.
#[derive(Args, Debug)]
pub struct ThrottleArgs {
    /// End the command when it has written nothing and its processes used no CPU for this many seconds (heavy's hang detector,
    /// `heavy run --stall`; passed as `THROTTLE_STALL`). Off unless given: pick it from the step's measured quiet gaps, with a wide margin.
    #[arg(long, value_name = "SECONDS")]
    pub stall: Option<f64>,
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
    built: &OnceLock<Result<Held, String>>,
) -> Result<Vec<String>, String> {
    let prefix = throttle_command_for(root, env, has, built, BUILD_LIMIT).map_err(|why| {
        format!("dev throttle: no machine-wide throttle slot, so `{}` was not started: {why}. Next: put `throttle` on PATH, set PADDOCK_THROTTLE, or fix the build of tools/throttle", command[0])
    })?;
    Ok(prefix.into_iter().chain(command.iter().cloned()).collect())
}

/// A line on stderr when the command was typed with its old name (`./dev heavy`, `./dev heavy-install`): the alias works until the old
/// name is retired, and says so once per call.
pub fn deprecated_name(old: &str, new: &str) {
    if std::env::args().nth(1).as_deref() == Some(old) {
        eprintln!("::warning::`./dev {old}` is now `./dev {new}`; the old name still works for now");
    }
}

/// `throttle`, against the real machine.
pub fn command(a: &ThrottleArgs) -> Result<Report, CliError> {
    deprecated_name("heavy", "throttle");
    if let Some(s) = a.stall
        && !(s.is_finite() && s > 0.0)
    {
        return Err(CliError::usage(format!("dev throttle: --stall {s} is not a number of seconds > 0")));
    }
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?;
    // The tree being committed in (a hook runs in an app's folder): heavy is built from this tree's tools/throttle.
    let root = crate::worktree::git::Git::at(&cwd)
        .text(&["rev-parse", "--show-toplevel"])
        .map(|t| PathBuf::from(t.trim()))
        .map_err(|e| CliError::failure(format!("dev throttle: {e}")))?;
    let built = OnceLock::new();
    let line = plan(&a.command, &env, &|p| on_path(&env, p), &root, &built).map_err(CliError::failure)?;
    let (program, args) = line.split_first().ok_or_else(|| CliError::failure("dev throttle: no command"))?;
    let mut cmd = paddock_proc::command(program);
    cmd.args(args);
    for (k, v) in child_env(a.stall, &env, &|| crate::places::dev_targets(&cwd, &|k| env.get(k).cloned()).ok()) {
        cmd.env(k, v);
    }
    // heavy's lock folder, which a reboot keeps (crate::pre_push::exec::throttle_dir_env).
    cmd.envs(crate::pre_push::exec::throttle_dir_env(&root, &env));
    let status = cmd
        .status()
        .map_err(|e| CliError::failure(format!("dev throttle: cannot start {program}: {e}")))?;
    // The hold ends with the command (and before `exit`, which runs no destructors).
    drop(built);
    if status.success() {
        return Ok(Report::new(json!({ "ok": true }), ""));
    }
    // The wrapped command's own status is ours: the exit code, or 128 plus the signal on Unix.
    let code = status.code().unwrap_or_else(|| signal_code(&status));
    eprintln!(
        "::error::dev throttle: `{}` ended with {status}. Next: fix what it printed above and commit again.",
        a.command[0]
    );
    std::process::exit(code);
}

/// What `dev heavy` adds to heavy's environment: `THROTTLE_STALL` from `--stall`, and `PADDOCK_DEV_TARGETS` (where heavy writes its run
/// logs, `<dev-targets>/runs`) from the one derivation, `places.rs`, unless the caller set it (ER-47: the caller's value wins).
fn child_env(stall: Option<f64>, env: &HashMap<String, String>, dev_targets: &dyn Fn() -> Option<PathBuf>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(s) = stall {
        out.push(("THROTTLE_STALL".to_string(), s.to_string()));
    }
    if !env.contains_key(crate::places::DEV_TARGETS_ENV)
        && let Some(d) = dev_targets()
    {
        out.push((crate::places::DEV_TARGETS_ENV.to_string(), d.to_string_lossy().into_owned()));
    }
    out
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
        let env = HashMap::from([("PADDOCK_THROTTLE".to_string(), "/x/heavy.sh".to_string())]);
        assert_eq!(
            plan(&cmd(), &env, &|_| false, Path::new("/nonexistent"), &built).unwrap(),
            ["/x/heavy.sh", "cargo", "clippy"]
        );
        let line = plan(&cmd(), &HashMap::new(), &|p| p == "throttle", Path::new("/nonexistent"), &built).unwrap();
        assert_eq!(line, ["throttle", "run", "pre-push", "--", "cargo", "clippy"]);
    }

    #[test]
    fn stall_and_the_dev_targets_folder_reach_heavy_and_a_callers_folder_wins() {
        let found = || Some(PathBuf::from("/dt"));
        assert_eq!(
            child_env(Some(600.0), &HashMap::new(), &found),
            [
                ("THROTTLE_STALL".to_string(), "600".to_string()),
                ("PADDOCK_DEV_TARGETS".to_string(), "/dt".to_string())
            ]
        );
        let env = HashMap::from([("PADDOCK_DEV_TARGETS".to_string(), "/mine".to_string())]);
        assert_eq!(child_env(None, &env, &found), []);
        assert_eq!(child_env(None, &HashMap::new(), &|| None), []);
    }

    #[test]
    fn the_override_off_without_the_test_variable_is_an_error() {
        let built = OnceLock::new();
        let env = HashMap::from([("PADDOCK_THROTTLE".to_string(), "off".to_string())]);
        assert!(plan(&cmd(), &env, &|_| false, Path::new("/nonexistent"), &built).is_err());
    }
}
