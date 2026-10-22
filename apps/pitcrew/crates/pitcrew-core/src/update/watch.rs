//! Keeping an eye on updates (ADR 0014): how long a runner has been behind the latest release (a countdown to the
//! owner's own deadline), and updates that go round in circles.
//!
//! **There is no server-enforced deadline for Gitea or Forgejo runners.** Unlike GitHub's hosted runner service (which
//! asks self-hosted runners to stay within 30 days of a release), neither Gitea's `act_runner` nor Forgejo's runner
//! documents a minimum version or a time limit after which the server refuses it (checked 2026-10-23; the servers keep
//! speaking the older runner protocol, and breaking changes arrive as ordinary release notes, e.g. Forgejo runner 12
//! needs `git`). So the countdown is the owner's own policy: `deadline_days` in the update policy, "remind me when a
//! runner has been behind for this long". It starts when Pitcrew first saw a newer release than the installed one.
//!
//! Loop detection reads the update outcomes in the history: the same target failing or rolling back again and again,
//! being installed again and again without ever sticking, or being postponed round after round.

use crate::time::Timestamp;
use crate::update::machine::UpdateOutcome;
use crate::version::RunnerVersion;
use serde::{Deserialize, Serialize};

const DAY_MS: u64 = 86_400_000;
/// The warning starts this many days before the deadline.
pub const SOON_DAYS: u64 = 7;
/// Outcomes older than this are not part of a loop.
pub const LOOP_WINDOW_DAYS: u64 = 7;
/// Failed or rolled back this many times in the window: a loop.
pub const FAILING_ATTEMPTS: usize = 3;
/// Installed this many times in the window (it does not stick): a loop.
pub const NOT_STICKING_ATTEMPTS: usize = 3;
/// Postponed this many times in the window: a loop.
pub const POSTPONED_ATTEMPTS: usize = 5;

/// Where a runner is against its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeadlineState {
    /// Plenty of time.
    Ok,
    /// Within [`SOON_DAYS`] days.
    Soon,
    /// Past the deadline.
    Overdue,
}

/// The countdown for a runner that is behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deadline {
    /// Since when a newer release has been available.
    pub behind_since: Timestamp,
    /// The deadline.
    pub due_at: Timestamp,
    /// Whole days left (negative once overdue).
    pub days_left: i64,
    /// The state.
    pub state: DeadlineState,
}

/// The deadline of a runner behind since `behind_since` (`None`: it is not behind, or `deadline_days` is 0).
pub fn deadline(deadline_days: u32, behind_since: Option<Timestamp>, now: Timestamp) -> Option<Deadline> {
    if deadline_days == 0 {
        return None;
    }
    let since = behind_since?;
    let due = since.0.saturating_add(u64::from(deadline_days) * DAY_MS);
    let days_left = if due >= now.0 {
        i64::try_from((due - now.0) / DAY_MS).unwrap_or(i64::MAX)
    } else {
        -i64::try_from((now.0 - due).div_ceil(DAY_MS)).unwrap_or(i64::MAX)
    };
    let state = if now.0 >= due {
        DeadlineState::Overdue
    } else if due - now.0 <= SOON_DAYS * DAY_MS {
        DeadlineState::Soon
    } else {
        DeadlineState::Ok
    };
    Some(Deadline {
        behind_since: since,
        due_at: Timestamp(due),
        days_left,
        state,
    })
}

/// What the new `behind_since` is after a check: kept while the runner stays behind, set when it first falls behind,
/// cleared when it has caught up.
pub fn next_behind_since(
    previous: Option<Timestamp>,
    installed: Option<&RunnerVersion>,
    latest: &RunnerVersion,
    now: Timestamp,
) -> Option<Timestamp> {
    match installed {
        Some(i) if i < latest && !latest.is_prerelease() => previous.or(Some(now)),
        _ => None,
    }
}

/// How an update goes round in circles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoopKind {
    /// It fails or is rolled back again and again.
    Failing,
    /// It installs, but the runner is back on the old version (or installs it again) each time.
    NotSticking,
    /// Every attempt is postponed (a job always runs, or the runner will not stop).
    Postponed,
}

/// An update loop found in the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLoop {
    /// The version being tried.
    pub target: RunnerVersion,
    /// How the attempts end.
    pub kind: LoopKind,
    /// How many attempts in the window.
    pub attempts: u32,
    /// The likely cause and what to do, for the user.
    pub cause: String,
}

/// Looks for a loop in the update outcomes of the last [`LOOP_WINDOW_DAYS`] days (`outcomes`: `(when, outcome)`).
/// Only the newest target is considered. Automatic updates should stop for the target of a loop.
pub fn detect_loop<'a>(outcomes: impl IntoIterator<Item = (Timestamp, &'a UpdateOutcome)>, now: Timestamp) -> Option<UpdateLoop> {
    let from = now.0.saturating_sub(LOOP_WINDOW_DAYS * DAY_MS);
    let recent: Vec<(Timestamp, &UpdateOutcome)> = outcomes.into_iter().filter(|(t, _)| t.0 >= from).collect();
    let target = recent.iter().max_by_key(|(t, _)| t.0).map(|(_, o)| plan_to(o).clone())?;
    let mine = || recent.iter().filter(|(_, o)| plan_to(o) == &target).map(|(_, o)| *o);
    let failing = mine()
        .filter(|o| matches!(o, UpdateOutcome::Failed { .. } | UpdateOutcome::RolledBack { .. }))
        .count();
    let stuck = mine().filter(|o| matches!(o, UpdateOutcome::Updated { .. })).count();
    let postponed = mine().filter(|o| matches!(o, UpdateOutcome::Postponed { .. })).count();
    let last_reason = mine()
        .filter_map(|o| match o {
            UpdateOutcome::Failed { reason, .. } => Some(reason.clone()),
            UpdateOutcome::RolledBack { reason, .. } => Some(format!("{reason:?}")),
            _ => None,
        })
        .next_back();
    let n = |c: usize| u32::try_from(c).unwrap_or(u32::MAX);
    if failing >= FAILING_ATTEMPTS {
        let why = last_reason.map_or(String::new(), |r| format!(" Last reason: {r}."));
        return Some(UpdateLoop {
            target,
            kind: LoopKind::Failing,
            attempts: n(failing),
            cause: format!("The update failed {failing} times in {LOOP_WINDOW_DAYS} days.{why} Automatic updates to it are paused; fix the cause (disk, network, permissions) and install it by hand, or skip the version."),
        });
    }
    if stuck >= NOT_STICKING_ATTEMPTS {
        return Some(UpdateLoop {
            target,
            kind: LoopKind::NotSticking,
            attempts: n(stuck),
            cause: format!("The version was installed {stuck} times in {LOOP_WINDOW_DAYS} days but does not stay. Something puts the old binary back (another supervisor, a read-only folder or a backup restore). Automatic updates to it are paused."),
        });
    }
    if postponed >= POSTPONED_ATTEMPTS {
        return Some(UpdateLoop {
            target,
            kind: LoopKind::Postponed,
            attempts: n(postponed),
            cause: format!("The update was postponed {postponed} times: a job is always running or the runner does not stop in time. Give it a quiet window (Update window) or drain the runner and update by hand."),
        });
    }
    None
}

fn plan_to(o: &UpdateOutcome) -> &RunnerVersion {
    match o {
        UpdateOutcome::Updated { plan }
        | UpdateOutcome::RolledBack { plan, .. }
        | UpdateOutcome::Postponed { plan, .. }
        | UpdateOutcome::Cancelled { plan }
        | UpdateOutcome::Failed { plan, .. } => &plan.to,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::machine::{PostponeReason, RollbackReason, UpdatePlan};

    fn v(s: &str) -> RunnerVersion {
        RunnerVersion::parse(s).unwrap()
    }

    fn plan(to: &str) -> UpdatePlan {
        UpdatePlan {
            from: Some(v("1.0.0")),
            to: v(to),
        }
    }

    fn days(d: u64) -> Timestamp {
        Timestamp(d * DAY_MS)
    }

    #[test]
    fn the_deadline_counts_down_warns_and_goes_negative() {
        assert_eq!(deadline(0, Some(days(1)), days(2)), None, "0 turns it off");
        assert_eq!(deadline(30, None, days(2)), None, "not behind");
        let d = deadline(30, Some(days(10)), days(15)).unwrap();
        assert_eq!((d.days_left, d.state, d.due_at), (25, DeadlineState::Ok, days(40)));
        let d = deadline(30, Some(days(10)), days(35)).unwrap();
        assert_eq!((d.days_left, d.state), (5, DeadlineState::Soon));
        let d = deadline(30, Some(days(10)), days(40)).unwrap();
        assert_eq!((d.days_left, d.state), (0, DeadlineState::Overdue));
        let d = deadline(30, Some(days(10)), days(43)).unwrap();
        assert_eq!((d.days_left, d.state), (-3, DeadlineState::Overdue));
    }

    #[test]
    fn behind_since_starts_stays_and_clears() {
        let (old, new) = (v("1.0.0"), v("1.1.0"));
        assert_eq!(next_behind_since(None, Some(&old), &new, days(5)), Some(days(5)));
        assert_eq!(next_behind_since(Some(days(2)), Some(&old), &new, days(5)), Some(days(2)));
        assert_eq!(next_behind_since(Some(days(2)), Some(&new), &new, days(5)), None);
        assert_eq!(
            next_behind_since(Some(days(2)), None, &new, days(5)),
            None,
            "unknown installed version"
        );
        assert_eq!(
            next_behind_since(None, Some(&old), &v("1.2.0-rc.1"), days(5)),
            None,
            "a pre-release is no deadline"
        );
    }

    fn failed(to: &str) -> UpdateOutcome {
        UpdateOutcome::Failed {
            plan: plan(to),
            reason: "disk full".into(),
            needs_attention: false,
        }
    }

    #[test]
    fn repeated_failures_are_a_loop_with_the_cause() {
        let o = [failed("1.1.0"), failed("1.1.0"), failed("1.1.0")];
        let l = detect_loop(o.iter().enumerate().map(|(i, o)| (days(10 + i as u64), o)), days(11)).unwrap();
        assert_eq!((l.kind, l.attempts, l.target.clone()), (LoopKind::Failing, 3, v("1.1.0")));
        assert!(l.cause.contains("disk full") && l.cause.contains("paused"));
        let two = [failed("1.1.0"), failed("1.1.0")];
        assert!(detect_loop(two.iter().map(|o| (days(10), o)), days(11)).is_none());
        // Old attempts and other targets do not count.
        let mixed = [failed("1.0.5"), failed("1.1.0"), failed("1.1.0")];
        assert!(detect_loop(mixed.iter().map(|o| (days(10), o)), days(11)).is_none());
        assert!(detect_loop(o.iter().map(|o| (days(1), o)), days(30)).is_none());
        let rolled = UpdateOutcome::RolledBack {
            plan: plan("1.1.0"),
            reason: RollbackReason::ExitedEarly,
        };
        let r = [rolled.clone(), rolled.clone(), failed("1.1.0")];
        assert_eq!(
            detect_loop(r.iter().map(|o| (days(10), o)), days(10)).unwrap().kind,
            LoopKind::Failing
        );
    }

    #[test]
    fn an_update_that_does_not_stick_and_one_that_is_always_postponed() {
        let ok = UpdateOutcome::Updated { plan: plan("1.1.0") };
        let o = [ok.clone(), ok.clone(), ok];
        assert_eq!(
            detect_loop(o.iter().map(|o| (days(10), o)), days(10)).unwrap().kind,
            LoopKind::NotSticking
        );
        let p = UpdateOutcome::Postponed {
            plan: plan("1.1.0"),
            reason: PostponeReason::Busy,
        };
        let five = vec![p.clone(); 5];
        assert_eq!(
            detect_loop(five.iter().map(|o| (days(10), o)), days(10)).unwrap().kind,
            LoopKind::Postponed
        );
        assert!(detect_loop(five[..4].iter().map(|o| (days(10), o)), days(10)).is_none());
        let none: [(Timestamp, &UpdateOutcome); 0] = [];
        assert!(detect_loop(none, days(10)).is_none());
        let c = UpdateOutcome::Cancelled { plan: plan("1.1.0") };
        assert!(detect_loop([(days(10), &c)], days(10)).is_none());
    }
}
