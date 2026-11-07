//! The tray popup's slow values (family ADR 0035 §5): alarms, disk space, the last issue and the pinned
//! counters, built by `dyno_core::widgets::popover_values` from state the agent already keeps. Nothing here reads the OS or
//! the disk on a request: volumes come from the cached list, counters are the newest
//! kept readings and issues come from the index.

use crate::agent::{lock, now, Shared};
use dyno_core::alarms::AlarmFilter;
use dyno_core::jobs::JobKind;
use dyno_core::protocol::AgentError;
use dyno_core::time::DAY;
use dyno_core::widgets::popover_values::{
    is_live_push, state, CountersInput, Inputs, IssuesInput, JobProgress, Minute, PopoverState, Source, ISSUE_DAYS,
};
use dyno_core::widgets::PopoverKind;
use paddock_jobs::wire::WireState;
use paddock_widgets::{PlacementId, Widget};
use std::sync::Arc;

/// The most issues looked at for the newest one of a week.
const ISSUE_LOOK: u32 = 200;

/// The values of the asked placements of the popup layout, in the layout's order. Placements that are not in the layout
/// or that are live (the app builds those) are left out.
pub fn values(shared: &Arc<Shared>, asked: &[PlacementId]) -> Result<Vec<PopoverState>, AgentError> {
    let t = now();
    let (placements, pinned) = {
        let core = lock(&shared.core);
        (
            core.settings.popover.layout().placements.clone(),
            core.settings.counters.pinned.clone(),
        )
    };
    let wanted: Vec<_> = placements
        .iter()
        .filter(|p| asked.contains(&p.id) && !is_live_push(p.widget.kind()))
        .collect();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let has = |k: PopoverKind| wanted.iter().any(|p| p.widget.kind() == k);

    let alarms = if has(PopoverKind::ActiveAlarms) {
        Source::Ready(lock(&shared.core).alarms.list(&AlarmFilter::default(), t))
    } else {
        Source::NotYet
    };
    let volumes = if has(PopoverKind::DiskSpace) {
        // The cached list only: never a listing on the request. Before the first one finishes the widget is loading.
        shared.disk.cached_volumes().map_or(Source::NotYet, Source::Ready)
    } else {
        Source::NotYet
    };
    let (issues, reading, scanned_at) = if has(PopoverKind::LastIssue) {
        let list = lock(&shared.core).store.issues(t.minus(ISSUE_DAYS * DAY), t, ISSUE_LOOK);
        let reading = job_progress(shared, JobKind::RescanIssues).map(|j| j.done);
        let scanned_at = shared.issues.snapshot().1;
        match list {
            Ok(l) => (Source::Ready(l), reading, scanned_at),
            Err(e) => (Source::Failed(e.to_string()), reading, scanned_at),
        }
    } else {
        (Source::NotYet, None, None)
    };
    let (readings, read_at) = if has(PopoverKind::PinnedCounters) {
        shared.counters.latest(&pinned, t)
    } else {
        (Vec::new(), None)
    };

    let minute = Minute::default();
    let mut inputs = Inputs::empty(t, &minute);
    inputs.alarms = alarms.as_slice();
    inputs.volumes = volumes.as_slice();
    inputs.issues = match &issues {
        Source::Ready(l) => Source::Ready(IssuesInput {
            recent: l,
            reading,
            scanned_at,
        }),
        Source::Failed(c) => Source::Failed(c.clone()),
        Source::NotYet => Source::NotYet,
    };
    if has(PopoverKind::PinnedCounters) {
        inputs.counters = Source::Ready(CountersInput {
            pinned: pinned.len(),
            readings: &readings,
            read_at,
        });
    }
    Ok(wanted
        .into_iter()
        .filter_map(|p| {
            state(&p.widget, &p.id, &inputs).map(|s| PopoverState {
                id: p.id.clone(),
                state: s,
            })
        })
        .collect())
}

/// The running job of `kind`'s own phase and count; none when no such job runs or it has not started a phase (no count is
/// invented for a job that reports none, ER-18).
fn job_progress(shared: &Shared, kind: JobKind) -> Option<JobProgress> {
    shared
        .jobs
        .statuses()
        .into_iter()
        .find(|s| s.kind == kind.slug() && !s.is_terminal())
        .and_then(|s| match s.state {
            WireState::Running(Some(phase)) => Some(JobProgress {
                phase: phase.label,
                done: phase.done,
                unit: match phase.unit {
                    paddock_jobs::Unit::Bytes => "bytes",
                    _ => "items",
                }
                .into(),
            }),
            _ => None,
        })
}
