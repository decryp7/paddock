//! The run listing of `ci flakes` and `ci timings`: Gitea's run list, a run's jobs with their times (every attempt), and job
//! logs (through the planner's [`GiteaRuns`]); the run list is the part `plan_since` does not have.

use super::time::ts;
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
    /// `success`, `failure`...
    pub conclusion: String,
    /// Start and end, milliseconds since the epoch.
    pub start: Option<f64>,
    /// End, milliseconds since the epoch.
    pub end: Option<f64>,
    /// Its steps: name and duration in seconds.
    pub steps: Vec<(String, Option<f64>)>,
}

impl TimedJob {
    /// Whether the job did not pass (a failure, a cancel, a job that never finished).
    pub fn failed(&self) -> bool {
        !matches!(self.conclusion.as_str(), "success" | "skipped")
    }

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
            conclusion: j["conclusion"].as_str().unwrap_or("").to_string(),
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
    /// Waits between retries; tests set it to nothing.
    pub backoff_ms: u64,
}

impl<'a> Client<'a> {
    /// `server` like `https://dev.decryptology.net`, `repo` like `decryp7/paddock`.
    pub fn new(http: &'a mut dyn Http, server: &str, repo: &str, token: &str) -> Client<'a> {
        Client {
            http,
            server: server.to_string(),
            repo: repo.to_string(),
            token: token.to_string(),
            backoff_ms: 500,
        }
    }

    fn api(&self) -> String {
        format!("{}/api/v1/repos/{}/actions", self.server.trim_end_matches('/'), self.repo)
    }

    fn retry<T>(&mut self, mut once: impl FnMut(&mut Client<'a>) -> Result<T, String>) -> Result<Option<T>, String> {
        for attempt in 0..4u64 {
            match once(self) {
                Ok(v) => return Ok(Some(v)),
                Err(e) if is_not_found(&e) => return Ok(None),
                Err(e) if attempt >= 3 => return Err(e),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(self.backoff_ms * (attempt + 1))),
            }
        }
        Ok(None)
    }

    /// One run by id; `Ok(None)` when the server has no such run (404).
    pub fn run(&mut self, id: u64) -> Result<Option<RunInfo>, String> {
        let url = format!("{}/runs/{id}", self.api());
        Ok(self.retry(|c| c.http.get_json(&url, &c.token))?.map(|v| RunInfo::from_json(&v)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;

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
        assert!(caller.failed() && caller.is_caller() && !caller.never_ran(), "{caller:?}");
        // A job the workflow skipped has no runner and epoch times: it is not failed at all.
        assert!(!named(&jobs, 34497).failed());
        // A failed job that ran steps is neither.
        let ran = named(&jobs, 34520);
        assert!(ran.failed() && !ran.is_caller() && !ran.never_ran());
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
