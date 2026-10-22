//! What happened to a runner over time — starts, exits, stalls, crash loops, jobs, updates — and the summaries the
//! window draws from it (uptime, restarts and jobs per day, a timeline).
//!
//! The service keeps a bounded list of events per runner ([`MAX_EVENTS`]) in its state file; summaries are computed
//! here, so the UI only draws them.

use crate::housekeeping::CleanupTrigger;
use crate::jobs::FinishedJob;
use crate::time::Timestamp;
use crate::update::UpdateOutcome;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Most events kept per runner (about two months of a busy CI machine).
pub const MAX_EVENTS: usize = 4000;
/// Most days one summary covers.
pub const MAX_DAYS: u32 = 90;
/// Most spans or marks one summary returns per kind (the newest are kept).
pub const MAX_SPANS: usize = 2000;
/// Most finished jobs one summary lists.
pub const MAX_RECENT_JOBS: usize = 1000;
/// One day in milliseconds.
pub const DAY_MS: u64 = 24 * 3600 * 1000;

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HistoryKind {
    /// The runner process started.
    Started,
    /// The runner process exited; `expected` when Pitcrew had asked it to stop.
    Exited {
        /// Its exit code, when it had one.
        code: Option<i32>,
        /// Pitcrew asked it to stop (stop, restart, update, drain).
        expected: bool,
    },
    /// The watchdog found it stalled and restarted it.
    Stalled {
        /// Timed-out polls in the window.
        timeouts: u32,
    },
    /// It exited too often; restarts were paused.
    CrashLoop {
        /// Exits in the window.
        exits: u32,
    },
    /// It could not be started.
    LaunchFailed {
        /// Why, for the user.
        reason: String,
    },
    /// The user stopped it.
    Stopped,
    /// It was drained and paused for maintenance.
    Paused,
    /// The user resumed it after maintenance.
    Resumed,
    /// A job ended.
    JobFinished {
        /// The job.
        job: FinishedJob,
    },
    /// An update ended.
    UpdateFinished {
        /// How.
        outcome: UpdateOutcome,
    },
    /// Disk housekeeping ran.
    Cleanup {
        /// What started it.
        trigger: CleanupTrigger,
        /// Bytes freed.
        freed_bytes: u64,
        /// Entries deleted.
        deleted: u32,
        /// Entries that could not be deleted (tried again next time).
        failed: u32,
    },
    /// The runner's `config.yaml` was saved from Pitcrew (ADR 0011).
    ConfigSaved {
        /// The runner restarts (after its job) to use it.
        restart: bool,
        /// The keys that changed, dotted; empty for an edit in the Advanced editor.
        changed: Vec<String>,
    },
    /// After the restart the runner declared the labels Pitcrew expected.
    LabelsVerified {
        /// The labels the runner declared.
        labels: Vec<String>,
    },
    /// After the restart the runner declared other labels than the file says.
    LabelsMismatch {
        /// The label names in the file.
        expected: Vec<String>,
        /// The label names the runner declared.
        declared: Vec<String>,
    },
    /// A process the runner started was stopped (ADR 0012).
    ProcessStopped {
        /// Its program name.
        name: String,
        /// Its process id.
        pid: u32,
        /// How long it had been running, in milliseconds.
        age_ms: u64,
        /// The rule that applied (`default 10 min`, `*GradleDaemon* 3 h`, `you`).
        rule: String,
        /// Who decided.
        by: StopReason,
        /// It ignored the request and was ended.
        forced: bool,
    },
}

/// Who stopped a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// Automatic cleanup of a leftover.
    Cleanup,
    /// The user chose Stop.
    User,
}

/// One event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEvent {
    /// When.
    pub at: Timestamp,
    /// What.
    #[serde(flatten)]
    pub kind: HistoryKind,
}

/// A runner's events, oldest first, bounded by [`MAX_EVENTS`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct History {
    events: VecDeque<HistoryEvent>,
}

impl History {
    /// An empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// A history from stored events (sorted by time and cut to the newest [`MAX_EVENTS`]).
    pub fn from_events(mut events: Vec<HistoryEvent>) -> Self {
        events.sort_by_key(|e| e.at);
        let skip = events.len().saturating_sub(MAX_EVENTS);
        History {
            events: events.into_iter().skip(skip).collect(),
        }
    }

    /// Adds an event, dropping the oldest beyond [`MAX_EVENTS`].
    pub fn push(&mut self, at: Timestamp, kind: HistoryKind) {
        self.events.push_back(HistoryEvent { at, kind });
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    /// The events, oldest first.
    pub fn events(&self) -> impl Iterator<Item = &HistoryEvent> {
        self.events.iter()
    }

    /// Number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The newest `n` events, newest first.
    pub fn recent(&self, n: usize) -> Vec<HistoryEvent> {
        self.events.iter().rev().take(n).cloned().collect()
    }

    /// The newest `n` finished jobs, oldest first.
    pub fn recent_jobs(&self, n: usize) -> Vec<FinishedJob> {
        let mut jobs: Vec<FinishedJob> = self
            .events
            .iter()
            .rev()
            .filter_map(|e| match &e.kind {
                HistoryKind::JobFinished { job } => Some(job.clone()),
                _ => None,
            })
            .take(n)
            .collect();
        jobs.reverse();
        jobs
    }

    /// Unplanned exits and stall restarts since `since`.
    pub fn restarts_since(&self, since: Timestamp) -> u32 {
        let n = self.events.iter().filter(|e| e.at >= since && is_restart(&e.kind)).count();
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

fn is_restart(k: &HistoryKind) -> bool {
    matches!(k, HistoryKind::Exited { expected: false, .. } | HistoryKind::Stalled { .. })
}

/// A span of time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    /// From.
    pub from: Timestamp,
    /// To.
    pub to: Timestamp,
}

/// A moment worth a mark on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkKind {
    /// An unplanned exit.
    Crash,
    /// A stall restart.
    Stall,
    /// Restarts were paused.
    CrashLoop,
    /// The runner could not be started.
    LaunchFailed,
    /// Updated.
    Updated,
    /// An update was rolled back.
    RolledBack,
    /// An update failed.
    UpdateFailed,
    /// An update was postponed or cancelled.
    UpdateSkipped,
    /// Disk housekeeping ran.
    Cleanup,
}

/// A mark on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    /// When.
    pub at: Timestamp,
    /// What.
    pub kind: MarkKind,
}

/// One local day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    /// The day's local midnight.
    pub start: Timestamp,
    /// Jobs that ended that day.
    pub jobs: u32,
    /// Unplanned exits and stall restarts.
    pub restarts: u32,
    /// Share of the day (so far, for today) the runner was up, in per mille.
    pub uptime_permille: u32,
    /// Time spent on jobs, in milliseconds.
    pub busy_ms: u64,
    /// Average duration of the jobs that ended that day, in milliseconds (0 without jobs).
    pub avg_job_ms: u64,
}

/// An update and how it ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEntry {
    /// When it ended.
    pub at: Timestamp,
    /// How.
    pub outcome: UpdateOutcome,
}

/// What the Activity view shows for one runner over a period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    /// Start of the period (local midnight of the first day).
    pub from: Timestamp,
    /// End of the period (now).
    pub to: Timestamp,
    /// One entry per day, oldest first.
    pub days: Vec<DaySummary>,
    /// Share of the period the runner was up, in per mille.
    pub uptime_permille: u32,
    /// Unplanned exits and stall restarts.
    pub restarts: u32,
    /// Jobs that ended.
    pub jobs: u32,
    /// When the runner was up.
    pub up: Vec<Span>,
    /// When it ran jobs.
    pub busy: Vec<Span>,
    /// Crashes, stalls and updates.
    pub marks: Vec<Mark>,
    /// Updates, newest last.
    pub updates: Vec<UpdateEntry>,
    /// The newest finished jobs, newest last (at most [`MAX_RECENT_JOBS`]).
    pub recent_jobs: Vec<FinishedJob>,
}

/// Local midnight of the day `t` falls in, for a zone `utc_offset_minutes` east of UTC.
pub fn local_day_start(t: Timestamp, utc_offset_minutes: i32) -> Timestamp {
    let offset = i64::from(utc_offset_minutes) * 60_000;
    let local = i128::from(t.0) + i128::from(offset);
    let day = i128::from(DAY_MS);
    let start = local.div_euclid(day) * day - i128::from(offset);
    Timestamp(u64::try_from(start.max(0)).unwrap_or(0))
}

/// Summarises `history` over the last `days` local days up to `now` (days are capped at [`MAX_DAYS`]).
pub fn summarize(history: &History, now: Timestamp, days: u32, utc_offset_minutes: i32) -> HistorySummary {
    let days = days.clamp(1, MAX_DAYS);
    let today = local_day_start(now, utc_offset_minutes);
    let from = Timestamp(today.0.saturating_sub(u64::from(days - 1) * DAY_MS));

    // Up intervals: from a start to the next exit, or to now.
    let mut up: Vec<Span> = vec![];
    let mut up_since: Option<Timestamp> = None;
    let mut busy = vec![];
    let mut marks = vec![];
    let mut updates = vec![];
    let mut recent_jobs = vec![];
    for e in history.events() {
        match &e.kind {
            HistoryKind::Started => {
                up_since.get_or_insert(e.at);
            }
            HistoryKind::Exited { expected, .. } => {
                if let Some(s) = up_since.take() {
                    up.push(Span { from: s, to: e.at });
                }
                if !expected {
                    marks.push(Mark {
                        at: e.at,
                        kind: MarkKind::Crash,
                    });
                }
            }
            HistoryKind::Stalled { .. } => marks.push(Mark {
                at: e.at,
                kind: MarkKind::Stall,
            }),
            HistoryKind::CrashLoop { .. } => marks.push(Mark {
                at: e.at,
                kind: MarkKind::CrashLoop,
            }),
            HistoryKind::LaunchFailed { .. } => marks.push(Mark {
                at: e.at,
                kind: MarkKind::LaunchFailed,
            }),
            HistoryKind::JobFinished { job } => {
                busy.push(Span {
                    from: job.started_at,
                    to: job.finished_at.max(job.started_at),
                });
                if job.finished_at >= from {
                    recent_jobs.push(job.clone());
                }
            }
            HistoryKind::UpdateFinished { outcome } => {
                let kind = match outcome {
                    UpdateOutcome::Updated { .. } => MarkKind::Updated,
                    UpdateOutcome::RolledBack { .. } => MarkKind::RolledBack,
                    UpdateOutcome::Failed { .. } => MarkKind::UpdateFailed,
                    UpdateOutcome::Postponed { .. } | UpdateOutcome::Cancelled { .. } => MarkKind::UpdateSkipped,
                };
                marks.push(Mark { at: e.at, kind });
                if e.at >= from {
                    updates.push(UpdateEntry {
                        at: e.at,
                        outcome: outcome.clone(),
                    });
                }
            }
            HistoryKind::Cleanup { .. } => marks.push(Mark {
                at: e.at,
                kind: MarkKind::Cleanup,
            }),
            HistoryKind::Stopped
            | HistoryKind::Paused
            | HistoryKind::Resumed
            | HistoryKind::ConfigSaved { .. }
            | HistoryKind::LabelsVerified { .. }
            | HistoryKind::LabelsMismatch { .. }
            | HistoryKind::ProcessStopped { .. } => {}
        }
    }
    if let Some(s) = up_since {
        up.push(Span { from: s, to: now.max(s) });
    }

    let clip = |spans: Vec<Span>| -> Vec<Span> {
        let mut out: Vec<Span> = spans
            .into_iter()
            .filter(|s| s.to > from && s.from < now)
            .map(|s| Span {
                from: s.from.max(from),
                to: s.to.min(now),
            })
            .collect();
        let skip = out.len().saturating_sub(MAX_SPANS);
        out.drain(..skip);
        out
    };
    let up = clip(up);
    let busy = clip(busy);
    marks.retain(|m| m.at >= from && m.at <= now);
    let skip = marks.len().saturating_sub(MAX_SPANS);
    marks.drain(..skip);
    let skip = recent_jobs.len().saturating_sub(MAX_RECENT_JOBS);
    recent_jobs.drain(..skip);

    let mut day_list = vec![];
    for d in 0..days {
        let start = Timestamp(from.0 + u64::from(d) * DAY_MS);
        let end = Timestamp(start.0 + DAY_MS).min(now);
        let length = end.0.saturating_sub(start.0);
        let in_day = |at: Timestamp| at >= start && at < Timestamp(start.0 + DAY_MS);
        let durations: Vec<u64> = history
            .events()
            .filter_map(|e| match &e.kind {
                HistoryKind::JobFinished { job } if in_day(job.finished_at) => Some(job.finished_at.0.saturating_sub(job.started_at.0)),
                _ => None,
            })
            .collect();
        let jobs = durations.len();
        let avg_job_ms = if jobs == 0 {
            0
        } else {
            durations.iter().sum::<u64>() / jobs as u64
        };
        let restarts = history.events().filter(|e| in_day(e.at) && is_restart(&e.kind)).count();
        day_list.push(DaySummary {
            start,
            jobs: u32::try_from(jobs).unwrap_or(u32::MAX),
            restarts: u32::try_from(restarts).unwrap_or(u32::MAX),
            uptime_permille: permille(overlap(&up, start, end), length),
            busy_ms: overlap(&busy, start, end),
            avg_job_ms,
        });
    }

    HistorySummary {
        from,
        to: now,
        uptime_permille: permille(overlap(&up, from, now), now.0.saturating_sub(from.0)),
        restarts: day_list.iter().map(|d| d.restarts).sum(),
        jobs: day_list.iter().map(|d| d.jobs).sum(),
        days: day_list,
        up,
        busy,
        marks,
        updates,
        recent_jobs,
    }
}

/// Milliseconds of `spans` inside `[start, end)`.
fn overlap(spans: &[Span], start: Timestamp, end: Timestamp) -> u64 {
    spans.iter().map(|s| s.to.min(end).0.saturating_sub(s.from.max(start).0)).sum()
}

fn permille(part: u64, whole: u64) -> u32 {
    if whole == 0 {
        return 0;
    }
    let p = u128::from(part.min(whole)) * 1000 / u128::from(whole);
    u32::try_from(p).unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobInfo;
    use crate::update::machine::{PostponeReason, RollbackReason};
    use crate::update::UpdatePlan;
    use crate::version::RunnerVersion;

    const H: u64 = 3600 * 1000;

    fn plan() -> UpdatePlan {
        UpdatePlan {
            from: Some(RunnerVersion::parse("5.1.0").unwrap()),
            to: RunnerVersion::parse("5.2.0").unwrap(),
        }
    }

    fn job(from: u64, to: u64) -> HistoryKind {
        HistoryKind::JobFinished {
            job: FinishedJob {
                info: JobInfo::default(),
                started_at: Timestamp(from),
                finished_at: Timestamp(to),
                outcome: None,
                peak_cpu_permille: 0,
                peak_mem_bytes: 0,
                leftovers: 0,
            },
        }
    }

    #[test]
    fn local_days_follow_the_zone() {
        // 2026-10-22T01:00Z is still the 21st in UTC-2 and the 22nd in UTC+8.
        let t = Timestamp(1_792_630_800_000);
        assert_eq!(local_day_start(t, 0).0 % DAY_MS, 0);
        assert!(local_day_start(t, -120) < local_day_start(t, 0));
        assert_eq!(local_day_start(t, 480).0, local_day_start(t, 0).0 - 8 * H);
        assert_eq!(local_day_start(Timestamp(0), 600), Timestamp(0), "never before the epoch");
    }

    #[test]
    fn uptime_restarts_and_jobs_per_day() {
        let day = 100 * DAY_MS;
        let mut h = History::new();
        h.push(Timestamp(day - 2 * H), HistoryKind::Started);
        h.push(
            Timestamp(day + 6 * H),
            HistoryKind::Exited {
                code: Some(1),
                expected: false,
            },
        );
        h.push(Timestamp(day + 6 * H), HistoryKind::CrashLoop { exits: 5 });
        h.push(Timestamp(day + 7 * H), HistoryKind::LaunchFailed { reason: "x".into() });
        h.push(Timestamp(day + 8 * H), HistoryKind::Started);
        h.push(Timestamp(day + 9 * H), job(day + 8 * H, day + 9 * H));
        h.push(Timestamp(day + 10 * H), HistoryKind::Stalled { timeouts: 6 });
        h.push(
            Timestamp(day + 10 * H),
            HistoryKind::Exited {
                code: None,
                expected: true,
            },
        );
        h.push(Timestamp(day + 10 * H), HistoryKind::Started);
        h.push(Timestamp(day + 11 * H), HistoryKind::Stopped);
        h.push(Timestamp(day + 11 * H), HistoryKind::Paused);
        h.push(Timestamp(day + 11 * H), HistoryKind::Resumed);
        h.push(
            Timestamp(day + 11 * H),
            HistoryKind::Cleanup {
                trigger: CleanupTrigger::Schedule,
                freed_bytes: 1,
                deleted: 1,
                failed: 0,
            },
        );
        h.push(
            Timestamp(day + 11 * H),
            HistoryKind::UpdateFinished {
                outcome: UpdateOutcome::Updated { plan: plan() },
            },
        );
        let now = Timestamp(day + 12 * H);
        let s = summarize(&h, now, 2, 0);
        assert_eq!(s.days.len(), 2);
        assert_eq!(s.from, Timestamp(day - DAY_MS));
        let today = s.days[1];
        assert_eq!(today.start, Timestamp(day));
        assert_eq!(today.jobs, 1);
        assert_eq!(today.restarts, 2, "one crash, one stall");
        // Up 0-6h and 8-12h of 12h so far.
        assert_eq!(today.uptime_permille, 833);
        assert_eq!(today.busy_ms, H);
        assert_eq!(today.avg_job_ms, H);
        assert_eq!(s.days[0].avg_job_ms, 0);
        assert_eq!(s.days[0].uptime_permille, 83, "up for the last 2 h of yesterday");
        assert_eq!((s.jobs, s.restarts), (1, 2));
        assert_eq!(s.updates.len(), 1);
        assert_eq!(s.recent_jobs.len(), 1);
        let kinds: Vec<MarkKind> = s.marks.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                MarkKind::Crash,
                MarkKind::CrashLoop,
                MarkKind::LaunchFailed,
                MarkKind::Stall,
                MarkKind::Cleanup,
                MarkKind::Updated
            ]
        );
        assert_eq!(s.up.first().map(|u| u.from), Some(s.from.max(Timestamp(day - 2 * H))));
        assert_eq!(h.restarts_since(Timestamp(day)), 2);
        assert_eq!(
            h.recent(2)[0].kind,
            HistoryKind::UpdateFinished {
                outcome: UpdateOutcome::Updated { plan: plan() }
            }
        );
        assert!(!h.is_empty());
    }

    #[test]
    fn every_update_outcome_has_a_mark() {
        let mut h = History::new();
        let outcomes = [
            UpdateOutcome::RolledBack {
                plan: plan(),
                reason: RollbackReason::ExitedEarly,
            },
            UpdateOutcome::Failed {
                plan: plan(),
                reason: "x".into(),
                needs_attention: true,
            },
            UpdateOutcome::Postponed {
                plan: plan(),
                reason: PostponeReason::Busy,
            },
            UpdateOutcome::Cancelled { plan: plan() },
        ];
        for (i, o) in outcomes.into_iter().enumerate() {
            h.push(Timestamp(1000 + i as u64), HistoryKind::UpdateFinished { outcome: o });
        }
        let s = summarize(&h, Timestamp(DAY_MS / 2), 1, 0);
        let kinds: Vec<MarkKind> = s.marks.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                MarkKind::RolledBack,
                MarkKind::UpdateFailed,
                MarkKind::UpdateSkipped,
                MarkKind::UpdateSkipped
            ]
        );
        assert_eq!(s.uptime_permille, 0);
    }

    #[test]
    fn history_is_bounded_and_sorted() {
        let mut h = History::new();
        for i in 0..(MAX_EVENTS + 10) {
            h.push(Timestamp(i as u64), HistoryKind::Started);
        }
        assert_eq!(h.len(), MAX_EVENTS);
        assert_eq!(h.events().next().map(|e| e.at), Some(Timestamp(10)));
        let events = vec![
            HistoryEvent {
                at: Timestamp(5),
                kind: HistoryKind::Stopped,
            },
            HistoryEvent {
                at: Timestamp(1),
                kind: HistoryKind::Started,
            },
        ];
        let h = History::from_events(events);
        assert_eq!(h.events().next().map(|e| e.at), Some(Timestamp(1)));
        let json = serde_json::to_string(&h).unwrap();
        assert!(json.starts_with(r#"[{"at":1,"kind":"started"}"#), "{json}");
        assert_eq!(serde_json::from_str::<History>(&json).unwrap(), h);
    }

    #[test]
    fn summaries_are_bounded() {
        let mut h = History::new();
        for i in 0..1500u64 {
            h.push(Timestamp(i * 10), job(i * 10, i * 10 + 5));
        }
        let s = summarize(&h, Timestamp(DAY_MS / 2), 500, 0);
        assert_eq!(s.days.len(), MAX_DAYS as usize);
        assert_eq!(s.recent_jobs.len(), MAX_RECENT_JOBS);
        for i in 1500..3000u64 {
            h.push(Timestamp(i * 10), job(i * 10, i * 10 + 5));
        }
        assert_eq!(summarize(&h, Timestamp(DAY_MS / 2), 1, 0).busy.len(), MAX_SPANS);
        assert_eq!(summarize(&History::new(), Timestamp(0), 0, 0).days.len(), 1);
        assert_eq!(permille(5, 0), 0);
    }

    #[test]
    fn the_newest_jobs_come_back_oldest_first() {
        let mut h = History::new();
        for n in 0..5u64 {
            h.push(Timestamp(n * 10), job(n * 10, n * 10 + 5));
            h.push(Timestamp(n * 10 + 6), HistoryKind::Started);
        }
        let jobs = h.recent_jobs(3);
        assert_eq!(jobs.iter().map(|j| j.started_at.0).collect::<Vec<_>>(), vec![20, 30, 40]);
        assert!(History::new().recent_jobs(3).is_empty());
    }
}
