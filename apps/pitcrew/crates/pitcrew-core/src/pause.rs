//! Pause windows (ADR 0013): runners that do not take jobs at certain times, or while the machine is busy with other
//! work. Both are carried out by draining (see [`crate::hold`]): a running job always finishes first.
//!
//! - **Schedule**: the runners are held while any window is open (local time; a window may span midnight).
//! - **When idle**: the runners work only when the machine is otherwise quiet. The machine counts as busy while the load
//!   *outside* the runners is at or above a threshold, and stays busy for a quiet period after it drops.

use crate::update::{LocalMoment, UpdateWindow};
use serde::{Deserialize, Serialize};

/// Most windows kept.
pub const MAX_WINDOWS: usize = 14;

/// What pauses the runners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PauseMode {
    /// Never.
    #[default]
    Off,
    /// While a window is open.
    Schedule,
    /// While the machine is busy with other work.
    WhenIdle,
}

/// The pause settings (defaults for every runner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PausePolicy {
    /// What pauses the runners.
    pub mode: PauseMode,
    /// The windows in which runners take no jobs (used by [`PauseMode::Schedule`]; the end may be before the start).
    pub windows: Vec<UpdateWindow>,
    /// The machine is busy when the load outside the runners is at least this many percent of all cores.
    pub busy_percent: u8,
    /// And stays busy this many minutes after the load drops.
    pub quiet_minutes: u32,
}

impl Default for PausePolicy {
    fn default() -> Self {
        PausePolicy {
            mode: PauseMode::Off,
            windows: Vec::new(),
            busy_percent: 40,
            quiet_minutes: 5,
        }
    }
}

impl PausePolicy {
    /// Checks the values, with a message for the user.
    pub fn validate(&self) -> Result<(), String> {
        if self.windows.len() > MAX_WINDOWS {
            return Err(format!("at most {MAX_WINDOWS} pause windows are allowed"));
        }
        if !(5..=100).contains(&self.busy_percent) {
            return Err("the busy threshold must be 5-100 percent".into());
        }
        if !(1..=240).contains(&self.quiet_minutes) {
            return Err("the quiet period must be 1-240 minutes".into());
        }
        Ok(())
    }

    /// Whether a window is open at `now` (always false unless the mode is [`PauseMode::Schedule`]).
    pub fn schedule_holds(&self, now: LocalMoment) -> bool {
        self.mode == PauseMode::Schedule && self.windows.iter().any(|w| w.contains(now))
    }

    /// When the current window ends, in minutes from `now` (to show "paused until"); `None` when none is open.
    pub fn minutes_until_open(&self, now: LocalMoment) -> Option<u32> {
        if !self.schedule_holds(now) {
            return None;
        }
        Some(
            (1..=1440u32)
                .find(|m| {
                    let t = (u32::from(now.time.minutes()) + m) % 1440;
                    let day = if u32::from(now.time.minutes()) + m >= 1440 {
                        next(now.weekday)
                    } else {
                        now.weekday
                    };
                    let at = LocalMoment {
                        weekday: day,
                        time: crate::update::TimeOfDay::new((t / 60) as u16, (t % 60) as u16).unwrap_or(now.time),
                    };
                    !self.windows.iter().any(|w| w.contains(at))
                })
                .unwrap_or(1440),
        )
    }
}

fn next(d: crate::update::Weekday) -> crate::update::Weekday {
    use crate::update::Weekday::*;
    match d {
        Mon => Tue,
        Tue => Wed,
        Wed => Thu,
        Thu => Fri,
        Fri => Sat,
        Sat => Sun,
        Sun => Mon,
    }
}

/// Follows the load outside the runners.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdleTracker {
    last_busy_ms: Option<u64>,
}

impl IdleTracker {
    /// Records a reading: `other_permille` is the machine's load minus the runners' share (per mille of all cores).
    pub fn observe(&mut self, policy: &PausePolicy, now_ms: u64, other_permille: u32) {
        if other_permille >= u32::from(policy.busy_percent) * 10 {
            self.last_busy_ms = Some(now_ms);
        }
    }

    /// Whether the machine counts as busy at `now_ms` (busy in the last `quiet_minutes`).
    pub fn busy(&self, policy: &PausePolicy, now_ms: u64) -> bool {
        policy.mode == PauseMode::WhenIdle
            && self
                .last_busy_ms
                .is_some_and(|t| now_ms.saturating_sub(t) < u64::from(policy.quiet_minutes) * 60_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::{TimeOfDay, Weekday};

    fn at(w: Weekday, h: u16, m: u16) -> LocalMoment {
        LocalMoment {
            weekday: w,
            time: TimeOfDay::new(h, m).unwrap(),
        }
    }

    fn night() -> PausePolicy {
        PausePolicy {
            mode: PauseMode::Schedule,
            windows: vec![UpdateWindow {
                days: vec![],
                start: TimeOfDay::new(22, 0).unwrap(),
                end: TimeOfDay::new(6, 0).unwrap(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_window_across_midnight_holds_and_other_modes_never_do() {
        let p = night();
        assert!(p.schedule_holds(at(Weekday::Mon, 23, 0)));
        assert!(p.schedule_holds(at(Weekday::Tue, 5, 59)));
        assert!(!p.schedule_holds(at(Weekday::Tue, 12, 0)));
        assert!(!PausePolicy {
            mode: PauseMode::Off,
            ..p.clone()
        }
        .schedule_holds(at(Weekday::Mon, 23, 0)));
        assert!(!PausePolicy {
            mode: PauseMode::WhenIdle,
            ..p
        }
        .schedule_holds(at(Weekday::Mon, 23, 0)));
    }

    #[test]
    fn minutes_until_the_window_closes() {
        let p = night();
        assert_eq!(p.minutes_until_open(at(Weekday::Mon, 23, 0)), Some(7 * 60));
        assert_eq!(p.minutes_until_open(at(Weekday::Tue, 5, 30)), Some(30));
        assert_eq!(p.minutes_until_open(at(Weekday::Tue, 12, 0)), None);
        let always = PausePolicy {
            mode: PauseMode::Schedule,
            windows: vec![UpdateWindow {
                days: vec![],
                start: TimeOfDay::new(0, 0).unwrap(),
                end: TimeOfDay::new(0, 0).unwrap(),
            }],
            ..Default::default()
        };
        assert_eq!(always.minutes_until_open(at(Weekday::Mon, 1, 0)), Some(1440), "capped at a day");
    }

    #[test]
    fn busy_means_load_outside_the_runners_and_lingers_for_the_quiet_period() {
        let p = PausePolicy {
            mode: PauseMode::WhenIdle,
            busy_percent: 40,
            quiet_minutes: 5,
            ..Default::default()
        };
        let mut t = IdleTracker::default();
        assert!(!t.busy(&p, 0), "unknown is idle: the runner is not held at start-up");
        t.observe(&p, 1_000, 399);
        assert!(!t.busy(&p, 1_000));
        t.observe(&p, 2_000, 400);
        assert!(t.busy(&p, 2_000));
        assert!(t.busy(&p, 2_000 + 299_999));
        assert!(!t.busy(&p, 2_000 + 300_000));
        assert!(
            !t.busy(&PausePolicy { mode: PauseMode::Off, ..p }, 2_000),
            "only the idle mode uses it"
        );
    }

    #[test]
    fn values_are_validated() {
        assert!(PausePolicy::default().validate().is_ok());
        assert!(PausePolicy {
            busy_percent: 1,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(PausePolicy {
            quiet_minutes: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        let many = PausePolicy {
            windows: vec![night().windows[0].clone(); MAX_WINDOWS + 1],
            ..Default::default()
        };
        assert!(many.validate().unwrap_err().contains("at most"));
    }
}
