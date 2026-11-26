//! The member's own CI trial, dispatched by `./dev merge sync-member` once the static rows passed (user decision 90, 2026-11-26).
//!
//! The failure it prevents (three trains): members that passed on the Mac failed CI only on Windows or Linux, found at the train's
//! run. The trial is the member's narrowed run on the train, through the command that already exists for it: `./dev ci dispatch
//! ci.yml --trial <member> --onto <train> --max <duration>` merges `origin/<member>` onto the train in the tool's own clone, runs the
//! merge check on it, pushes `trial/<member>` and dispatches ci.yml with `base` = the train tip, so the plan's affected jobs run
//! (and only them), on the legs the plan names. It prints `run <id>`; this step prints it too. `./dev merge check --gates` reads the
//! run later ([`super::member_trial`]).
//!
//! The trial merges what origin holds, so the sync tip is first pushed there as the scratch ref `sync/<member>` (the integrator's
//! role: the integrator runs sync-member and may push shared branches; the member's own branch stays the author's) and the trial is of
//! that ref (`trial/sync/<member>`). A push that fails is NOT MEASURED, nothing dispatched.
//!
//! `--no-trial "<reason>"` skips it and logs the reason, like `--old-base-anyway`.

use super::gates::GateRun;
use paddock_cli::Exit;
use paddock_proc::{Limits, command, run_bounded};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

/// The default of `--trial-max`: the longest the dispatch waits for a busy runner.
pub const DEFAULT_MAX: &str = "30m";

/// The dispatch waits for the runner at most `--trial-max` and then ends itself; this is the outer hang detector, wide beyond any
/// sensible `--trial-max`, not a speed limit.
const TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// Starts the trial.
pub trait Dispatcher {
    /// `ci dispatch ci.yml --trial <member> --onto <train> --max <max>` in `root`; an error is a failure to start.
    fn dispatch(&self, root: &Path, member: &str, train: &str, max: &str) -> Result<GateRun, String>;
}

/// The real dispatcher (this tool, or `PADDOCK_SYNC_TRIAL_DISPATCH`: the tests' stand-in).
pub struct Real;

impl Dispatcher for Real {
    fn dispatch(&self, root: &Path, member: &str, train: &str, max: &str) -> Result<GateRun, String> {
        let program = match std::env::var_os("PADDOCK_SYNC_TRIAL_DISPATCH") {
            Some(p) => p.into(),
            None => crate::tool_cache::self_exe().map_err(|e| format!("this program cannot be found: {e}"))?,
        };
        let mut cmd = command(program);
        cmd.args([
            "ci", "dispatch", "ci.yml", "--trial", member, "--onto", train, "--max", max, "--root",
        ])
        .arg(root)
        .current_dir(root);
        let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("`ci dispatch` could not run: {e}"))?;
        let code = match out.outcome {
            paddock_proc::Outcome::Exited(st) => st.code(),
            _ => None,
        };
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        Ok(GateRun {
            code,
            text: format!("{}\n{}", stdout, out.stderr_text()).trim().to_string(),
            wire: None,
        })
    }
}

/// What a step of the trial says: lines, data, exit.
pub struct Step {
    /// The lines.
    pub text: Vec<String>,
    /// The structured answer.
    pub data: serde_json::Value,
    /// `Ok`, or `NotMeasured` (a trial that was not started is not a pass).
    pub exit: Exit,
}

/// The line printed when `--no-trial` skips the trial.
pub fn skipped(reason: &str) -> Step {
    Step {
        text: vec![format!(
            "::warning::merge sync-member: no CI trial dispatched (--no-trial: {reason})"
        )],
        data: json!({"outcome": "skipped", "reason": reason}),
        exit: Exit::Ok,
    }
}

/// Said out loud when there is nothing to dispatch yet; not a failure.
pub fn not_run(why: &str) -> Step {
    Step {
        text: vec![format!("CI trial not dispatched ({why})")],
        data: json!({"outcome": "not run", "why": why}),
        exit: Exit::Ok,
    }
}

/// The scratch ref the sync tip is pushed to for the trial: `sync/<member>`.
pub fn sync_ref(member: &str) -> String {
    format!("sync/{member}")
}

/// The sync tip could not be pushed: nothing is dispatched.
pub fn not_pushed(synced: &str, why: &str) -> Step {
    let why = format!("the sync tip could not be pushed to origin as {synced}: {}", why.trim());
    Step {
        text: vec![format!(
            "::error::merge sync-member: CI trial NOT DISPATCHED: {why}. Next step: fix the push (the integrator role pushes `sync/<member>` with a lease: a refusal as `stale info` means someone else moved it, so `git fetch origin sync/<member>` and look, or check the remote and your access), then run this again; or --no-trial \"<reason>\"."
        )],
        data: json!({"outcome": "not measured", "why": why}),
        exit: Exit::NotMeasured,
    }
}

/// The run id a dispatch printed: the last line that is exactly `run <digits>`.
pub fn run_id(text: &str) -> Option<u64> {
    text.lines()
        .rev()
        .filter_map(|l| l.trim().strip_prefix("run "))
        .find_map(|n| n.trim().parse().ok())
}

/// The step for what the dispatch answered: the run id on a dispatch that exited 0 and printed one; anything else is not a started trial.
pub fn interpret(answer: Result<GateRun, String>, member: &str, train: &str) -> Step {
    let fail = |why: String, tail: &str| Step {
        text: vec![
            tail.to_string(),
            format!(
                "::error::merge sync-member: CI trial NOT DISPATCHED: {why}. Next step: read the lines above, fix that on the member, push and re-sync; or run `./dev ci dispatch ci.yml --trial {member} --onto {train} --max {DEFAULT_MAX}` yourself; or --no-trial \"<reason>\". A member with no trial is not a member that passed."
            ),
        ],
        data: json!({"outcome": "not measured", "why": why}),
        exit: Exit::NotMeasured,
    };
    let run = match answer {
        Err(why) => return fail(why, ""),
        Ok(r) => r,
    };
    match (run.code, run_id(&run.text)) {
        (Some(0), Some(id)) => Step {
            text: vec![format!(
                "CI trial dispatched: run {id} (the affected jobs of {member} on {train}); read it with `./dev merge check --gates` or `./dev wait --ci-run {id} --max <duration>`"
            )],
            data: json!({"outcome": "dispatched", "run": id}),
            exit: Exit::Ok,
        },
        (Some(0), None) => fail(
            "`ci dispatch` exited 0 but printed no `run <id>` line (an exit code alone proves nothing)".into(),
            &run.text,
        ),
        (code, _) => fail(
            code.map_or("`ci dispatch` was ended before it finished".to_string(), |c| {
                format!("`ci dispatch` exited {c}")
            }),
            &run.text,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(code: i32, text: &str) -> Result<GateRun, String> {
        Ok(GateRun {
            code: Some(code),
            text: text.into(),
            wire: None,
        })
    }

    #[test]
    fn the_run_id_is_the_last_run_line() {
        assert_eq!(run_id("waiting\nrun 8123\n"), Some(8123));
        assert_eq!(run_id("run 1\nrun 2"), Some(2));
        assert_eq!(run_id("running tests\nrun abc"), None);
        assert_eq!(run_id(""), None);
    }

    #[test]
    fn a_dispatch_that_printed_a_run_is_started_and_names_it() {
        let s = interpret(ran(0, "guards ok\nrun 8123"), "feat/x", "origin/train/t");
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("run 8123"), "{:?}", s.text);
        assert_eq!(s.data["run"], 8123);
    }

    /// Positive control (ER-33): no run line, a failing dispatch and a dispatch that could not start are not a started trial.
    #[test]
    fn a_trial_that_was_not_started_is_not_a_pass() {
        for a in [
            ran(0, "guards ok"),
            ran(5, "::error::conflict in a.txt"),
            ran(1, "run 9"),
            Err("no tool".into()),
        ] {
            let s = interpret(a, "feat/x", "origin/train/t");
            assert_eq!(s.exit, Exit::NotMeasured);
            assert!(s.text.join("\n").contains("NOT DISPATCHED"), "{:?}", s.text);
        }
    }

    #[test]
    fn a_tip_that_cannot_be_pushed_dispatches_nothing() {
        assert_eq!(sync_ref("feat/x"), "sync/feat/x");
        let s = not_pushed("sync/feat/x", "permission denied");
        assert_eq!(s.exit, Exit::NotMeasured);
        assert!(s.text.join("\n").contains("NOT DISPATCHED") && s.text.join("\n").contains("permission denied"));
    }

    #[test]
    fn the_escape_is_logged_with_its_reason() {
        let s = skipped("docs-only");
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("--no-trial: docs-only"));
    }
}
