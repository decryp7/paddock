// ci-runs-reader: ci schedule-skip: is a train in CI (queued or running dispatch runs)
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
//! **Upper bound.** A skip never leaves a workflow unmeasured for long: it is not skipped when its last scheduled run was itself
//! skipped for a train (never twice in a row, which bounds a weekly workflow), nor when its last measured scheduled run is older
//! than `--max-gap-hours` (36 h for a nightly: one skipped night at most). Measured means a completed, not cancelled scheduled
//! run of this workflow whose gate did not log the skip notice. When that history cannot be read the run proceeds.
//!
//! The pure decision is [`decide`] and [`bound`]; [`read`] lists the dispatch runs and [`read_past`] the workflow's scheduled
//! runs through RT-8's [`Client`].

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
    /// A train is in CI, but skipping would leave the workflow unmeasured too long (or that cannot be told): it proceeds.
    Overdue {
        /// The train runs found.
        trains: Vec<TrainRun>,
        /// Why it is not skipped.
        why: String,
    },
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
            Decision::Overdue { trains, why } => Some(format!("::notice::{}", overdue_line(trains, why))),
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
            Decision::Overdue { trains, why } => format!("### Schedule skip\n\n{}\n", overdue_line(trains, why)),
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
        "{}{} in CI (runs {}); runs on the next schedule after it lands",
        SKIP_NOTICE.trim_start_matches("::notice::"),
        branches.into_iter().collect::<Vec<_>>().join(", "),
        runs.join(", ")
    )
}

fn overdue_line(trains: &[TrainRun], why: &str) -> String {
    let branches: BTreeSet<&str> = trains.iter().map(|t| t.branch.as_str()).collect();
    format!(
        "not skipped although train {} is in CI: {why}",
        branches.into_iter().collect::<Vec<_>>().join(", ")
    )
}

/// The notice a skipped run's gate logs; [`read_past`] looks for it to tell a skipped run from a measured one.
pub const SKIP_NOTICE: &str = "::notice::skipped: train ";

/// A completed scheduled run of the workflow, newest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Past {
    /// The run's id.
    pub id: u64,
    /// When it was created (milliseconds), when the API says.
    pub created_ms: Option<f64>,
    /// Whether it was skipped for a train (its gate logged [`SKIP_NOTICE`]).
    pub train_skip: bool,
}

/// Why a skip would leave the workflow unmeasured too long, or `None` when it may skip. `past` is newest first.
pub fn bound(workflow: &str, past: &[Past], now_ms: f64, max_gap_hours: f64) -> Option<String> {
    if let Some(last) = past.first().filter(|p| p.train_skip) {
        return Some(format!(
            "its last scheduled run ({}) was already skipped for a train, and a workflow is never skipped twice in a row",
            last.id
        ));
    }
    let Some(measured) = past.iter().find(|p| !p.train_skip) else {
        return Some(format!("no measured scheduled run of {workflow} was found"));
    };
    match measured.created_ms {
        Some(c) if (now_ms - c) / 3_600_000.0 <= max_gap_hours => None,
        Some(c) => Some(format!(
            "its last measured scheduled run ({}) is {:.0} h old, over the {max_gap_hours:.0} h limit",
            measured.id,
            (now_ms - c) / 3_600_000.0
        )),
        None => Some(format!("the time of its last measured scheduled run ({}) is unknown", measured.id)),
    }
}

/// A skip turned into a run when [`bound`] (or an unreadable history) says so; any other decision unchanged.
pub fn bounded(d: Decision, workflow: &str, past: Result<Vec<Past>, String>, now_ms: f64, max_gap_hours: f64) -> Decision {
    let Decision::Skip { trains, stale } = d else { return d };
    let why = match past {
        Err(e) => Some(format!(
            "the scheduled runs of {workflow} could not be read ({e}), so the skip limit cannot be checked"
        )),
        Ok(p) => bound(workflow, &p, now_ms, max_gap_hours),
    };
    match why {
        Some(why) => Decision::Overdue { trains, why },
        None => Decision::Skip { trains, stale },
    }
}

/// The workflow's completed, not cancelled scheduled runs, newest first, up to and including the newest measured one (at most
/// eight). A run is a train skip when a job with a schedule-skip (or plan action) step logged [`SKIP_NOTICE`].
pub fn read_past(client: &mut Client, workflow: &str) -> Result<Vec<Past>, String> {
    let mut out = Vec::new();
    for page in 1..=3 {
        let p = client.runs_page_where(page, "event=schedule")?;
        let n = p.runs.len();
        for r in p
            .runs
            .iter()
            .filter(|r| r.workflow() == workflow && r.finished() && r.conclusion != "cancelled")
        {
            let jobs = client.timed_jobs(r.id)?.unwrap_or_default();
            let mut train_skip = false;
            for j in jobs.iter().filter(|j| j.conclusion == "success") {
                let gate = j
                    .steps
                    .iter()
                    .any(|(name, _)| name.contains("actions/schedule-skip") || name.contains("actions/plan"));
                if gate && client.log(j.id)?.is_some_and(|l| l.contains(SKIP_NOTICE)) {
                    train_skip = true;
                    break;
                }
            }
            out.push(Past {
                id: r.id,
                created_ms: ts(&r.created_at),
                train_skip,
            });
            if !train_skip || out.len() >= 8 {
                return Ok(out);
            }
        }
        if n < 50 {
            break;
        }
    }
    Ok(out)
}

/// The whole decision for a run of `workflow`: read the dispatch runs (scheduled runs only), decide, and when that says skip, bound
/// the skip by the workflow's own scheduled history. A reading that fails runs the workflow (it fails open).
pub fn decide_scheduled(client: &mut Client, event: &str, workflow: &str, now_ms: f64, max_gap_hours: f64) -> Decision {
    let runs = if event == "schedule" {
        read(client, now_ms)
    } else {
        Ok(Vec::new())
    };
    let d = decide(event, runs, now_ms);
    if d.skip() {
        let past = read_past(client, workflow);
        return bounded(d, workflow, past, now_ms, max_gap_hours);
    }
    d
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
            head_branch: String::new(),
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

    fn past(id: u64, created: &str, train_skip: bool) -> Past {
        Past {
            id,
            created_ms: ts(created),
            train_skip,
        }
    }

    fn a_skip() -> Decision {
        decide(
            "schedule",
            Ok(vec![run(
                7990,
                "ci.yml@refs/heads/train/t",
                "workflow_dispatch",
                "queued",
                "2026-11-12T05:10:00+08:00",
            )]),
            now(),
        )
    }

    #[test]
    fn a_skip_never_leaves_a_workflow_unmeasured_past_its_limit_nor_skips_twice_in_a_row() {
        // Measured 35 h ago: inside 36 h, the skip stands.
        let ok = vec![past(1, "2026-11-10T19:00:00+08:00", false)];
        assert!(bounded(a_skip(), "nightly.yml", Ok(ok), now(), 36.0).skip());
        // Measured 37 h ago: over the limit, it runs and says why.
        let late = vec![past(1, "2026-11-10T17:00:00+08:00", false)];
        let d = bounded(a_skip(), "nightly.yml", Ok(late), now(), 36.0);
        assert!(!d.skip());
        assert!(d.annotation().unwrap().contains("is 37 h old, over the 36 h limit"), "{d:?}");
        // The last run was itself a train skip (a weekly workflow measured within its limit): never twice in a row.
        let weekly = vec![
            past(2, "2026-11-11T20:00:00+08:00", true),
            past(1, "2026-11-08T20:00:00+08:00", false),
        ];
        assert!(
            bounded(a_skip(), "debrief-mutants.yml", Ok(weekly.clone()), now(), 200.0)
                .annotation()
                .unwrap()
                .contains("never skipped twice")
        );
        assert!(bounded(a_skip(), "debrief-mutants.yml", Ok(weekly[1..].to_vec()), now(), 200.0).skip());
        // Never measured, or the history unreadable: it runs.
        assert!(!bounded(a_skip(), "x.yml", Ok(vec![]), now(), 36.0).skip());
        assert!(
            bounded(a_skip(), "x.yml", Err("HTTP 500".into()), now(), 36.0)
                .annotation()
                .unwrap()
                .contains("cannot be checked")
        );
        // Any other decision is left as it is.
        assert_eq!(
            bounded(Decision::NotScheduled, "x.yml", Err("x".into()), now(), 36.0),
            Decision::NotScheduled
        );
    }

    #[test]
    fn the_past_runs_tell_a_train_skip_by_the_gates_notice() {
        let api = "https://gitea.invalid/api/v1/repos/o/r/actions";
        let mut http = Fake::default();
        let r = |id: u64, path: &str, conclusion: &str, created: &str| json!({"id": id, "path": path, "event": "schedule", "status": "completed", "conclusion": conclusion, "created_at": created});
        http.json(
            &format!("{api}/runs?event=schedule&limit=50&page=1"),
            json!({"workflow_runs": [
                r(30, "nightly.yml@refs/heads/main", "success", "2026-11-12T05:03:00+08:00"),
                r(29, "canary.yml@refs/heads/main", "success", "2026-11-11T22:23:00+08:00"),
                r(28, "nightly.yml@refs/heads/main", "cancelled", "2026-11-11T05:03:00+08:00"),
                r(27, "nightly.yml@refs/heads/main", "failure", "2026-11-10T05:03:00+08:00"),
            ]}),
        );
        let job =
            |id: u64, step: &str| json!({"id": id, "name": "gate", "conclusion": "success", "runner_id": 1, "steps": [{"name": step}]});
        http.json(
            &format!("{api}/runs/30/jobs?limit=50&page=1"),
            json!({"jobs": [job(300, "Run ./.gitea/actions/plan")]}),
        );
        http.json(
            &format!("{api}/runs/27/jobs?limit=50&page=1"),
            json!({"jobs": [job(270, "Run ./.gitea/actions/plan")]}),
        );
        http.log(
            300,
            "2026-11-12T05:03:10Z ::notice::skipped: train train/x in CI (runs ci.yml 1); runs on the next schedule after it lands\n",
        );
        http.log(270, "2026-11-10T05:03:10Z run=true\n");
        let mut client = Client::new(&mut http, "https://gitea.invalid", "o/r", "t");
        client.backoff_ms = 0;
        let p = read_past(&mut client, "nightly.yml").unwrap();
        assert_eq!(
            p.iter().map(|x| (x.id, x.train_skip)).collect::<Vec<_>>(),
            [(30, true), (27, false)]
        );
        assert!(bound("nightly.yml", &p, now(), 36.0).unwrap().contains("never skipped twice"));
    }

    #[test]
    fn a_train_in_ci_after_a_skipped_last_run_still_runs_the_workflow() {
        let api = "https://gitea.invalid/api/v1/repos/o/r/actions";
        let mut http = Fake::default();
        http.json(
            &format!("{api}/runs?event=workflow_dispatch&limit=50&page=1"),
            json!({"workflow_runs": [{"id": 7990, "path": "ci.yml@refs/heads/train/t", "event": "workflow_dispatch", "status": "queued", "created_at": "2026-11-12T05:10:00+08:00"}]}),
        );
        http.json(
            &format!("{api}/runs?event=schedule&limit=50&page=1"),
            json!({"workflow_runs": [{"id": 30, "path": "nightly.yml@refs/heads/main", "event": "schedule", "status": "completed", "conclusion": "success", "created_at": "2026-11-12T05:03:00+08:00"}]}),
        );
        http.json(
            &format!("{api}/runs/30/jobs?limit=50&page=1"),
            json!({"jobs": [{"id": 300, "name": "gate", "conclusion": "success", "runner_id": 1, "steps": [{"name": "Run ./.gitea/actions/plan"}]}]}),
        );
        http.log(
            300,
            "::notice::skipped: train train/x in CI (runs ci.yml 1); runs on the next schedule after it lands\n",
        );
        let mut client = Client::new(&mut http, "https://gitea.invalid", "o/r", "t");
        client.backoff_ms = 0;
        let d = decide_scheduled(&mut client, "schedule", "nightly.yml", now(), 36.0);
        assert!(!d.skip(), "{d:?}");
        assert!(d.annotation().unwrap().contains("never skipped twice"), "{d:?}");
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
