// ci-runs-reader: the shared Gitea client that ./dev ci verdict, triage and schedule-skip use
//! The run listing of the flake history and `ci verdict`: Gitea's run list, a run's jobs with their times (every attempt), and job
//! logs (through the planner's [`GiteaRuns`]); the run list is the part `plan_since` does not have.

use super::time::ts;
use crate::job_outcome::JobOutcome;
use crate::plan_since::GiteaRuns;
use crate::wait::Http;
use serde_json::Value;

/// One run of the list (`GET .../actions/runs`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunInfo {
    /// The run's id.
    pub id: u64,
    /// The workflow path with its ref (`dyno-release.yml@refs/heads/main`).
    pub path: String,
    /// `push`, `pull_request`, `workflow_dispatch`...
    pub event: String,
    /// `completed`, `in_progress`, `queued`...
    pub status: String,
    /// `success`, `failure`, `cancelled`, `skipped`, or empty while it runs.
    pub conclusion: String,
    /// The commit it tested.
    pub head_sha: String,
    /// The branch the run was made on (`main`); empty when the server did not say (a tag run).
    pub head_branch: String,
    /// When it was created, as the API prints it.
    pub created_at: String,
    /// Which attempt the run is on: a rerun in Gitea is another attempt of the same run, so the run's `conclusion` is the
    /// last attempt's and earlier attempts are read through [`Client::attempt_jobs`].
    pub run_attempt: u64,
    /// The run's title as the list shows it (`display_title`): the commit subject, or the workflow's `run-name` (a dispatch through
    /// `./dev ci dispatch` names its `dispatch_id` there, [`super::dispatch::DispatchTag`]).
    pub title: String,
}

impl RunInfo {
    /// The workflow file (`dyno-release.yml`).
    pub fn workflow(&self) -> &str {
        self.path.split('@').next().unwrap_or("")
    }

    /// The ref the workflow ran on (`refs/heads/main`), empty when the path has none.
    pub fn git_ref(&self) -> &str {
        self.path.split_once('@').map_or("", |(_, r)| r)
    }

    /// Whether the run is over (the API says `completed`; older servers put the conclusion in `status`).
    pub fn finished(&self) -> bool {
        self.status == "completed" || matches!(self.status.as_str(), "success" | "failure" | "cancelled" | "skipped")
    }

    fn from_json(v: &Value) -> RunInfo {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        RunInfo {
            id: v["id"].as_u64().unwrap_or(0),
            path: s("path"),
            event: s("event"),
            status: s("status"),
            conclusion: s("conclusion"),
            head_sha: s("head_sha"),
            head_branch: s("head_branch"),
            created_at: s("created_at"),
            run_attempt: v["run_attempt"].as_u64().filter(|n| *n >= 1).unwrap_or(1),
            title: s("display_title"),
        }
    }
}

/// One page of the run list, newest first.
pub struct RunsPage {
    /// The runs of the page.
    pub runs: Vec<RunInfo>,
    /// The server's count of all runs (0 when it did not say).
    pub total: u64,
}

/// A job of a run with its times (`GET .../runs/<id>/jobs`): what the timings need.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedJob {
    /// The job's id.
    pub id: u64,
    /// Its display name.
    pub name: String,
    /// The runner's id; 0 for a job that was never given a runner (skipped, or cancelled while queued).
    pub runner_id: u64,
    /// The runner's name, when the job ran on one.
    pub runner: Option<String>,
    /// How it ended (its `conclusion`, parsed once).
    pub outcome: JobOutcome,
    /// Start and end, milliseconds since the epoch.
    pub start: Option<f64>,
    /// End, milliseconds since the epoch.
    pub end: Option<f64>,
    /// Its steps: name and duration in seconds.
    pub steps: Vec<(String, Option<f64>)>,
    /// The job's page (`html_url`), where its log reads.
    pub html_url: String,
    /// The names of its steps whose conclusion is `failure` (the jobs API lists each step with `name`, `number` and `conclusion`).
    pub failed_steps: Vec<String>,
    /// `queued`, `waiting`, `in_progress`, `completed`... (empty when the server did not say).
    pub status: String,
    /// The runner labels it runs on (`macos-latest`): its `runs-on`, with a matrix value filled in.
    pub labels: Vec<String>,
}

impl TimedJob {
    /// Whether the job is the caller of a reusable workflow: no runner and no steps of its own, but a real start before its
    /// end (it spans its children, which are listed beside it). It has no log and nothing to measure of its own.
    pub fn is_caller(&self) -> bool {
        self.runner_id == 0 && self.steps.is_empty() && self.start.zip(self.end).is_some_and(|(s, e)| s < e)
    }

    /// Whether the job has no step timings: no runner took it and it is not a caller (no real times, or start equal to end),
    /// or every step started and ended in the same second (or has no time). Such a job has no log and nothing to measure.
    pub fn never_ran(&self) -> bool {
        if self.is_caller() {
            return false;
        }
        self.runner_id == 0 || self.steps.iter().all(|(_, secs)| secs.is_none_or(|s| s <= 0.0))
    }

    fn from_json(j: &Value) -> TimedJob {
        let t = |v: &Value, k: &str| ts(v[k].as_str().unwrap_or(""));
        TimedJob {
            id: j["id"].as_u64().unwrap_or(0),
            name: j["name"].as_str().unwrap_or("").to_string(),
            runner_id: j["runner_id"].as_u64().unwrap_or(0),
            runner: j["runner_name"]
                .as_str()
                .filter(|r| !r.is_empty() && *r != "undefined")
                .map(str::to_string),
            outcome: JobOutcome::from_api(j["conclusion"].as_str().unwrap_or("")),
            start: t(j, "started_at"),
            end: t(j, "completed_at"),
            steps: j["steps"]
                .as_array()
                .map(|steps| {
                    steps
                        .iter()
                        .map(|s| {
                            let secs = t(s, "started_at").zip(t(s, "completed_at")).map(|(a, b)| (b - a) / 1000.0);
                            (s["name"].as_str().unwrap_or("").to_string(), secs)
                        })
                        .collect()
                })
                .unwrap_or_default(),
            html_url: j["html_url"].as_str().unwrap_or("").to_string(),
            failed_steps: j["steps"]
                .as_array()
                .map(|steps| {
                    steps
                        .iter()
                        .filter(|s| s["conclusion"] == "failure")
                        .filter_map(|s| s["name"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            status: j["status"].as_str().unwrap_or("").to_string(),
            labels: j["labels"]
                .as_array()
                .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        }
    }
}

/// How the server says a path is missing (curl's `--fail-with-body` text).
fn is_not_found(e: &str) -> bool {
    e.contains("error: 404") || e.contains("HTTP 404")
}

/// The Gitea Actions API of one repository. The token travels in a header only (the [`Http`] contract).
pub struct Client<'a> {
    http: &'a mut dyn Http,
    server: String,
    repo: String,
    token: String,
}

impl<'a> Client<'a> {
    /// `server` like `https://dev.decryptology.net`, `repo` like `decryp7/paddock`.
    pub fn new(http: &'a mut dyn Http, server: &str, repo: &str, token: &str) -> Client<'a> {
        Client {
            http,
            server: server.to_string(),
            repo: repo.to_string(),
            token: token.to_string(),
        }
    }

    fn api(&self) -> String {
        format!("{}/api/v1/repos/{}/actions", self.server.trim_end_matches('/'), self.repo)
    }

    /// One read; a 404 is `Ok(None)`. There is no retry here: the shared HTTP layer ([`crate::wait::Retrying`]) is the one place a
    /// refusing server is held and retried (ER-22).
    fn retry<T>(&mut self, mut once: impl FnMut(&mut Client<'a>) -> Result<T, String>) -> Result<Option<T>, String> {
        match once(self) {
            Ok(v) => Ok(Some(v)),
            Err(e) if is_not_found(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// One run by id; `Ok(None)` when the server has no such run (404).
    pub fn run(&mut self, id: u64) -> Result<Option<RunInfo>, String> {
        let url = format!("{}/runs/{id}", self.api());
        Ok(self.retry(|c| c.http.get_json(&url, &c.token))?.map(|v| RunInfo::from_json(&v)))
    }

    /// The head commit of a branch (`GET .../branches/<branch>`); `Ok(None)` when the branch is gone (404) or names no commit.
    pub fn branch_head(&mut self, branch: &str) -> Result<Option<String>, String> {
        let url = format!(
            "{}/api/v1/repos/{}/branches/{}",
            self.server.trim_end_matches('/'),
            self.repo,
            encode_path(branch)
        );
        let v = self.retry(|c| c.http.get_json(&url, &c.token))?;
        Ok(v.and_then(|v| v["commit"]["id"].as_str().filter(|s| !s.is_empty()).map(str::to_string)))
    }

    /// The parents of a commit (`GET .../git/commits/<sha>`), first parent first; `Ok(None)` when the server has no such commit (404).
    pub fn commit_parents(&mut self, sha: &str) -> Result<Option<Vec<String>>, String> {
        let url = format!("{}/api/v1/repos/{}/git/commits/{sha}", self.server.trim_end_matches('/'), self.repo);
        let v = self.retry(|c| c.http.get_json(&url, &c.token))?;
        Ok(v.map(|v| {
            v["parents"]
                .as_array()
                .map(|a| a.iter().filter_map(|p| p["sha"].as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        }))
    }

    /// Page `page` of the run list (50 a page), an empty page for a missing one.
    pub fn runs_page(&mut self, page: u32) -> Result<RunsPage, String> {
        self.runs_page_where(page, "")
    }

    /// Page `page` of the run list narrowed by the API's own filters (`event=workflow_dispatch`; empty: every run).
    pub fn runs_page_where(&mut self, page: u32, query: &str) -> Result<RunsPage, String> {
        let query = if query.is_empty() { String::new() } else { format!("{query}&") };
        let url = format!("{}/runs?{query}limit=50&page={page}", self.api());
        let body = self.retry(|c| c.http.get_json(&url, &c.token))?.unwrap_or(Value::Null);
        Ok(RunsPage {
            runs: body["workflow_runs"]
                .as_array()
                .map(|a| a.iter().map(RunInfo::from_json).collect())
                .unwrap_or_default(),
            total: body["total_count"].as_u64().unwrap_or(0),
        })
    }

    /// The jobs of a run's last attempt with their times (up to four pages of 50); `None` when the server has no job list (404).
    pub fn timed_jobs(&mut self, run: u64) -> Result<Option<Vec<TimedJob>>, String> {
        self.attempt_jobs(run, None)
    }

    /// The jobs of one earlier attempt of a run (`None`: the last attempt, which `/jobs` lists). `Ok(None)` is a 404 on the
    /// first page: the server has no such list, which is not the same as a run with no jobs.
    pub fn attempt_jobs(&mut self, run: u64, attempt: Option<u64>) -> Result<Option<Vec<TimedJob>>, String> {
        let mut jobs = Vec::new();
        for page in 1..=4 {
            let url = match attempt {
                None => format!("{}/runs/{run}/jobs?limit=50&page={page}", self.api()),
                Some(n) => format!("{}/runs/{run}/attempts/{n}/jobs?limit=50&page={page}", self.api()),
            };
            let Some(body) = self.retry(|c| c.http.get_json(&url, &c.token))? else {
                if page == 1 {
                    return Ok(None);
                }
                break;
            };
            let batch: Vec<TimedJob> = body["jobs"]
                .as_array()
                .map(|a| a.iter().map(TimedJob::from_json).collect())
                .unwrap_or_default();
            let n = batch.len();
            jobs.extend(batch);
            if n < 50 {
                break;
            }
        }
        Ok(Some(jobs))
    }

    /// A job's log (read through the planner's reader); `Ok(None)` when the server has none (404).
    pub fn log(&mut self, job: u64) -> Result<Option<String>, String> {
        let (server, repo) = (self.server.clone(), self.repo.clone());
        self.retry(|c| GiteaRuns::new(c.http, &server, &repo, c.token.clone()).log(job))
    }
}

/// Percent-encodes a path whose `/` separators stay (a branch name): every other byte outside the unreserved set is `%XX`.
pub fn encode_path(p: &str) -> String {
    let mut out = String::new();
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_branch_path_is_encoded_per_segment() {
        assert_eq!(super::encode_path("train/2026-11-19_17.x"), "train/2026-11-19_17.x");
        assert_eq!(super::encode_path("train/a b#c%?"), "train/a%20b%23c%25%3F");
        assert_eq!(super::encode_path("tr\u{e4}in"), "tr%C3%A4in");
    }

    use super::*;
    use crate::ci_runs::testutil::Fake;
    use crate::wait::{Http, READ_HOLD, Retrying};
    use std::cell::RefCell;
    use std::sync::atomic::AtomicU32;
    use std::time::Duration;

    /// Fails with a 503 `fail` times (every time when `usize::MAX`), then answers a run; counts the calls.
    struct Flaky {
        fail: usize,
        calls: usize,
    }
    impl Http for Flaky {
        fn get_json(&mut self, _url: &str, _token: &str) -> Result<Value, String> {
            self.calls += 1;
            if self.calls <= self.fail {
                return Err("curl: (22) The requested URL returned error: 503".into());
            }
            Ok(serde_json::json!({"id": 9, "status": "completed", "conclusion": "success"}))
        }
    }

    impl Http for &mut Flaky {
        fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String> {
            (**self).get_json(url, token)
        }
    }

    /// The client has no retry loop of its own: a read is retried exactly by the shared layer's schedule, once per layer (ER-22).
    #[test]
    fn a_client_read_is_retried_by_the_layer_only() {
        let (good, sleeps) = (AtomicU32::new(1), RefCell::new(Vec::new()));
        let mut flaky = Flaky { fail: 2, calls: 0 };
        let run = {
            let mut layer = Retrying::new(&mut flaky, &good, Box::new(|d| sleeps.borrow_mut().push(d)), Box::new(|_| {}));
            Client::new(&mut layer, "https://gitea.invalid", "o/r", "t").run(9).unwrap()
        };
        assert!(run.is_some());
        assert_eq!(flaky.calls, 3, "two refusals and the answer: not 4 attempts per layer attempt");
        assert_eq!(*sleeps.borrow(), [2, 4].map(Duration::from_secs));

        // Never answering: the layer's whole schedule, not a client schedule on top of it.
        let (good, sleeps) = (AtomicU32::new(1), RefCell::new(Vec::new()));
        let mut down = Flaky {
            fail: usize::MAX,
            calls: 0,
        };
        let err = {
            let mut layer = Retrying::new(&mut down, &good, Box::new(|d| sleeps.borrow_mut().push(d)), Box::new(|_| {}));
            Client::new(&mut layer, "https://gitea.invalid", "o/r", "t").run(9).unwrap_err()
        };
        assert!(err.contains("503"), "{err}");
        assert_eq!(down.calls, sleeps.borrow().len() + 1, "one call per pause plus the last");
        assert!(sleeps.borrow().iter().sum::<Duration>() <= READ_HOLD);
    }

    /// The job list of run 7880 of the real server (a dyno release run), saved through the API.
    const RUN_7880: &str = include_str!("../../tests/ci-runs/run-7880-jobs.json");

    fn jobs_7880() -> Vec<TimedJob> {
        let mut f = Fake::default();
        f.json(
            "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions/runs/7880/jobs?limit=50&page=1",
            serde_json::from_str(RUN_7880).unwrap(),
        );
        let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "t");
        c.attempt_jobs(7880, None).unwrap().unwrap()
    }

    fn named(jobs: &[TimedJob], id: u64) -> &TimedJob {
        jobs.iter().find(|j| j.id == id).unwrap()
    }

    #[test]
    fn a_reusable_workflow_caller_is_not_a_job_that_never_ran() {
        let jobs = jobs_7880();
        // `installers` (34496): no runner, no steps, but a real start and end; its children are listed beside it.
        let caller = named(&jobs, 34496);
        assert!(
            caller.outcome == JobOutcome::Failed && caller.is_caller() && !caller.never_ran(),
            "{caller:?}"
        );
        // A job the workflow skipped has no runner and epoch times: it is skipped, not failed.
        assert_eq!(named(&jobs, 34497).outcome, JobOutcome::Skipped);
        // A failed job that ran steps is neither.
        let ran = named(&jobs, 34520);
        assert!(ran.outcome == JobOutcome::Failed && !ran.is_caller() && !ran.never_ran());
    }

    #[test]
    fn no_runner_and_no_real_times_is_a_job_that_never_ran() {
        let mut j = named(&jobs_7880(), 34496).clone();
        j.start = None;
        j.end = None;
        assert!(!j.is_caller() && j.never_ran());
        let mut j = named(&jobs_7880(), 34496).clone();
        j.end = j.start;
        assert!(!j.is_caller() && j.never_ran());
    }
}
