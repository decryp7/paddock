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
//! **Blocking since 2026-11-24 (user, recorded in ADR 0098's status).** A failing gate exits 5 (a gate that could not look, 6),
//! which is the integrator's cue to take the member out and send it back. It was report-only from 2026-11-13 (ER-41, D3 (a)): the
//! result printed with a `::warning::` and the exit code unchanged; [`ENFORCED`] false restores that.

use super::generator_step::{Step, checkout_problem};
use super::{Merge, git::Git, short};
use paddock_cli::{Exit, Outcome, Severity, Subject};
use paddock_proc::{Limits, command, run_bounded};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

/// Whether a failing member gate changes the exit code: true since the user's decision of 2026-11-24 (ADR 0098's status). No first
/// green run on a real train was recorded (ER-41's record, D3 (a)): the user flipped it on the check-value audit's evidence instead.
pub const ENFORCED: bool = true;

/// An outer bound on the gate run (ER-32). `check --pre-push` has no total of its own: each row has its own cap on its own run time
/// ([`ROW_CAP`](crate::pre_push::exec::ROW_CAP) and the grace beyond it for the stack capture), and a wait for the heavy slot goes on
/// while the queue moves and ends only after [`QUEUE_STALL`](crate::pre_push::exec::QUEUE_STALL) without progress. So this one only
/// ends a hang of the gate itself and must never end a healthy run: at least one worst row (a whole wait for the slot, the row cap and
/// its grace) and one heavy build; the figure is the measured one below.
///
/// Measured cause (ER-12, docs/benchmarks.md, "Pre-push heavy queue progress gaps"): the longest push gate run seen is 95 minutes (5,705 s,
/// the push of fix/push-gate-queue-stall on 2026-11-21: 27 rows passed, each waiting its turn behind other agents' steps; the refused
/// pushes of the same day ran 33 to 76 minutes); 24 hours is 15 times that. A backstop for a hung gate, not a judgement: the row cap and
/// the queue's progress end every normal run first.
const TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

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
    /// Runs `check --pre-push --member-gate --base <base>` in `root`; an error is a failure to start.
    fn run(&self, root: &Path, base: &str) -> Result<GateRun, String>;
}

/// The program that runs the gates: this tool, or `PADDOCK_MERGE_CHECK_GATES` (tests point it at a stand-in).
fn program() -> Result<std::path::PathBuf, String> {
    match std::env::var_os("PADDOCK_MERGE_CHECK_GATES") {
        Some(p) => Ok(p.into()),
        None => crate::tool_cache::self_exe().map_err(|e| format!("this program cannot be found: {e}")),
    }
}

/// The real gates: `<this tool> check --pre-push --member-gate --base <base> --root <root>`.
pub struct Real;

impl Gates for Real {
    fn run(&self, root: &Path, base: &str) -> Result<GateRun, String> {
        let mut cmd = command(program()?);
        cmd.args(["--json", "check", "--pre-push", "--member-gate", "--base", base, "--root"])
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
    run_from(root, git, merge, gates, enforced, None)
}

/// [`run`] measured from `base` (`--gates-base`) instead of the merge's first parent: the rows of the whole range, so a later push of
/// the same tree finds a pass record for each of them.
pub fn run_from(root: &Path, git: &Git, merge: &Merge, gates: &dyn Gates, enforced: bool, base: Option<&str>) -> Step {
    let Some(first) = merge.parents.first() else {
        return not_run("the merge has no parents");
    };
    let base = base.unwrap_or(first.as_str());
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
        vec![
            format!("member gate: passed (`check --pre-push --member-gate --base {base}`)"),
            run.text.clone(),
        ],
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
            "{level}::member gate NOT MEASURED: {why}. Next step: run `./dev check --pre-push --member-gate --base {base}` yourself in this worktree and fix what it prints; a member whose gates did not give a verdict on this merge is not a member that passed, and not a member that failed.{report_only}"
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
            format!("`check --pre-push --member-gate --base {base}` {how} and printed no outcome (an exit code alone proves nothing)"),
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
                    "{level}::member gate FAILED: `check --pre-push --member-gate --base {base}` found problems on the combined tree. Next step: take the member out (reset the train to {base} in your own worktree) and send it back to its author with the rows above; do not fix it on the train.{report_only}"
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

// ENFORCED is the user's decision recorded in ADR 0098's status (2026-11-24); turning it off again is a change of that record too.
#[cfg(test)]
const _: () = assert!(ENFORCED);

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
            // The next step names the member-gate command: a plain `check --pre-push` skips the audits and would not reproduce the finding.
            assert!(text.contains("`check --pre-push --member-gate --base "), "{text}");
        }
    }

    #[test]
    fn a_gate_that_could_not_look_is_not_a_pass_even_while_report_only() {
        let s = map(&answer(Some(6), nm()), false);
        assert_eq!(s.exit, Exit::Ok);
        let text = s.text.join("\n");
        assert!(text.contains("NOT MEASURED"));
        assert!(text.contains("run `./dev check --pre-push --member-gate --base "), "{text}");
        assert!(!text.contains("./dev check --pre-push --base"), "{text}");
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
        assert!(
            s.text.join("\n").contains("`check --pre-push --member-gate --base "),
            "the pass names the command that ran"
        );
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

    /// The member gate's outer bound is never shorter than one worst row (a whole wait for the slot, then the whole row cap with its
    /// grace) plus one heavy build: a shorter one would end a healthy long run as "could not run" before the gate's own guards could say
    /// anything.
    #[test]
    fn the_outer_bound_outlasts_everything_the_gate_allows_itself() {
        use crate::pre_push::exec::WAIT_BACKSTOP;
        use crate::pre_push::exec::{CAP_GRACE, ROW_CAP};
        use crate::pre_push::heavy_tool::BUILD_LIMIT;
        let worst_row = ROW_CAP + CAP_GRACE + WAIT_BACKSTOP;
        assert!(TIMEOUT > worst_row + BUILD_LIMIT + Duration::from_secs(5 * 60), "{TIMEOUT:?}");
        assert_eq!(
            TIMEOUT,
            Duration::from_secs(24 * 60 * 60),
            "the figure written next to the constant"
        );
    }

    /// The mapping without the checkout precondition.
    fn map(g: &Fake, enforced: bool) -> Step {
        interpret(g.0.clone(), "abc1234", enforced, &subject())
    }
}
