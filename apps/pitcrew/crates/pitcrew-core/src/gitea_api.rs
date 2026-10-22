//! The bits of the Gitea API Pitcrew reads to describe the current job (with a read-only token the user enters):
//! the workflow run's title, workflow file, branch and page. Forgejo serves the same API.
//!
//! `GET /api/v1/repos/{owner}/{repo}/actions/runs/{run_id}` (Gitea 1.24+, Forgejo 11+). Older servers answer 404;
//! the window then shows what the runner's log said.

use crate::history::{local_day_start, DAY_MS, MAX_DAYS};
use crate::jobs::{FinishedJob, JobInfo, MAX_FIELD_LEN};
use crate::time::{parse_rfc3339, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Largest API response Pitcrew parses.
pub const MAX_API_BODY: usize = 256 * 1024;

/// The API URL of the job's workflow run, when its repository is known (not masked) and the server is HTTP(S).
pub fn run_api_url(job: &JobInfo) -> Option<String> {
    let server = job
        .server
        .as_deref()
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"))?;
    let repo = job.known_repository()?;
    let run = job.run_id?;
    Some(format!("{server}/api/v1/repos/{repo}/actions/runs/{run}"))
}

/// What the API says about a workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetails {
    /// The run's title (usually the commit message's first line).
    pub title: Option<String>,
    /// The workflow file, e.g. `build.yml`.
    pub workflow: Option<String>,
    /// The branch.
    pub branch: Option<String>,
    /// What started it (`push`, `pull_request`, `schedule`, …).
    pub event: Option<String>,
    /// The run's page.
    pub url: Option<String>,
}

/// Parses a run response.
pub fn parse_run(body: &str) -> Result<RunDetails, String> {
    if body.len() > MAX_API_BODY {
        return Err("the server's answer is too large".into());
    }
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("the server's answer is not JSON: {e}"))?;
    if !v.is_object() {
        return Err("the server's answer is not a workflow run".into());
    }
    let text = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(|s| s.chars().take(MAX_FIELD_LEN).collect::<String>())
            .filter(|s| !s.is_empty())
    };
    let workflow = text("path").map(|p| {
        let file = p.split('@').next().unwrap_or(&p);
        file.rsplit('/').next().unwrap_or(file).to_string()
    });
    Ok(RunDetails {
        title: text("display_title").or_else(|| text("title")),
        workflow,
        branch: text("head_branch"),
        event: text("event"),
        url: text("html_url").filter(|u| u.starts_with("https://") || u.starts_with("http://")),
    })
}

/// The API URL of a job (`/actions/jobs/{job_id}`), when its repository is known.
pub fn job_api_url(job: &JobInfo) -> Option<String> {
    let server = job
        .server
        .as_deref()
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"))?;
    let repo = job.known_repository()?;
    let id = job.job_id?;
    Some(format!("{server}/api/v1/repos/{repo}/actions/jobs/{id}"))
}

/// How a job ended, per the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Conclusion {
    /// Succeeded.
    Success,
    /// Failed.
    Failure,
    /// Cancelled.
    Cancelled,
    /// Skipped.
    Skipped,
    /// Not finished, or not known.
    Unknown,
}

/// What the API says about a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDetails {
    /// How it ended.
    pub conclusion: Conclusion,
    /// How long it waited for a runner, in milliseconds (`started_at - created_at`).
    pub queued_ms: Option<u64>,
}

/// Parses a job response.
pub fn parse_job(body: &str) -> Result<JobDetails, String> {
    if body.len() > MAX_API_BODY {
        return Err("the server's answer is too large".into());
    }
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("the server's answer is not JSON: {e}"))?;
    if !v.is_object() {
        return Err("the server's answer is not a job".into());
    }
    let time = |k: &str| v.get(k).and_then(|x| x.as_str()).and_then(parse_rfc3339);
    let conclusion = match v.get("conclusion").and_then(|c| c.as_str()).unwrap_or("") {
        "success" => Conclusion::Success,
        "failure" => Conclusion::Failure,
        "cancelled" => Conclusion::Cancelled,
        "skipped" => Conclusion::Skipped,
        _ => Conclusion::Unknown,
    };
    let queued_ms = match (time("created_at"), time("started_at")) {
        (Some(c), Some(s)) if s >= c => Some(s.0 - c.0),
        _ => None,
    };
    Ok(JobDetails { conclusion, queued_ms })
}

/// Jobs per local day by outcome, with the average queue wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDay {
    /// Local midnight.
    pub start: Timestamp,
    /// Succeeded.
    pub success: u32,
    /// Failed.
    pub failure: u32,
    /// Cancelled or skipped.
    pub cancelled: u32,
    /// Outcome not known (no token, masked repository, older server).
    pub unknown: u32,
    /// Average wait for a runner of the jobs whose wait is known, in milliseconds.
    pub avg_queued_ms: Option<u64>,
}

/// Groups `jobs` by the local day they ended on, over the last `days` days, with the outcomes known by job id.
pub fn job_days(
    jobs: &[FinishedJob],
    details: &HashMap<u64, JobDetails>,
    now: Timestamp,
    days: u32,
    utc_offset_minutes: i32,
) -> Vec<JobDay> {
    let days = days.clamp(1, MAX_DAYS);
    let first = Timestamp(
        local_day_start(now, utc_offset_minutes)
            .0
            .saturating_sub(u64::from(days - 1) * DAY_MS),
    );
    let mut out: Vec<(JobDay, u64, u64)> = (0..days)
        .map(|d| {
            let day = JobDay {
                start: Timestamp(first.0 + u64::from(d) * DAY_MS),
                success: 0,
                failure: 0,
                cancelled: 0,
                unknown: 0,
                avg_queued_ms: None,
            };
            (day, 0, 0)
        })
        .collect();
    for j in jobs {
        if j.finished_at < first || j.finished_at > now {
            continue;
        }
        let index = usize::try_from((j.finished_at.0 - first.0) / DAY_MS).unwrap_or(usize::MAX);
        let Some(slot) = out.get_mut(index) else { continue };
        let d = j.info.job_id.and_then(|id| details.get(&id));
        match d.map(|d| d.conclusion) {
            Some(Conclusion::Success) => slot.0.success += 1,
            Some(Conclusion::Failure) => slot.0.failure += 1,
            Some(Conclusion::Cancelled | Conclusion::Skipped) => slot.0.cancelled += 1,
            _ => slot.0.unknown += 1,
        }
        if let Some(q) = d.and_then(|d| d.queued_ms) {
            slot.1 += q;
            slot.2 += 1;
        }
    }
    out.into_iter()
        .map(|(mut day, sum, n)| {
            day.avg_queued_ms = (n > 0).then(|| sum / n);
            day
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jobs_and_groups_them_by_day() {
        let mut j = job();
        j.job_id = Some(8288);
        assert_eq!(
            job_api_url(&j).as_deref(),
            Some("https://git.example.com/api/v1/repos/owner/app/actions/jobs/8288")
        );
        assert_eq!(job_api_url(&job()), None);
        let d = parse_job(r#"{"conclusion":"failure","created_at":"2026-10-22T05:00:00Z","started_at":"2026-10-22T05:01:30Z"}"#).unwrap();
        assert_eq!(
            d,
            JobDetails {
                conclusion: Conclusion::Failure,
                queued_ms: Some(90_000)
            }
        );
        for (c, want) in [
            ("success", Conclusion::Success),
            ("cancelled", Conclusion::Cancelled),
            ("skipped", Conclusion::Skipped),
            ("", Conclusion::Unknown),
        ] {
            assert_eq!(parse_job(&format!(r#"{{"conclusion":"{c}"}}"#)).unwrap().conclusion, want);
        }
        let backwards = r#"{"created_at":"2026-10-22T05:02:00Z","started_at":"2026-10-22T05:01:30Z"}"#;
        assert_eq!(parse_job(backwards).unwrap().queued_ms, None);
        assert!(parse_job("1").is_err() && parse_job("x").is_err() && parse_job(&" ".repeat(MAX_API_BODY + 1)).is_err());

        let now = Timestamp(100 * DAY_MS + 3_600_000);
        let fin = |id: Option<u64>, at: u64| FinishedJob {
            info: JobInfo {
                job_id: id,
                ..JobInfo::default()
            },
            started_at: Timestamp(at - 1000),
            finished_at: Timestamp(at),
        };
        let jobs = vec![
            fin(Some(1), 100 * DAY_MS + 10),
            fin(Some(2), 99 * DAY_MS + 10),
            fin(None, 100 * DAY_MS + 20),
            fin(Some(3), 100 * DAY_MS + 30),
            fin(Some(9), DAY_MS),
            fin(Some(4), 100 * DAY_MS + 40),
        ];
        let mut details = HashMap::new();
        let det = |conclusion, queued_ms| JobDetails { conclusion, queued_ms };
        details.insert(1, det(Conclusion::Success, Some(1000)));
        details.insert(2, det(Conclusion::Failure, None));
        details.insert(3, det(Conclusion::Cancelled, Some(3000)));
        let days = job_days(&jobs, &details, now, 2, 0);
        assert_eq!(days.len(), 2);
        assert_eq!((days[0].failure, days[0].avg_queued_ms), (1, None));
        assert_eq!((days[1].success, days[1].cancelled, days[1].unknown), (1, 1, 2));
        assert_eq!(days[1].avg_queued_ms, Some(2000));
    }

    fn job() -> JobInfo {
        JobInfo {
            repository: Some("owner/app".into()),
            run_id: Some(5983),
            server: Some("https://git.example.com".into()),
            ..JobInfo::default()
        }
    }

    #[test]
    fn builds_the_run_url_only_for_known_repositories() {
        assert_eq!(
            run_api_url(&job()).as_deref(),
            Some("https://git.example.com/api/v1/repos/owner/app/actions/runs/5983")
        );
        let mut masked = job();
        masked.repository = Some("***/app".into());
        assert_eq!(run_api_url(&masked), None);
        let mut ftp = job();
        ftp.server = Some("ftp://x".into());
        assert_eq!(run_api_url(&ftp), None);
    }

    #[test]
    fn parses_a_run() {
        let body = r#"{"id":5983,"display_title":"fix: the thing","path":".gitea/workflows/build.yml@refs/heads/main","head_branch":"main","event":"push","html_url":"https://git.example.com/owner/app/actions/runs/215"}"#;
        let r = parse_run(body).unwrap();
        assert_eq!(r.title.as_deref(), Some("fix: the thing"));
        assert_eq!(r.workflow.as_deref(), Some("build.yml"));
        assert_eq!(r.branch.as_deref(), Some("main"));
        assert_eq!(r.event.as_deref(), Some("push"));
        assert_eq!(r.url.as_deref(), Some("https://git.example.com/owner/app/actions/runs/215"));
        let bad_link = parse_run(r#"{"html_url":"javascript:alert(1)","title":"t"}"#).unwrap();
        assert_eq!(bad_link.url, None);
        assert_eq!(bad_link.title.as_deref(), Some("t"));
        assert!(parse_run("[]").is_err());
        assert!(parse_run("nope").is_err());
        assert!(parse_run(&" ".repeat(MAX_API_BODY + 1)).is_err());
    }
}
