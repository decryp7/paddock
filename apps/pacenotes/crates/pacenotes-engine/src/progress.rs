//! The job reporter as the index's progress port.

use pacenotes_index::Progress;
use paddock_jobs::{JobContext, Phase, UncancellableGuard};
use std::sync::Mutex;

/// The phase that swaps the new index in: it cannot be interrupted half-way, so the job says so while it runs.
const SAVE_PHASE: &str = "save";

/// A [`Progress`] over a running job. A phase that repeats the current one does not restart it (progress never goes
/// backwards); the "save" phase marks the job as not cancellable until the next phase or the end.
pub struct JobProgress<'a> {
    ctx: &'a JobContext,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    current: Option<&'static str>,
    /// What the current phase has counted and what it expects: the total is raised whenever the count passes it, so a bar never
    /// shows more done than the whole.
    done: u64,
    total: Option<u64>,
    guard: Option<UncancellableGuard>,
}

impl<'a> JobProgress<'a> {
    /// Reports to `ctx`.
    pub fn new(ctx: &'a JobContext) -> JobProgress<'a> {
        JobProgress {
            ctx,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Progress for JobProgress<'_> {
    fn phase(&self, id: &'static str, label: &'static str, total: Option<u64>) {
        let mut state = self.state();
        if state.current == Some(id) {
            if let Some(total) = total {
                state.total = Some(total.max(state.done));
                self.ctx.set_total(total.max(state.done));
            }
            return;
        }
        state.guard = None;
        state.current = Some(id);
        state.done = 0;
        state.total = total;
        self.ctx.phase(match total {
            Some(total) => Phase::items(id, label, total),
            None => Phase::discovering(id, label),
        });
        if id == SAVE_PHASE {
            state.guard = Some(self.ctx.uncancellable("The index is being swapped in"));
        }
    }

    fn add(&self, n: u64) {
        let mut state = self.state();
        state.done += n;
        if let Some(total) = state.total
            && state.done > total
        {
            state.total = Some(state.done);
            self.ctx.set_total(state.done);
        }
        self.ctx.add(n);
    }

    fn set_total(&self, total: u64) {
        let mut state = self.state();
        let total = total.max(state.done);
        state.total = Some(total);
        self.ctx.set_total(total);
    }
}
