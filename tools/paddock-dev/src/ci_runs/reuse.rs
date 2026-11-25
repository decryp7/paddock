// ci-runs-reader: ci reuse: does a full green run already cover this commit (ER-49)
//! `ci reuse`: whether a push to main ends early because a full green run of the same workflow already tested this exact commit
//! (the train's own run, when main fast-forwards to the train head). ER-49: test a commit once.
//!
//! The answer comes only from [`verdict::verdict_excluding`], the one reader of the runs for "is there a full green run of W on
//! S" (the asking run itself left out: it is still going). Reuse needs more than a green verdict:
//! - a run that is green must have no FLAKY test in a job log that ran (2026-11-21: green with one marked is not the run `tag.yml` can
//!   use, so the post-landing run on main has to be a real one); a run that is red only with FLAKY tests of untouched targets is
//!   green with fix items on the one definition (ADR 0117 WP-4) and is reused (a tag then takes its one allowed retry, ER-49);
//! - anything that cannot be read (the runs, the jobs, a log) means "run": the step fails open to the full run, never to a
//!   skipped one.
//!
//! - "complete" has one meaning (ER-15, ADR 0113): the run declared a scope that covers a push to main's (`run_scope`), engines
//!   included. A run that declares less than that (an engine, an app, a job) is neither reused here nor counted by the verdict
//!   reader, and `tag.yml` never reads a green verdict that skipped it (the 2026-11-12 schedule-skipped and rerun-only runs counted
//!   green were the same class). A push to main's scope has no WebKit since 2026-11-24 (the nightly's), so a train's full dispatch
//!   covers it.
//!
//! A run that reuses another declares that it ran nothing (`PLAN_REUSE`, `dev plan --ci`), so the reader never counts it; the reused run is the one
//! that decides the commit.

use super::client::Client;
use super::logfind;
use super::verdict::{self, Verdict};
use crate::job_outcome::JobOutcome;

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
        .filter(|j| !j.is_caller() && !j.never_ran() && j.outcome != JobOutcome::Skipped)
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

/// Decides for `workflow` on `sha`; `current` is the asking run's id (left out of the verdict); `before` is main's tip before this push
/// (the push event's `before`, [`super::green::push_before`]). The basis is the verdict's: the declared scope once its producer is
/// recorded, the old line before (ER-41).
pub fn decide(client: &mut Client, workflow: &str, sha: &str, current: Option<u64>, before: Option<&str>) -> Reuse {
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    decide_with(client, workflow, sha, current, verdict::scope_decisive(workflow), &root, before)
}

/// [`decide`] with the basis and the checkout given. The push's base is searched on main's own line ([`super::green::push_chain`]): a
/// train lands by fast-forward, so the first-parent line of `sha` is the train's and never holds main's previous tip.
pub fn decide_with(
    client: &mut Client,
    workflow: &str,
    sha: &str,
    current: Option<u64>,
    scope: bool,
    root: &std::path::Path,
    before: Option<&str>,
) -> Reuse {
    decide_with_touched(
        client,
        workflow,
        sha,
        current,
        scope,
        &mut || super::green::push_chain(root, sha, before),
        &mut |c, s| super::green::touched_in(c, workflow, root, s, scope, None),
        &mut |from, base| super::green::diff_covers(root, from, base, sha),
    )
}

/// [`decide_with`] with the way to read the push's candidate bases (newest first), what a commit's change touched, and whether a
/// landing run's diff from `from` covers the push's diff from `base` (`covers(from, base)`, asked only when they differ) given.
#[allow(clippy::too_many_arguments)]
pub fn decide_with_touched(
    client: &mut Client,
    workflow: &str,
    sha: &str,
    current: Option<u64>,
    scope: bool,
    first_parent: &mut dyn FnMut() -> Vec<String>,
    touched: &mut dyn FnMut(&mut Client, &str) -> Option<super::green::Touched>,
    covers: &mut dyn FnMut(&str, &str) -> bool,
) -> Reuse {
    // A landing run that ran exactly its own diff's narrowed selection is reused only when its diff covers this push's: its need was
    // diffed from this push's own base, the last complete green commit of main before it (review round 2 P1-A), or from an older
    // commit whose diff holds every file the push's does. Never a run that tested less than the push.
    let (v, landing_base) = verdict::landing_verdict_with(client, workflow, sha, current, scope, touched);
    if let Some(from) = landing_base {
        let push_base = super::green::find_base(client, workflow, &first_parent(), scope, touched).map(|b| b.map(|b| b.sha));
        match push_base {
            Ok(Some(b)) if b == from || covers(&from, &b) => {}
            Ok(b) => {
                return Reuse::Run(format!(
                    "the landing run tested the diff from {from}, this push's diff is from {} (the last complete green commit of main) and the landing's does not cover it: it runs its own",
                    b.as_deref().unwrap_or("no complete commit")
                ));
            }
            Err(e) => return Reuse::Run(format!("this push's base could not be read ({e})")),
        }
    }
    reuse_of(client, v)
}

/// What a verdict means for the run that asks.
fn reuse_of(client: &mut Client, v: Verdict) -> Reuse {
    match v {
        Verdict::Green { run } => match clean_of_flaky(client, run) {
            Ok(logs) => Reuse::Skip { run, logs },
            Err(why) => Reuse::Run(why),
        },
        // ADR 0117 WP-4: a complete run whose only failures are FLAKY tests of untouched targets is green for a landing, so the push
        // after it reuses it. The landing queue recorded its fix items on the pull requests (queue/tick.rs). A tag asks for a run
        // without them: its one allowed retry is a manual workflow_dispatch of ci.yml on that commit (tag.yml, ER-49), which this
        // step never skips (it runs only for a push to main).
        Verdict::GreenWithFixItems { run, .. } => Reuse::Skip { run, logs: 0 },
        Verdict::Red { reason, .. } => Reuse::Run(reason),
        Verdict::NotMeasured(why) => Reuse::Run(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;
    use serde_json::json;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";
    const FLAKY_LOG: &str = include_str!("../../tests/ci-runs/nextest-r1-pass.log");

    fn run(id: u64, event: &str, branch: &str, conclusion: &str) -> serde_json::Value {
        json!({"id": id, "path": format!("ci.yml@refs/heads/{branch}"), "event": event, "head_branch": branch, "status": "completed",
               "conclusion": conclusion, "head_sha": SHA, "created_at": "2026-11-21T00:00:00Z"})
    }

    /// The server: the runs of the commit, the plan log of each full run, and `jobs` (id, log) for the run `green`.
    fn server(runs: Vec<serde_json::Value>, full: &[u64], jobs_of: u64, jobs: &[(u64, Option<&str>)]) -> Fake {
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
            f.log(
                plan,
                if full.contains(&id) {
                    crate::run_scope::TEST_COMPLETE
                } else {
                    crate::run_scope::TEST_PARTIAL
                },
            );
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
        decide_with(&mut c, "ci.yml", SHA, current, true, std::path::Path::new("."), None)
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
    fn an_os_only_run_of_the_commit_is_not_reused_as_its_complete_verdict() {
        // Run 7's plan declares a scope that covers a push to main's, but for `os=linux` only: the member gate's Linux trial.
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "success")],
            &[7],
            7,
            &[(71, Some("ok\n"))],
        );
        f.log(7 + 5000, crate::run_scope::TEST_OS_ONLY);
        let r = ask(&mut f, Some(9));
        assert!(matches!(&r, Reuse::Run(why) if why.contains("one operating system")), "{r:?}");
        // Positive control: the same run without the os flag is reused.
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "success")],
            &[7],
            7,
            &[(71, Some("ok\n"))],
        );
        assert!(ask(&mut f, Some(9)).skip());
    }

    #[test]
    fn a_full_green_dispatch_that_declares_less_than_a_push_to_main_is_not_reused_and_one_that_covers_it_is() {
        // Run 7 below declares a scope smaller than a push to main's (an engine or a job less).
        let mut f = server(
            vec![run(7, "workflow_dispatch", "train/x", "success")],
            &[],
            7,
            &[(71, Some("ok\n"))],
        );
        let r = ask(&mut f, Some(9));
        assert!(matches!(&r, Reuse::Run(why) if why.contains("full run")), "{r:?}");
        // The same run with its scope declared complete is reused.
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

    #[test]
    fn a_failed_run_with_only_untouched_flaky_failures_is_reused_by_the_push_after_it() {
        // ADR 0117 WP-4: the landing took it as green with fix items, so the push to main reuses it instead of testing again. The
        // run is train 19's 8610 (its real, scrubbed job records and the crate job's log).
        let mut r = run(8610, "workflow_dispatch", "train/x", "failure");
        r["head_sha"] = json!(SHA);
        let mut f = server(vec![r], &[8610], 8610, &[]);
        f.json(
            &format!("{API}/runs/8610/jobs?limit=50&page=1"),
            serde_json::from_str(include_str!("../../tests/ci-runs/run-8610-jobs.json")).unwrap(),
        );
        f.log(44709, crate::run_scope::TEST_COMPLETE);
        f.log(44761, include_str!("../../tests/ci-runs/nextest-run8610-crate-flaky-only.log"));
        f.log(44768, include_str!("../../tests/ci-runs/nextest-run8610-app-gates-flaky-only.log"));
        let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
        c.backoff_ms = 0;
        let mut ask = |flags: &[&str]| {
            let t = crate::ci_runs::green::Touched {
                flags: flags.iter().map(|a| a.to_string()).collect(),
                ..Default::default()
            };
            decide_with_touched(
                &mut c,
                "ci.yml",
                SHA,
                None,
                true,
                &mut Vec::new,
                &mut |_, _| Some(t.clone()),
                &mut |_, _| false,
            )
        };
        assert!(matches!(ask(&[]), Reuse::Skip { run: 8610, .. }));
        // Positive control: when the plan reached the crates, the FLAKY test is red and the push runs.
        assert!(matches!(ask(&["crates"]), Reuse::Run(_)));
    }
}
