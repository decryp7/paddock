//! The supervisor's holds: draining a runner by itself when the disk is nearly full or a pause window is open, and
//! resuming it when the reason is gone (ADR 0013). The decisions are `pitcrew_core::{hold, disk_guard, pause}`'s.

use super::*;
use pitcrew_core::disk_guard::{self, DiskLevel};
use pitcrew_core::hold::{wanted_reasons, HoldStep};
use pitcrew_core::pause::IdleTracker;
use pitcrew_core::protocol::HoldInfo;

/// The machine-wide part of the hold logic: how busy the machine is outside the runners.
#[derive(Default)]
pub(super) struct Holds {
    pub(super) idle: IdleTracker,
}

impl Supervisor {
    /// Once per probe tick: works out which reasons apply to `name` and drains or resumes it. The free space comes from
    /// the cache, so the OS is asked at most once a minute however often this runs.
    pub(super) fn tick_hold(&mut self, name: &RunnerName, now: Timestamp) {
        let Some(r) = self.runners.get(name) else { return };
        let guard = self.config.defaults.disk_guard;
        let dir = r.cfg.dir.clone();
        let free = self.probe.disk_space(&dir, now).map(|(f, _)| f);
        let pause = &self.config.defaults.pause;
        let Some(r) = self.runners.get_mut(name) else { return };
        r.disk_hold = disk_guard::hold_wanted(&guard, free, r.disk_hold);
        let (moment, _) = (self.env.local)(now);
        let schedule = pause.schedule_holds(moment);
        let busy = self.holds.idle.busy(pause, now.0);
        let reasons = wanted_reasons(r.disk_hold, schedule, busy);
        r.hold_reasons = reasons.clone();
        r.hold_minutes_left = if schedule { pause.minutes_until_open(moment) } else { None };
        if disk_guard::level(&guard, free) == DiskLevel::Critical && guard.enabled {
            r.clean_wanted = true;
        }
        let drained = r.ctl.paused_or_draining();
        // A runner the user stopped, or one in a crash loop, is none of the hold's business.
        let step = if r.ctl.wanted() || drained {
            r.hold.step(!reasons.is_empty(), drained)
        } else {
            HoldStep::Nothing
        };
        let Some(reason) = reasons.first().copied().or(r.last_hold_reason) else {
            return;
        };
        match step {
            HoldStep::Nothing => {}
            HoldStep::Drain => {
                r.last_hold_reason = Some(reason);
                let actions = r.ctl.handle(Input::Drain, now);
                r.log
                    .supervisor(now, &format!("held: {}; finishing the current job, then pausing", reason.words()));
                self.act(name, actions, now);
                self.record(name, HistoryKind::Held { reason }, now);
                self.notify(notify::held(name, reason, free, now));
            }
            HoldStep::Resume => {
                let actions = r.ctl.handle(Input::Resume, now);
                r.log
                    .supervisor(now, "released: the reason for the hold is gone; taking jobs again");
                r.last_hold_reason = None;
                self.act(name, actions, now);
                self.record(name, HistoryKind::Released { reason }, now);
                self.notify(notify::released(name, reason, now));
            }
        }
        if let Some(r) = self.runners.get(name) {
            let applied = r.hold;
            let st = self.state.runners.entry(name.to_string()).or_default();
            if st.hold != applied {
                st.hold = applied;
                self.dirty = true;
            }
        }
    }

    /// The load outside the runners, from a fresh machine sample (per mille).
    pub(super) fn observe_idle(&mut self, now: Timestamp, cpu_permille: u32, runners_permille: u32) {
        let other = cpu_permille.saturating_sub(runners_permille);
        self.holds.idle.observe(&self.config.defaults.pause, now.0, other);
    }

    /// A user's own start/stop/pause/resume takes the runner out of Pitcrew's hold.
    pub(super) fn user_acted(&mut self, runner: &RunnerName, resumes: bool) {
        if let Some(r) = self.runners.get_mut(runner) {
            r.hold.user_acted(resumes);
        }
    }

    /// The hold for the snapshot.
    pub(super) fn hold_info(&self, r: &Runner) -> Option<HoldInfo> {
        (!r.hold_reasons.is_empty()).then(|| HoldInfo {
            reasons: r.hold_reasons.clone(),
            paused: r.ctl.life() == Life::Paused,
            minutes_left: r.hold_minutes_left,
        })
    }
}
