//! `ci schedule-skip`: whether a scheduled run on `main` ends early because a train is in CI (user, 2026-11-12; ADR 0093
//! amendment). A train's runs are `workflow_dispatch` runs on a `train/*` branch; while one is queued or running, `main` is about
//! to move, so a scheduled full suite on today's `main` measures a tree that is about to be replaced and takes runner slots
//! from the train.
//!
//! The answer is `skip=true|false`, written to `GITHUB_OUTPUT` (or standard output without it), with one `::notice::` or
//! `::warning::` line and a job-summary line saying why. It never fails the run: when the Actions API cannot be read the
//! scheduled run proceeds (fail open) and says so. A skipped run measured nothing: a reader of scheduled results counts it as
//! not measured, never as green, and the green markers (`nightly-mark`) are not written for it.
//!
//! The pure decision is [`decide`]; [`read`] lists the dispatch runs through RT-8's [`Client`].

use super::client::{Client, RunInfo};
use super::time::ts;
use std::collections::BTreeSet;

/// How many pages of 50 dispatch runs are read at most (a train's runs are among the newest).
pub const MAX_PAGES: u32 = 4;

/// A run older than this is not taken as a train in CI: a run the server never finished (a lost runner) must not stop every
/// schedule for good. It is named in the log instead.
pub const STALE_MS: f64 = 24.0 * 3600.0 * 1000.0;

/// A queued or running dispatch run on a train branch.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainRun {
    /// The run's id.
    pub id: u64,
    /// The branch (`train/2026-11-11-5`).
    pub branch: String,
    /// The workflow file (`debrief.yml`).
    pub workflow: String,
    /// `queued`, `in_progress`, `waiting`...
    pub status: String,
}

/// What the check decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Not a scheduled run: nothing to decide.
    NotScheduled,
    /// A train is in CI: the scheduled run ends early.
    Skip {
        /// The train runs found (newest first), at least one.
        trains: Vec<TrainRun>,
        /// Train runs older than [`STALE_MS`] that were not counted.
        stale: Vec<TrainRun>,
    },
    /// No train is in CI: the scheduled run proceeds.
    Run {
        /// Train runs older than [`STALE_MS`] that were not counted.
        stale: Vec<TrainRun>,
    },
    /// The runs could not be read: the scheduled run proceeds (fail open).
    Unreadable(String),
}

impl Decision {
    /// Whether the scheduled run ends early.
    pub fn skip(&self) -> bool {
        matches!(self, Decision::Skip { .. })
    }

    /// The one annotation line for the job log, if any.
    pub fn annotation(&self) -> Option<String> {
        match self {
            Decision::NotScheduled | Decision::Run { .. } => None,
            Decision::Skip { trains, .. } => Some(format!("::notice::{}", skip_line(trains))),
            Decision::Unreadable(e) => Some(format!(
                "::warning::schedule skip: the Actions runs could not be read ({e}); this scheduled run proceeds (fail open). Check that the job token can read the repository's Actions runs."
            )),
        }
    }

    /// The job-summary text.
    pub fn summary(&self) -> String {
        let stale = |s: &[TrainRun]| {
            if s.is_empty() {
                String::new()
            } else {
                let ids: Vec<String> = s.iter().map(|r| format!("{} ({})", r.id, r.branch)).collect();
                format!(
                    "\n\nNot counted, older than 24 h and still listed as running (check the runner): {}.",
                    ids.join(", ")
                )
            }
        };
        match self {
            Decision::NotScheduled => "### Schedule skip\n\nNot a scheduled run: nothing to check.\n".into(),
            Decision::Skip { trains, stale: s } => format!(
                "### Schedule skip\n\n{}\n\nThis run measured nothing: a reader of scheduled results counts it as not measured, never green (ADR 0093, amendment 2026-11-12).{}\n",
                skip_line(trains),
                stale(s)
            ),
            Decision::Run { stale: s } => format!("### Schedule skip\n\nNo train is in CI: the scheduled run proceeds.{}\n", stale(s)),
            Decision::Unreadable(e) => format!(
                "### Schedule skip\n\nNot checked: the Actions runs could not be read ({e}). The scheduled run proceeds (fail open).\n"
            ),
        }
    }
}

fn skip_line(trains: &[TrainRun]) -> String {
    let branches: BTreeSet<&str> = trains.iter().map(|t| t.branch.as_str()).collect();
    let runs: Vec<String> = trains.iter().map(|t| format!("{} {}", t.workflow, t.id)).collect();
    format!(
        "skipped: train {} in CI (runs {}); runs on the next schedule after it lands",
        branches.into_iter().collect::<Vec<_>>().join(", "),
        runs.join(", ")
    )
}

/// The train branch a run's `path` names (`ci.yml@refs/heads/train/x` is `train/x`), `None` for any other ref.
pub fn train_branch(path: &str) -> Option<&str> {
    path.split_once('@')
        .and_then(|(_, r)| r.strip_prefix("refs/heads/"))
        .filter(|b| b.starts_with("train/"))
}

/// The queued or running dispatch runs on train branches among `runs`, split into the ones that count and the stale ones.
pub fn trains_in_ci(runs: &[RunInfo], now_ms: f64) -> (Vec<TrainRun>, Vec<TrainRun>) {
    let (mut live, mut stale) = (Vec::new(), Vec::new());
    for r in runs.iter().filter(|r| r.event == "workflow_dispatch" && !r.finished()) {
        let Some(branch) = train_branch(&r.path) else { continue };
        let t = TrainRun {
            id: r.id,
            branch: branch.to_string(),
            workflow: r.workflow().to_string(),
            status: r.status.clone(),
        };
        // A run whose creation time cannot be read counts: the API always gives one, and counting it only delays a schedule.
        if ts(&r.created_at).is_some_and(|c| now_ms - c > STALE_MS) {
            stale.push(t);
        } else {
            live.push(t);
        }
    }
    (live, stale)
}

/// The decision for an event and the dispatch runs read (or why they could not be read).
pub fn decide(event: &str, runs: Result<Vec<RunInfo>, String>, now_ms: f64) -> Decision {
    if event != "schedule" {
        return Decision::NotScheduled;
    }
    match runs {
        Err(e) => Decision::Unreadable(e),
        Ok(runs) => {
            let (trains, stale) = trains_in_ci(&runs, now_ms);
            if trains.is_empty() {
                Decision::Run { stale }
            } else {
                Decision::Skip { trains, stale }
            }
        }
    }
}

/// The newest dispatch runs, page by page, until a short page, a page whose newest run is older than [`STALE_MS`], or
/// [`MAX_PAGES`].
pub fn read(client: &mut Client, now_ms: f64) -> Result<Vec<RunInfo>, String> {
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let p = client.runs_page_where(page, "event=workflow_dispatch")?;
        let n = p.runs.len();
        let old = p
            .runs
            .first()
            .and_then(|r| ts(&r.created_at))
            .is_some_and(|c| now_ms - c > STALE_MS);
        out.extend(p.runs);
        if n < 50 || old {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;
    use serde_json::json;

    const NOW: &str = "2026-11-12T06:00:00+08:00";

    fn now() -> f64 {
        ts(NOW).unwrap()
    }

    fn run(id: u64, path: &str, event: &str, status: &str, created: &str) -> RunInfo {
        RunInfo {
            id,
            path: path.into(),
            event: event.into(),
            status: status.into(),
            conclusion: String::new(),
            head_sha: "h".into(),
            created_at: created.into(),
            run_attempt: 1,
        }
    }

    #[test]
    fn a_queued_or_running_train_dispatch_skips_a_scheduled_run_and_nothing_else_does() {
        let runs = vec![
            run(
                7990,
                "debrief.yml@refs/heads/train/2026-11-11-5",
                "workflow_dispatch",
                "in_progress",
                "2026-11-12T05:10:00+08:00",
            ),
            run(
                7991,
                "ci.yml@refs/heads/train/2026-11-11-5",
                "workflow_dispatch",
                "queued",
                "2026-11-12T05:11:00+08:00",
            ),
            run(
                7992,
                "ci.yml@refs/heads/fix/x",
                "workflow_dispatch",
                "in_progress",
                "2026-11-12T05:12:00+08:00",
            ),
            run(
                7993,
                "ci.yml@refs/heads/train/2026-11-10-4",
                "workflow_dispatch",
                "completed",
                "2026-11-12T01:00:00+08:00",
            ),
            run(
                7994,
                "ci.yml@refs/heads/train/2026-11-10-4",
                "push",
                "in_progress",
                "2026-11-12T05:00:00+08:00",
            ),
        ];
        let d = decide("schedule", Ok(runs.clone()), now());
        assert!(d.skip());
        let Decision::Skip { trains, .. } = &d else { unreachable!() };
        assert_eq!(trains.iter().map(|t| t.id).collect::<Vec<_>>(), [7990, 7991]);
        assert_eq!(
            d.annotation().unwrap(),
            "::notice::skipped: train train/2026-11-11-5 in CI (runs debrief.yml 7990, ci.yml 7991); runs on the next schedule after it lands"
        );
        assert!(d.summary().contains("counts it as not measured, never green"));
        // Without the train's runs: no skip.
        let d = decide("schedule", Ok(runs[2..].to_vec()), now());
        assert_eq!(d, Decision::Run { stale: vec![] });
        assert_eq!(d.annotation(), None);
        // Not a scheduled run: never skipped, nothing read.
        assert_eq!(decide("workflow_dispatch", Ok(runs), now()), Decision::NotScheduled);
    }

    #[test]
    fn an_unreadable_api_lets_the_scheduled_run_proceed_and_says_so() {
        let d = decide("schedule", Err("HTTP 401".into()), now());
        assert!(!d.skip());
        assert!(
            d.annotation()
                .unwrap()
                .starts_with("::warning::schedule skip: the Actions runs could not be read (HTTP 401)")
        );
        assert!(d.summary().contains("proceeds (fail open)"));
    }

    #[test]
    fn a_train_run_still_listed_as_running_after_a_day_is_named_but_does_not_stop_the_schedule() {
        let runs = vec![run(
            7001,
            "ci.yml@refs/heads/train/2026-11-09-3",
            "workflow_dispatch",
            "in_progress",
            "2026-11-10T05:00:00+08:00",
        )];
        let d = decide("schedule", Ok(runs), now());
        let Decision::Run { stale } = &d else { panic!("{d:?}") };
        assert_eq!(stale[0].id, 7001);
        assert!(d.summary().contains("7001 (train/2026-11-09-3)"));
    }

    #[test]
    fn the_dispatch_runs_are_read_page_by_page_until_a_short_or_old_page() {
        let api = "https://gitea.invalid/api/v1/repos/o/r/actions/runs?event=workflow_dispatch&limit=50";
        let page = |n: usize, created: &str| {
            let runs: Vec<_> = (0..n)
                .map(|i| json!({"id": i, "path": "ci.yml@refs/heads/fix/x", "event": "workflow_dispatch", "status": "completed", "created_at": created}))
                .collect();
            json!({"workflow_runs": runs, "total_count": 500})
        };
        let mut http = Fake::default();
        http.json(&format!("{api}&page=1"), page(50, "2026-11-12T05:00:00+08:00"));
        http.json(&format!("{api}&page=2"), page(50, "2026-11-12T04:00:00+08:00"));
        http.json(&format!("{api}&page=3"), page(3, "2026-11-12T03:00:00+08:00"));
        let mut client = Client::new(&mut http, "https://gitea.invalid", "o/r", "t");
        client.backoff_ms = 0;
        assert_eq!(read(&mut client, now()).unwrap().len(), 103);
        let mut http = Fake::default();
        http.json(&format!("{api}&page=1"), page(50, "2026-11-10T05:00:00+08:00"));
        http.json(&format!("{api}&page=2"), page(50, "2026-11-10T04:00:00+08:00"));
        let mut client = Client::new(&mut http, "https://gitea.invalid", "o/r", "t");
        client.backoff_ms = 0;
        assert_eq!(
            read(&mut client, now()).unwrap().len(),
            50,
            "a page of runs older than a day ends the reading"
        );
        assert_eq!(train_branch("ci.yml@refs/heads/train/a"), Some("train/a"));
        assert_eq!(train_branch("ci.yml@refs/heads/main"), None);
        assert_eq!(train_branch("ci.yml@refs/tags/train/a"), None);
    }
}
