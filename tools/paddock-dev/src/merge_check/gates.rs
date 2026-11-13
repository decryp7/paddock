//! The member gate of `./dev merge check --gates` (ADR 0098 WP-1, D1 (b)): after the detectors, the fast gates of
//! `./dev check --pre-push` run on the merge commit with `--base` the train before the merge (`<merge>^1`), so the plan is the
//! member's own change and the gates see the combined tree: Clippy (the canonicalize ban), conformance (C53, C71), commit-lint,
//! lockfiles and the generator checks. `pre_push` does the work; this module only starts it and reads its exit code, so nothing
//! here depends on its internals.
//!
//! The checkout must be the merge result (the gates run in it, as the generator step does): the integrator has just merged the
//! member in their own worktree, so HEAD is the merge. Any other checkout is NOT MEASURED, never a pass (ER-33).
//!
//! **Report-only until the first green run (ER-41, D3 (a)).** While [`ENFORCED`] is false the result is printed and the exit code
//! is unchanged, with a `::warning::` line that says the member would be sent back. The first green run on a real train is
//! recorded in ADR 0098 (WP-1) by the train's head; the change that records it flips [`ENFORCED`], and a failing gate then
//! exits 5 (a gate that could not look, 6), which is the integrator's cue to take the member out and send it back.

use super::generator_step::{Step, checkout_problem};
use super::{Merge, git::Git, short};
use paddock_cli::Exit;
use paddock_proc::{Limits, Outcome, command, run_bounded};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

/// Whether a failing member gate changes the exit code. False until ADR 0098 records the first green run on a real train.
pub const ENFORCED: bool = false;

/// An outer bound on the gate run (ER-32): `check --pre-push` has its own 15 minute budget; this one only ends a hang.
const TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// What the gates said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRun {
    /// The exit code of `check --pre-push` (`None`: it was ended by the bound or a signal).
    pub code: Option<i32>,
    /// What it printed (standard output, then standard error), trimmed.
    pub text: String,
}

/// Starts the fast gates of a checkout against a base.
pub trait Gates {
    /// Runs `check --pre-push --base <base>` in `root`; an error is a failure to start.
    fn run(&self, root: &Path, base: &str) -> Result<GateRun, String>;
}

/// The program that runs the gates: this tool, or `PADDOCK_MERGE_CHECK_GATES` (tests point it at a stand-in).
fn program() -> Result<std::path::PathBuf, String> {
    match std::env::var_os("PADDOCK_MERGE_CHECK_GATES") {
        Some(p) => Ok(p.into()),
        None => std::env::current_exe().map_err(|e| format!("this program cannot be found: {e}")),
    }
}

/// The real gates: `<this tool> check --pre-push --base <base> --root <root>`.
pub struct Real;

impl Gates for Real {
    fn run(&self, root: &Path, base: &str) -> Result<GateRun, String> {
        let mut cmd = command(program()?);
        cmd.args(["check", "--pre-push", "--base", base, "--root"])
            .arg(root)
            .current_dir(root);
        let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("`check --pre-push` could not run: {e}"))?;
        let code = match out.outcome {
            Outcome::Exited(st) => st.code(),
            _ => None,
        };
        let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), out.stderr_text());
        Ok(GateRun {
            code,
            text: text.trim().to_string(),
        })
    }
}

fn step(exit: Exit, outcome: &str, base: &str, text: Vec<String>, rows: &str) -> Step {
    Step {
        text,
        data: json!({"outcome": outcome, "base": base, "enforced": ENFORCED, "rows": rows}),
        exit,
    }
}

/// Said out loud when the step was not asked for or cannot apply; not a failure.
pub fn not_run(why: &str) -> Step {
    Step {
        text: vec![format!("member gate not run ({why})")],
        data: json!({"outcome": "not run", "why": why}),
        exit: Exit::Ok,
    }
}

/// Runs the gates on the merge `merge` (checked out in `root`) and turns the answer into a step. `enforced` is [`ENFORCED`] outside
/// the tests.
pub fn run(root: &Path, git: &Git, merge: &Merge, gates: &dyn Gates, enforced: bool) -> Step {
    let Some(base) = merge.parents.first() else {
        return not_run("the merge has no parents");
    };
    if let Some((why, next)) = checkout_problem(git, merge) {
        let (level, report_only) = wording(enforced);
        return step(
            report(enforced, Exit::NotMeasured),
            "not measured",
            short(base),
            vec![format!(
                "{level}::member gate NOT MEASURED: {why}. Next step: {next}; a member whose gates did not run is not a member that passed.{report_only}"
            )],
            "",
        );
    }
    interpret(gates.run(root, base), short(base), enforced)
}

/// What a failure costs: nothing while report-only, the exit code once enforced.
fn report(enforced: bool, enforced_exit: Exit) -> Exit {
    if enforced { enforced_exit } else { Exit::Ok }
}

fn wording(enforced: bool) -> (&'static str, String) {
    if enforced {
        ("::error", String::new())
    } else {
        (
            "::warning",
            " REPORT-ONLY until its first green run on a real train is recorded in ADR 0098 (ER-41): the exit code is unchanged.".into(),
        )
    }
}

/// The step for what `check --pre-push` answered.
fn interpret(answer: Result<GateRun, String>, base: &str, enforced: bool) -> Step {
    let (level, report_only) = wording(enforced);
    match answer {
        Err(why) => step(
            report(enforced, Exit::NotMeasured),
            "not measured",
            base,
            vec![format!(
                "{level}::member gate NOT MEASURED: {why}. Next step: run `./dev check --pre-push --base {base}` yourself in this worktree; a member whose gates did not run is not a member that passed.{report_only}"
            )],
            "",
        ),
        Ok(GateRun { code: Some(0), text }) => step(
            Exit::Ok,
            "passed",
            base,
            vec![format!("member gate: passed (`check --pre-push --base {base}`)"), text.clone()],
            &text,
        ),
        Ok(GateRun { code: Some(6), text }) => step(
            report(enforced, Exit::NotMeasured),
            "not measured",
            base,
            vec![
                text.clone(),
                format!(
                    "{level}::member gate NOT MEASURED: some gate of `check --pre-push --base {base}` could not look. Next step: fix what its rows name and run it again; a gate that did not look is not a gate that passed.{report_only}"
                ),
            ],
            &text,
        ),
        Ok(GateRun { code, text }) => {
            let how = code.map_or("was ended before it finished".to_string(), |c| format!("exited {c}"));
            step(
                report(enforced, Exit::Findings),
                "failed",
                base,
                vec![
                    text.clone(),
                    format!(
                        "{level}::member gate FAILED: `check --pre-push --base {base}` {how}; the merge of this member fails on the combined tree. Next step: take the member out (reset the train to {base} in your own worktree) and send it back to its author with the rows above; do not fix it on the train.{report_only}"
                    ),
                ],
                &text,
            )
        }
    }
}

// Flip ENFORCED only in the change that records the first green train run in ADR 0098 (this fails the build until it is decided).
#[cfg(test)]
const _: () = assert!(!ENFORCED);

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Result<GateRun, String>);
    impl Gates for Fake {
        fn run(&self, _root: &Path, _base: &str) -> Result<GateRun, String> {
            self.0.clone()
        }
    }

    fn run_of(code: Option<i32>) -> Fake {
        Fake(Ok(GateRun {
            code,
            text: "fmt ok\nclippy FAILED".into(),
        }))
    }

    // The checkout precondition and the two-member merge are covered through the real binary in tests/it/merge_check.rs; here
    // only the mapping from the gates' answer to the step is pinned.
    #[test]
    fn a_failing_gate_changes_the_exit_only_when_enforced() {
        for (enforced, want) in [(false, Exit::Ok), (true, Exit::Findings)] {
            let s = map(&run_of(Some(5)), enforced);
            assert_eq!(s.exit, want, "enforced={enforced}");
            let text = s.text.join("\n");
            assert!(
                text.contains(if enforced {
                    "::error::member gate FAILED"
                } else {
                    "::warning::member gate FAILED"
                }),
                "{text}"
            );
            assert_eq!(text.contains("REPORT-ONLY"), !enforced);
        }
    }

    #[test]
    fn a_gate_that_could_not_look_is_not_a_pass_even_while_report_only() {
        let s = map(&run_of(Some(6)), false);
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("NOT MEASURED"));
        assert_eq!(map(&run_of(Some(6)), true).exit, Exit::NotMeasured);
        assert_eq!(map(&Fake(Err("no tool".into())), true).exit, Exit::NotMeasured);
        assert_eq!(map(&run_of(None), true).exit, Exit::Findings);
    }

    /// The mapping without the checkout precondition.
    fn map(g: &Fake, enforced: bool) -> Step {
        interpret(g.0.clone(), "abc1234", enforced)
    }
}
