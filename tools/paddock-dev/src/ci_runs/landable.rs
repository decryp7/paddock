//! `Landable`: a train may land on a full run that is red only by failures ER-12 lets it land on. It is a train-only value, never a
//! [`super::verdict::Verdict`]: the reader's `Green` stays the one answer the tag gate takes (a tag needs a green run), and this
//! is a separate, sealed answer of one function, built only from
//!
//! * the newest complete full run of that exact commit (the reader's own `Red { run: Some }`), finished `failure`, whose earlier
//!   attempts (a rerun keeps the run id) hold no failed job, and
//! * that run's triage document: the same run and commit, `ok`, `skipped_jobs` 0, counts that add up to the rows.
//!
//! It passes only when every row is a named test, never a job, and is either a FLAKY test the caller accepts (the ER-12 check is the
//! caller's `flaky_ok`) or ALREADY FAILING ON MAIN (the baseline is a main run). A NEW, UNKNOWN or FAILED row, a job row, a skipped
//! job, another run or commit refuses it.

use super::client::{Client, RunInfo, TimedJob};
use super::verdict::{Earlier, RunResult, Verdict, read_earlier, run_result, verdict};
use crate::plan_since::{job_needs, key_by_name, name_templates};
use crate::wait::CurlHttp;
use serde_json::Value;

/// A full run that may be landed on, with what was accepted. Only [`judge`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landable {
    /// The run.
    pub run: u64,
    /// How many failing rows were accepted FLAKY tests.
    pub flaky: usize,
    /// How many were already failing on main.
    pub already_failing: usize,
    /// One line per accepted row, for the log.
    pub lines: Vec<String>,
    _seal: Seal,
}

/// Private: nothing outside this module can name it, so nothing outside can build a [`Landable`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seal;

impl Landable {
    /// A value for the tests of the callers (sequence, landing): the real one only comes from [`judge`].
    #[cfg(test)]
    pub fn for_test(run: u64, flaky: usize, already_failing: usize, lines: &[&str]) -> Landable {
        Landable {
            run,
            flaky,
            already_failing,
            lines: lines.iter().map(|l| l.to_string()).collect(),
            _seal: Seal,
        }
    }

    /// "landed on a full run with N accepted FLAKY, M already failing on main".
    pub fn summary(&self) -> String {
        format!(
            "full run {} with {} accepted FLAKY, {} already failing on main",
            self.run, self.flaky, self.already_failing
        )
    }
}

/// The checks of the run itself, before its triage: the commit, the workflow, `failure` and clean earlier attempts.
pub fn check_run(run: &RunInfo, workflow: &str, sha: &str, earlier: &mut dyn FnMut(&RunInfo) -> Earlier) -> Result<(), String> {
    if run.workflow() != workflow {
        return Err(format!("run {} is a run of {}, not of {workflow}", run.id, run.workflow()));
    }
    if run.head_sha != sha {
        return Err(format!("run {} tested {}, not {sha}", run.id, run.head_sha));
    }
    match run_result(run) {
        RunResult::Failure => {}
        other => return Err(format!("run {} is {other:?}, not a finished failure", run.id)),
    }
    if run.run_attempt > 1 {
        match earlier(run) {
            Earlier::Clean => {}
            Earlier::Failed(what) => {
                return Err(format!(
                    "run {} was rerun after failed jobs ({what}) that no triage covers: run it in full again",
                    run.id
                ));
            }
            Earlier::Unknown(why) => return Err(format!("run {}'s earlier attempts could not be read ({why})", run.id)),
        }
    }
    Ok(())
}

/// The names of the jobs that were skipped because a job they `need` failed (a failed job hides its dependents: they end `skipped`
/// and name no failure). The needs and the display-name templates are read from the workflow text with `plan_since`'s readers; a
/// job whose name matches no single job key is not placed (it cannot be judged). Dependents of dependents count.
pub fn skipped_behind_failed(jobs: &[TimedJob], workflow_text: &str) -> Vec<String> {
    let templates = name_templates(workflow_text);
    let needs = job_needs(workflow_text);
    let mut failed: std::collections::BTreeSet<String> = jobs
        .iter()
        .filter(|j| j.failed())
        .filter_map(|j| key_by_name(&j.name, &templates))
        .collect();
    let mut hidden: Vec<String> = Vec::new();
    loop {
        let mut added = false;
        let pending: Vec<&TimedJob> = jobs
            .iter()
            .filter(|j| j.conclusion == "skipped" && !hidden.contains(&j.name))
            .collect();
        for j in pending {
            let Some(key) = key_by_name(&j.name, &templates) else { continue };
            let behind = needs
                .iter()
                .find(|(k, _)| *k == key)
                .is_some_and(|(_, n)| n.iter().any(|n| n != "plan" && failed.contains(n)));
            if behind {
                hidden.push(j.name.clone());
                failed.insert(key);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    hidden
}

/// The app a failing row belongs to: the `app <name>` of its job, else the workflow's own app (`debrief.yml` is `debrief`; `ci.yml`
/// is no app). `None` when it cannot be told, which no accepted test may be.
pub fn app_of_job(workflow: &str, job: &str) -> Option<String> {
    let head = job.split(" (").next().unwrap_or(job);
    if let Some(app) = head.strip_prefix("app ") {
        return Some(app.trim().to_string());
    }
    let stem = workflow.strip_suffix(".yml")?;
    (stem != "ci" && !stem.contains('-')).then(|| stem.to_string())
}

/// Builds a [`Landable`] for the full `workflow` run of `sha` (see the module). `workflow_text` is the workflow file at `sha` (for
/// the `needs` of its jobs); `flaky_ok(app, test)` is the caller's ER-12 check of a FLAKY row's test name and its app (empty when it
/// cannot be told). `Err` is the reason it is not landable (a green run needs no `Landable`).
pub fn landable(
    (server, repo, token): (&str, &str, &str),
    workflow: &str,
    sha: &str,
    workflow_text: &str,
    flaky_ok: &dyn Fn(&str, &str) -> Result<(), String>,
) -> Result<Landable, String> {
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, server, repo, token);
    let run = match verdict(&mut client, workflow, sha) {
        Verdict::Red { run: Some(id), .. } => id,
        Verdict::Red { run: None, reason } => return Err(format!("no full {workflow} run of {sha} to land on: {reason}")),
        Verdict::NotMeasured(why) => return Err(format!("not measured: {why}")),
        Verdict::Green { .. } => return Err("the run is green: no Landable is needed".into()),
    };
    let info = client.run(run)?.ok_or_else(|| format!("run {run} is gone"))?;
    {
        let cell = std::cell::RefCell::new(&mut client);
        check_run(&info, workflow, sha, &mut |r| read_earlier(&mut cell.borrow_mut(), r))?;
    }
    let jobs = client
        .timed_jobs(run)?
        .ok_or_else(|| format!("the server has no job list for run {run}"))?;
    let hidden = skipped_behind_failed(&jobs, workflow_text);
    if !hidden.is_empty() {
        return Err(format!(
            "{} job(s) were skipped behind a failed job they need and name no failure: {}",
            hidden.len(),
            hidden.join(", ")
        ));
    }
    let req = super::triage::Request {
        run,
        against: None,
        app: None,
        days: 14,
        history: true,
        repo: repo.to_string(),
        now_secs: super::time::now_secs(),
    };
    let report = super::triage::collect(&mut client, &req, &mut |l| eprintln!("{l}"))?;
    let mut doc = report.json();
    doc["ok"] = Value::Bool(true);
    let landable = judge(&doc, run, sha, workflow, flaky_ok).map_err(|reasons| reasons.join("; "))?;
    // Read the verdict again: the answer is the same run as when it was judged (a newer full run, started meanwhile, would decide
    // otherwise and this one is no longer the newest).
    match verdict(&mut client, workflow, sha) {
        Verdict::Red { run: Some(again), .. } if again == run => Ok(landable),
        other => Err(format!("the newest full run changed while it was judged ({other:?}); ask again")),
    }
}

/// The pure judgement of the triage document of `run` at `sha`. Every reason is a line.
pub fn judge(
    doc: &Value,
    run: u64,
    sha: &str,
    workflow: &str,
    flaky_ok: &dyn Fn(&str, &str) -> Result<(), String>,
) -> Result<Landable, Vec<String>> {
    if doc.get("ok") != Some(&Value::Bool(true)) {
        return Err(vec!["the triage did not succeed (ok is not true)".into()]);
    }
    let mut why = Vec::new();
    if doc.get("run").and_then(Value::as_u64) != Some(run) {
        why.push(format!(
            "the triage is of run {}, not run {run}",
            doc.get("run").map_or("none".to_string(), Value::to_string)
        ));
    }
    if doc.get("head").and_then(Value::as_str) != Some(sha) {
        why.push(format!(
            "the triage is of commit {}, not {sha}",
            doc.get("head").and_then(Value::as_str).unwrap_or("none")
        ));
    }
    match doc.get("skipped_jobs").and_then(Value::as_u64) {
        Some(0) => {}
        Some(n) => why.push(format!("{n} job(s) were skipped")),
        None => why.push("the triage has no skipped_jobs count".into()),
    }
    let (Some(rows), Some(counts)) = (
        doc.get("rows").and_then(Value::as_array),
        doc.get("counts").and_then(Value::as_object),
    ) else {
        why.push("the triage has no rows or counts".into());
        return Err(why);
    };
    if rows.is_empty() {
        why.push("a red run with no failing rows: the triage could not see the failure".into());
    }
    let mut counted = 0u64;
    for (name, v) in counts {
        match v.as_u64() {
            Some(n) => counted += n,
            None => why.push(format!("count {name} is not a whole number")),
        }
    }
    if counted != rows.len() as u64 {
        why.push(format!("counts add up to {counted}, not to the {} row(s)", rows.len()));
    }
    let baseline_main = doc.pointer("/baseline/main") == Some(&Value::Bool(true));
    let (mut flaky, mut already, mut lines) = (0, 0, Vec::new());
    for row in rows {
        let status = row.get("status").and_then(Value::as_str).unwrap_or("");
        let job = row.get("job").and_then(Value::as_str).unwrap_or("?");
        let named = row.get("test").and_then(Value::as_str).filter(|t| !t.is_empty());
        let whole = row.get("whole_job").is_some_and(|w| !matches!(w, Value::Null | Value::Bool(false)));
        let Some(test) = named.filter(|_| !whole) else {
            why.push(format!("a job row, not a named test ({job}): {status}"));
            continue;
        };
        let cut: String = test.chars().take(120).collect();
        match status {
            "flaky" => match flaky_ok(app_of_job(workflow, job).as_deref().unwrap_or(""), test) {
                Ok(()) => {
                    flaky += 1;
                    lines.push(format!("accepted FLAKY ({job}): {cut}"));
                }
                Err(e) => why.push(format!("FLAKY and not acceptable ({cut}): {e}")),
            },
            "already-failing" if baseline_main => {
                already += 1;
                lines.push(format!("ALREADY FAILING ON MAIN ({job}): {cut}"));
            }
            "already-failing" => why.push(format!("already failing, but not on a main run ({cut})")),
            other => why.push(format!("{}: {cut}", if other.is_empty() { "unreadable" } else { other })),
        }
    }
    if why.is_empty() {
        Ok(Landable {
            run,
            flaky,
            already_failing: already,
            lines,
            _seal: Seal,
        })
    } else {
        Err(why)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const A11Y: &str = "[webkit] › e2e/a11y.e2e.ts:6:7 › main screen has no serious accessibility violations (dark)";
    const OLD: &str = "[chromium] › e2e/old.e2e.ts:9:1 › an old failure";

    fn row(job: &str, test: Option<&str>, whole: bool, status: &str) -> Value {
        json!({"job": job, "test": test, "whole_job": whole, "status": status})
    }

    fn doc(rows: Vec<Value>) -> Value {
        let n = rows.len();
        json!({"ok": true, "run": 8400, "head": SHA, "skipped_jobs": 0, "counts": {"x": n},
               "baseline": {"run": 8289, "head": "f5f", "main": true}, "rows": rows})
    }

    fn yes(_: &str, _: &str) -> Result<(), String> {
        Ok(())
    }

    /// The positive control: one accepted flaky test (and one already failing on main) is landable, and says so.
    #[test]
    fn one_accepted_flaky_test_and_an_old_failure_on_main_are_landable() {
        let d = doc(vec![
            row("e2e", Some(A11Y), false, "flaky"),
            row("e2e", Some(OLD), false, "already-failing"),
        ]);
        let l = judge(&d, 8400, SHA, "debrief.yml", &yes).unwrap();
        assert_eq!((l.run, l.flaky, l.already_failing), (8400, 1, 1));
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.summary(), "full run 8400 with 1 accepted FLAKY, 1 already failing on main");
    }

    fn refused(d: &Value, ok: &dyn Fn(&str, &str) -> Result<(), String>) -> String {
        judge(d, 8400, SHA, "debrief.yml", ok).expect_err("must refuse").join("; ")
    }

    #[test]
    fn a_job_row_a_new_or_unknown_row_and_a_failed_one_are_refused() {
        let flaky = || row("e2e", Some(A11Y), false, "flaky");
        assert!(refused(&doc(vec![flaky(), row("build", None, true, "failed")]), &yes).contains("a job row"));
        assert!(refused(&doc(vec![flaky(), row("e2e", Some(A11Y), true, "flaky")]), &yes).contains("a job row"));
        for status in ["new", "unknown", "failed", ""] {
            let why = refused(&doc(vec![flaky(), row("e2e", Some(OLD), false, status)]), &yes);
            assert!(
                why.contains(if status.is_empty() { "unreadable" } else { status }),
                "{status}: {why}"
            );
        }
    }

    #[test]
    fn a_flaky_test_the_train_touches_is_refused_by_the_callers_check() {
        let d = doc(vec![row("e2e", Some(A11Y), false, "flaky")]);
        let why = refused(&d, &|_, _| Err("the train changes its feature".into()));
        assert!(
            why.contains("FLAKY and not acceptable") && why.contains("changes its feature"),
            "{why}"
        );
    }

    #[test]
    fn a_skipped_job_another_run_another_commit_or_a_bad_count_is_refused() {
        let base = || doc(vec![row("e2e", Some(A11Y), false, "flaky")]);
        let mut d = base();
        d["skipped_jobs"] = json!(1);
        assert!(refused(&d, &yes).contains("1 job(s) were skipped"));
        let mut d = base();
        d.as_object_mut().unwrap().remove("skipped_jobs");
        assert!(refused(&d, &yes).contains("no skipped_jobs"));
        let mut d = base();
        d["run"] = json!(8399);
        assert!(refused(&d, &yes).contains("not run 8400"));
        let mut d = base();
        d["head"] = json!("ffffffffffffffffffffffffffffffffffffffff");
        assert!(refused(&d, &yes).contains("not 0123456789"));
        let mut d = base();
        d["counts"] = json!({"x": 2});
        assert!(refused(&d, &yes).contains("add up to 2"));
        let mut d = base();
        d["counts"] = json!({"x": "1"});
        assert!(refused(&d, &yes).contains("not a whole number"));
        let mut d = base();
        d["ok"] = json!(false);
        assert!(refused(&d, &yes).contains("did not succeed"));
        assert!(refused(&doc(vec![]), &yes).contains("no failing rows"));
    }

    #[test]
    fn an_old_failure_needs_a_main_baseline() {
        let mut d = doc(vec![row("e2e", Some(OLD), false, "already-failing")]);
        d["baseline"]["main"] = json!(false);
        assert!(refused(&d, &yes).contains("not on a main run"));
        d.as_object_mut().unwrap().remove("baseline");
        assert!(refused(&d, &yes).contains("not on a main run"));
    }

    fn info(sha: &str, conclusion: &str, attempt: u64) -> RunInfo {
        RunInfo {
            id: 8400,
            path: "ci.yml@refs/heads/train/x".into(),
            event: "workflow_dispatch".into(),
            status: "completed".into(),
            conclusion: conclusion.into(),
            head_sha: sha.into(),
            head_branch: "train/x".into(),
            created_at: String::new(),
            run_attempt: attempt,
        }
    }

    #[test]
    fn the_run_must_be_a_finished_failure_of_the_exact_commit_with_clean_earlier_attempts() {
        let clean = &mut |_: &RunInfo| Earlier::Clean;
        assert!(check_run(&info(SHA, "failure", 1), "ci.yml", SHA, clean).is_ok());
        assert!(check_run(&info(SHA, "failure", 2), "ci.yml", SHA, clean).is_ok());
        assert!(
            check_run(&info("other", "failure", 1), "ci.yml", SHA, clean)
                .unwrap_err()
                .contains("tested other")
        );
        assert!(
            check_run(&info(SHA, "failure", 1), "debrief.yml", SHA, clean)
                .unwrap_err()
                .contains("not of debrief.yml")
        );
        for c in ["success", "cancelled", ""] {
            assert!(check_run(&info(SHA, c, 1), "ci.yml", SHA, clean).is_err(), "{c}");
        }
        let failed = &mut |_: &RunInfo| Earlier::Failed("job x".into());
        assert!(
            check_run(&info(SHA, "failure", 2), "ci.yml", SHA, failed)
                .unwrap_err()
                .contains("rerun")
        );
        let unknown = &mut |_: &RunInfo| Earlier::Unknown("404".into());
        assert!(check_run(&info(SHA, "failure", 3), "ci.yml", SHA, unknown).is_err());
    }

    #[test]
    fn the_app_of_a_row_comes_from_its_job_or_the_apps_own_workflow() {
        assert_eq!(app_of_job("ci.yml", "app dyno (macOS)"), Some("dyno".into()));
        assert_eq!(app_of_job("ci.yml", "app debrief"), Some("debrief".into()));
        assert_eq!(app_of_job("debrief.yml", "end-to-end (shard 3/4)"), Some("debrief".into()));
        assert_eq!(app_of_job("ci.yml", "ui"), None, "ci.yml is no app");
        assert_eq!(app_of_job("dyno-release.yml", "build"), None);
    }

    /// The judge hands the closure the row's app, so a check can tie the test to its own app.
    #[test]
    fn the_flaky_check_gets_the_rows_app() {
        use std::cell::RefCell;
        let seen = RefCell::new(Vec::new());
        let d = doc(vec![
            row("end-to-end (shard 3/4)", Some(A11Y), false, "flaky"),
            row("app dyno (macOS)", Some(A11Y), false, "flaky"),
        ]);
        let rec = |app: &str, _: &str| {
            seen.borrow_mut().push(app.to_string());
            Ok(())
        };
        judge(&d, 8400, SHA, "debrief.yml", &rec).unwrap();
        assert_eq!(*seen.borrow(), vec!["debrief".to_string(), "dyno".to_string()]);
    }

    fn job(name: &str, conclusion: &str) -> TimedJob {
        TimedJob {
            id: 1,
            name: name.into(),
            runner_id: 1,
            runner: None,
            conclusion: conclusion.into(),
            start: None,
            end: None,
            steps: Vec::new(),
            status: "completed".into(),
            labels: Vec::new(),
        }
    }

    const WORKFLOW: &str = "name: x\njobs:\n  plan:\n    name: plan\n  rust-lint:\n    name: rust format + clippy\n    needs: [plan]\n  bundle:\n    name: bundle (${{ matrix.os }})\n    needs: [plan, rust-lint]\n  release:\n    name: release\n    needs: [plan, bundle]\n  docs:\n    name: docs\n    needs: [plan]\n";

    /// A job that failed hides the jobs that need it (and theirs): they end `skipped` and name no failure.
    #[test]
    fn jobs_skipped_behind_a_failed_need_are_counted_and_a_skip_by_design_is_not() {
        let jobs = vec![
            job("plan", "success"),
            job("rust format + clippy", "failure"),
            job("bundle (macOS)", "skipped"),
            job("release", "skipped"),
            job("docs", "skipped"),
        ];
        assert_eq!(
            skipped_behind_failed(&jobs, WORKFLOW),
            vec!["bundle (macOS)".to_string(), "release".to_string()]
        );
        // nothing failed: the same skips are by design
        let jobs = vec![
            job("plan", "success"),
            job("rust format + clippy", "success"),
            job("bundle (macOS)", "skipped"),
            job("release", "skipped"),
        ];
        assert!(skipped_behind_failed(&jobs, WORKFLOW).is_empty());
    }
}
