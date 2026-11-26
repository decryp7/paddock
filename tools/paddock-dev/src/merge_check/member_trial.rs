//! The member's own CI trial, read by `./dev merge check --gates` (user decision 90, 2026-11-26). `./dev merge sync-member` dispatches
//! the member's narrowed run on the train (`ci dispatch ci.yml --trial <member> --onto <train>`: the plan's affected jobs against
//! the train tip, on the legs the plan names); this step asks the one reader of the runs ([`crate::ci_runs::verdict::trial`], through
//! `ci trial-verdict`) what that run says about the member's head:
//!
//! - green: printed with the run id; red: one `::error::` and exit 5 (the member goes back to its author, who fixes it on the member
//!   and re-syncs, which dispatches a new trial);
//! - still running, no listed run of this head, or a run that could not be read: never a pass (ER-33), exit 6 for a run that is
//!   running or unreadable. A head with no trial run at all is only a warning while [`ENFORCE_NO_RUN`] is false (ER-41: it blocks after
//!   the first green train that used the trial is recorded in docs/lessons.md);
//! - `--no-trial "<reason>"` skips it and logs the reason, like `--old-base-anyway`.
//!
//! The reader runs as a child (`<this tool> --json ci trial-verdict ...`, or `PADDOCK_MERGE_CHECK_TRIAL`: the tests' stand-in), and the
//! answer is its `state`; an exit code alone is never read as a pass.

use super::gates::GateRun;
use super::generator_step::Step;
use paddock_cli::Exit;
use paddock_proc::{Limits, command, run_bounded};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

/// Whether a head with no trial run is refused. False (report-only, ER-41) until the first green train that used trials is recorded.
pub const ENFORCE_NO_RUN: bool = false;

/// The reader reads the run list over the network: a hang detector, wide (the API answers in seconds), not a speed limit.
const TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Asks what the member's trial says.
pub trait Reader {
    /// `ci trial-verdict --branch <branch> --member-sha <sha>`; an error is a failure to start.
    fn read(&self, root: &Path, branch: &str, member_sha: &str) -> Result<GateRun, String>;
}

/// The real reader (this tool, or `PADDOCK_MERGE_CHECK_TRIAL`).
pub struct Real;

impl Reader for Real {
    fn read(&self, root: &Path, branch: &str, member_sha: &str) -> Result<GateRun, String> {
        let program = match std::env::var_os("PADDOCK_MERGE_CHECK_TRIAL") {
            Some(p) => p.into(),
            None => crate::tool_cache::self_exe().map_err(|e| format!("this program cannot be found: {e}"))?,
        };
        let mut cmd = command(program);
        cmd.args(["--json", "ci", "trial-verdict", "--branch", branch, "--member-sha", member_sha])
            .current_dir(root);
        let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("`ci trial-verdict` could not run: {e}"))?;
        let code = match out.outcome {
            paddock_proc::Outcome::Exited(st) => st.code(),
            _ => None,
        };
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let wire = stdout
            .lines()
            .find(|l| l.starts_with('{'))
            .and_then(|l| serde_json::from_str(l).ok());
        Ok(GateRun {
            code,
            text: format!("{}\n{}", stdout, out.stderr_text()).trim().to_string(),
            wire,
        })
    }
}

/// The line printed when `--no-trial` skips the read.
pub fn skipped(reason: &str) -> Step {
    Step {
        text: vec![format!(
            "::warning::merge check: the member's trial run NOT read (--no-trial: {reason})"
        )],
        data: json!({"outcome": "skipped", "reason": reason}),
        exit: Exit::Ok,
    }
}

/// Said out loud when there is no member to ask about (a branch checked before it is merged, a member with no name).
pub fn not_run(why: &str) -> Step {
    Step {
        text: vec![format!("member trial not read ({why})")],
        data: json!({"outcome": "not run", "why": why}),
        exit: Exit::Ok,
    }
}

/// The step for what the reader answered.
pub fn interpret(answer: Result<GateRun, String>, branch: &str) -> Step {
    let redo = "run `./dev merge sync-member <train>` after merging the train into the member (it dispatches a new trial)";
    let not_measured = |why: String| Step {
        text: vec![format!(
            "::warning::merge check: the trial of {branch} NOT MEASURED: {why}. Next step: run `./dev ci trial-verdict --branch {branch} --member-sha <head>` yourself; a member whose trial was not read is not a member that passed."
        )],
        data: json!({"outcome": "not measured", "why": why}),
        exit: Exit::NotMeasured,
    };
    let run = match answer {
        Err(why) => return not_measured(why),
        Ok(r) => r,
    };
    let Some(state) = run.wire.as_ref().and_then(|w| w["state"].as_str()) else {
        let how = run
            .code
            .map_or("was ended before it finished".to_string(), |c| format!("exited {c}"));
        return not_measured(format!(
            "`ci trial-verdict` {how} and printed no state (an exit code alone proves nothing)"
        ));
    };
    let id = run.wire.as_ref().and_then(|w| w["run"].as_u64());
    let named = id.map_or(String::new(), |r| format!("run {r} "));
    let data = |outcome: &str| json!({"outcome": outcome, "run": id});
    match (state, run.code) {
        ("green", Some(0)) => Step {
            text: vec![format!(
                "member trial: {named}of {branch} is green (its own affected jobs on the train)"
            )],
            data: data("green"),
            exit: Exit::Ok,
        },
        ("red", Some(5)) => Step {
            text: vec![
                run.text.clone(),
                format!(
                    "::error::merge check: the trial {named}of {branch} is red. Next step: fix on the member, then re-sync ({redo}) and offer it again; `--no-trial \"<reason>\"` is the logged escape."
                ),
            ],
            data: data("red"),
            exit: Exit::Findings,
        },
        ("running", Some(6)) => Step {
            text: vec![format!(
                "::warning::merge check: the trial {named}of {branch} is still running: NOT a pass. Next step: wait for it (`./dev wait --ci-run <id> --max <duration>`), then run this again."
            )],
            data: data("running"),
            exit: Exit::NotMeasured,
        },
        ("no-run", Some(6)) => Step {
            text: vec![format!(
                "::warning::merge check: no trial run of this head of {branch} is listed: not a pass. Next step: {redo}.{}",
                if ENFORCE_NO_RUN {
                    ""
                } else {
                    " Report-only (ER-41): the exit code is unchanged until the first green train that used trials is recorded."
                }
            )],
            data: data("no-run"),
            exit: if ENFORCE_NO_RUN { Exit::NotMeasured } else { Exit::Ok },
        },
        ("not-measured", Some(6)) => not_measured(run.text.clone()),
        (other, code) => not_measured(format!(
            "its state ({other:?}) and its exit code ({code:?}) disagree or are unknown"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(state: &str, code: i32) -> Result<GateRun, String> {
        Ok(GateRun {
            code: Some(code),
            text: format!("trial of b: {state}"),
            wire: Some(json!({"ok": true, "state": state, "run": 77})),
        })
    }

    #[test]
    fn green_is_a_pass_that_names_the_run() {
        let s = interpret(answer("green", 0), "b");
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("run 77 of b is green"));
    }

    #[test]
    fn red_blocks_with_one_error_and_the_next_step() {
        let s = interpret(answer("red", 5), "b");
        assert_eq!(s.exit, Exit::Findings);
        let t = s.text.join("\n");
        assert_eq!(t.matches("::error::").count(), 1, "{t}");
        assert!(t.contains("fix on the member, then re-sync") && t.contains("--no-trial"), "{t}");
    }

    /// Positive control (ER-33): a run that is still going, an unreadable one, no state and a state that disagrees with the exit are never a pass.
    #[test]
    fn running_unread_and_contradicting_answers_are_not_a_pass() {
        let s = interpret(answer("running", 6), "b");
        assert_eq!(s.exit, Exit::NotMeasured);
        assert!(s.text.join("\n").contains("still running") && s.text.join("\n").contains("NOT a pass"));
        assert_eq!(interpret(answer("not-measured", 6), "b").exit, Exit::NotMeasured);
        assert_eq!(interpret(Err("no tool".into()), "b").exit, Exit::NotMeasured);
        let none = Ok(GateRun {
            code: Some(0),
            text: String::new(),
            wire: None,
        });
        assert_eq!(interpret(none, "b").exit, Exit::NotMeasured);
        assert_eq!(
            interpret(answer("green", 5), "b").exit,
            Exit::NotMeasured,
            "green that exited 5 contradicts itself"
        );
        assert_eq!(
            interpret(answer("red", 0), "b").exit,
            Exit::NotMeasured,
            "red that exited 0 is not a pass"
        );
        assert_eq!(interpret(answer("purple", 0), "b").exit, Exit::NotMeasured);
    }

    #[test]
    fn a_head_with_no_trial_run_is_a_warning_while_report_only() {
        let s = interpret(answer("no-run", 6), "b");
        assert_eq!(s.exit, if ENFORCE_NO_RUN { Exit::NotMeasured } else { Exit::Ok });
        assert!(s.text.join("\n").contains("not a pass"));
    }

    #[test]
    fn the_escape_is_logged_with_its_reason() {
        let s = skipped("flaky job, rerun by hand");
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("--no-trial: flaky job, rerun by hand"));
    }
}
