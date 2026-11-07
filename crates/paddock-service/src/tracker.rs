//! The repair's progress, shared between the thread that repairs and the commands that report it: one repair at a time,
//! the current phase in words, a success that fades and a failure that stays until the next repair.

use crate::repair::{RepairOutcome, Step};
use serde::Serialize;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// How long "running again" stays on screen.
pub const SUCCESS_SHOWN: Duration = Duration::from_secs(120);
/// What the UI shows while the repair works and before its first phase.
pub const WORKING_MESSAGE: &str = "Repairing the background service… (this can take a minute)";
/// What the UI shows when it worked.
pub const SUCCESS_MESSAGE: &str = "The background service is running again.";

/// Where the repair is.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepairState {
    /// Working.
    Working,
    /// Done: registered (and answering, unless the message says otherwise).
    Succeeded,
    /// Did not work; the message has next steps.
    Failed,
}

/// What the UI shows about the repair.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairNotice {
    /// Where the repair is.
    pub state: RepairState,
    /// The words to show.
    pub message: String,
    /// The current phase while working.
    pub step: Option<String>,
}

/// The progress of the repairs of one service. Times are readings of the caller's monotonic [`paddock_health::Clock`] (the
/// tracker never reads a clock itself, so a test or a model passes the time it wants).
#[derive(Debug)]
pub struct RepairTracker {
    inner: Mutex<Option<(RepairNotice, Duration)>>,
}

impl Default for RepairTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl RepairTracker {
    /// Nothing happened yet.
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(RepairNotice, Duration)>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a repair unless one is working; `false` when one is.
    pub fn start(&self, now: Duration) -> bool {
        let mut g = self.lock();
        if g.as_ref()
            .is_some_and(|(n, _)| n.state == RepairState::Working)
        {
            return false;
        }
        *g = Some((
            RepairNotice {
                state: RepairState::Working,
                message: WORKING_MESSAGE.to_string(),
                step: None,
            },
            now,
        ));
        true
    }

    /// Whether a repair is working.
    pub fn working(&self) -> bool {
        self.lock()
            .as_ref()
            .is_some_and(|(n, _)| n.state == RepairState::Working)
    }

    /// The repair reached `step`.
    pub fn step(&self, step: &Step) {
        if let Some((n, _)) = self.lock().as_mut()
            && n.state == RepairState::Working
        {
            n.step = Some(step.to_string());
        }
    }

    /// The repair ended with `outcome`.
    pub fn finish(&self, outcome: &RepairOutcome, now: Duration) {
        let (state, message) = match outcome {
            RepairOutcome::Recovered => (RepairState::Succeeded, SUCCESS_MESSAGE.to_string()),
            RepairOutcome::Registered => (
                RepairState::Succeeded,
                "The background service is registered; it has not answered yet and may still start.".to_string(),
            ),
            RepairOutcome::NeedsApproval => (
                RepairState::Failed,
                "macOS needs your approval: allow the app in System Settings > General > Login Items.".to_string(),
            ),
            RepairOutcome::NeedsElevation => (
                RepairState::Failed,
                "This needs administrator rights; confirm Windows' permission prompt.".to_string(),
            ),
            RepairOutcome::Declined => (
                RepairState::Failed,
                "The permission prompt was declined; nothing was changed.".to_string(),
            ),
            RepairOutcome::Cancelled => (RepairState::Failed, "The repair was stopped; nothing was changed.".to_string()),
            RepairOutcome::Unavailable(why) | RepairOutcome::Failed(why) => (RepairState::Failed, why.clone()),
        };
        *self.lock() = Some((
            RepairNotice {
                state,
                message,
                step: None,
            },
            now,
        ));
    }

    /// The notice to show now: a success fades after [`SUCCESS_SHOWN`]; working and failure stay.
    pub fn current(&self, now: Duration) -> Option<RepairNotice> {
        let g = self.lock();
        let (notice, since) = g.as_ref()?;
        if notice.state == RepairState::Succeeded && now.saturating_sub(*since) > SUCCESS_SHOWN {
            return None;
        }
        Some(notice.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_repair_at_a_time_with_its_phase() {
        let t = RepairTracker::default();
        let t0 = Duration::ZERO;
        assert!(!t.working());
        t.step(&Step::Removing);
        assert_eq!(t.current(t0), None, "a phase without a repair is ignored");
        assert!(t.start(t0));
        assert!(!t.start(t0));
        assert!(t.working());
        t.step(&Step::Removing);
        let n = t.current(t0).unwrap();
        assert_eq!(
            (n.state, n.message.as_str(), n.step.as_deref()),
            (
                RepairState::Working,
                WORKING_MESSAGE,
                Some("Removing the old registration")
            )
        );
        assert!(
            t.current(t0 + Duration::from_secs(3600)).is_some(),
            "working never fades"
        );
        t.finish(&RepairOutcome::Failed("no".into()), t0);
        t.step(&Step::Registering);
        assert_eq!(
            t.current(t0).unwrap().step,
            None,
            "a phase after the end is ignored"
        );
        assert!(t.start(t0), "a finished repair can be started again");
    }

    #[test]
    fn a_success_fades_and_a_failure_stays() {
        let t = RepairTracker::new();
        let t0 = Duration::ZERO;
        t.finish(&RepairOutcome::Recovered, t0);
        assert_eq!(
            t.current(t0 + Duration::from_secs(5)).unwrap().message,
            SUCCESS_MESSAGE
        );
        assert_eq!(t.current(t0 + SUCCESS_SHOWN + Duration::from_secs(1)), None);
        for (o, needle) in [
            (RepairOutcome::Registered, "may still start"),
            (RepairOutcome::NeedsApproval, "Login Items"),
            (RepairOutcome::NeedsElevation, "administrator"),
            (RepairOutcome::Declined, "declined"),
            (RepairOutcome::Cancelled, "stopped"),
            (RepairOutcome::Unavailable("move it".into()), "move it"),
            (RepairOutcome::Failed("why".into()), "why"),
        ] {
            t.finish(&o, t0);
            assert!(t.current(t0).unwrap().message.contains(needle), "{o:?}");
        }
        assert!(t.current(t0 + Duration::from_secs(3600)).is_some());
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let t = std::sync::Arc::new(RepairTracker::new());
        let t2 = t.clone();
        let _ = std::thread::spawn(move || {
            let _g = t2.inner.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(t.start(Duration::ZERO));
    }

    #[test]
    fn the_notice_serialises_in_camel_case() {
        let json = serde_json::to_string(&RepairNotice {
            state: RepairState::Working,
            message: "m".into(),
            step: None,
        })
        .unwrap();
        assert_eq!(json, r#"{"state":"working","message":"m","step":null}"#);
    }
}
