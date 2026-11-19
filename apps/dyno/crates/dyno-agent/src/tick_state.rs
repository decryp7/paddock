//! What a sampler tick changes in memory besides the store, kept only when the tick's writes are stored (review of the ADR 0108
//! DY-7 change, where the class showed up three times: the history writer, the alarm engine and the alert cooldowns each moved
//! ahead of a store that had rolled back). [`TickState::of`] takes the state before the tick's batch; [`TickState::restore`] puts it
//! back when [`crate::store::Batch::commit`] fails, so the next tick sees what the store has and announces it again.
//!
//! It holds the history writer (raw-point mark, the minute's app sums, the prune mark), the alarm engine, the alert evaluator
//! (what is building up and the cooldowns), the custom-rule evaluator and the flight recorder's last automatic capture, which
//! together are a few small maps; the recorder's sample ring is not part of it (a tick only appends to it, and the samples are
//! true whether or not they were stored).

use crate::agent::Core;

/// The in-memory state a tick changes, as it was before the tick.
pub struct TickState {
    writer: crate::rollup::HistoryWriter,
    alerts: dyno_core::alerts::Alerts,
    custom: dyno_core::alarms::custom::CustomEvaluator,
    recorder_auto: Option<dyno_core::time::Timestamp>,
    /// The alarm engine, copied only by a tick that has alarm work ([`TickState::cover_alarms`]): it can hold
    /// `LOAD_LIMIT` alarms, and most ticks change none (third review D2; apps/dyno/docs/performance.md).
    alarms: Option<crate::alarms::AlarmCentre>,
}

impl TickState {
    /// The state of `core` now, before the tick writes anything: the small parts always.
    pub fn of(core: &Core) -> TickState {
        TickState {
            writer: core.writer.clone(),
            alerts: core.alerts.clone(),
            custom: core.custom.clone(),
            recorder_auto: core.recorder.auto_mark(),
            alarms: None,
        }
    }

    /// Adds the alarm engine as it is now; called before the tick's first alarm change, and only when it has one.
    pub fn cover_alarms(&mut self, core: &Core) {
        if self.alarms.is_none() {
            self.alarms = Some(core.alarms.clone());
        }
    }

    /// Whether the alarm engine was copied (tests and the measurement).
    pub fn covers_alarms(&self) -> bool {
        self.alarms.is_some()
    }

    /// Puts `core` back as it was: the tick's writes were not stored.
    pub fn restore(self, core: &mut Core) {
        core.writer = self.writer;
        core.alerts = self.alerts;
        core.custom = self.custom;
        core.recorder.restore_auto_mark(self.recorder_auto);
        if let Some(alarms) = self.alarms {
            core.alarms = alarms;
        }
    }
}
