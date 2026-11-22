// ci-runs-reader: ci reuse: does a full green run already cover this commit (ER-49)
//! `ci reuse`: whether a push to main ends early because a full green run of the same workflow already tested this exact commit
//! (the train's own run, when main fast-forwards to the train head). ER-49: test a commit once.
//!
//! A scheduled run (`--event schedule`, the nightly) asks the same reader for more ([`Asker::Schedule`]): the run must have declared the
//! whole tree (ci.yml) or the nightly's own job (an app workflow), because a night tests more than a push to main does.
//!
//! The answer comes only from [`verdict::verdict_excluding`], the one reader of the runs for "is there a full green run of W on
//! S" (the asking run itself left out: it is still going). Reuse needs more than a green verdict:
//! - every job that ran must have a readable log without a FLAKY test (2026-11-21: a run that was red only with FLAKY tests, or
//!   green with one marked, is not the run `tag.yml` can use, so the post-landing run on main has to be a real one);
//! - anything that cannot be read (the runs, the jobs, a log) means "run": the step fails open to the full run, never to a
//!   skipped one.
//!
//! - "complete" has one meaning (ER-15, ADR 0113): the run declared a scope that covers a push to main's (`run_scope`), engines
//!   included. A train's full dispatch without WebKit declares an engine less, so it is neither reused here nor counted by the
//!   verdict reader: the push to main runs, WebKit
//!   included, and `tag.yml` never reads a green verdict that skipped it (the 2026-11-12 schedule-skipped and
//!   rerun-only runs counted green were the same class).
//!
//! A run that reuses another declares that it ran nothing (`PLAN_REUSE`, `dev plan --ci`), so the reader never counts it; the reused run is the one
//! that decides the commit.

use super::client::Client;
use super::logfind;
use super::verdict::{self, Verdict};
use crate::run_scope::Asker;

/// What the check decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Reuse {
    /// A full green run with no FLAKY test already tested this commit: this run ends early.
    Skip {
        /// The reused run.
        run: u64,
        /// How many job logs were read to check for FLAKY tests.
        logs: usize,
    },
    /// This run goes on, and why.
    Run(String),
}

impl Reuse {
    /// Whether the run ends early.
    pub fn skip(&self) -> bool {
        matches!(self, Reuse::Skip { .. })
    }

    /// The one annotation line for the job log.
    pub fn annotation(&self, workflow: &str, sha: &str) -> String {
        let short = sha.get(..10).unwrap_or(sha);
        match self {
            Reuse::Skip { run, logs } => format!(
                "::notice::{workflow} on {short}: run {run} already tested this commit in full and green, with no FLAKY test ({logs} job logs read); this run ends early and reuses it (ER-49)."
            ),
            Reuse::Run(why) => format!("::notice::{workflow} on {short}: no earlier run is reused, this run tests the commit: {why}"),
        }
    }

    /// The job-summary line.
    pub fn summary(&self, workflow: &str) -> String {
        match self {
            Reuse::Skip { run, .. } => format!(
                "### Reused run\n\nNo jobs ran: {workflow} run {run} already tested this commit in full, green, with no FLAKY test (ER-49).\n"
            ),
            Reuse::Run(why) => format!("### Reused run\n\nNone: {why}.\n"),
        }
    }
}

/// Reads the jobs of the green run `run` and refuses it when a job that ran has a FLAKY test, a log that cannot be read, or a log
/// whose test runner did not finish. Returns the number of logs read.
fn clean_of_flaky(client: &mut Client, run: u64) -> Result<usize, String> {
    let jobs = match client.timed_jobs(run) {
        Ok(Some(j)) => j,
        Ok(None) => return Err(format!("run {run} has no job list on the server")),
        Err(e) => return Err(format!("the jobs of run {run} could not be listed: {e}")),
    };
    let mut read = 0;
    for job in jobs
        .iter()
        .filter(|j| !j.is_caller() && !j.never_ran() && j.conclusion != "skipped")
    {
        match client.log(job.id) {
            Ok(Some(log)) => {
                read += 1;
                let f = logfind::read(&log);
                if let Some(t) = f.flaky.iter().next() {
                    return Err(format!(
                        "job {:?} of run {run} has a FLAKY test ({t}), and tag.yml needs a run without one",
                        job.name
                    ));
                }
                if let Some(why) = f.broken {
                    return Err(format!("the log of job {:?} of run {run} is not measured ({why})", job.name));
                }
            }
            Ok(None) => return Err(format!("job {:?} of run {run} has no log", job.name)),
            Err(e) => return Err(format!("the log of job {:?} of run {run} could not be read: {e}", job.name)),
        }
    }
    Ok(read)
}

/// Decides for `workflow` on `sha`; `current` is the asking run's id (left out of the verdict). The basis is the verdict's: the declared
/// scope once its producer is recorded, the old line before (ER-41).
pub fn decide(client: &mut Client, workflow: &str, sha: &str, current: Option<u64>, asker: Asker) -> Reuse {
    decide_with(client, workflow, sha, current, verdict::scope_decisive(workflow), asker)
}

/// [`decide`] with the basis given. A scheduled asker is reused for only on the declared scope (`scope`): the old
/// `plan verdict: complete=` line says what a push to main does and cannot say a night's whole tree or its extra jobs, so on that
/// basis the night runs (ER-33, ER-41: the schedule is a new use of the reader, switched on with the producer's record).
pub fn decide_with(client: &mut Client, workflow: &str, sha: &str, current: Option<u64>, scope: bool, asker: Asker) -> Reuse {
    if asker == Asker::Schedule && !scope {
        return Reuse::Run(format!(
            "a scheduled run is reused only on the declared scope, and {} does not name {} yet (ER-41, ADR 0113)",
            crate::run_scope::REGISTER,
            crate::run_scope::producer_of(workflow)
        ));
    }
    match verdict::verdict_for(client, workflow, sha, current, scope, asker) {
        Verdict::Green { run } => match clean_of_flaky(client, run) {
            Ok(logs) => Reuse::Skip { run, logs },
            Err(why) => Reuse::Run(why),
        },
        Verdict::Red { reason, .. } => Reuse::Run(reason),
        Verdict::NotMeasured(why) => Reuse::Run(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;
    use crate::run_scope::Asker;
    use serde_json::json;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";
    const FLAKY_LOG: &str = include_str!("../../tests/ci-runs/nextest-r1-pass.log");

    fn run(id: u64, event: &str, branch: &str, conclusion: &str) -> serde_json::Value {
        run_of("ci.yml", id, event, branch, conclusion)
    }

    fn run_of(workflow: &str, id: u64, event: &str, branch: &str, conclusion: &str) -> serde_json::Value {
        json!({"id": id, "path": format!("{workflow}@refs/heads/{branch}"), "event": event, "head_branch": branch, "status": "completed",
               "conclusion": conclusion, "head_sha": SHA, "created_at": "2026-11-21T00:00:00Z"})
    }

    /// The server: the runs of the commit, the plan log of each full run, and `jobs` (id, log) for the run `green`.
    fn server(runs: Vec<serde_json::Value>, full: &[u64], jobs_of: u64, jobs: &[(u64, Option<&str>)]) -> Fake {
        let logs: Vec<(u64, &str)> = runs
            .iter()
            .map(|r| {
                let id = r["id"].as_u64().unwrap();
                (
                    id,
                    if full.contains(&id) {
                        crate::run_scope::TEST_COMPLETE
                    } else {
                        crate::run_scope::TEST_PARTIAL
                    },
                )
            })
            .collect();
        server_logs(runs, &logs, jobs_of, jobs)
    }

    /// [`server`] with the plan log of each run given: `(run id, log)`.
    fn server_logs(runs: Vec<serde_json::Value>, plan_logs: &[(u64, &str)], jobs_of: u64, jobs: &[(u64, Option<&str>)]) -> Fake {
        let mut f = Fake::default();
        f.json(
            &format!("{API}/runs?head_sha={SHA}&limit=50&page=1"),
            json!({"total_count": runs.len(), "workflow_runs": runs}),
        );
        let step = json!([{"name": "s", "started_at": "2026-11-21T00:00:00Z", "completed_at": "2026-11-21T00:01:00Z"}]);
        for r in &runs {
            let id = r["id"].as_u64().unwrap();
            let plan = id + 5000;
            let mut list = vec![
                json!({"id": plan, "name": "plan (affected projects)", "conclusion": "success", "runner_id": 1, "steps": step}),
                // The one job the declared scope names (run_scope::TEST_COMPLETE): it ran and has a log.
                json!({"id": id + 9000, "name": "conformance (checks, release scripts)", "conclusion": "success", "runner_id": 1, "steps": step}),
            ];
            f.log(id + 9000, "ok\n");
            if id == jobs_of {
                for (jid, _) in jobs {
                    list.push(json!({"id": jid, "name": format!("job {jid}"), "conclusion": "success", "runner_id": 1, "steps": step}));
                }
                // A job that never ran (skipped): it has no log and is not read.
                list.push(json!({"id": 999_999, "name": "skipped job", "conclusion": "skipped", "runner_id": 0, "steps": []}));
            }
            f.json(
                &format!("{API}/runs/{id}/jobs?limit=50&page=1"),
                json!({"total_count": list.len(), "jobs": list}),
            );
            f.log(plan, plan_logs.iter().find(|(i, _)| *i == id).map_or("", |(_, l)| l));
        }
        for (jid, log) in jobs {
            match log {
                Some(l) => f.log(*jid, l),
                None => f.fail(*jid, "curl exited with 22: 500"),
            }
        }
        f
    }

    fn ask(f: &mut Fake, current: Option<u64>) -> Reuse {
        let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "t");
        c.backoff_ms = 0;
        decide_with(&mut c, "ci.yml", SHA, current, true, Asker::Push)
    }

    fn ask_for(f: &mut Fake, workflow: &str, asker: Asker, scope: bool) -> Reuse {
        let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "t");
        c.backoff_ms = 0;
        decide_with(&mut c, workflow, SHA, Some(9), scope, asker)
    }

    /// A debrief.yml scope line: what `./dev ci scope --workflow debrief.yml` prints for these flags.
    fn debrief_line(run: bool, nightly: bool) -> String {
        let engines = ["chromium".to_string(), "webkit".to_string()];
        format!(
            "{}\n",
            crate::run_scope::app_scope("debrief.yml", "debrief", true, run, false, nightly, &engines).line()
        )
    }

    #[test]
    fn a_scheduled_ci_run_reuses_a_whole_tree_run_and_never_a_push_to_mains_own_plan() {
        // Run 7: a train's full dispatch (its need is the whole tree). Run 6: an earlier push to main, complete for a push but
        // its need is only its own affected plan: a night tests more.
        let tree = |id: u64| {
            server_logs(
                vec![run(id, "workflow_dispatch", "train/x", "success")],
                &[(id, crate::run_scope::TEST_COMPLETE_TREE)],
                id,
                &[(id * 10 + 1, Some("ok\n"))],
            )
        };
        let r = ask_for(&mut tree(7), "ci.yml", Asker::Schedule, true);
        assert_eq!(r, Reuse::Skip { run: 7, logs: 3 }, "{r:?}");
        let mut push = server_logs(
            vec![run(6, "push", "main", "success")],
            &[(6, crate::run_scope::TEST_COMPLETE)],
            6,
            &[(61, Some("ok\n"))],
        );
        let r = ask_for(&mut push, "ci.yml", Asker::Schedule, true);
        assert!(matches!(&r, Reuse::Run(why) if why.contains("whole tree")), "{r:?}");
        // Positive control: the same run is reused for a push to main.
        assert!(ask_for(&mut push, "ci.yml", Asker::Push, true).skip());
        // A whole-tree run without WebKit declares an engine less than its own need: not complete, so not reused by a night either.
        let mut no_webkit = server_logs(
            vec![run(5, "workflow_dispatch", "train/x", "success")],
            &[(
                5,
                &crate::run_scope::TEST_COMPLETE_TREE.replace(
                    "\"ran\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"]",
                    "\"ran\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\"]",
                ),
            )],
            5,
            &[(51, Some("ok\n"))],
        );
        let r = ask_for(&mut no_webkit, "ci.yml", Asker::Schedule, true);
        assert!(matches!(&r, Reuse::Run(why) if why.contains("webkit")), "{r:?}");
    }

    #[test]
    fn a_scheduled_debrief_run_reuses_only_a_run_that_declared_the_nightly_job() {
        // Run 7: an earlier full push or dispatch (budgets, benchmark and scheduled-only steps did not run). Run 8: a night that ran.
        let lines = [(7, debrief_line(true, false)), (8, debrief_line(true, true))];
        let one = |id: u64| {
            let logs: Vec<(u64, &str)> = lines.iter().filter(|(i, _)| *i == id).map(|(i, l)| (*i, l.as_str())).collect();
            server_logs(
                vec![run_of("debrief.yml", id, "workflow_dispatch", "main", "success")],
                &logs,
                id,
                &[(id * 10 + 1, Some("ok\n"))],
            )
        };
        let r = ask_for(&mut one(7), "debrief.yml", Asker::Schedule, true);
        assert!(matches!(&r, Reuse::Run(why) if why.contains("nightly")), "{r:?}");
        let r = ask_for(&mut one(8), "debrief.yml", Asker::Schedule, true);
        assert!(matches!(&r, Reuse::Skip { run: 8, .. }), "{r:?}");
        // Positive control: the full run without the nightly job is reused for a push to main.
        assert!(ask_for(&mut one(7), "debrief.yml", Asker::Push, true).skip());
    }

    #[test]
    fn a_night_that_was_itself_skipped_or_reused_is_never_the_run_a_later_night_reuses() {
        // A skipped or reusing night declares run=false: it ran nothing, so it carries no nightly job and is no evidence.
        let line = debrief_line(false, true);
        let mut f = server_logs(
            vec![run_of("debrief.yml", 7, "main", "main", "success")],
            &[(7, line.as_str())],
            7,
            &[(71, Some("ok\n"))],
        );
        let r = ask_for(&mut f, "debrief.yml", Asker::Schedule, true);
        assert!(matches!(&r, Reuse::Run(_)), "{r:?}");
    }

    #[test]
    fn a_scheduled_run_is_never_reused_on_the_old_basis_and_keeps_the_flaky_refusal() {
        // Until the producer is recorded (ER-41) the old line decides, and it cannot say a night's scope: the night runs, without
        // reading a single run.
        let mut none = Fake::default();
        let r = ask_for(&mut none, "ci.yml", Asker::Schedule, false);
        assert!(
            matches!(&r, Reuse::Run(why) if why.contains("not name") && why.contains("ci.yml")),
            "{r:?}"
        );
        // The FLAKY refusal holds for a night too (positive control: the same server without the marker is reused).
        let flaky = |log: &'static str| {
            server_logs(
                vec![run(7, "workflow_dispatch", "train/x", "success")],
                &[(7, crate::run_scope::TEST_COMPLETE_TREE)],
                7,
                &[(72, Some(log))],
            )
        };
        let r = ask_for(&mut flaky(FLAKY_LOG), "ci.yml", Asker::Schedule, true);
        assert!(matches!(&r, Reuse::Run(why) if why.contains("FLAKY")), "{r:?}");
        assert!(ask_for(&mut flaky("ok\n"), "ci.yml", Asker::Schedule, true).skip());
    }

    #[test]
    fn a_full_green_train_run_on_the_same_commit_is_reused() {
        // Run 7 is the train's dispatch (full, green); run 9 is this push to main, still going.
        let mut me = run(9, "push", "main", "");
        me["status"] = json!("in_progress");
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "success"), me],
            &[7],
            7,
            &[(71, Some("ok\n"))],
        );
        let r = ask(&mut f, Some(9));
        assert_eq!(r, Reuse::Skip { run: 7, logs: 3 }, "{r:?}");
        let note = r.annotation("ci.yml", SHA);
        assert!(
            note.contains("run 7") && note.contains("ends early") && note.contains("ER-49"),
            "{note}"
        );
        assert!(r.summary("ci.yml").contains("run 7"));
    }

    #[test]
    fn a_full_green_dispatch_without_webkit_is_not_reused_and_one_with_webkit_is() {
        // ci.yml declares a scope without the WebKit engine for a full dispatch that did not run it (a smaller scope than a push to main): run 7 below.
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "success")],
            &[],
            7,
            &[(71, Some("ok\n"))],
        );
        let r = ask(&mut f, Some(9));
        assert!(matches!(&r, Reuse::Run(why) if why.contains("full run")), "{r:?}");
        // The same run with the WebKit scope declared complete is reused.
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "success")],
            &[7],
            7,
            &[(71, Some("ok\n"))],
        );
        assert!(ask(&mut f, Some(9)).skip());
    }

    #[test]
    fn a_red_train_run_is_not_reused() {
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "failure")],
            &[7],
            7,
            &[(71, Some("ok\n"))],
        );
        let r = ask(&mut f, Some(9));
        assert!(matches!(&r, Reuse::Run(why) if why.contains("failure")), "{r:?}");
    }

    #[test]
    fn a_green_run_with_a_flaky_test_is_not_reused_because_tag_needs_a_run_without_one() {
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "success")],
            &[7],
            7,
            &[(71, Some("ok\n")), (72, Some(FLAKY_LOG))],
        );
        let r = ask(&mut f, Some(9));
        assert!(
            matches!(&r, Reuse::Run(why) if why.contains("FLAKY") && why.contains("nxflaky")),
            "{r:?}"
        );
        assert!(!r.skip());
    }

    #[test]
    fn positive_control_the_flaky_check_sees_the_marker_and_goes_green_without_it() {
        // Same server, the one log swapped: the flaky log decides, nothing else.
        let with = ask(
            &mut server(
                vec![run(7, "workflow_dispatch", "train/x", "success")],
                &[7],
                7,
                &[(72, Some(FLAKY_LOG))],
            ),
            None,
        );
        let without = ask(
            &mut server(
                vec![run(7, "workflow_dispatch", "train/x", "success")],
                &[7],
                7,
                &[(72, Some("ok\n"))],
            ),
            None,
        );
        assert!(!with.skip() && without.skip(), "{with:?} {without:?}");
    }

    #[test]
    fn a_log_that_cannot_be_read_means_run() {
        let mut f = server(vec![run(7, "workflow_dispatch", "train/x", "success")], &[7], 7, &[(71, None)]);
        let r = ask(&mut f, Some(9));
        assert!(matches!(&r, Reuse::Run(why) if why.contains("could not be read")), "{r:?}");
    }

    #[test]
    fn a_partial_run_and_the_asking_run_itself_are_never_evidence() {
        // Run 7 is a base dispatch (its scope is partial); run 9 is this run, whose own plan log already declares a complete scope.
        let mut f = server(
            vec![run(7, "workflow_dispatch", "feat/x", "success"), run(9, "push", "main", "success")],
            &[9],
            9,
            &[(91, Some("ok\n"))],
        );
        let r = ask(&mut f, Some(9));
        assert!(matches!(&r, Reuse::Run(_)), "{r:?}");
        // Without leaving itself out, the asking run would be its own proof.
        assert!(ask(&mut f, None).skip());
    }

    #[test]
    fn an_earlier_full_run_still_going_runs_the_commit_with_a_notice_naming_it_never_a_warning() {
        let mut going = run(7, "workflow_dispatch", "train/x", "");
        going["status"] = json!("in_progress");
        let mut f = server(vec![going], &[7], 7, &[(71, Some("ok\n"))]);
        let r = ask(&mut f, Some(9));
        assert!(
            matches!(&r, Reuse::Run(why) if why.contains('7') && why.contains("in_progress")),
            "{r:?}"
        );
        let line = r.annotation("ci.yml", SHA);
        assert!(line.starts_with("::notice::") && !line.contains("token"), "{line}");
    }

    #[test]
    fn no_run_at_all_or_an_unreadable_list_means_run() {
        let mut f = server(vec![], &[], 0, &[]);
        assert!(matches!(ask(&mut f, Some(9)), Reuse::Run(_)));
        let mut f = Fake::default();
        f.error(&format!("{API}/runs?head_sha={SHA}&limit=50&page=1"), "curl exited with 22: 500");
        assert!(matches!(ask(&mut f, Some(9)), Reuse::Run(_)));
    }
}
