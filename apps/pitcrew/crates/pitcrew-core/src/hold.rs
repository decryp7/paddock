//! Holds: Pitcrew pauses a runner by itself (drain) for a reason and resumes it when the reason is gone, without ever
//! touching a pause the user made (low disk and pause windows, ADR 0013).
//!
//! The decision is a small pure state machine, [`HoldTracker`], fed once per tick with whether any hold is wanted and
//! whether the runner is drained now. The user always wins: pausing or resuming by hand takes the runner out of
//! Pitcrew's hands until the reason has gone away.

use serde::{Deserialize, Serialize};

/// Why Pitcrew holds a runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HoldReason {
    /// The runner's disk is nearly full.
    LowDisk,
    /// A pause window (schedule) is open.
    Schedule,
    /// The machine is busy with other work (only-when-idle).
    Busy,
}

impl HoldReason {
    /// The reason in words for the history and notifications.
    pub fn words(self) -> &'static str {
        match self {
            HoldReason::LowDisk => "the disk is nearly full",
            HoldReason::Schedule => "a pause window is open",
            HoldReason::Busy => "the machine is busy with other work",
        }
    }
}

/// The hold reasons wanted now, in a fixed order (the first is the headline reason).
pub fn wanted_reasons(low_disk: bool, schedule: bool, busy: bool) -> Vec<HoldReason> {
    let mut v = Vec::new();
    if low_disk {
        v.push(HoldReason::LowDisk);
    }
    if schedule {
        v.push(HoldReason::Schedule);
    }
    if busy {
        v.push(HoldReason::Busy);
    }
    v
}

/// What the service should do for this runner now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldStep {
    /// Nothing.
    Nothing,
    /// Drain the runner (finish the job, then pause).
    Drain,
    /// Resume the runner.
    Resume,
}

/// Tracks whether the current drain is Pitcrew's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HoldTracker {
    /// Pitcrew drained the runner for a reason and will resume it.
    pub applied: bool,
    /// The user overrode the hold; Pitcrew leaves the runner alone until the reason is gone.
    pub suppressed: bool,
}

impl HoldTracker {
    /// The next step. `wanted`: some hold reason applies; `drained`: the runner is paused or draining.
    pub fn step(&mut self, wanted: bool, drained: bool) -> HoldStep {
        if self.applied {
            if !drained {
                // The user resumed (or started) it: their choice stands until the reason goes away.
                self.applied = false;
                self.suppressed = wanted;
                return HoldStep::Nothing;
            }
            if !wanted {
                self.applied = false;
                return HoldStep::Resume;
            }
            return HoldStep::Nothing;
        }
        if !wanted {
            self.suppressed = false;
            return HoldStep::Nothing;
        }
        if !drained && !self.suppressed {
            self.applied = true;
            return HoldStep::Drain;
        }
        HoldStep::Nothing
    }

    /// The user paused, resumed, started, stopped or restarted the runner by hand: the pause is theirs now.
    pub fn user_acted(&mut self, resumed: bool) {
        self.applied = false;
        if resumed {
            self.suppressed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drains_when_wanted_and_resumes_when_the_reason_is_gone() {
        let mut t = HoldTracker::default();
        assert_eq!(t.step(false, false), HoldStep::Nothing);
        assert_eq!(t.step(true, false), HoldStep::Drain);
        assert_eq!(t.step(true, true), HoldStep::Nothing, "no second drain while it drains");
        assert_eq!(t.step(false, true), HoldStep::Resume);
        assert_eq!(t.step(false, false), HoldStep::Nothing);
    }

    #[test]
    fn a_pause_by_the_user_is_never_resumed_by_pitcrew() {
        let mut t = HoldTracker::default();
        assert_eq!(t.step(true, true), HoldStep::Nothing, "already paused by the user");
        assert_eq!(t.step(false, true), HoldStep::Nothing);
        // The user pauses while a hold of ours is on: it is theirs now.
        let mut t = HoldTracker::default();
        assert_eq!(t.step(true, false), HoldStep::Drain);
        t.user_acted(false);
        assert_eq!(t.step(false, true), HoldStep::Nothing);
    }

    #[test]
    fn resuming_by_hand_overrides_the_hold_until_the_reason_is_gone() {
        let mut t = HoldTracker::default();
        assert_eq!(t.step(true, false), HoldStep::Drain);
        t.user_acted(true);
        assert_eq!(t.step(true, false), HoldStep::Nothing, "suppressed");
        assert_eq!(t.step(true, false), HoldStep::Nothing);
        assert_eq!(t.step(false, false), HoldStep::Nothing);
        assert_eq!(t.step(true, false), HoldStep::Drain, "a new episode holds again");
        // Resumed without telling the tracker (for example a start from the tray): detected from the state.
        assert_eq!(t.step(true, false), HoldStep::Nothing);
        assert_eq!(t.step(true, false), HoldStep::Nothing);
    }

    #[test]
    fn reasons_are_ordered_and_have_words() {
        assert_eq!(
            wanted_reasons(true, true, true),
            vec![HoldReason::LowDisk, HoldReason::Schedule, HoldReason::Busy]
        );
        assert!(wanted_reasons(false, false, false).is_empty());
        assert!(HoldReason::LowDisk.words().contains("disk"));
        assert!(HoldReason::Schedule.words().contains("window"));
        assert!(HoldReason::Busy.words().contains("busy"));
    }
}
