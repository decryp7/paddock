//! The train's ER-53 split: the FAST rows run locally first, CI is dispatched, and the SLOW rows (the macOS host tests, `tests <ws>`)
//! run beside it; a slow failure cancels the run (`./dev ci cancel <run-id>`), and a landing (ER-49) needs BOTH the complete green CI
//! run and the green slow rows on the same commit (user, 2026-11-26: Linux tests are CI's Linux jobs, no container runs on this Mac).
//!
//! - [`RowSet`] picks the rows of `./dev check --pre-push --set fast|slow|all`.
//! - A slow run writes a [`SlowRecord`] for the commit it checked: first unfinished, then finished with each row's verdict. A record
//!   that is missing, unfinished, for another commit or holds a row that was not measured is "not held" (ER-33), never a pass.
//! - [`landing`] is the one place that combines the CI verdict with the [`SlowState`].
//!
//! Pure over its inputs; the record's folder is passed in.

use super::exec::{Kind, Row};
use super::gates::Planned;
use clap::ValueEnum;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Which rows of the plan a run takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum RowSet {
    /// Every row (a developer's push, the member gate as before).
    #[default]
    All,
    /// Everything but the slow rows: fmt, lockfiles, commit-lint, gates-register, conformance, Clippy (host and Windows),
    /// conformance-tests, member-audits and the merge-check rows. Run first; CI is dispatched when it is green.
    Fast,
    /// The slow rows: the host tests (`tests <ws>`). Run beside CI after the dispatch.
    Slow,
}

/// Whether the row `id` is a slow row: the macOS host tests of the member gate, `tests` and `tests <workspace>`.
pub fn is_slow(id: &str) -> bool {
    id == "tests" || id.starts_with("tests ")
}

fn id_of(p: &Planned) -> &str {
    match p {
        Planned::Run(g) => &g.id,
        Planned::Skip { id, .. } | Planned::NotMeasured { id, .. } | Planned::Broken { id, .. } => id,
    }
}

/// The rows of `set`, in the planned order.
pub fn select(planned: Vec<Planned>, set: RowSet) -> Vec<Planned> {
    planned
        .into_iter()
        .filter(|p| match set {
            RowSet::All => true,
            RowSet::Fast => !is_slow(id_of(p)),
            RowSet::Slow => is_slow(id_of(p)),
        })
        .collect()
}

/// One step of the train's flow, in order (the commands to run for the commit `sha` of the train branch, `dispatch` being the full
/// `./dev ci dispatch` line the integrator uses, which prints the `run <id>` handle the later steps name).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// What the step is.
    pub what: &'static str,
    /// The command line.
    pub command: String,
}

/// The ER-53 flow: the fast rows locally, THEN the CI dispatch, the slow rows beside it (a slow failure cancels the run), and last the
/// landing check, which needs both halves. `run_id` is the handle the dispatch printed (`run <id>`), known only after step 2.
pub fn train_flow(dispatch: &str, run_id: &str) -> Vec<Step> {
    let step = |what, command: String| Step { what, command };
    vec![
        step(
            "fast rows, locally",
            "./dev check --pre-push --member-gate --priority train --set fast".into(),
        ),
        step("dispatch CI (prints `run <id>`)", dispatch.to_string()),
        step(
            "slow rows beside CI; a failure cancels the run",
            format!("./dev check --pre-push --member-gate --priority train --set slow --cancel-ci-run {run_id}"),
        ),
        step("landing needs both halves", "./dev ci landing-held --sha <sha>".into()),
    ]
}

/// What the slow rows of one commit came to.
#[derive(Debug, Clone, PartialEq)]
pub enum SlowState {
    /// Every slow row passed or does not apply, and the run finished.
    Green,
    /// A slow row failed (the ids).
    Failed(Vec<String>),
    /// Nothing is held: no record, an unfinished run, another commit's record, or a row that was not measured (why).
    NotHeld(String),
}

/// The record of a slow run on disk: `<state>/slow-rows/<sha>.json`.
pub fn record_path(state: &Path, sha: &str) -> PathBuf {
    state.join("slow-rows").join(format!("{sha}.json"))
}

fn write(state: &Path, sha: &str, value: &Value) -> Result<(), String> {
    let path = record_path(state, sha);
    std::fs::create_dir_all(path.parent().unwrap_or(state)).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(&path, value.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

/// A slow run on `sha` has begun: the record says unfinished until [`finish`] replaces it (a run that dies leaves "not held").
pub fn start(state: &Path, sha: &str) -> Result<(), String> {
    write(state, sha, &json!({ "sha": sha, "finished": false, "rows": [] }))
}

/// The verdict word stored for a row.
fn word(row: &Row) -> &'static str {
    match row.kind() {
        Kind::Passed => "passed",
        Kind::Skipped => "skipped",
        Kind::Failed => "failed",
        _ => "not-measured",
    }
}

/// The slow run on `sha` has ended: every slow row with its verdict.
pub fn finish(state: &Path, sha: &str, rows: &[(String, Row)]) -> Result<(), String> {
    let rows: Vec<Value> = rows
        .iter()
        .filter(|(id, _)| is_slow(id))
        .map(|(id, r)| json!({ "id": id, "verdict": word(r) }))
        .collect();
    write(state, sha, &json!({ "sha": sha, "finished": true, "rows": rows }))
}

/// Reads the record of `sha`: [`SlowState::NotHeld`] unless a finished run of this commit says every slow row passed or did not apply.
pub fn read(state: &Path, sha: &str) -> SlowState {
    let path = record_path(state, sha);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return SlowState::NotHeld(format!(
            "no slow-row run is recorded for {sha}: run `./dev check --pre-push --member-gate --set slow` on it"
        ));
    };
    state_of(&text, sha)
}

/// [`read`] over the record's text.
pub fn state_of(text: &str, sha: &str) -> SlowState {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return SlowState::NotHeld("the slow-row record cannot be read".into());
    };
    if v["sha"].as_str() != Some(sha) {
        return SlowState::NotHeld(format!("the slow-row record is for another commit than {sha}"));
    }
    if v["finished"] != true {
        return SlowState::NotHeld(format!(
            "the slow rows of {sha} have not finished (a run that was interrupted or is still going)"
        ));
    }
    let rows = v["rows"].as_array().cloned().unwrap_or_default();
    let ids = |want: &str| -> Vec<String> {
        rows.iter()
            .filter(|r| r["verdict"] == want)
            .filter_map(|r| r["id"].as_str().map(String::from))
            .collect()
    };
    let failed = ids("failed");
    if !failed.is_empty() {
        return SlowState::Failed(failed);
    }
    let unknown: Vec<String> = rows
        .iter()
        .filter(|r| !matches!(r["verdict"].as_str(), Some("passed" | "skipped")))
        .filter_map(|r| r["id"].as_str().map(String::from))
        .collect();
    if !unknown.is_empty() {
        return SlowState::NotHeld(format!("slow row(s) not measured: {}", unknown.join(", ")));
    }
    SlowState::Green
}

/// What CI said about the commit (the `ci verdict --landing` answer, reduced).
#[derive(Debug, Clone, PartialEq)]
pub enum Ci {
    /// A complete green run (or green with fix items, which a landing accepts).
    Green,
    /// Red, or only partial runs (why).
    Red(String),
    /// Not measured: no run, a run still going, a cancelled run, an unreadable list (why).
    NotMeasured(String),
}

/// Whether the commit may land (ER-49).
#[derive(Debug, Clone, PartialEq)]
pub enum Landing {
    /// The complete green CI run AND the green slow rows, on the same commit.
    Landable,
    /// Something failed (why): fix it.
    Refused(String),
    /// Something is not held (why): wait for it or run it; never a pass (ER-33).
    NotHeld(String),
}

/// The one rule: both the CI verdict and the slow rows must hold. A failure on either side refuses; otherwise anything unfinished or
/// unread is "not held".
pub fn landing(ci: &Ci, slow: &SlowState) -> Landing {
    if let Ci::Red(why) = ci {
        return Landing::Refused(format!("CI is not green: {why}"));
    }
    if let SlowState::Failed(ids) = slow {
        return Landing::Refused(format!("the local slow rows failed: {}", ids.join(", ")));
    }
    match (ci, slow) {
        (Ci::Green, SlowState::Green) => Landing::Landable,
        (Ci::NotMeasured(why), _) => Landing::NotHeld(format!("CI: {why}")),
        (_, SlowState::NotHeld(why)) => Landing::NotHeld(why.clone()),
        _ => Landing::NotHeld("unreachable combination".into()),
    }
}

/// Whether a finished slow run asks for the CI run to be cancelled: a failed slow row, and a run id to name (the handle `./dev ci
/// dispatch` printed). Anything else leaves the run alone.
pub fn should_cancel(rows: &[(String, Row)], run: Option<u64>) -> Option<u64> {
    let failed = rows.iter().any(|(id, r)| is_slow(id) && r.kind() == Kind::Failed);
    run.filter(|_| failed)
}

/// After a slow run: when [`should_cancel`] names a run, cancels exactly that run through `cancel` (the real one is `./dev ci cancel`'s
/// [`crate::ci_runs::cancel::cancel_run`]) and returns the line to print; a failed cancel is a `::warning::` (the run keeps going, the
/// row's own failure still refuses the landing).
pub fn cancel_failed_run(
    rows: &[(String, Row)],
    run: Option<u64>,
    cancel: &mut dyn FnMut(u64) -> Result<String, String>,
) -> Option<String> {
    let id = should_cancel(rows, run)?;
    Some(match cancel(id) {
        Ok(line) => format!("pre-push: a slow row failed, so CI run {id} is cancelled: {line}"),
        Err(e) => format!(
            "::warning::pre-push: a slow row failed but CI run {id} could not be cancelled ({e}); cancel it by hand: ./dev ci cancel {id}"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> Row {
        Row::skipped("n/a".to_string())
    }

    #[test]
    fn the_sets_split_the_plan_at_the_host_tests() {
        let skip = |id: &str| Planned::Skip {
            id: id.into(),
            reason: "r".into(),
        };
        let plan = vec![
            skip("fmt crates"),
            skip("tests crates"),
            skip("tests"),
            skip("conformance-tests"),
            skip("clippy crates"),
            skip("tests apps/dyno"),
        ];
        let ids = |set| select(plan.clone(), set).iter().map(|p| id_of(p).to_string()).collect::<Vec<_>>();
        assert_eq!(ids(RowSet::Slow), ["tests crates", "tests", "tests apps/dyno"]);
        assert_eq!(
            ids(RowSet::Fast),
            ["fmt crates", "conformance-tests", "clippy crates"],
            "conformance-tests is fast, not a `tests` row"
        );
        assert_eq!(ids(RowSet::All).len(), 6);
    }

    /// Fast first, then the dispatch, then the slow rows beside it, and the landing check last; the cancel names the dispatch's run id.
    #[test]
    fn the_train_flow_runs_the_fast_rows_then_dispatches_then_the_slow_rows_then_checks_the_landing() {
        let flow = train_flow("./dev ci dispatch ci.yml --ref train/x --max 40m", "9071");
        let at = |needle: &str| {
            flow.iter()
                .position(|s| s.command.contains(needle))
                .unwrap_or_else(|| panic!("{needle} missing: {flow:?}"))
        };
        assert!(
            at("--set fast") < at("ci dispatch") && at("ci dispatch") < at("--set slow") && at("--set slow") < at("landing-held"),
            "{flow:?}"
        );
        assert!(
            flow[2].command.contains("--cancel-ci-run 9071"),
            "the slow step names the run it may cancel"
        );
        assert!(
            !flow
                .iter()
                .any(|s| s.command.contains("docker") || s.command.contains("linux-container")),
            "no container on this Mac"
        );
    }

    #[test]
    fn a_slow_run_is_not_held_until_it_finished_on_the_same_commit() {
        let d = tempfile::tempdir().unwrap();
        assert!(matches!(read(d.path(), "abc"), SlowState::NotHeld(w) if w.contains("no slow-row run")));
        start(d.path(), "abc").unwrap();
        assert!(
            matches!(read(d.path(), "abc"), SlowState::NotHeld(w) if w.contains("not finished")),
            "an unfinished run is not held"
        );
        finish(
            d.path(),
            "abc",
            &[("tests crates".into(), Row::skipped("x")), ("fmt crates".into(), ok())],
        )
        .unwrap();
        assert_eq!(read(d.path(), "abc"), SlowState::Green);
        assert!(
            matches!(read(d.path(), "def"), SlowState::NotHeld(_)),
            "another commit has no record"
        );
        let text = std::fs::read_to_string(record_path(d.path(), "abc")).unwrap();
        assert!(matches!(state_of(&text, "zzz"), SlowState::NotHeld(w) if w.contains("another commit")));
        assert!(
            matches!(state_of("{", "abc"), SlowState::NotHeld(_)),
            "a corrupt record is not held"
        );
    }

    #[test]
    fn a_failed_or_unmeasured_slow_row_is_never_green() {
        let text = |verdict: &str| json!({"sha": "a", "finished": true, "rows": [{"id": "tests crates", "verdict": verdict}]}).to_string();
        assert_eq!(state_of(&text("failed"), "a"), SlowState::Failed(vec!["tests crates".into()]));
        assert!(matches!(state_of(&text("not-measured"), "a"), SlowState::NotHeld(_)));
        assert_eq!(state_of(&text("passed"), "a"), SlowState::Green);
    }

    /// Landing needs both halves on the same commit; an unfinished slow row is "not held", never a pass (ER-33).
    #[test]
    fn landing_needs_both_the_green_ci_run_and_the_green_slow_rows() {
        let unfinished = SlowState::NotHeld("not finished".into());
        assert_eq!(landing(&Ci::Green, &SlowState::Green), Landing::Landable);
        assert!(
            matches!(landing(&Ci::Green, &unfinished), Landing::NotHeld(_)),
            "CI green, slow rows unfinished"
        );
        assert!(matches!(
            landing(&Ci::NotMeasured("cancelled".into()), &SlowState::Green),
            Landing::NotHeld(_)
        ));
        assert!(matches!(landing(&Ci::Red("x".into()), &SlowState::Green), Landing::Refused(_)));
        assert!(matches!(
            landing(&Ci::Green, &SlowState::Failed(vec!["tests crates".into()])),
            Landing::Refused(_)
        ));
        assert!(
            matches!(
                landing(&Ci::NotMeasured("running".into()), &SlowState::Failed(vec!["t".into()])),
                Landing::Refused(_)
            ),
            "a failure refuses even while CI runs"
        );
        assert!(matches!(landing(&Ci::Red("x".into()), &unfinished), Landing::Refused(_)));
    }

    /// A slow failure cancels the run at once, by its handle, exactly once; a pass or a fast failure never does.
    #[test]
    fn a_slow_failure_cancels_that_run_once_and_nothing_else_does() {
        let failed = Row::failed(
            "tests crates",
            &crate::pre_push::exec::test_subject(),
            "t".into(),
            String::new(),
            "n".into(),
            1.0,
        );
        let mut cancelled: Vec<u64> = Vec::new();
        let mut cancel = |id: u64| {
            cancelled.push(id);
            Ok("cancel sent".to_string())
        };
        let line = cancel_failed_run(&[("tests crates".into(), failed.clone())], Some(9071), &mut cancel).expect("a line");
        assert!(line.contains("9071") && line.contains("cancelled"), "{line}");
        assert!(cancel_failed_run(&[("tests crates".into(), ok())], Some(9071), &mut cancel).is_none());
        assert!(cancel_failed_run(&[("fmt crates".into(), failed.clone())], Some(9071), &mut cancel).is_none());
        assert!(cancel_failed_run(&[("tests crates".into(), failed.clone())], None, &mut cancel).is_none());
        assert_eq!(cancelled, [9071], "one cancel, for the handle given");
        let mut broken = |_: u64| Err::<String, String>("HTTP 502".into());
        let warn = cancel_failed_run(&[("tests crates".into(), failed)], Some(9071), &mut broken).expect("a line");
        assert!(warn.starts_with("::warning::") && warn.contains("./dev ci cancel 9071"), "{warn}");
    }

    #[test]
    fn only_a_failed_slow_row_with_a_run_id_cancels_the_run() {
        let failed = Row::failed(
            "tests crates",
            &crate::pre_push::exec::test_subject(),
            "a test failed".into(),
            String::new(),
            "fix it".into(),
            1.0,
        );
        assert_eq!(should_cancel(&[("tests crates".into(), failed.clone())], Some(9071)), Some(9071));
        assert_eq!(
            should_cancel(&[("tests crates".into(), failed.clone())], None),
            None,
            "no handle, no cancel (never the newest run)"
        );
        assert_eq!(
            should_cancel(&[("fmt crates".into(), failed)], Some(9071)),
            None,
            "a fast row failing is not this command's business"
        );
        assert_eq!(should_cancel(&[("tests crates".into(), ok())], Some(9071)), None);
    }
}
