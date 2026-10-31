//! The tray popup's slow values (family ADR 0035 §5): alarms, disk space, backup, the last issue and the pinned
//! counters, built by `dyno_core::widgets::popover_values` from state the agent already keeps. Nothing here reads the OS or
//! the disk on a request: volumes come from the cached list, the backup report is the last one, counters are the newest
//! kept readings and issues come from the index.

use crate::agent::{lock, now, Shared};
use dyno_core::alarms::AlarmFilter;
use dyno_core::jobs::JobKind;
use dyno_core::protocol::AgentError;
use dyno_core::time::DAY;
use dyno_core::widgets::popover_values::{is_live_push, state, CountersInput, Inputs, Minute, PopoverState, Source, ISSUE_DAYS};
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
        Source::Ready(shared.disk.volumes())
    } else {
        Source::NotYet
    };
    let backup = if has(PopoverKind::BackupHealth) {
        Source::Ready(crate::extras::backup_check::last_report(shared))
    } else {
        Source::NotYet
    };
    let (issues, reading) = if has(PopoverKind::LastIssue) {
        let list = lock(&shared.core).store.issues(t.minus(ISSUE_DAYS * DAY), t, ISSUE_LOOK);
        let reading = reading_logs(shared);
        match list {
            Ok(l) => (Source::Ready(l), reading),
            Err(e) => (Source::Failed(e.to_string()), reading),
        }
    } else {
        (Source::NotYet, None)
    };
    let readings = if has(PopoverKind::PinnedCounters) {
        shared.counters.latest(&pinned, t)
    } else {
        Vec::new()
    };

    let minute = Minute::default();
    let mut inputs = Inputs::empty(t, &minute);
    inputs.alarms = alarms.as_slice();
    inputs.volumes = volumes.as_slice();
    inputs.backup = backup.as_ref();
    inputs.issues = match &issues {
        Source::Ready(l) => Source::Ready(dyno_core::widgets::popover_values::IssuesInput { recent: l, reading }),
        Source::Failed(c) => Source::Failed(c.clone()),
        Source::NotYet => Source::NotYet,
    };
    if has(PopoverKind::PinnedCounters) {
        inputs.counters = Source::Ready(CountersInput {
            pinned: pinned.len(),
            readings: &readings,
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

/// Events read so far while the issue job runs (0 before its first phase), else none.
fn reading_logs(shared: &Shared) -> Option<u64> {
    shared
        .jobs
        .statuses()
        .into_iter()
        .find(|s| s.kind == JobKind::RescanIssues.slug() && !s.is_terminal())
        .map(|s| match s.state {
            WireState::Running(Some(phase)) => phase.done,
            _ => 0,
        })
}
