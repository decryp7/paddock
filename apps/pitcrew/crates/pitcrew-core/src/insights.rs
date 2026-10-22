//! Workflow insights (ADR 0015): the slowest and the flaky jobs, how long jobs wait for a runner, the success rate per
//! repository over time, and what to do about it.
//!
//! Pure aggregation over the jobs Pitcrew has seen finish (the history) joined with what the Gitea API says about them
//! (outcome and queue time; the app reads the API with the owner's read token, cached and bounded, see
//! `gitea_api`). Nothing here talks to a server. Jobs whose outcome is unknown count towards durations, runner load and
//! the totals but not towards success rates or flakiness.

use crate::gitea_api::Conclusion;
use crate::history::{local_day_start, DAY_MS, MAX_DAYS};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most rows in each ranking.
pub const MAX_ROWS: usize = 10;
/// Most repositories reported.
pub const MAX_REPOS: usize = 30;
/// Fewest runs with a known outcome before a job can be called flaky.
pub const FLAKY_MIN_RUNS: u32 = 4;
/// Queue wait that is worth a word: the average, in milliseconds.
pub const QUEUE_NOTICE_MS: u64 = 60_000;
/// Queue wait that is a problem: the 95th percentile, in milliseconds.
pub const QUEUE_PROBLEM_P95_MS: u64 = 5 * 60_000;
/// A runner this busy (per mille of the window) is at its limit.
pub const BUSY_LIMIT_PERMILLE: u32 = 800;
/// Runners all quieter than this have capacity to spare.
pub const IDLE_PERMILLE: u32 = 200;

/// One finished job with what the server told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightJob {
    /// The runner that ran it.
    pub runner: String,
    /// `owner/repo` (masked repositories are left out by the caller).
    pub repo: String,
    /// The job's name.
    pub job: String,
    /// When it started.
    pub started: Timestamp,
    /// When it ended.
    pub finished: Timestamp,
    /// How it ended, per the server (`Unknown` when not known).
    pub conclusion: Conclusion,
    /// How long it waited for a runner.
    pub queued_ms: Option<u64>,
}

/// One day of a repository's results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoDay {
    /// Local midnight.
    pub start: Timestamp,
    /// Succeeded.
    pub success: u32,
    /// Failed.
    pub failure: u32,
}

/// A repository over the range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInsight {
    /// `owner/repo`.
    pub repo: String,
    /// Jobs seen.
    pub jobs: u32,
    /// Succeeded.
    pub success: u32,
    /// Failed.
    pub failure: u32,
    /// Success share of the jobs that succeeded or failed, per mille (`None`: none known).
    pub success_permille: Option<u32>,
    /// Success share in the first half of the range, per mille.
    pub earlier_permille: Option<u32>,
    /// Success share in the second half of the range, per mille.
    pub later_permille: Option<u32>,
    /// Average wait for a runner, milliseconds.
    pub avg_queued_ms: Option<u64>,
    /// Results per day, oldest first.
    pub days: Vec<RepoDay>,
}

/// A job (by repository and name) ranked by its duration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStat {
    /// `owner/repo`.
    pub repo: String,
    /// The job's name.
    pub job: String,
    /// Runs.
    pub runs: u32,
    /// Average duration, milliseconds.
    pub avg_ms: u64,
    /// 95th percentile duration, milliseconds.
    pub p95_ms: u64,
    /// Longest duration, milliseconds.
    pub max_ms: u64,
}

/// A job that passes and fails without a change in between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlakyJob {
    /// `owner/repo`.
    pub repo: String,
    /// The job's name.
    pub job: String,
    /// Runs with a known outcome.
    pub runs: u32,
    /// Failures among them.
    pub failures: u32,
    /// How often the outcome switched between consecutive runs.
    pub flips: u32,
}

/// How long jobs wait for a runner.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStats {
    /// Jobs whose wait is known.
    pub samples: u32,
    /// Average wait, milliseconds.
    pub avg_ms: Option<u64>,
    /// 95th percentile, milliseconds.
    pub p95_ms: Option<u64>,
    /// Longest wait, milliseconds.
    pub max_ms: Option<u64>,
}

/// How busy a runner was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerLoad {
    /// The runner.
    pub runner: String,
    /// Jobs.
    pub jobs: u32,
    /// The share of the range with a job running, per mille.
    pub busy_permille: u32,
}

/// What kind of advice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdviceKind {
    /// Add a runner or raise capacity.
    AddCapacity,
    /// Jobs wait although runners are free: look at labels and capacity settings.
    CheckLabels,
    /// There is capacity to spare.
    SpareCapacity,
    /// A repository's success rate dropped.
    SuccessDropped,
}

/// One piece of advice, in words, derived from the numbers above.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    /// What kind.
    pub kind: AdviceKind,
    /// A short headline.
    pub title: String,
    /// The reasoning with the numbers and the next step.
    pub detail: String,
}

/// Everything the Insights page shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    /// The range in days.
    pub range_days: u32,
    /// Jobs in the range.
    pub jobs: u32,
    /// Jobs whose outcome the server told.
    pub jobs_with_outcome: u32,
    /// Repositories, most jobs first.
    pub repos: Vec<RepoInsight>,
    /// The slowest jobs by average duration.
    pub slowest: Vec<JobStat>,
    /// The flakiest jobs.
    pub flaky: Vec<FlakyJob>,
    /// Waiting for a runner.
    pub queue: QueueStats,
    /// The runners' load.
    pub runners: Vec<RunnerLoad>,
    /// What to do about it.
    pub advice: Vec<Advice>,
}

/// The insight jobs of one runner's finished jobs, with the server's details by job id where they are known. Jobs of a
/// masked or unknown repository are left out (nothing to group them by).
pub fn from_finished(
    runner: &str,
    jobs: &[crate::jobs::FinishedJob],
    details: &std::collections::HashMap<u64, crate::gitea_api::JobDetails>,
) -> Vec<InsightJob> {
    jobs.iter()
        .filter_map(|j| {
            let repo = j.info.known_repository()?.to_string();
            let d = j.info.job_id.and_then(|id| details.get(&id));
            Some(InsightJob {
                runner: runner.to_string(),
                repo,
                job: j.info.job.clone().unwrap_or_else(|| "(unnamed job)".to_string()),
                started: j.started_at,
                finished: j.finished_at,
                conclusion: d.map(|d| d.conclusion).or(j.outcome).unwrap_or(Conclusion::Unknown),
                queued_ms: d.and_then(|d| d.queued_ms),
            })
        })
        .collect()
}

fn percentile(sorted: &[u64], p: u32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (sorted.len() * p as usize).div_ceil(100).clamp(1, sorted.len()) - 1;
    sorted[idx]
}

fn avg(v: &[u64]) -> u64 {
    if v.is_empty() {
        0
    } else {
        v.iter().sum::<u64>() / v.len() as u64
    }
}

fn permille(part: u32, whole: u32) -> Option<u32> {
    (whole > 0).then(|| (u64::from(part) * 1000 / u64::from(whole)) as u32)
}

/// Length of the union of `intervals` (start, end) in milliseconds.
fn union_ms(mut intervals: Vec<(u64, u64)>) -> u64 {
    intervals.sort_unstable();
    let mut total = 0;
    let mut cur: Option<(u64, u64)> = None;
    for (s, e) in intervals {
        match cur {
            Some((cs, ce)) if s <= ce => cur = Some((cs, ce.max(e))),
            Some((cs, ce)) => {
                total += ce - cs;
                cur = Some((s, e));
            }
            None => cur = Some((s, e)),
        }
    }
    if let Some((cs, ce)) = cur {
        total += ce - cs;
    }
    total
}

/// The insights over the last `range_days` local days ending at `now`.
pub fn compute(jobs: &[InsightJob], now: Timestamp, range_days: u32, utc_offset_minutes: i32) -> Insights {
    let range_days = range_days.clamp(1, MAX_DAYS);
    let first_day = local_day_start(now, utc_offset_minutes)
        .0
        .saturating_sub(u64::from(range_days - 1) * DAY_MS);
    let from = Timestamp(first_day);
    let window_ms = now.0.saturating_sub(first_day).max(1);
    let mut in_range: Vec<&InsightJob> = jobs.iter().filter(|j| j.finished >= from && j.finished <= now).collect();
    in_range.sort_by_key(|j| j.finished);

    let known = |c: Conclusion| matches!(c, Conclusion::Success | Conclusion::Failure);
    let mut out = Insights {
        range_days,
        jobs: u32::try_from(in_range.len()).unwrap_or(u32::MAX),
        jobs_with_outcome: u32::try_from(in_range.iter().filter(|j| known(j.conclusion)).count()).unwrap_or(u32::MAX),
        ..Insights::default()
    };

    // Repositories.
    let mid = first_day + window_ms / 2;
    let mut repos: BTreeMap<&str, Vec<&InsightJob>> = BTreeMap::new();
    for j in &in_range {
        repos.entry(j.repo.as_str()).or_default().push(j);
    }
    let mut repo_rows: Vec<RepoInsight> = repos
        .iter()
        .map(|(repo, js)| {
            let count =
                |pred: &dyn Fn(&&&InsightJob) -> bool, c: Conclusion| js.iter().filter(pred).filter(|j| j.conclusion == c).count() as u32;
            let all = |_: &&&InsightJob| true;
            let (s, f) = (count(&all, Conclusion::Success), count(&all, Conclusion::Failure));
            let early = |j: &&&InsightJob| j.finished.0 < mid;
            let late = |j: &&&InsightJob| j.finished.0 >= mid;
            let (es, ef) = (count(&early, Conclusion::Success), count(&early, Conclusion::Failure));
            let (ls, lf) = (count(&late, Conclusion::Success), count(&late, Conclusion::Failure));
            let q: Vec<u64> = js.iter().filter_map(|j| j.queued_ms).collect();
            let days = (0..range_days)
                .map(|d| {
                    let start = first_day + u64::from(d) * DAY_MS;
                    let in_day = |j: &&&InsightJob| j.finished.0 >= start && j.finished.0 < start + DAY_MS;
                    RepoDay {
                        start: Timestamp(start),
                        success: count(&in_day, Conclusion::Success),
                        failure: count(&in_day, Conclusion::Failure),
                    }
                })
                .collect();
            RepoInsight {
                repo: (*repo).to_string(),
                jobs: js.len() as u32,
                success: s,
                failure: f,
                success_permille: permille(s, s + f),
                earlier_permille: permille(es, es + ef),
                later_permille: permille(ls, ls + lf),
                avg_queued_ms: (!q.is_empty()).then(|| avg(&q)),
                days,
            }
        })
        .collect();
    repo_rows.sort_by(|a, b| b.jobs.cmp(&a.jobs).then_with(|| a.repo.cmp(&b.repo)));
    repo_rows.truncate(MAX_REPOS);
    out.repos = repo_rows;

    // Slowest and flaky jobs, by (repository, job name).
    let mut by_job: BTreeMap<(&str, &str), Vec<&InsightJob>> = BTreeMap::new();
    for j in &in_range {
        by_job.entry((j.repo.as_str(), j.job.as_str())).or_default().push(j);
    }
    let mut slowest: Vec<JobStat> = by_job
        .iter()
        .map(|((repo, job), js)| {
            let mut d: Vec<u64> = js.iter().map(|j| j.finished.0.saturating_sub(j.started.0)).collect();
            d.sort_unstable();
            JobStat {
                repo: (*repo).to_string(),
                job: (*job).to_string(),
                runs: js.len() as u32,
                avg_ms: avg(&d),
                p95_ms: percentile(&d, 95),
                max_ms: d.last().copied().unwrap_or(0),
            }
        })
        .collect();
    slowest.sort_by(|a, b| b.avg_ms.cmp(&a.avg_ms).then_with(|| (&a.repo, &a.job).cmp(&(&b.repo, &b.job))));
    slowest.truncate(MAX_ROWS);
    out.slowest = slowest;

    let mut flaky: Vec<FlakyJob> = by_job
        .iter()
        .filter_map(|((repo, job), js)| {
            let seq: Vec<bool> = js
                .iter()
                .filter(|j| known(j.conclusion))
                .map(|j| j.conclusion == Conclusion::Success)
                .collect();
            let runs = seq.len() as u32;
            let failures = seq.iter().filter(|ok| !**ok).count() as u32;
            let flips = seq.windows(2).filter(|w| w[0] != w[1]).count() as u32;
            let rate = permille(failures, runs)?;
            (runs >= FLAKY_MIN_RUNS && failures > 0 && failures < runs && flips >= 2 && (100..=900).contains(&rate)).then(|| FlakyJob {
                repo: (*repo).to_string(),
                job: (*job).to_string(),
                runs,
                failures,
                flips,
            })
        })
        .collect();
    flaky.sort_by(|a, b| {
        b.flips
            .cmp(&a.flips)
            .then_with(|| b.failures.cmp(&a.failures))
            .then_with(|| (&a.repo, &a.job).cmp(&(&b.repo, &b.job)))
    });
    flaky.truncate(MAX_ROWS);
    out.flaky = flaky;

    // Queue time.
    let mut q: Vec<u64> = in_range.iter().filter_map(|j| j.queued_ms).collect();
    q.sort_unstable();
    out.queue = QueueStats {
        samples: q.len() as u32,
        avg_ms: (!q.is_empty()).then(|| avg(&q)),
        p95_ms: (!q.is_empty()).then(|| percentile(&q, 95)),
        max_ms: q.last().copied(),
    };

    // Runner load.
    let mut by_runner: BTreeMap<&str, Vec<&InsightJob>> = BTreeMap::new();
    for j in &in_range {
        by_runner.entry(j.runner.as_str()).or_default().push(j);
    }
    let mut runners: Vec<RunnerLoad> = by_runner
        .iter()
        .map(|(runner, js)| {
            let spans = js
                .iter()
                .map(|j| (j.started.0.max(first_day), j.finished.0.min(now.0)))
                .filter(|(s, e)| e > s)
                .collect();
            let busy = union_ms(spans);
            RunnerLoad {
                runner: (*runner).to_string(),
                jobs: js.len() as u32,
                busy_permille: u32::try_from((busy * 1000 / window_ms).min(1000)).unwrap_or(1000),
            }
        })
        .collect();
    runners.sort_by(|a, b| b.busy_permille.cmp(&a.busy_permille).then_with(|| a.runner.cmp(&b.runner)));
    out.runners = runners;

    out.advice = advise(&out);
    out
}

fn human_ms(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{} h {} min", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

/// Advice from the numbers: capacity, queue time and success rates (ADR 0015).
pub fn advise(i: &Insights) -> Vec<Advice> {
    let mut out = Vec::new();
    let busiest = i.runners.first();
    if let (Some(avg_ms), Some(p95)) = (i.queue.avg_ms, i.queue.p95_ms) {
        let waits = avg_ms >= QUEUE_NOTICE_MS || p95 >= QUEUE_PROBLEM_P95_MS;
        let at_limit = busiest.is_some_and(|r| r.busy_permille >= BUSY_LIMIT_PERMILLE);
        if waits && at_limit {
            let r = busiest.map_or_else(String::new, |r| {
                format!("{} was busy {}% of the time", r.runner, r.busy_permille / 10)
            });
            out.push(Advice {
                kind: AdviceKind::AddCapacity,
                title: "Jobs wait for a runner: add a runner or raise capacity".into(),
                detail: format!(
                    "Jobs waited {} on average and {} at the 95th percentile ({} jobs measured); {r}. Register another runner, or raise `runner.capacity` in the busiest runner's configuration if the machine has room (Runners > Configuration).",
                    human_ms(avg_ms),
                    human_ms(p95),
                    i.queue.samples
                ),
            });
        } else if waits {
            out.push(Advice {
                kind: AdviceKind::CheckLabels,
                title: "Jobs wait although the runners are not busy".into(),
                detail: format!(
                    "Jobs waited {} on average ({} at the 95th percentile) but no runner was busy more than {}% of the time. The wait is probably not capacity: check that the workflows' `runs-on` labels match a runner's labels, and that the runners were online when the jobs were queued.",
                    human_ms(avg_ms),
                    human_ms(p95),
                    busiest.map_or(0, |r| r.busy_permille / 10)
                ),
            });
        }
    }
    if !i.runners.is_empty()
        && i.runners.iter().all(|r| r.busy_permille < IDLE_PERMILLE)
        && i.queue.avg_ms.is_none_or(|a| a < QUEUE_NOTICE_MS)
        && i.jobs >= 10
    {
        out.push(Advice {
            kind: AdviceKind::SpareCapacity,
            title: "There is capacity to spare".into(),
            detail: format!(
                "Every runner was busy less than {}% of the time and jobs hardly waited. You could pause a runner during quiet hours (Settings > Pause windows).",
                IDLE_PERMILLE / 10
            ),
        });
    }
    for r in &i.repos {
        if let (Some(e), Some(l)) = (r.earlier_permille, r.later_permille) {
            if e >= l + 150 && r.success + r.failure >= 8 {
                out.push(Advice {
                    kind: AdviceKind::SuccessDropped,
                    title: format!("{}: the success rate dropped", r.repo),
                    detail: format!(
                        "It was {}% in the first half of the range and {}% in the second. Look at the newest failures; if they are flaky jobs (see Flaky jobs) a retry policy may help.",
                        e / 10,
                        l / 10
                    ),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = 3_600_000;

    fn now() -> Timestamp {
        Timestamp(40 * DAY_MS + 12 * H)
    }

    fn job(runner: &str, repo: &str, name: &str, ago_h: u64, dur_min: u64, c: Conclusion, q: Option<u64>) -> InsightJob {
        let finished = now().0 - ago_h * H;
        InsightJob {
            runner: runner.into(),
            repo: repo.into(),
            job: name.into(),
            started: Timestamp(finished - dur_min * 60_000),
            finished: Timestamp(finished),
            conclusion: c,
            queued_ms: q,
        }
    }

    #[test]
    fn slowest_flaky_success_rates_and_queue_are_computed() {
        use Conclusion::*;
        let mut jobs = vec![];
        // A flaky job: pass, fail, pass, fail, pass.
        for (i, c) in [Success, Failure, Success, Failure, Success].into_iter().enumerate() {
            jobs.push(job("a", "o/app", "test", 100 - i as u64 * 10, 10, c, Some(5_000)));
        }
        // A slow and steady one, and a broken one.
        for i in 0..4 {
            jobs.push(job("a", "o/app", "build", 90 - i * 10, 40, Success, Some(30_000)));
            jobs.push(job("b", "o/lib", "lint", 85 - i * 10, 1, Failure, None));
        }
        let r = compute(&jobs, now(), 7, 0);
        assert_eq!((r.range_days, r.jobs, r.jobs_with_outcome), (7, 13, 13));
        assert_eq!(r.slowest[0].job, "build");
        assert_eq!((r.slowest[0].avg_ms, r.slowest[0].runs), (40 * 60_000, 4));
        assert_eq!(r.flaky.len(), 1, "{:?}", r.flaky);
        assert_eq!((r.flaky[0].job.as_str(), r.flaky[0].flips, r.flaky[0].failures), ("test", 4, 2));
        let app = r.repos.iter().find(|x| x.repo == "o/app").unwrap();
        assert_eq!((app.success, app.failure, app.success_permille), (7, 2, Some(777)));
        let lib = r.repos.iter().find(|x| x.repo == "o/lib").unwrap();
        assert_eq!(lib.success_permille, Some(0));
        assert_eq!(app.days.len(), 7);
        assert_eq!(app.days.iter().map(|d| d.success + d.failure).sum::<u32>(), 9);
        assert_eq!(r.queue.samples, 9);
        assert_eq!(r.queue.max_ms, Some(30_000));
        assert_eq!(app.avg_queued_ms, Some((5_000 * 5 + 30_000 * 4) / 9));
    }

    #[test]
    fn unknown_outcomes_count_for_durations_but_not_for_rates_and_old_jobs_drop_out() {
        let jobs = vec![
            job("a", "o/x", "j", 5, 3, Conclusion::Unknown, None),
            job("a", "o/x", "j", 40 * 24, 3, Conclusion::Failure, None),
        ];
        let r = compute(&jobs, now(), 7, 0);
        assert_eq!((r.jobs, r.jobs_with_outcome), (1, 0));
        assert_eq!(r.repos[0].success_permille, None);
        assert!(r.flaky.is_empty() && r.queue.avg_ms.is_none());
        let empty = compute(&[], now(), 0, 0);
        assert_eq!((empty.range_days, empty.jobs), (1, 0));
        assert!(empty.advice.is_empty());
    }

    #[test]
    fn busy_time_is_the_union_of_job_intervals() {
        // Two back-to-back 1 h jobs cover 2 of the 12 h since local midnight.
        let a = job("a", "o/x", "j", 10, 60, Conclusion::Success, None);
        let b = job("a", "o/x", "k", 9, 60, Conclusion::Success, None);
        let r = compute(&[a, b], now(), 1, 0);
        let load = &r.runners[0];
        assert_eq!(load.jobs, 2);
        assert_eq!(load.busy_permille, 166, "{load:?}");
    }

    #[test]
    fn advice_follows_the_numbers() {
        let mut i = Insights {
            jobs: 50,
            queue: QueueStats {
                samples: 40,
                avg_ms: Some(180_000),
                p95_ms: Some(600_000),
                max_ms: Some(900_000),
            },
            runners: vec![RunnerLoad {
                runner: "mac".into(),
                jobs: 30,
                busy_permille: 950,
            }],
            ..Insights::default()
        };
        let a = advise(&i);
        assert_eq!(a[0].kind, AdviceKind::AddCapacity);
        assert!(
            a[0].detail.contains("3 min 0 s") && a[0].detail.contains("mac was busy 95%"),
            "{}",
            a[0].detail
        );
        i.runners[0].busy_permille = 300;
        assert_eq!(advise(&i)[0].kind, AdviceKind::CheckLabels);
        i.queue.avg_ms = Some(2_000);
        i.queue.p95_ms = Some(9_000);
        i.runners[0].busy_permille = 100;
        assert_eq!(advise(&i)[0].kind, AdviceKind::SpareCapacity);
        i.repos.push(RepoInsight {
            repo: "o/app".into(),
            jobs: 20,
            success: 12,
            failure: 8,
            success_permille: Some(600),
            earlier_permille: Some(900),
            later_permille: Some(500),
            avg_queued_ms: None,
            days: vec![],
        });
        assert!(advise(&i).iter().any(|a| a.kind == AdviceKind::SuccessDropped));
        assert!(advise(&Insights::default()).is_empty());
    }

    #[test]
    fn finished_jobs_become_insight_jobs_and_masked_repositories_are_left_out() {
        use crate::gitea_api::JobDetails;
        use crate::jobs::{FinishedJob, JobInfo};
        let mk = |repo: &str, id: u64| FinishedJob {
            info: JobInfo {
                repository: Some(repo.into()),
                job: Some("build".into()),
                job_id: Some(id),
                ..JobInfo::default()
            },
            started_at: Timestamp(1000),
            finished_at: Timestamp(5000),
            outcome: None,
            peak_cpu_permille: 0,
            peak_mem_bytes: 0,
            leftovers: 0,
        };
        let mut details = std::collections::HashMap::new();
        details.insert(
            1,
            JobDetails {
                conclusion: Conclusion::Failure,
                queued_ms: Some(7),
            },
        );
        let out = from_finished("mac", &[mk("o/app", 1), mk("***/app", 2), mk("o/lib", 3)], &details);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].conclusion, out[0].queued_ms), (Conclusion::Failure, Some(7)));
        assert_eq!(out[1].conclusion, Conclusion::Unknown);
        assert_eq!(out[0].runner, "mac");
    }

    #[test]
    fn helpers() {
        assert_eq!(percentile(&[], 95), 0);
        assert_eq!(percentile(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 95), 10);
        assert_eq!(percentile(&[7], 50), 7);
        assert_eq!(human_ms(3_700_000), "1 h 1 min");
        assert_eq!(human_ms(42_000), "42 s");
        assert_eq!(union_ms(vec![(0, 5), (3, 10), (20, 25)]), 15);
    }
}
