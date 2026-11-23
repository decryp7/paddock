// ci-runs-reader: the verdict reader itself
//! `ci verdict`: is there a full green run of workflow W on commit S? The one reader of the Actions runs for a pass/fail answer
//! about a commit (the tag workflow asks it; `conformance/test/ci-verdict-reader.test.mjs` keeps it the only one).
//!
//! Rules (the user, 2026-11-12; ER-33):
//! - A run counts only if its plan job declared a scope that covers a push to main's (ADR 0113): the line
//!   `plan scope: {json}` ([`SCOPE_LINE`], read by [`run_scope::read`]) holds what the run did (`ran`: jobs, engines, apps, whole
//!   tests) and what a push to main does for the same tree (`need`), both written by the planner, and [`Declared::complete`] compares
//!   them: `ran` covers `need`, the run is not a rerun of failures, and `need` is not empty. No workflow decides "complete" with
//!   a line of its own, so a smaller scope (a skipped or reused run that ran nothing, a `since_run` dispatch, a base dispatch, a
//!   full run without WebKit, an app-only dispatch) is never written as complete (the schedule-skipped and rerun-only
//!   runs counted green on 2026-11-12; the WebKit hole of the D4 reuse). A line of another workflow, two lines, an unreadable one
//!   or another version is not measured, never a pass. "Ran means ran" (as GitHub's required checks: a skipped one blocks): every job
//!   the run declared has a job of its name in the run, and none was skipped or cancelled ([`run_scope::check_ran`]: a skipped or
//!   cancelled one makes it partial, a missing one not measured; a failed one is the run's own red).
//! - Report-only phase (ER-41): until `docs/ci-scope-first-runs.toml` names a producer (`./dev ci scope-record` records its first green
//!   scope run and switches it), the old `plan verdict: complete=` line that producer still prints decides, and the scope view is
//!   printed as a `::notice::` (`scope_decisive`, [`verdict_with`]). The earlier line (`plan mode: full=`) states the scope for the rerun
//!   planner only and proves nothing here.
//! - Gitea's in-place "rerun failed jobs" keeps the run id: a deciding run with `run_attempt > 1` reads its earlier attempts,
//!   and any failed job there is red (an unreadable attempt is not measured).
//! - One definition of green (ADR 0117 WP-4, [`super::green`]): a full run whose only failures are FLAKY tests of targets the change
//!   did not touch is [`Verdict::GreenWithFixItems`] (a landing and a push reuse accept it; a tag needs [`Verdict::Green`], exit 5),
//!   a FLAKY test in a touched target, a test that failed both tries or a failed step that is no test is red, an unreadable test
//!   report is not measured. The hint "last fully tested commit" and the planner's base for a push to main come from the same
//!   [`green::find_base`] over main's first-parent commits, whatever their push runs concluded.
//! - The newest full run decides (a green old run does not cover a newer red one).
//! - Unknown or unreadable is not measured, never a pass: the run list or a newer run's log cannot be read, no run is listed,
//!   or the deciding run has not finished.
//!
//! Exit codes: 0 full green, 5 red, green with fix items (for a tag), or only partial runs exist, 6 not measured.

use super::client::{Client, RunInfo};
use super::green::{self, GradedResult, Touched};
use crate::plan_since::{COMPLETE_LINE, read_complete, workflow_file};
use crate::run_scope::{self, SCOPE_LINE};
use paddock_cli::outcome::{
    CheckId, Evidence, Finding, LiveRepo, Outcome, Reason, RunId, Sha, Subject, ToolVersion, TreeId, UnixSeconds, measure,
};
use paddock_cli::{Exit, Report};

/// What the check found.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The newest full run finished and succeeded.
    Green {
        /// Its id.
        run: u64,
    },
    /// The newest complete run failed only in FLAKY tests of targets the change did not touch (ADR 0117 WP-4, ER-12): a commit that
    /// may be a base and may land, each item recorded as a fix item. A tag needs [`Verdict::Green`] and takes the one allowed retry.
    GreenWithFixItems {
        /// Its id.
        run: u64,
        /// The FLAKY tests, `job: test`.
        items: Vec<String>,
    },
    /// The newest full run finished and did not succeed, or only partial runs exist.
    Red {
        /// The deciding run, when a full run exists.
        run: Option<u64>,
        /// What to fix.
        reason: String,
    },
    /// Nothing was measured (no run, an unreadable list or log, a full run still going).
    NotMeasured(String),
}

/// Whether one run was full, as far as it could be read.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// A full run.
    Full,
    /// Not a full run (the plan job says so, or has no marker).
    Partial(String),
    /// Could not be told (an unreadable job list or log): it might be a newer full run.
    Unknown(String),
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// What the earlier attempts of a run (a rerun of failed jobs keeps the run id) hold.
#[derive(Debug, Clone, PartialEq)]
pub enum Earlier {
    /// No failed job in any earlier attempt (or no earlier attempt).
    Clean,
    /// An earlier attempt had a failed job.
    Failed(String),
    /// An earlier attempt could not be read.
    Unknown(String),
}

/// The pure decision over the runs listed for a commit. `mode` is asked for each run, newest first,
/// and only until a full run is found. No run is full without a complete declared scope, a push to main included: a push
/// whose plan said `run=false` ends green having tested nothing (ER-31 (c)).
pub fn decide(
    runs: &[RunInfo],
    workflow: &str,
    sha: &str,
    mode: &mut dyn FnMut(&RunInfo) -> Mode,
    earlier: &mut dyn FnMut(&RunInfo) -> Earlier,
    failures: &mut dyn FnMut(&RunInfo) -> GradedResult,
) -> Verdict {
    let mut mine: Vec<&RunInfo> = runs
        .iter()
        .filter(|r| r.head_sha == sha && workflow_file(&r.path) == workflow)
        .collect();
    mine.sort_by_key(|r| std::cmp::Reverse(r.id));
    if mine.is_empty() {
        return Verdict::NotMeasured(format!(
            "the Actions API lists no {workflow} run for {}: push the commit and let {workflow} finish green on it",
            short(sha)
        ));
    }
    let mut partial = Vec::new();
    for r in &mine {
        let m = mode(r);
        match m {
            Mode::Full => {
                if !r.finished() {
                    return Verdict::NotMeasured(format!(
                        "the newest full {workflow} run ({}) for {} is {}: wait for it (`./dev wait --ci-run {} --max 2h`), then ask again",
                        r.id,
                        short(sha),
                        r.status,
                        r.id
                    ));
                }
                let state = if r.conclusion.is_empty() {
                    r.status.as_str()
                } else {
                    r.conclusion.as_str()
                };
                if state == "success" && r.run_attempt > 1 {
                    match earlier(r) {
                        Earlier::Clean => {}
                        Earlier::Failed(what) => {
                            return Verdict::Red {
                                run: Some(r.id),
                                reason: format!(
                                    "the full {workflow} run ({}) for {} is green only after a rerun of failed jobs ({what}): run it in full again on this commit",
                                    r.id,
                                    short(sha)
                                ),
                            };
                        }
                        Earlier::Unknown(why) => {
                            return Verdict::NotMeasured(format!(
                                "run {} of {workflow} for {} was rerun and its earlier attempts could not be read ({why}): check GITEA_TOKEN can read the Actions runs, then ask again",
                                r.id,
                                short(sha)
                            ));
                        }
                    }
                }
                if state == "success" {
                    return Verdict::Green { run: r.id };
                }
                return match failures(r) {
                    GradedResult::GreenWithFixItems(items) => Verdict::GreenWithFixItems { run: r.id, items },
                    GradedResult::Green => Verdict::Green { run: r.id },
                    GradedResult::NotMeasured(why) => Verdict::NotMeasured(format!(
                        "the newest full {workflow} run ({}) for {} is {state} and its failures cannot be graded ({why}): check GITEA_TOKEN can read the Actions logs, then ask again",
                        r.id,
                        short(sha)
                    )),
                    GradedResult::Red(why) => Verdict::Red {
                        run: Some(r.id),
                        reason: format!(
                            "the newest full {workflow} run ({}) for {} is {state}, not success ({why}): fix it, get a full run green on this commit",
                            r.id,
                            short(sha)
                        ),
                    },
                };
            }
            Mode::Partial(why) => partial.push(format!("{} ({why})", r.id)),
            Mode::Unknown(why) => {
                return Verdict::NotMeasured(format!(
                    "run {} of {workflow} for {} might be a full run but its mode could not be read ({why}): check GITEA_TOKEN can read the Actions logs, then ask again",
                    r.id,
                    short(sha)
                ));
            }
        }
    }
    Verdict::Red {
        run: None,
        reason: format!(
            "no {workflow} run for {} is a full run (a push to main, or a plan job that declared a complete scope (\"{SCOPE_LINE}\")); only partial: {}. Run it in full on this commit",
            short(sha),
            partial.join(", ")
        ),
    }
}

/// The failed jobs of the earlier attempts of a run (1 up to `run_attempt - 1`).
pub(super) fn read_earlier(client: &mut Client, run: &RunInfo) -> Earlier {
    for n in 1..run.run_attempt {
        match client.attempt_jobs(run.id, Some(n)) {
            Ok(Some(jobs)) => {
                if let Some(j) = jobs.iter().find(|j| j.failed()) {
                    return Earlier::Failed(format!("attempt {n}: job {:?} was {}", j.name, j.conclusion));
                }
            }
            Ok(None) => return Earlier::Unknown(format!("the server has no job list for attempt {n}")),
            Err(e) => return Earlier::Unknown(format!("attempt {n}: {e}")),
        }
    }
    Earlier::Clean
}

/// The mode a plan job's log gives on the new basis (ADR 0113): the declared scope covers a push to main's, and the jobs it declared ran
/// ("ran means ran": a declared job that was skipped or cancelled makes it partial, one with no job of its name is not measured). A
/// run that has not finished is not checked for its jobs: the decision waits for it.
fn scope_mode(run: &RunInfo, jobs: &[super::client::TimedJob], log: &str) -> Mode {
    match run_scope::read(log) {
        Some(Ok(d)) if d.workflow != run.workflow() => {
            Mode::Partial(format!("its plan job declared the scope of {}, not of this workflow", d.workflow))
        }
        Some(Ok(d)) => match d.complete() {
            Err(why) => Mode::Partial(format!("its declared scope is not complete: {why}")),
            Ok(()) if !run.finished() => Mode::Full,
            Ok(()) => {
                let pairs: Vec<(String, String)> = jobs.iter().map(|j| (j.name.clone(), j.conclusion.clone())).collect();
                match run_scope::check_ran(&d, &pairs) {
                    Ok(()) => Mode::Full,
                    Err(run_scope::RanProblem::NotRun(why)) => Mode::Partial(format!("its declared scope was not run: {why}")),
                    Err(run_scope::RanProblem::Missing(why)) => {
                        Mode::Unknown(format!("its declared scope cannot be checked against its jobs: {why}"))
                    }
                }
            }
        },
        Some(Err(e)) => Mode::Unknown(e),
        None => Mode::Partial(format!("its plan job logged no \"{SCOPE_LINE}\" line")),
    }
}

/// The mode on the old basis, the `plan verdict: complete=` line a producer keeps printing until its first green scope run is recorded
/// (ER-41, `docs/ci-scope-first-runs.toml`).
fn legacy_mode(log: &str) -> Mode {
    match read_complete(log) {
        Some(Ok(true)) => Mode::Full,
        Some(Ok(false)) => Mode::Partial("its plan job logged complete=false".into()),
        Some(Err(e)) => Mode::Unknown(e),
        None => Mode::Partial(format!("its plan job logged no \"{COMPLETE_LINE}\" line")),
    }
}

/// The words of a mode for the report-only notice.
fn describe(m: &Mode) -> String {
    match m {
        Mode::Full => "declares a complete scope".into(),
        Mode::Partial(why) => format!("is partial on the declared scope: {why}"),
        Mode::Unknown(why) => format!("is not measured on the declared scope: {why}"),
    }
}

/// How one run's plan job declared its mode. With `scope_decisive` (its producer is recorded in the register) the declared scope
/// decides; without it the old line decides and the scope view is printed as a notice, so a run of the report-only phase shows what
/// the new basis would say (ER-41).
fn read_mode(client: &mut Client, run: &RunInfo, scope_decisive: bool) -> Mode {
    read_mode_jobs(client, run, scope_decisive).0
}

/// [`read_mode`] with the run's jobs it read for it (the failed ones are graded by [`super::green`]; empty when they could not be listed).
pub(super) fn read_mode_jobs(client: &mut Client, run: &RunInfo, scope_decisive: bool) -> (Mode, Vec<super::client::TimedJob>) {
    let jobs = match client.timed_jobs(run.id) {
        Ok(Some(j)) => j,
        Ok(None) => return (Mode::Unknown("the server has no job list for the run".into()), Vec::new()),
        Err(e) => return (Mode::Unknown(format!("the jobs could not be listed: {e}")), Vec::new()),
    };
    let Some(plan) = jobs.iter().find(|j| j.name == "plan" || j.name.starts_with("plan ")) else {
        return (Mode::Partial("no plan job".into()), jobs);
    };
    let log = match client.log(plan.id) {
        Ok(Some(log)) => log,
        Ok(None) => return (Mode::Unknown("the plan job has no log".into()), jobs),
        Err(e) => return (Mode::Unknown(format!("the plan job's log could not be read: {e}")), jobs),
    };
    let scope = scope_mode(run, &jobs, &log);
    if scope_decisive {
        return (scope, jobs);
    }
    eprintln!(
        "::notice::scope view (report-only; {} does not name {}): run {} {}",
        run_scope::REGISTER,
        run_scope::producer_of(run.workflow()),
        run.id,
        describe(&scope)
    );
    (legacy_mode(&log), jobs)
}

/// Reads the runs listed for `sha` (up to four pages of 50) and decides.
pub fn verdict(client: &mut Client, workflow: &str, sha: &str) -> Verdict {
    verdict_excluding(client, workflow, sha, None)
}

/// Whether the declared scope decides for `workflow`: its producer is recorded in the register of the current repository (ER-41). The
/// reason it is not (no register, a register of another shape) is a warning, never a switch.
pub fn scope_decisive(workflow: &str) -> bool {
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (decisive, why) = run_scope::decisive_in(&root, workflow);
    if let Some(why) = why {
        eprintln!("::warning::ci verdict: {why}");
    }
    decisive
}

/// [`verdict`] without one run of the list: the run that is asking (a push to main asking whether an earlier full green run
/// already tested its commit, `ci reuse`). That run is still going, so its plan log is not final and it is never evidence for
/// itself.
pub fn verdict_excluding(client: &mut Client, workflow: &str, sha: &str, exclude: Option<u64>) -> Verdict {
    verdict_with(client, workflow, sha, exclude, scope_decisive(workflow))
}

/// [`verdict_excluding`] with the basis given: `scope` decides on the declared scope, otherwise on the old line (the report-only phase).
pub fn verdict_with(client: &mut Client, workflow: &str, sha: &str, exclude: Option<u64>, scope: bool) -> Verdict {
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let first_parent = green::first_parent_before(&root, sha);
    verdict_with_touched(client, workflow, sha, exclude, scope, &first_parent, &mut |s| {
        green::plan_touched(&root, s)
    })
}

/// [`verdict_with`] with the first-parent commits before `sha` (newest first) and the way to read what a commit's change touched given.
pub fn verdict_with_touched(
    client: &mut Client,
    workflow: &str,
    sha: &str,
    exclude: Option<u64>,
    scope: bool,
    first_parent: &[String],
    touched: &mut dyn FnMut(&str) -> Option<Touched>,
) -> Verdict {
    match verdict_core(client, workflow, sha, exclude, scope, touched) {
        Verdict::Red { run: None, reason } => {
            let next = match last_full(client, workflow, scope, first_parent, touched) {
                Some(last) => {
                    format!("the last fully tested commit on main is {last}: tag that one, or dispatch a full run on this commit")
                }
                None => "no fully tested commit was found among the newest runs of main: dispatch a full run on this commit".into(),
            };
            Verdict::Red {
                run: None,
                reason: format!("{reason}. Next: {next}"),
            }
        }
        other => other,
    }
}

/// The decision over the runs listed for `sha`, without the hint of the last fully tested commit.
fn verdict_core(
    client: &mut Client,
    workflow: &str,
    sha: &str,
    exclude: Option<u64>,
    scope: bool,
    touched: &mut dyn FnMut(&str) -> Option<Touched>,
) -> Verdict {
    let query = format!("head_sha={sha}");
    let mut runs = Vec::new();
    for page in 1..=4 {
        match client.runs_page_where(page, &query) {
            Ok(p) => {
                let n = p.runs.len();
                runs.extend(p.runs);
                if n < 50 {
                    break;
                }
                if page == 4 {
                    return Verdict::NotMeasured(format!("{} has more than 200 runs; the list was cut at four pages", short(sha)));
                }
            }
            Err(e) => return Verdict::NotMeasured(format!("the runs of {} could not be listed: {e}", short(sha))),
        }
    }
    runs.retain(|r| Some(r.id) != exclude);
    // A run's jobs and log are read only for a run whose mode is needed.
    let client = std::cell::RefCell::new(client);
    let mut modes = |r: &RunInfo| read_mode(&mut client.borrow_mut(), r, scope);
    let mut earlier = |r: &RunInfo| read_earlier(&mut client.borrow_mut(), r);
    let touched = std::cell::RefCell::new(touched);
    let mut failures = |r: &RunInfo| {
        let mut c = client.borrow_mut();
        match c.timed_jobs(r.id) {
            Ok(Some(jobs)) => green::read_failures(&mut c, workflow, &jobs, &mut || (touched.borrow_mut())(sha)),
            Ok(None) => GradedResult::NotMeasured("the server has no job list for the run".into()),
            Err(e) => GradedResult::NotMeasured(format!("the jobs could not be listed: {e}")),
        }
    };
    decide(&runs, workflow, sha, &mut modes, &mut earlier, &mut failures)
}

/// ER-41: checks that `run_id` is a first green run of `workflow` on the declared scope: the run is of this workflow, finished and
/// green, its plan job declared a scope that covers a push to main's, and every job it declared ran. `Ok` carries what was checked;
/// `Err` says why this run cannot be recorded. The register is written by the caller only after this.
pub fn first_green_scope(client: &mut Client, workflow: &str, run_id: u64) -> Result<String, String> {
    let run = match client.run(run_id) {
        Ok(Some(r)) => r,
        Ok(None) => return Err(format!("run {run_id} does not exist on the server")),
        Err(e) => return Err(format!("run {run_id} could not be read: {e}")),
    };
    if run.workflow() != workflow {
        return Err(format!("run {run_id} is a run of {}, not of {workflow}", run.workflow()));
    }
    if !run.finished() {
        return Err(format!("run {run_id} is {}: record it when it has finished", run.status));
    }
    let state = if run.conclusion.is_empty() {
        run.status.as_str()
    } else {
        run.conclusion.as_str()
    };
    if state != "success" {
        return Err(format!("run {run_id} is {state}, not success: a first run is a green one"));
    }
    match read_mode(client, &run, true) {
        Mode::Full => Ok(format!(
            "run {run_id} of {workflow} on {} is green and declared a complete scope that ran",
            short(&run.head_sha)
        )),
        Mode::Partial(why) => Err(format!("run {run_id} is not a complete scope: {why}")),
        Mode::Unknown(why) => Err(format!("run {run_id} could not be measured: {why}")),
    }
}

/// The last of `first_parent` that the one definition of green accepts ([`green::find_base`]): complete, and
/// green or green with fix items, whatever the conclusion of its push run (a train's dispatch run names the commit that lands, and
/// the pushes after it were cancelled or reused it).
fn last_full(
    client: &mut Client,
    workflow: &str,
    scope: bool,
    first_parent: &[String],
    touched: &mut dyn FnMut(&str) -> Option<Touched>,
) -> Option<String> {
    green::find_base(client, workflow, first_parent, scope, touched)
        .ok()
        .flatten()
        .map(|b| b.sha)
}

/// The verdict as the shared outcome (ADR 0089 amendment, WP-T4): a green full run is [`Outcome::KnownCommit`] with the run as its
/// evidence, made only by `Evidence::adopt` after `live` has re-read the commit (the one caller of `Evidence::ci_run`, held by C124);
/// red is failing findings; anything else is not measured. The exit comes from this outcome, never from the enum.
pub fn outcome(v: &Verdict, workflow: &str, sha: &str, live: &dyn LiveRepo) -> Outcome {
    let tool = || ToolVersion::new(concat!("paddock-dev ci verdict ", env!("CARGO_PKG_VERSION")));
    let check = || CheckId::new(format!("ci-verdict:{workflow}"));
    match v {
        Verdict::Green { run } => {
            let evidence = Evidence::ci_run(check(), workflow, RunId(*run), Sha::new(sha), tool(), UnixSeconds::now());
            Evidence::adopt(evidence, live)
        }
        Verdict::Red { run, reason } => {
            let subject = match run {
                Some(run) => Subject::CiRun {
                    workflow: workflow.to_string(),
                    run: RunId(*run),
                    sha: Sha::new(sha),
                },
                None => Subject::Commit {
                    sha: Sha::new(sha),
                    tree: live.tree_of(&Sha::new(sha)).unwrap_or_else(|| TreeId::new("unknown")),
                },
            };
            measure(check(), subject, tool(), UnixSeconds::now(), || {
                Ok(vec![Finding::failing(reason.clone())])
            })
        }
        Verdict::GreenWithFixItems { run, items } => measure(
            check(),
            Subject::CiRun {
                workflow: workflow.to_string(),
                run: RunId(*run),
                sha: Sha::new(sha),
            },
            tool(),
            UnixSeconds::now(),
            || {
                Ok(vec![Finding::failing(format!(
                    "run {run} is green with fix items ({}): a landing accepts it, a tag needs a run without them: run ci.yml once more on this commit",
                    items.join("; ")
                ))])
            },
        ),
        Verdict::NotMeasured(why) => Outcome::NotMeasured(Reason::new(why.clone())),
    }
}

/// The report of a verdict: data, one line, and the exit code, all read from its shared [`outcome`].
pub fn report(v: &Verdict, workflow: &str, sha: &str, live: &dyn LiveRepo) -> Report {
    let head = format!("{workflow} on {}", short(sha));
    let o = outcome(v, workflow, sha, live);
    let (kind, run, text, exit) = match (&o, v) {
        (Outcome::KnownCommit(_), Verdict::Green { run }) => ("green", Some(*run), format!("{head}: full green (run {run})"), Exit::Ok),
        (Outcome::Clean(_) | Outcome::KnownCommit(_), _) => (
            "not_measured",
            None,
            format!("{head}: not measured. a pass without a matching run"),
            Exit::NotMeasured,
        ),
        (Outcome::Findings(_), Verdict::Red { run, reason }) => ("red", *run, format!("{head}: red or partial. {reason}"), Exit::Findings),
        (Outcome::Findings(_), Verdict::GreenWithFixItems { run, items }) => (
            "green_with_fix_items",
            Some(*run),
            format!("{head}: green with {} fix items (run {run}); a tag needs none", items.len()),
            Exit::Findings,
        ),
        (other, _) => (
            "not_measured",
            None,
            format!("{head}: not measured. {}", other.detail()),
            Exit::NotMeasured,
        ),
    };
    let data = serde_json::json!({"verdict": kind, "workflow": workflow, "sha": sha, "run": run, "detail": text, "outcome": o.json()});
    Report::new(data, text).with_exit(exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;
    use serde_json::json;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";

    fn run(id: u64, path: &str, event: &str, branch: &str, status: &str, conclusion: &str) -> serde_json::Value {
        json!({"id": id, "path": path, "event": event, "head_branch": branch, "status": status, "conclusion": conclusion, "head_sha": SHA, "created_at": "2026-11-12T00:00:00Z"})
    }

    fn main_push(id: u64, conclusion: &str) -> serde_json::Value {
        run(id, "ci.yml@refs/heads/main", "push", "main", "completed", conclusion)
    }

    fn dispatch(id: u64, conclusion: &str) -> serde_json::Value {
        run(
            id,
            "ci.yml@refs/heads/train/x",
            "workflow_dispatch",
            "train/x",
            "completed",
            conclusion,
        )
    }

    /// A fake server: the runs of the sha, and for each (run id, its plan job id, its plan log or None for an unreadable one).
    fn server(runs: Vec<serde_json::Value>, plans: &[(u64, u64, Option<&str>)]) -> Fake {
        let mut f = Fake::default();
        f.json(
            &format!("{API}/runs?head_sha={SHA}&limit=50&page=1"),
            json!({"total_count": runs.len(), "workflow_runs": runs}),
        );
        // A push to main is full only through its plan log too: by default every one declares a complete scope.
        let defaults: Vec<(u64, u64, Option<&str>)> = runs
            .iter()
            .filter(|r| r["event"] == "push" && r["head_branch"] == "main")
            .map(|r| r["id"].as_u64().unwrap())
            .filter(|id| !plans.iter().any(|p| p.0 == *id))
            .map(|id| (id, id + 5000, Some(run_scope::TEST_COMPLETE)))
            .collect();
        for (run, job, log) in plans.iter().chain(defaults.iter()) {
            f.json(
                &format!("{API}/runs/{run}/jobs?limit=50&page=1"),
                json!({"total_count": 2, "jobs": [
                    {"id": job, "name": "plan (affected projects)", "conclusion": "success"},
                    {"id": job + 1000, "name": "conformance (checks, release scripts)", "conclusion": "success"}]}),
            );
            if let Some(l) = log {
                f.log(*job, l);
            }
        }
        f
    }

    fn ask(f: &mut Fake) -> Verdict {
        let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "t");
        c.backoff_ms = 0;
        verdict_with_touched(&mut c, "ci.yml", SHA, None, true, &["e".repeat(40)], &mut |_: &str| {
            Some(Touched::default())
        })
    }

    /// The same on the old basis, the report-only phase's (ER-41).
    fn ask_old(f: &mut Fake) -> Verdict {
        let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "t");
        c.backoff_ms = 0;
        verdict_with_touched(&mut c, "ci.yml", SHA, None, false, &["e".repeat(40)], &mut |_: &str| {
            Some(Touched::default())
        })
    }

    fn code(v: &Verdict) -> i32 {
        report(v, "ci.yml", SHA, &Knows(true)).exit.code()
    }

    #[test]
    fn a_green_push_to_main_is_full_green_when_its_plan_logged_complete_true() {
        let mut f = server(vec![main_push(5, "success")], &[]);
        let v = ask(&mut f);
        assert_eq!(v, Verdict::Green { run: 5 });
        assert_eq!(code(&v), 0);
    }

    #[test]
    fn a_push_to_main_whose_plan_skipped_it_is_not_full_and_names_the_last_fully_tested_commit() {
        // Run 8 (this commit) was skipped by its plan: green, every job skipped. Run 6 (an earlier commit) was complete.
        let skipped = run_scope::TEST_PARTIAL;
        let earlier_run = || {
            let mut r = main_push(6, "success");
            r["head_sha"] = json!("e".repeat(40));
            r
        };
        let earlier = earlier_run();
        let mut f = server(vec![main_push(8, "success")], &[(8, 80, Some(skipped))]);
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 2, "workflow_runs": [main_push(8, "success"), earlier]}),
        );
        f.json(
            &format!("{API}/runs?head_sha={}&limit=50&page=1", "e".repeat(40)),
            json!({"total_count": 1, "workflow_runs": [earlier_run()]}),
        );
        f.json(
            &format!("{API}/runs/6/jobs?limit=50&page=1"),
            json!({"total_count": 2, "jobs": [{"id": 60, "name": "plan (affected projects)", "conclusion": "success"}, {"id": 61, "name": "conformance (checks, release scripts)", "conclusion": "success"}]}),
        );
        f.log(60, run_scope::TEST_COMPLETE);
        let v = ask(&mut f);
        assert_eq!(code(&v), 5, "{v:?}");
        assert!(
            matches!(&v, Verdict::Red { run: None, reason } if reason.contains(&"e".repeat(40)) && reason.contains("tag that one")),
            "{v:?}"
        );
        // A push run with no verdict line at all (a run from before it existed) is not full either; with no complete commit listed
        // the next step is a full dispatch.
        let v = ask(&mut server(
            vec![main_push(8, "success")],
            &[(8, 80, Some("plan mode: full=true\n"))],
        ));
        assert!(
            matches!(&v, Verdict::Red { run: None, reason } if reason.contains("dispatch a full run")),
            "{v:?}"
        );
    }

    #[test]
    fn the_hint_names_a_commit_tested_by_a_train_run_when_its_own_push_was_reused() {
        // This commit has no full run. The newest commit on main, e, was fast-forwarded from a train: its push run 6 reused the
        // train's run (nothing ran), and its train dispatch run 7 is the full green one that decided it.
        let e = "e".repeat(40);
        let mut push_e = main_push(6, "success");
        push_e["head_sha"] = json!(e);
        let mut train_e = dispatch(7, "success");
        train_e["head_sha"] = json!(e);
        let mut f = server(vec![main_push(8, "success")], &[(8, 80, Some(run_scope::TEST_PARTIAL))]);
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 3, "workflow_runs": [main_push(8, "success"), push_e.clone(), train_e.clone()]}),
        );
        f.json(
            &format!("{API}/runs?head_sha={e}&limit=50&page=1"),
            json!({"total_count": 2, "workflow_runs": [push_e, train_e]}),
        );
        for (run, job, log) in [(6, 60, run_scope::TEST_PARTIAL), (7, 70, run_scope::TEST_COMPLETE)] {
            f.json(
                &format!("{API}/runs/{run}/jobs?limit=50&page=1"),
                json!({"total_count": 2, "jobs": [{"id": job, "name": "plan (affected projects)", "conclusion": "success"}, {"id": job + 1, "name": "conformance (checks, release scripts)", "conclusion": "success"}]}),
            );
            f.log(job, log);
        }
        let v = ask(&mut f);
        assert!(matches!(&v, Verdict::Red { run: None, reason } if reason.contains(&e)), "{v:?}");
        // Positive control: the train's run partial too, and no commit is named.
        f.log(70, run_scope::TEST_PARTIAL);
        let v = ask(&mut f);
        assert!(
            matches!(&v, Verdict::Red { run: None, reason } if !reason.contains(&e) && reason.contains("dispatch a full run")),
            "{v:?}"
        );
    }

    #[test]
    fn a_red_full_run_is_exit_5_and_the_newest_full_run_decides() {
        let v = ask(&mut server(vec![main_push(5, "failure")], &[]));
        assert!(matches!(v, Verdict::Red { run: Some(5), .. }), "{v:?}");
        assert_eq!(code(&v), 5);
        // An old green run does not cover a newer red one, and an old red one does not hide a newer green one.
        assert!(matches!(
            ask(&mut server(vec![main_push(4, "success"), main_push(6, "failure")], &[])),
            Verdict::Red { run: Some(6), .. }
        ));
        assert_eq!(
            ask(&mut server(vec![main_push(4, "failure"), main_push(6, "success")], &[])),
            Verdict::Green { run: 6 }
        );
    }

    #[test]
    fn a_dispatch_run_counts_only_when_its_plan_job_declared_a_complete_scope() {
        let stamped = format!("2026-11-12T00:00:00Z {}", run_scope::TEST_COMPLETE);
        let mut f = server(vec![dispatch(9, "success")], &[(9, 90, Some(stamped.as_str()))]);
        assert_eq!(ask(&mut f), Verdict::Green { run: 9 });
        let mut f = server(vec![dispatch(9, "success")], &[(9, 90, Some(run_scope::TEST_PARTIAL))]);
        let v = ask(&mut f);
        assert!(matches!(v, Verdict::Red { run: None, .. }), "a partial run only is exit 5: {v:?}");
        assert_eq!(code(&v), 5);
        // A log from before the marker existed is not full either.
        let v = ask(&mut server(vec![dispatch(9, "success")], &[(9, 90, Some("no marker here\n"))]));
        assert_eq!(code(&v), 5, "{v:?}");
    }

    #[test]
    fn a_green_partial_rerun_cannot_clear_a_red_full_run() {
        let runs = vec![main_push(5, "failure"), dispatch(9, "success")];
        let v = ask(&mut server(runs, &[(9, 90, Some(run_scope::TEST_PARTIAL))]));
        assert!(matches!(v, Verdict::Red { run: Some(5), .. }), "{v:?}");
    }

    #[test]
    fn a_push_from_another_branch_and_a_pull_request_are_never_full_by_themselves() {
        let feature = run(3, "ci.yml@refs/heads/feature/x", "push", "feature/x", "completed", "success");
        let pr = run(4, "ci.yml@refs/pull/1/head", "pull_request", "feature/x", "completed", "success");
        let f = &mut server(vec![feature, pr], &[(3, 30, Some("")), (4, 40, Some(""))]);
        assert_eq!(code(&ask(f)), 5);
    }

    #[test]
    fn unknown_is_not_measured_never_a_pass() {
        // No run listed for the commit.
        let v = ask(&mut server(vec![], &[]));
        assert!(matches!(v, Verdict::NotMeasured(_)), "{v:?}");
        assert_eq!(code(&v), 6);
        // Another workflow's or another commit's runs do not count.
        let other = run(5, "nightly.yml@refs/heads/main", "push", "main", "completed", "success");
        assert_eq!(code(&ask(&mut server(vec![other], &[]))), 6);
        // The run list is unreadable (a 404 here, the same as any error after the retries).
        assert_eq!(code(&ask(&mut Fake::default())), 6);
        // A newer run whose plan log cannot be read might be the full one.
        let runs = vec![main_push(4, "success"), dispatch(9, "success")];
        assert_eq!(code(&ask(&mut server(runs.clone(), &[(9, 90, None)]))), 6);
        // A log with a mode that is not true or false.
        assert_eq!(code(&ask(&mut server(runs, &[(9, 90, Some("plan scope: nope\n"))]))), 6);
        // The deciding full run has not finished.
        let going = run(5, "ci.yml@refs/heads/main", "push", "main", "in_progress", "");
        assert_eq!(code(&ask(&mut server(vec![going], &[]))), 6);
    }

    #[test]
    fn an_unreadable_older_run_does_not_matter_once_a_newer_full_run_decided() {
        let runs = vec![dispatch(2, "success"), main_push(6, "success")];
        assert_eq!(ask(&mut server(runs, &[(2, 20, None)])), Verdict::Green { run: 6 });
    }

    #[test]
    fn a_schedule_skipped_or_since_run_run_that_only_logged_the_scope_line_is_not_full() {
        // debrief.yml logs `plan mode: full=true` BEFORE it decides: a scheduled run skipped for a train (run=false, every job
        // skipped, conclusion success) and a since_run + bundles dispatch both log it, and only the verdict line tells them apart.
        let scope_only = run_scope::TEST_PARTIAL;
        let v = ask(&mut server(vec![dispatch(9, "success")], &[(9, 90, Some(scope_only))]));
        assert_eq!(code(&v), 5, "{v:?}");
        // The old scope line alone, with no verdict line, is not enough either.
        let v = ask(&mut server(
            vec![dispatch(9, "success")],
            &[(9, 90, Some("plan mode: full=true\n"))],
        ));
        assert!(matches!(v, Verdict::Red { run: None, .. }), "{v:?}");
        // And it cannot clear a red full run.
        let runs = vec![main_push(5, "failure"), dispatch(9, "success")];
        let v = ask(&mut server(runs, &[(9, 90, Some(scope_only))]));
        assert!(matches!(v, Verdict::Red { run: Some(5), .. }), "{v:?}");
    }

    fn rerun(mut r: serde_json::Value, attempt: u64) -> serde_json::Value {
        r["run_attempt"] = json!(attempt);
        r
    }

    fn attempt(f: &mut Fake, run: u64, n: u64, jobs: serde_json::Value) {
        f.json(
            &format!("{API}/runs/{run}/attempts/{n}/jobs?limit=50&page=1"),
            json!({"total_count": 1, "jobs": jobs}),
        );
    }

    #[test]
    fn a_run_green_after_a_rerun_of_failed_jobs_is_red_when_an_earlier_attempt_failed() {
        let mut f = server(vec![rerun(main_push(5, "success"), 2)], &[]);
        attempt(&mut f, 5, 1, json!([{"id": 1, "name": "tests", "conclusion": "failure"}]));
        let v = ask(&mut f);
        assert!(
            matches!(v, Verdict::Red { run: Some(5), ref reason } if reason.contains("rerun")),
            "{v:?}"
        );
        // Earlier attempts that only passed or skipped jobs do not matter.
        let mut f = server(vec![rerun(main_push(5, "success"), 3)], &[]);
        for n in [1, 2] {
            attempt(
                &mut f,
                5,
                n,
                json!([{"id": 1, "name": "tests", "conclusion": "success"}, {"id": 2, "name": "x", "conclusion": "skipped"}]),
            );
        }
        assert_eq!(ask(&mut f), Verdict::Green { run: 5 });
        // A first attempt reads no attempt list at all.
        let mut f = server(vec![main_push(5, "success")], &[]);
        ask(&mut f);
        assert!(f.requested().iter().all(|u| !u.contains("/attempts/")), "{:?}", f.requested());
    }

    #[test]
    fn an_unreadable_earlier_attempt_is_not_measured() {
        // Attempt 1 is listed, attempt 2 is a 404.
        let mut f = server(vec![rerun(main_push(5, "success"), 3)], &[]);
        attempt(&mut f, 5, 1, json!([{"id": 1, "name": "tests", "conclusion": "success"}]));
        let v = ask(&mut f);
        assert!(matches!(v, Verdict::NotMeasured(_)), "{v:?}");
        assert_eq!(code(&v), 6);
    }

    #[test]
    fn only_a_push_event_on_main_is_full_without_a_log_and_only_this_commit_and_workflow_count() {
        // A dispatch whose branch is main is not a push: its plan job decides (here: partial).
        let on_main = run(7, "ci.yml@refs/heads/main", "workflow_dispatch", "main", "completed", "success");
        let v = ask(&mut server(vec![on_main], &[(7, 70, Some(run_scope::TEST_PARTIAL))]));
        assert_eq!(code(&v), 5, "{v:?}");
        // A green run of another commit in the list does not count.
        let mut other = main_push(8, "success");
        other["head_sha"] = json!("f".repeat(40));
        assert_eq!(code(&ask(&mut server(vec![other], &[]))), 6);
    }

    #[test]
    fn a_job_list_error_is_unknown_not_partial_and_no_plan_job_is_never_full() {
        let runs = vec![main_push(4, "success"), dispatch(9, "success")];
        // No job list for the newer run (404): it might be the full one.
        assert_eq!(code(&ask(&mut server(runs.clone(), &[]))), 6);
        // A server error on the job list is the same: not measured, never partial.
        let mut f = server(runs.clone(), &[]);
        f.error(&format!("{API}/runs/9/jobs?limit=50&page=1"), "curl exited with 22: error: 500");
        assert_eq!(code(&ask(&mut f)), 6);
        // A job list without a plan job is partial: the older green main push decides.
        let mut f = server(runs.clone(), &[]);
        f.json(
            &format!("{API}/runs/9/jobs?limit=50&page=1"),
            json!({"total_count": 1, "jobs": [{"id": 1, "name": "tests", "conclusion": "success"}]}),
        );
        assert_eq!(ask(&mut f), Verdict::Green { run: 4 });
        let mut f = server(vec![dispatch(9, "success")], &[]);
        f.json(
            &format!("{API}/runs/9/jobs?limit=50&page=1"),
            json!({"total_count": 1, "jobs": [{"id": 1, "name": "tests", "conclusion": "success"}]}),
        );
        assert_eq!(code(&ask(&mut f)), 5);
    }

    #[test]
    fn a_full_fourth_page_is_not_measured() {
        let mut f = Fake::default();
        for page in 1..=4 {
            let runs: Vec<_> = (0..50)
                .map(|i| {
                    run(
                        page * 100 + i,
                        "nightly.yml@refs/heads/main",
                        "push",
                        "main",
                        "completed",
                        "success",
                    )
                })
                .collect();
            f.json(
                &format!("{API}/runs?head_sha={SHA}&limit=50&page={page}"),
                json!({"total_count": 400, "workflow_runs": runs}),
            );
        }
        let v = ask(&mut f);
        assert!(matches!(&v, Verdict::NotMeasured(w) if w.contains("more than 200")), "{v:?}");
    }

    /// A repository that knows the commit, standing in for the live one.
    struct Knows(bool);
    impl LiveRepo for Knows {
        fn tree_of(&self, _: &Sha) -> Option<TreeId> {
            self.0.then(|| TreeId::new("t1"))
        }
    }

    #[test]
    fn the_report_names_the_run_the_workflow_and_the_next_step() {
        let r = report(&Verdict::Green { run: 5 }, "ci.yml", SHA, &Knows(true));
        assert_eq!(r.data["verdict"], "green");
        assert_eq!(r.data["run"], 5);
        assert_eq!(r.data["outcome"]["status"], "known-commit");
        assert!(r.text.contains("ci.yml") && r.text.contains(&SHA[..10]), "{}", r.text);
        let r = report(&Verdict::NotMeasured("no run".into()), "ci.yml", SHA, &Knows(true));
        assert_eq!(r.data["verdict"], "not_measured");
        assert_eq!(r.exit, Exit::NotMeasured);
        let r = report(
            &Verdict::Red {
                run: None,
                reason: "only partial runs".into(),
            },
            "ci.yml",
            SHA,
            &Knows(true),
        );
        assert_eq!(r.data["verdict"], "red");
        assert_eq!(r.exit, Exit::Findings);
    }

    /// "Delete the check": a green answer for a commit the repository does not have is not measured, never a pass.
    #[test]
    fn a_green_run_for_a_commit_the_repository_lacks_is_not_measured() {
        let r = report(&Verdict::Green { run: 5 }, "ci.yml", SHA, &Knows(false));
        assert_eq!(r.exit, Exit::NotMeasured);
        assert_eq!(r.data["verdict"], "not_measured");
    }

    /// "Skip the gate": a consumer given this command's exit code alone, or JSON without evidence, or evidence for another commit,
    /// reads not measured.
    #[test]
    fn a_consumer_never_reads_the_exit_code_alone_as_a_pass() {
        let asked = Subject::Commit {
            sha: Sha::new(SHA),
            tree: TreeId::new("t1"),
        };
        for wire in [
            serde_json::json!({}),
            serde_json::json!({"status": "known-commit"}),
            serde_json::json!({"status": "clean"}),
        ] {
            assert!(matches!(Outcome::from_wire(&wire, &asked, true), Outcome::NotMeasured(_)), "{wire}");
        }
        let green = report(&Verdict::Green { run: 5 }, "ci.yml", SHA, &Knows(true)).data["outcome"].clone();
        let other = Subject::Commit {
            sha: Sha::new("ffff"),
            tree: TreeId::new("t1"),
        };
        assert!(matches!(Outcome::from_wire(&green, &other, true), Outcome::NotMeasured(_)));
        assert!(matches!(Outcome::from_wire(&green, &asked, false), Outcome::NotMeasured(_)));
        assert!(matches!(Outcome::from_wire(&green, &asked, true), Outcome::KnownCommit(_)));
    }
    /// A ci.yml declaration of a run that ran `jobs` of `need_jobs` with the WebKit engine, as `ci_scope` would write it.
    fn declared_line(ran_jobs: &[&str], need_jobs: &[&str]) -> String {
        let scope = |jobs: &[&str]| run_scope::Scope {
            jobs: jobs.iter().map(|j| j.to_string()).collect(),
            engines: ["chromium", "webkit"].map(String::from).into(),
            apps: Default::default(),
            whole: true,
        };
        format!("{}\n", run_scope::ci_scope(scope(ran_jobs), false, scope(need_jobs)).line())
    }

    /// Run 5 (a push to main) with the plan log `log` and these jobs besides its plan job: `(id, name, conclusion)`.
    fn push_with_jobs(log: &str, jobs: &[(u64, &str, &str)]) -> Fake {
        let mut f = server(vec![main_push(5, "success")], &[(5, 50, Some(log))]);
        let mut list = vec![json!({"id": 50, "name": "plan (affected projects)", "conclusion": "success"})];
        list.extend(jobs.iter().map(|(id, n, c)| json!({"id": id, "name": n, "conclusion": c})));
        f.json(
            &format!("{API}/runs/5/jobs?limit=50&page=1"),
            json!({"total_count": list.len(), "jobs": list}),
        );
        f
    }

    #[test]
    fn a_declared_job_that_was_skipped_is_partial_dropping_the_webkit_output_is_not_green() {
        // The run declares WebKit (jobs `e2e` and `e2e-webkit`). With the planner's `webkit` output dropped the e2e-webkit job is
        // skipped, and a green run must not stand for a scope it did not run (as GitHub's required checks: a skipped one blocks).
        let line = declared_line(&["conformance", "e2e", "e2e-webkit"], &["conformance", "e2e", "e2e-webkit"]);
        let jobs = |webkit: &'static str| {
            [
                (51, "conformance (checks, release scripts)", "success"),
                (52, "reference apps (shard 1/2)", "success"),
                (53, "reference apps (shard 2/2)", "success"),
                (54, "reference apps in WebKit (macOS)", webkit),
            ]
        };
        // Positive control: every declared job succeeded.
        assert_eq!(ask(&mut push_with_jobs(&line, &jobs("success"))), Verdict::Green { run: 5 });
        let skipped = ask(&mut push_with_jobs(&line, &jobs("skipped")));
        assert!(
            matches!(&skipped, Verdict::Red { run: None, reason } if reason.contains("e2e-webkit") && reason.contains("skipped")),
            "{skipped:?}"
        );
        assert_eq!(code(&skipped), 5);
        // Cancelled is the same; a failed job is the run's own red, not a partial scope.
        assert!(matches!(
            ask(&mut push_with_jobs(&line, &jobs("cancelled"))),
            Verdict::Red { run: None, .. }
        ));
        let failed_run = {
            let mut f = push_with_jobs(&line, &jobs("failure"));
            f.json(
                &format!("{API}/runs?head_sha={SHA}&limit=50&page=1"),
                json!({"total_count": 1, "workflow_runs": [main_push(5, "failure")]}),
            );
            f
        };
        // The failed job's log is read (green.rs): one that is no test report is a failed step, red; none at all is not measured.
        let mut with_log = failed_run;
        with_log.log(54, "error: the WebKit step failed\n");
        assert!(matches!(ask(&mut with_log), Verdict::Red { run: Some(5), .. }));
        let mut no_log = push_with_jobs(&line, &jobs("failure"));
        no_log.json(
            &format!("{API}/runs?head_sha={SHA}&limit=50&page=1"),
            json!({"total_count": 1, "workflow_runs": [main_push(5, "failure")]}),
        );
        assert!(matches!(ask(&mut no_log), Verdict::NotMeasured(_)));
    }

    #[test]
    fn a_declared_job_with_no_job_of_its_name_is_not_measured() {
        let line = declared_line(&["conformance", "e2e-webkit"], &["conformance", "e2e-webkit"]);
        let v = ask(&mut push_with_jobs(
            &line,
            &[(51, "conformance (checks, release scripts)", "success")],
        ));
        assert!(matches!(&v, Verdict::NotMeasured(why) if why.contains("e2e-webkit")), "{v:?}");
        assert_eq!(code(&v), 6);
    }

    #[test]
    fn a_push_whose_declared_need_is_empty_is_partial_and_names_the_last_fully_tested_commit() {
        // Nothing declared needed nothing proves nothing. It is partial like any other, and the refusal names the commit to tag.
        let e = "e".repeat(40);
        let mut earlier = main_push(6, "success");
        earlier["head_sha"] = json!(e);
        let empty = declared_line(&[], &[]);
        let mut f = push_with_jobs(&empty, &[]);
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 2, "workflow_runs": [main_push(5, "success"), earlier.clone()]}),
        );
        f.json(
            &format!("{API}/runs?head_sha={e}&limit=50&page=1"),
            json!({"total_count": 1, "workflow_runs": [earlier]}),
        );
        f.json(
            &format!("{API}/runs/6/jobs?limit=50&page=1"),
            json!({"total_count": 2, "jobs": [
                {"id": 60, "name": "plan (affected projects)", "conclusion": "success"},
                {"id": 61, "name": "conformance (checks, release scripts)", "conclusion": "success"}]}),
        );
        f.log(60, run_scope::TEST_COMPLETE);
        let v = ask(&mut f);
        assert!(
            matches!(&v, Verdict::Red { run: None, reason } if reason.contains("empty") && reason.contains("the last fully tested commit on main is") && reason.contains(&e)),
            "{v:?}"
        );
        assert_eq!(code(&v), 5);
    }

    #[test]
    fn until_its_producer_is_recorded_the_old_line_decides_and_the_scope_view_is_only_a_notice() {
        // Report-only (ER-41): a run whose old line says complete and whose declared scope is partial is green on the old basis and
        // not on the new one; the reverse is red on the old basis. The positive control for each is the other call.
        let both = |old: &str, scope: &str| {
            push_with_jobs(
                &format!("{old}{scope}"),
                &[(51, "conformance (checks, release scripts)", "success")],
            )
        };
        let old_complete = "plan verdict: complete=true\n";
        let old_partial = "plan verdict: complete=false\n";
        assert_eq!(ask_old(&mut both(old_complete, run_scope::TEST_PARTIAL)), Verdict::Green { run: 5 });
        assert!(matches!(
            ask(&mut both(old_complete, run_scope::TEST_PARTIAL)),
            Verdict::Red { run: None, .. }
        ));
        assert!(matches!(
            ask_old(&mut both(old_partial, run_scope::TEST_COMPLETE)),
            Verdict::Red { run: None, .. }
        ));
        assert_eq!(ask(&mut both(old_partial, run_scope::TEST_COMPLETE)), Verdict::Green { run: 5 });
        // A run with no scope line at all (printed by a producer that has not run yet) is still decided by the old line.
        assert_eq!(ask_old(&mut both(old_complete, "")), Verdict::Green { run: 5 });
    }
    fn record(f: &mut Fake, workflow: &str, id: u64) -> Result<String, String> {
        let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "t");
        c.backoff_ms = 0;
        first_green_scope(&mut c, workflow, id)
    }

    #[test]
    fn the_first_green_scope_run_is_recorded_only_for_a_finished_green_complete_scope_that_ran() {
        let ok_jobs = [(51, "conformance (checks, release scripts)", "success")];
        let with_run = |mut f: Fake, conclusion: &str, status: &str| {
            let mut r = main_push(5, conclusion);
            r["status"] = json!(status);
            f.json(&format!("{API}/runs/5"), r);
            f
        };
        // Positive control: a green run, a complete scope, its job ran.
        let mut f = with_run(push_with_jobs(run_scope::TEST_COMPLETE, &ok_jobs), "success", "completed");
        assert!(record(&mut f, "ci.yml", 5).unwrap().contains("declared a complete scope"));
        // One reason at a time: another workflow, not finished, red, a partial scope, a declared job that did not run, no such run.
        let mut f = with_run(push_with_jobs(run_scope::TEST_COMPLETE, &ok_jobs), "success", "completed");
        assert!(record(&mut f, "debrief.yml", 5).unwrap_err().contains("not of debrief.yml"));
        let mut f = with_run(push_with_jobs(run_scope::TEST_COMPLETE, &ok_jobs), "", "in_progress");
        assert!(record(&mut f, "ci.yml", 5).unwrap_err().contains("record it when it has finished"));
        let mut f = with_run(push_with_jobs(run_scope::TEST_COMPLETE, &ok_jobs), "failure", "completed");
        assert!(record(&mut f, "ci.yml", 5).unwrap_err().contains("not success"));
        let mut f = with_run(push_with_jobs(run_scope::TEST_PARTIAL, &ok_jobs), "success", "completed");
        assert!(record(&mut f, "ci.yml", 5).unwrap_err().contains("not a complete scope"));
        let mut f = with_run(
            push_with_jobs(
                run_scope::TEST_COMPLETE,
                &[(51, "conformance (checks, release scripts)", "skipped")],
            ),
            "success",
            "completed",
        );
        assert!(record(&mut f, "ci.yml", 5).unwrap_err().contains("was skipped"));
        let mut f = Fake::default();
        assert!(record(&mut f, "ci.yml", 5).unwrap_err().contains("does not exist"));
    }

    #[test]
    fn a_failed_full_run_with_only_untouched_flaky_failures_is_green_with_fix_items_and_a_tag_gets_exit_5() {
        // The 8610 shape: the one definition of green (green.rs) read through the verdict.
        let mut f = server(vec![dispatch(9, "failure")], &[(9, 90, Some(run_scope::TEST_COMPLETE))]);
        f.json(
            &format!("{API}/runs/9/jobs?limit=50&page=1"),
            json!({"total_count": 3, "jobs": [
                {"id": 90, "name": "plan (affected projects)", "conclusion": "success"},
                {"id": 91, "name": "conformance (checks, release scripts)", "conclusion": "success"},
                {"id": 92, "name": "crate (windows-latest)", "conclusion": "failure",
                 "steps": [{"name": "Tests (workspace, every feature)", "number": 3, "status": "completed", "conclusion": "failure"}]}]}),
        );
        f.log(
            92,
            &format!("Job: crate\n{}", include_str!("../../tests/ci-runs/nextest-r1-flaky-only.log")),
        );
        let mut ask_with = |apps: &[&str]| {
            let t = Touched {
                flags: apps.iter().filter(|a| **a == "crates").map(|a| a.to_string()).collect(),
                ..Touched::default()
            };
            let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
            c.backoff_ms = 0;
            verdict_with_touched(&mut c, "ci.yml", SHA, None, true, &[], &mut |_: &str| Some(t.clone()))
        };
        let v = ask_with(&[]);
        assert!(
            matches!(&v, Verdict::GreenWithFixItems { run: 9, items } if items.len() == 1),
            "{v:?}"
        );
        assert_eq!(code(&v), 5, "a tag needs a run without fix items");
        assert!(report(&v, "ci.yml", SHA, &Knows(true)).text.contains("green with 1 fix items"));
        let v = ask_with(&["crates"]);
        assert!(
            matches!(&v, Verdict::Red { run: Some(9), reason } if reason.contains("touched")),
            "{v:?}"
        );
    }
}
