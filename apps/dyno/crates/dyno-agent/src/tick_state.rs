//! What a sampler tick changes in memory besides the store, kept only when the tick's commit succeeds (review of the ADR 0108
//! DY-7 change, where the class showed up three times: the history writer, the alarm engine and the alert cooldowns each moved
//! ahead of a store that had rolled back). [`TickState::of`] takes the state before the tick's batch; [`TickState::restore`] puts it
//! back when [`crate::store::Batch::commit`] fails, so the next tick sees what the store has and announces it again.
//!
//! It holds the history writer (raw-point mark, the minute's app sums, the prune mark), the alert evaluator (what is building up
//! and the cooldowns), the custom-rule evaluator and the flight recorder's last automatic capture, which together are a few
//! small maps; the alarm engine is not copied but read back from the store after a failed commit, and only when the tick had
//! alarm work. A single writer that fails inside the tick undoes only its own state (Dyno ADR 0008, "Store writers"); this is
//! the undo for the commit itself failing. The recorder's sample ring is not part of it (a tick only appends to it, and the
//! samples are true whether or not they were stored).

use crate::agent::Core;

/// The in-memory state a tick changes, as it was before the tick.
pub struct TickState {
    writer: crate::rollup::HistoryWriter,
    alerts: dyno_core::alerts::Alerts,
    custom: dyno_core::alarms::custom::CustomEvaluator,
    recorder_auto: Option<dyno_core::time::Timestamp>,
    /// The tick had alarm work: on a failed commit the alarm engine is read back from the store, which then holds what the
    /// tick started from. No copy is taken (it can hold `LOAD_LIMIT` alarms; apps/dyno/docs/performance.md).
    alarms_touched: bool,
}

impl TickState {
    /// The state of `core` now, before the tick writes anything.
    pub fn of(core: &Core) -> TickState {
        TickState {
            writer: core.writer.clone(),
            alerts: core.alerts.clone(),
            custom: core.custom.clone(),
            recorder_auto: core.recorder.auto_mark(),
            alarms_touched: false,
        }
    }

    /// The tick is about to change alarms.
    pub fn touch_alarms(&mut self) {
        self.alarms_touched = true;
    }

    /// Whether the tick had alarm work (tests and the measurement).
    pub fn touched_alarms(&self) -> bool {
        self.alarms_touched
    }

    /// Puts `core` back as it was, after the tick's commit failed and was rolled back.
    pub fn restore(self, core: &mut Core) {
        core.writer = self.writer;
        core.alerts = self.alerts;
        core.custom = self.custom;
        core.recorder.restore_auto_mark(self.recorder_auto);
        if self.alarms_touched {
            let crate::agent::Core { alarms, store, .. } = core;
            alarms.reload(store);
        }
    }
}
