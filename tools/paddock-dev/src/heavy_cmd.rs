//! `dev heavy -- <command...>`: run a command through the machine-wide heavy slot (ER-24), found by the one function
//! `check --pre-push` uses ([`crate::pre_push::exec::heavy_prefix`]), so a git hook does not depend on `heavy.sh` being on PATH.
//! When no slot tool is found it prints one warning line and runs the command plain; it never stays silently unwrapped.

use crate::pre_push::exec::{heavy_prefix, sibling_heavy};
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

/// Arguments of `dev heavy`.
#[derive(Args, Debug)]
pub struct HeavyArgs {
    /// The command and its arguments (after `--`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub command: Vec<String>,
}

/// The full command line to run, and the warning to print when no slot tool was found.
pub fn plan(command: &[String], env: &HashMap<String, String>, has: &dyn Fn(&str) -> bool, root: &Path) -> (Vec<String>, Option<String>) {
    let prefix = heavy_prefix(env, has, &|| sibling_heavy(root, env));
    let warn = prefix.is_empty().then(|| {
        "dev heavy: no heavy slot tool found (set PADDOCK_HEAVY or ENGINEERING_RULES, or put heavy.sh on PATH): running without the machine-wide slot".to_string()
    });
    (prefix.into_iter().chain(command.iter().cloned()).collect(), warn)
}

/// `heavy`, against the real machine.
pub fn command(a: &HeavyArgs) -> Result<Report, CliError> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let path = env.get("PATH").cloned().unwrap_or_default();
    let has = |p: &str| std::env::split_paths(&path).any(|d| d.join(p).is_file());
    let root = std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?;
    let (line, warn) = plan(&a.command, &env, &has, &root);
    if let Some(w) = warn {
        eprintln!("{w}");
    }
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
    fn when_no_slot_tool_is_found_the_command_is_unchanged_with_a_warning_and_paddock_heavy_wins() {
        let env = HashMap::from([("ENGINEERING_RULES".to_string(), "/nonexistent".to_string())]);
        // no PATH tool, no sibling file: one warning, the command unchanged
        let (line, warn) = plan(&cmd(), &env, &|_| false, Path::new("/nonexistent"));
        assert_eq!(line, cmd());
        assert!(warn.unwrap().contains("no heavy slot tool found"));
        // PADDOCK_HEAVY wins
        let env = HashMap::from([("PADDOCK_HEAVY".to_string(), "/x/heavy.sh".to_string())]);
        let (line, warn) = plan(&cmd(), &env, &|_| false, Path::new("/nonexistent"));
        assert_eq!(line, ["/x/heavy.sh", "cargo", "clippy"]);
        assert!(warn.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn engineering_rules_variable_finds_the_script_off_path() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/heavy");
        let env = HashMap::from([("ENGINEERING_RULES".to_string(), dir.to_string())]);
        let (line, warn) = plan(&cmd(), &env, &|_| false, Path::new("/nonexistent"));
        assert_eq!(line, [format!("{dir}/scripts/heavy.sh"), "cargo".into(), "clippy".into()]);
        assert!(warn.is_none());
    }
}
