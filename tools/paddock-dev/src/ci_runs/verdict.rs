//! `ci verdict`: is there a full green run of workflow W on commit S? The one reader of the Actions runs for a pass/fail answer
//! about a commit (the tag workflow asks it; `conformance/test/ci-verdict-reader.test.mjs` keeps it the only one).
//!
//! Rules (the user, 2026-11-12; ER-33):
//! - A run counts only if it is a push to main, or its plan job logged `plan verdict: complete=true`
//!   ([`COMPLETE_LINE`], read by [`read_complete`]), printed after the plan decided: the plan ran its jobs, the run is not a
//!   rerun of failures and the mode is full. The earlier scope line (`plan mode: full=`) is not enough: a schedule-skipped run
//!   or a `since_run` dispatch logs `full=true` and ends green having tested nothing or less. A base dispatch or a pull request
//!   tests less and never counts, so it can neither clear a red full run nor stand in for one.
//! - Gitea's in-place "rerun failed jobs" keeps the run id: a deciding run with `run_attempt > 1` reads its earlier attempts,
//!   and any failed job there is red (an unreadable attempt is not measured).
//! - The newest full run decides (a green old run does not cover a newer red one).
//! - Unknown or unreadable is not measured, never a pass: the run list or a newer run's log cannot be read, no run is listed,
//!   or the deciding run has not finished.
//!
//! Exit codes: 0 full green, 5 red, or only partial runs exist, 6 not measured.

use super::client::{Client, RunInfo};
use crate::plan_since::{COMPLETE_LINE, read_complete, workflow_file};
use paddock_cli::{Exit, Report};

/// What the check found.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The newest full run finished and succeeded.
    Green {
        /// Its id.
        run: u64,
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

/// Whether a run is a push to main: full without asking its log.
fn is_main_push(r: &RunInfo) -> bool {
    r.event == "push" && r.head_branch == "main"
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

/// The pure decision over the runs listed for a commit. `mode` is asked for a run that is not a push to main, newest first,
/// and only until a full run is found.
pub fn decide(
    runs: &[RunInfo],
    workflow: &str,
    sha: &str,
    mode: &mut dyn FnMut(&RunInfo) -> Mode,
    earlier: &mut dyn FnMut(&RunInfo) -> Earlier,
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
        let m = if is_main_push(r) { Mode::Full } else { mode(r) };
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
                return if state == "success" {
                    Verdict::Green { run: r.id }
                } else {
                    Verdict::Red {
                        run: Some(r.id),
                        reason: format!(
                            "the newest full {workflow} run ({}) for {} is {state}, not success: fix it, get a full run green on this commit",
                            r.id,
                            short(sha)
                        ),
                    }
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
            "no {workflow} run for {} is a full run (a push to main, or a plan job that logged \"{COMPLETE_LINE}true\"); only partial: {}. Run it in full on this commit",
            short(sha),
            partial.join(", ")
        ),
    }
}

/// The failed jobs of the earlier attempts of a run (1 up to `run_attempt - 1`).
fn read_earlier(client: &mut Client, run: &RunInfo) -> Earlier {
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

/// How one run's plan job logged its mode.
fn read_mode(client: &mut Client, run: &RunInfo) -> Mode {
    let jobs = match client.timed_jobs(run.id) {
        Ok(Some(j)) => j,
        Ok(None) => return Mode::Unknown("the server has no job list for the run".into()),
        Err(e) => return Mode::Unknown(format!("the jobs could not be listed: {e}")),
    };
    let Some(plan) = jobs.iter().find(|j| j.name == "plan" || j.name.starts_with("plan ")) else {
        return Mode::Partial("no plan job".into());
    };
    match client.log(plan.id) {
        Ok(Some(log)) => match read_complete(&log) {
            Some(Ok(true)) => Mode::Full,
            Some(Ok(false)) => Mode::Partial("its plan job logged complete=false".into()),
            Some(Err(e)) => Mode::Unknown(e),
            None => Mode::Partial(format!("its plan job logged no \"{COMPLETE_LINE}\" line")),
        },
        Ok(None) => Mode::Unknown("the plan job has no log".into()),
        Err(e) => Mode::Unknown(format!("the plan job's log could not be read: {e}")),
    }
}

/// Reads the runs listed for `sha` (up to four pages of 50) and decides.
pub fn verdict(client: &mut Client, workflow: &str, sha: &str) -> Verdict {
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
    // A run's jobs and log are read only for a run whose mode is needed.
    let client = std::cell::RefCell::new(client);
    let mut modes = |r: &RunInfo| read_mode(&mut client.borrow_mut(), r);
    let mut earlier = |r: &RunInfo| read_earlier(&mut client.borrow_mut(), r);
    decide(&runs, workflow, sha, &mut modes, &mut earlier)
}

/// The report of a verdict: data, one line, and the exit code.
pub fn report(v: &Verdict, workflow: &str, sha: &str) -> Report {
    let head = format!("{workflow} on {}", short(sha));
    let (kind, run, text, exit) = match v {
        Verdict::Green { run } => ("green", Some(*run), format!("{head}: full green (run {run})"), Exit::Ok),
        Verdict::Red { run, reason } => ("red", *run, format!("{head}: red or partial. {reason}"), Exit::Findings),
        Verdict::NotMeasured(why) => ("not_measured", None, format!("{head}: not measured. {why}"), Exit::NotMeasured),
    };
    let data = serde_json::json!({"verdict": kind, "workflow": workflow, "sha": sha, "run": run, "detail": text});
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
        for (run, job, log) in plans {
            f.json(
                &format!("{API}/runs/{run}/jobs?limit=50&page=1"),
                json!({"total_count": 2, "jobs": [
                    {"id": job, "name": "plan (affected projects)", "conclusion": "success"},
                    {"id": job + 1000, "name": "tests", "conclusion": "success"}]}),
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
        verdict(&mut c, "ci.yml", SHA)
    }

    fn code(v: &Verdict) -> i32 {
        report(v, "ci.yml", SHA).exit.code()
    }

    #[test]
    fn a_green_push_to_main_is_full_green_without_reading_any_log() {
        let mut f = server(vec![main_push(5, "success")], &[]);
        let v = ask(&mut f);
        assert_eq!(v, Verdict::Green { run: 5 });
        assert_eq!(code(&v), 0);
        assert!(
            f.requested().iter().all(|u| !u.contains("/jobs")),
            "a main push needs no log: {:?}",
            f.requested()
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
    fn a_dispatch_run_counts_only_when_its_plan_job_logged_full_true() {
        let mut f = server(
            vec![dispatch(9, "success")],
            &[(9, 90, Some("2026-11-12T00:00:00Z plan verdict: complete=true\n"))],
        );
        assert_eq!(ask(&mut f), Verdict::Green { run: 9 });
        let mut f = server(vec![dispatch(9, "success")], &[(9, 90, Some("x\nplan verdict: complete=false\n"))]);
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
        let v = ask(&mut server(runs, &[(9, 90, Some("plan verdict: complete=false\n"))]));
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
        assert_eq!(code(&ask(&mut server(runs, &[(9, 90, Some("plan verdict: complete=maybe\n"))]))), 6);
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
        let scope_only = "plan mode: full=true\nplan verdict: complete=false\n";
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
        let v = ask(&mut server(vec![on_main], &[(7, 70, Some("plan verdict: complete=false\n"))]));
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

    #[test]
    fn the_report_names_the_run_the_workflow_and_the_next_step() {
        let r = report(&Verdict::Green { run: 5 }, "ci.yml", SHA);
        assert_eq!(r.data["verdict"], "green");
        assert_eq!(r.data["run"], 5);
        assert!(r.text.contains("ci.yml") && r.text.contains(&SHA[..10]), "{}", r.text);
        let r = report(&Verdict::NotMeasured("no run".into()), "ci.yml", SHA);
        assert_eq!(r.data["verdict"], "not_measured");
        assert_eq!(r.exit, Exit::NotMeasured);
        let r = report(
            &Verdict::Red {
                run: None,
                reason: "only partial runs".into(),
            },
            "ci.yml",
            SHA,
        );
        assert_eq!(r.data["verdict"], "red");
        assert_eq!(r.exit, Exit::Findings);
    }
}
