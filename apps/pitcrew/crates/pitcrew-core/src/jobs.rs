//! Jobs: what a runner is working on, from its log ("task started" lines) and its child processes.
//!
//! Gitea's runner logs `msg="task started"` with the job, repository, run and task numbers when it takes a job, but
//! nothing when the job ends (checked against `gitea-runner` 5.2.0 on 2026-10-22). A job therefore counts as running
//! while the runner has child processes (the job's shells, tools and actions) and for [`IDLE_CONFIRM`] after the
//! last time it had any: in host mode the runner starts a fresh shell per step, so a short gap between two steps must
//! not look like the end of the job (an update or a drain would then stop a busy runner).

use crate::logline;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long a runner must have had no job processes before its job counts as finished.
pub const IDLE_CONFIRM: Duration = Duration::from_secs(20);
/// Longest text kept from one field of a "task started" line.
pub const MAX_FIELD_LEN: usize = 256;

/// What the runner said about a job when it took it. Every field is optional: older runners log fewer fields, and
/// the runner masks secrets in its own log, so the repository can contain `***` (e.g. when the owner's name is also
/// a secret's value).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    /// `owner/repo`, possibly masked.
    pub repository: Option<String>,
    /// The job's name in the workflow.
    pub job: Option<String>,
    /// The server's id of the job.
    pub job_id: Option<u64>,
    /// The server's id of the workflow run.
    pub run_id: Option<u64>,
    /// The run's number in its repository (what the web UI shows and links to).
    pub run_number: Option<u64>,
    /// The runner task's id.
    pub task_id: Option<u64>,
    /// The server's address, e.g. `https://git.example.com`.
    pub server: Option<String>,
}

impl JobInfo {
    /// The job of a "task started" line (fields it lacks stay `None`).
    pub fn from_task_started(line: &str) -> JobInfo {
        let mut info = JobInfo::default();
        for (key, value) in logline::fields(line) {
            let text = || Some(value.chars().take(MAX_FIELD_LEN).collect::<String>()).filter(|v: &String| !v.is_empty());
            let number = || value.parse::<u64>().ok();
            match key.as_str() {
                "repository" => info.repository = text(),
                "job" => info.job = text(),
                "job_id" => info.job_id = number(),
                "run_id" => info.run_id = number(),
                "run_number" => info.run_number = number(),
                "task_id" => info.task_id = number(),
                "address" => info.server = text().map(|s| s.trim_end_matches('/').to_string()),
                _ => {}
            }
        }
        info
    }

    /// The repository when the runner did not mask any part of it and it looks like `owner/repo`.
    pub fn known_repository(&self) -> Option<&str> {
        self.repository.as_deref().filter(|r| {
            let mut parts = r.split('/');
            let ok =
                |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)));
            ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
        })
    }

    /// The run's page on the server (`<server>/<owner>/<repo>/actions/runs/<run number>`), when everything needed is
    /// known and the server is HTTPS (or plain HTTP, which some internal servers use; the link only opens a browser).
    pub fn run_url(&self) -> Option<String> {
        let server = self
            .server
            .as_deref()
            .filter(|s| s.starts_with("https://") || s.starts_with("http://"))?;
        let repo = self.known_repository()?;
        let run = self.run_number?;
        Some(format!("{server}/{repo}/actions/runs/{run}"))
    }
}

/// The job running now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentJob {
    /// What the runner said about it (empty when Pitcrew only saw its processes).
    pub info: JobInfo,
    /// When Pitcrew saw it start.
    pub started_at: Timestamp,
}

/// A job that ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedJob {
    /// What the runner said about it.
    pub info: JobInfo,
    /// When it started.
    pub started_at: Timestamp,
    /// When it ended (when the runner was last seen working on it).
    pub finished_at: Timestamp,
}

/// Whether a child process of the runner belongs to a job. The console host Windows attaches to every console
/// program (the runner runs in a hidden console of its own) is not a job.
pub fn is_job_process(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !matches!(n.as_str(), "conhost.exe" | "conhost" | "openconsole.exe")
}

/// Follows one runner's jobs.
#[derive(Debug, Clone, Default)]
pub struct JobTracker {
    current: Option<CurrentJob>,
    children: bool,
    last_activity: Option<Timestamp>,
}

impl JobTracker {
    /// A tracker that has seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The runner logged "task started". With one job at a time, a new task also ends the previous one.
    pub fn task_started(&mut self, info: JobInfo, now: Timestamp) -> Option<FinishedJob> {
        let previous = self.take_finished(now);
        self.current = Some(CurrentJob { info, started_at: now });
        self.last_activity = Some(now);
        previous
    }

    /// The result of a look at the runner's child processes (only job processes, see [`is_job_process`]).
    pub fn observe_children(&mut self, present: bool, now: Timestamp) -> Option<FinishedJob> {
        self.children = present;
        if present {
            self.last_activity = Some(now);
            if self.current.is_none() {
                self.current = Some(CurrentJob {
                    info: JobInfo::default(),
                    started_at: now,
                });
            }
            return None;
        }
        match self.last_activity {
            Some(t) if now.since(t) >= IDLE_CONFIRM => self.take_finished(now),
            _ => None,
        }
    }

    /// The runner process ended: so did its job.
    pub fn process_exited(&mut self, now: Timestamp) -> Option<FinishedJob> {
        self.children = false;
        let done = self.take_finished(now);
        self.last_activity = None;
        done
    }

    /// Whether a job runs (or ran less than [`IDLE_CONFIRM`] ago).
    pub fn running(&self, now: Timestamp) -> bool {
        self.children || self.last_activity.is_some_and(|t| now.since(t) < IDLE_CONFIRM)
    }

    /// The job running now, when known.
    pub fn current(&self) -> Option<&CurrentJob> {
        self.current.as_ref()
    }

    fn take_finished(&mut self, now: Timestamp) -> Option<FinishedJob> {
        let job = self.current.take()?;
        let finished_at = self.last_activity.unwrap_or(now).max(job.started_at);
        Some(FinishedJob {
            info: job.info,
            started_at: job.started_at,
            finished_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"time="2026-10-22T13:36:25+08:00" level=info msg="task started" address="https://git.example.com/" job=tauri-smoke job_id=8288 repository=owner/app run_id=5983 run_number=215 task_id=8271"#;

    fn t(s: u64) -> Timestamp {
        Timestamp(s * 1000)
    }

    #[test]
    fn reads_a_task_started_line() {
        let j = JobInfo::from_task_started(LINE);
        assert_eq!(j.repository.as_deref(), Some("owner/app"));
        assert_eq!(j.job.as_deref(), Some("tauri-smoke"));
        assert_eq!(
            (j.job_id, j.run_id, j.run_number, j.task_id),
            (Some(8288), Some(5983), Some(215), Some(8271))
        );
        assert_eq!(j.server.as_deref(), Some("https://git.example.com"));
        assert_eq!(j.run_url().as_deref(), Some("https://git.example.com/owner/app/actions/runs/215"));
        let bare = JobInfo::from_task_started(r#"msg="task started" job= run_id=x"#);
        assert_eq!(bare, JobInfo::default());
        let long = JobInfo::from_task_started(&format!("job={}", "j".repeat(1000)));
        assert_eq!(long.job.map(|j| j.len()), Some(MAX_FIELD_LEN));
    }

    #[test]
    fn masked_or_odd_repositories_give_no_link() {
        let mut j = JobInfo::from_task_started(LINE);
        for repo in ["***/app", "owner", "a/b/c", "/app", "own er/app"] {
            j.repository = Some(repo.into());
            assert_eq!(j.known_repository(), None, "{repo}");
            assert_eq!(j.run_url(), None);
        }
        j.repository = Some("owner/app".into());
        j.server = Some("ftp://x".into());
        assert_eq!(j.run_url(), None);
        j.server = Some("http://intranet".into());
        j.run_number = None;
        assert_eq!(j.run_url(), None);
    }

    #[test]
    fn console_hosts_are_not_jobs() {
        assert!(!is_job_process("conhost.exe"));
        assert!(!is_job_process("OpenConsole.exe"));
        assert!(is_job_process("bash.exe"));
        assert!(is_job_process("node"));
    }

    #[test]
    fn a_job_ends_after_its_processes_are_gone_for_a_while() {
        let mut tr = JobTracker::new();
        assert!(!tr.running(t(0)));
        assert_eq!(tr.task_started(JobInfo::from_task_started(LINE), t(10)), None);
        assert!(tr.running(t(10)));
        assert_eq!(tr.observe_children(true, t(15)), None);
        // A gap between two steps.
        assert_eq!(tr.observe_children(false, t(20)), None);
        assert!(tr.running(t(20)));
        assert_eq!(tr.observe_children(true, t(25)), None);
        assert_eq!(tr.observe_children(false, t(30)), None);
        let done = tr.observe_children(false, t(46)).unwrap();
        assert_eq!((done.started_at, done.finished_at), (t(10), t(25)));
        assert_eq!(done.info.job.as_deref(), Some("tauri-smoke"));
        assert!(!tr.running(t(46)));
        assert!(tr.current().is_none());
        assert_eq!(tr.observe_children(false, t(100)), None);
    }

    #[test]
    fn processes_without_a_logged_task_are_a_job_too() {
        let mut tr = JobTracker::new();
        tr.observe_children(true, t(5));
        assert_eq!(tr.current().map(|c| c.started_at), Some(t(5)));
        assert_eq!(tr.current().map(|c| c.info.clone()), Some(JobInfo::default()));
        let done = tr.process_exited(t(9)).unwrap();
        assert_eq!(done.finished_at, t(5));
        assert!(!tr.running(t(9)));
        assert_eq!(tr.process_exited(t(10)), None);
    }

    #[test]
    fn a_new_task_ends_the_previous_one() {
        let mut tr = JobTracker::new();
        tr.task_started(JobInfo::default(), t(1));
        let prev = tr.task_started(JobInfo::from_task_started(LINE), t(50)).unwrap();
        assert_eq!(prev.started_at, t(1));
        assert_eq!(tr.current().map(|c| c.started_at), Some(t(50)));
    }
}
