//! The member gate of `./dev merge check --gates` (ADR 0098 WP-1, D1 (b)): after the detectors, the fast gates of
//! `./dev check --pre-push` run on the merge commit with `--base` the train before the merge (`<merge>^1`), so the plan is the
//! member's own change and the gates see the combined tree: Clippy (the canonicalize ban), conformance (C53, C71), commit-lint,
//! lockfiles and the generator checks. `pre_push` does the work; this module only starts it (`--json`) and reads the outcome it
//! prints, through `Outcome::from_wire` against the merge's commit and tree (ADR 0089 amendment, WP-T5). The exit code alone is
//! never read as a pass: exit 0 also means "nothing to check" and, on its own branch, "a known commit", and neither says this
//! merge was measured. A `known-commit` is refused here (this asks about the merged tree, not about a commit proven earlier).
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
use paddock_cli::{Exit, Outcome, Severity, Subject};
use paddock_proc::{Limits, command, run_bounded};
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
    /// The first JSON object on its standard output (`--json`), when it printed one.
    pub wire: Option<serde_json::Value>,
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
        cmd.args(["--json", "check", "--pre-push", "--base", base, "--root"])
            .arg(root)
            .current_dir(root);
        let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("`check --pre-push` could not run: {e}"))?;
        let code = match out.outcome {
            paddock_proc::Outcome::Exited(st) => st.code(),
            _ => None,
        };
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let wire = stdout
            .lines()
            .find(|l| l.starts_with('{'))
            .and_then(|l| serde_json::from_str(l).ok());
        let text = format!("{}\n{}", stdout, out.stderr_text());
        Ok(GateRun {
            code,
            text: text.trim().to_string(),
            wire,
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
    interpret(gates.run(root, base), short(base), enforced, &merge.subject())
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

fn passed(base: &str, run: &GateRun) -> Step {
    step(
        Exit::Ok,
        "passed",
        base,
        vec![format!("member gate: passed (`check --pre-push --base {base}`)"), run.text.clone()],
        &run.text,
    )
}

/// The step for what `check --pre-push` answered: its outcome object, read for this merge's commit and tree. A pass is only an
/// outcome that says `clean` with evidence for exactly this subject; findings are only findings with evidence for it too; the exit
/// code decides nothing by itself, and every other answer (no object, no evidence, another commit, a known commit, not applicable, a
/// broken check, a bound that ended the run) is not measured.
fn interpret(answer: Result<GateRun, String>, base: &str, enforced: bool, subject: &Subject) -> Step {
    let (level, report_only) = wording(enforced);
    let not_measured = |why: String, text: Vec<String>, rows: &str| {
        let mut lines = text;
        lines.push(format!(
            "{level}::member gate NOT MEASURED: {why}. Next step: run `./dev check --pre-push --base {base}` yourself in this worktree and fix what it prints; a member whose gates did not give a verdict on this merge is not a member that passed, and not a member that failed.{report_only}"
        ));
        step(report(enforced, Exit::NotMeasured), "not measured", base, lines, rows)
    };
    let run = match answer {
        Err(why) => return not_measured(why, Vec::new(), ""),
        Ok(run) => run,
    };
    let Some(wire) = run.wire.as_ref().and_then(|w| w.get("outcome")) else {
        let how = run
            .code
            .map_or("was ended before it finished".to_string(), |c| format!("exited {c}"));
        return not_measured(
            format!("`check --pre-push --base {base}` {how} and printed no outcome (an exit code alone proves nothing)"),
            vec![run.text.clone()],
            &run.text,
        );
    };
    match Outcome::from_wire(wire, subject, false) {
        // The exit must agree with the object: a pass that exited non-zero, or findings that exited 0, is an answer that contradicts itself.
        Outcome::Clean(_) if run.code == Some(0) => passed(base, &run),
        Outcome::Findings(f) if run.code == Some(0) && !f.findings().as_slice().iter().any(|x| x.severity == Severity::Failing) => {
            passed(base, &run)
        }
        Outcome::Findings(f) if run.code == Some(5) && f.findings().as_slice().iter().any(|x| x.severity == Severity::Failing) => step(
            report(enforced, Exit::Findings),
            "failed",
            base,
            vec![
                run.text.clone(),
                format!(
                    "{level}::member gate FAILED: `check --pre-push --base {base}` found problems on the combined tree. Next step: take the member out (reset the train to {base} in your own worktree) and send it back to its author with the rows above; do not fix it on the train.{report_only}"
                ),
            ],
            &run.text,
        ),
        other => {
            let said = match &other {
                Outcome::Clean(_) | Outcome::Findings(_) => {
                    format!("its outcome ({}) and its exit code ({:?}) disagree", other.name(), run.code)
                }
                _ => other.detail(),
            };
            not_measured(said, vec![run.text.clone()], &run.text)
        }
    }
}

// Flip ENFORCED only in the change that records the first green train run in ADR 0098 (this fails the build until it is decided).
#[cfg(test)]
const _: () = assert!(!ENFORCED);

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_cli::outcome::{Sha, ToolVersion, TreeId, UnixSeconds};
    use paddock_cli::{CheckId, Finding, measure};
    use serde_json::Value;

    struct Fake(Result<GateRun, String>);
    impl Gates for Fake {
        fn run(&self, _root: &Path, _base: &str) -> Result<GateRun, String> {
            self.0.clone()
        }
    }

    fn subject() -> Subject {
        Subject::Commit {
            sha: Sha::new("m1"),
            tree: TreeId::new("t1"),
        }
    }

    /// An outcome the way the push gate makes it: through `measure`, for `subject`.
    fn made(subject: Subject, findings: Vec<Finding>) -> Value {
        measure(CheckId::new("pre-push"), subject, ToolVersion::new("t"), UnixSeconds(1), || {
            Ok(findings)
        })
        .json()
    }

    fn answer(code: Option<i32>, outcome: Option<Value>) -> Fake {
        Fake(Ok(GateRun {
            code,
            text: "fmt ok\nclippy FAILED".into(),
            wire: outcome.map(|o| serde_json::json!({ "outcome": o })),
        }))
    }

    fn clean() -> Option<Value> {
        Some(made(subject(), Vec::new()))
    }

    fn failing() -> Option<Value> {
        Some(made(subject(), vec![Finding::failing("clippy")]))
    }

    fn nm() -> Option<Value> {
        Some(serde_json::json!({"status": "not-measured", "reason": "no docker"}))
    }

    // The checkout precondition and the two-member merge are covered through the real binary in tests/it/merge_check.rs; here
    // only the mapping from the gates' answer to the step is pinned.
    #[test]
    fn a_failing_gate_changes_the_exit_only_when_enforced() {
        for (enforced, want) in [(false, Exit::Ok), (true, Exit::Findings)] {
            let s = map(&answer(Some(5), failing()), enforced);
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
        let s = map(&answer(Some(6), nm()), false);
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("NOT MEASURED"));
        assert_eq!(map(&answer(Some(6), nm()), true).exit, Exit::NotMeasured);
        assert_eq!(map(&Fake(Err("no tool".into())), true).exit, Exit::NotMeasured);
        // Only findings with evidence for this merge are a failed gate. A broken tool (7), a usage error (2), any other exit and a
        // run ended by the bound or a signal did not look: a member is never sent back for the tool's own failure.
        for code in [Some(7), Some(2), Some(1), Some(137), None] {
            let s = map(&answer(code, None), true);
            assert_eq!(s.exit, Exit::NotMeasured, "{code:?}");
            assert!(!s.text.join("\n").contains("member gate FAILED"), "{code:?}");
        }
        assert_eq!(map(&answer(Some(5), failing()), true).exit, Exit::Findings);
    }

    /// A pass is an outcome with evidence for this merge, and only that.
    #[test]
    fn a_pass_needs_evidence_for_this_merge_and_an_exit_code_alone_is_never_one() {
        let s = map(&answer(Some(0), clean()), true);
        assert_eq!((s.exit, s.data["outcome"].as_str()), (Exit::Ok, Some("passed")));
    }

    /// "Skip the gate": a stand-in that exits 0 with no JSON, JSON without evidence, evidence for another commit, a known commit,
    /// a not-applicable run and an outcome that contradicts the exit are all not measured.
    #[test]
    fn skipping_the_gate_cannot_pass() {
        let other = Subject::Commit {
            sha: Sha::new("m2"),
            tree: TreeId::new("t1"),
        };
        let known =
            serde_json::json!({"status": "known-commit", "not_rechecked": true, "evidence": made(subject(), Vec::new())["evidence"]});
        let standins: Vec<Fake> = vec![
            answer(Some(0), None),
            answer(Some(0), Some(serde_json::json!({"status": "clean"}))),
            answer(Some(0), Some(serde_json::json!({"status": "passed"}))),
            answer(Some(0), Some(made(other, Vec::new()))),
            answer(Some(0), Some(known)),
            answer(
                Some(0),
                Some(serde_json::json!({"status": "not-applicable", "reason": "nothing changed"})),
            ),
            answer(Some(5), clean()),
            answer(Some(0), failing()),
            Fake(Ok(GateRun {
                code: Some(0),
                text: "ok".into(),
                wire: Some(serde_json::json!({"gates": []})),
            })),
        ];
        for g in &standins {
            let s = map(g, true);
            assert_eq!(s.exit, Exit::NotMeasured, "{:?}", g.0);
            assert_ne!(s.data["outcome"], "passed", "{:?}", g.0);
        }
    }

    /// The mapping without the checkout precondition.
    fn map(g: &Fake, enforced: bool) -> Step {
        interpret(g.0.clone(), "abc1234", enforced, &subject())
    }
}
