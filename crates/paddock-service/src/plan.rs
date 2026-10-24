//! Deciding what to do about a registration, as one pure function ([`plan`]) of what was observed. It encodes rule 24
//! and the cases that reached users:
//!
//! - **The user's switch wins.** Switched off (Login Items, Task Manager > Startup, Services, `systemctl disable`) or
//!   waiting for approval is reported, never repaired, not even when Repair is pressed: a repair would switch it back on.
//! - **A stale registration is repaired** (R03): launchd refuses the job (EX_CONFIG, a stale launch constraint) and the
//!   service does not answer.
//! - **Once per version change** the registration is replaced where the OS pins the build (macOS: every ad-hoc build
//!   has a new cdhash), instead of waiting for the failure; elsewhere only when the definition differs.
//! - **A missing or moved program** is re-pointed at this copy.
//! - **Two copies installed:** a registration made by a newer version is left to that copy (no ping-pong), unless its
//!   program is gone.
//! - **Never install unasked:** nothing registered stays unregistered until the user asks.
//! - Where every change needs an OS prompt (a Windows service from the app), a repair is offered, never started.

use crate::backend::{Capabilities, Inspection, Job, Registration};
use std::cmp::Ordering;

/// Why the registration is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The user pressed Repair.
    Requested,
    /// The OS refuses to start what is registered and the service does not answer.
    Refused,
    /// Another version registered it.
    VersionChanged,
    /// The registered program is gone.
    ProgramMissing,
    /// The registered program is another copy's.
    Moved,
}

impl Reason {
    /// Words for the log.
    pub fn describe(self) -> &'static str {
        match self {
            Reason::Requested => "repair requested",
            Reason::Refused => "the OS refuses to start the registered service",
            Reason::VersionChanged => "the app was updated",
            Reason::ProgramMissing => "the registered program is gone",
            Reason::Moved => "the registration points at another copy of the app",
        }
    }
}

/// Why nothing is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    /// Nothing is registered (and nobody asked).
    NotInstalled,
    /// Registered by this version, running or not refused.
    Healthy,
    /// The user switched it off.
    SwitchedOff,
    /// Waiting for the user's approval in Login Items.
    NeedsApproval,
    /// It cannot be registered from here.
    Unavailable(String),
    /// A newer version registered it.
    NewerOwner(String),
}

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Nothing.
    Leave(Hold),
    /// The registration is current: only remember that this version looked after it.
    MarkCurrent,
    /// Replace the registration now (unregister, evict, wait until gone, register).
    Replace(Reason),
    /// A replacement is needed but needs an OS prompt: offer Repair.
    Offer(Reason),
}

/// What [`plan`] looks at.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// The backend's look at the registration.
    pub inspection: &'a Inspection,
    /// The service answers (its own health check).
    pub answers: bool,
    /// The version that registered it last, from the state file.
    pub recorded_version: Option<&'a str>,
    /// This app's version.
    pub current_version: &'a str,
    /// The user pressed Repair.
    pub requested: bool,
    /// What the backend allows.
    pub caps: Capabilities,
}

/// Compares two versions (`1.2.3`, `1.2.3-beta.1`, a leading `v`): numeric parts first, a pre-release is older than
/// its release. `None` when either is not a version.
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    fn parse(v: &str) -> Option<(Vec<u64>, Option<&str>)> {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v.split('+').next()?, None),
        };
        let nums: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
        let nums = nums.filter(|n| !n.is_empty() && n.len() <= 4)?;
        Some((nums, pre))
    }
    let (an, ap) = parse(a)?;
    let (bn, bp) = parse(b)?;
    let len = an.len().max(bn.len());
    let pad = |n: &[u64]| {
        (0..len)
            .map(|i| n.get(i).copied().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    Some(pad(&an).cmp(&pad(&bn)).then_with(|| match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(y),
    }))
}

/// Decides; see the module documentation for the rules.
pub fn plan(f: &Facts<'_>) -> Plan {
    let i = f.inspection;
    match &i.registration {
        Registration::SwitchedOff => return Plan::Leave(Hold::SwitchedOff),
        Registration::NeedsApproval => return Plan::Leave(Hold::NeedsApproval),
        Registration::Unavailable(why) => return Plan::Leave(Hold::Unavailable(why.clone())),
        Registration::Absent | Registration::Registered => {}
    }
    if f.requested {
        return Plan::Replace(Reason::Requested);
    }
    if !i.present() {
        return Plan::Leave(Hold::NotInstalled);
    }
    let fix = |r: Reason| {
        if f.caps.automatic {
            Plan::Replace(r)
        } else {
            Plan::Offer(r)
        }
    };
    if i.program_exists == Some(false) {
        return fix(Reason::ProgramMissing);
    }
    if let Some(recorded) = f.recorded_version
        && compare_versions(recorded, f.current_version) == Some(Ordering::Greater)
    {
        return Plan::Leave(Hold::NewerOwner(recorded.to_string()));
    }
    if i.job == Job::Refused && !f.answers {
        return fix(Reason::Refused);
    }
    if i.points_here == Some(false) {
        return fix(Reason::Moved);
    }
    if f.recorded_version != Some(f.current_version) {
        let current =
            i.definition_current == Some(true) && i.registration == Registration::Registered;
        return if !f.caps.pinned_identity && current {
            Plan::MarkCurrent
        } else {
            fix(Reason::VersionChanged)
        };
    }
    Plan::Leave(Hold::Healthy)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: Capabilities = Capabilities {
        automatic: true,
        pinned_identity: true,
        login_items: true,
    };
    const LINUX: Capabilities = Capabilities {
        automatic: true,
        pinned_identity: false,
        login_items: false,
    };
    const UAC: Capabilities = Capabilities {
        automatic: false,
        pinned_identity: false,
        login_items: false,
    };

    fn registered(job: Job) -> Inspection {
        Inspection {
            registration: Registration::Registered,
            job,
            ..Inspection::absent()
        }
    }

    fn facts<'a>(i: &'a Inspection, caps: Capabilities, recorded: Option<&'a str>) -> Facts<'a> {
        Facts {
            inspection: i,
            answers: false,
            recorded_version: recorded,
            current_version: "1.0.4",
            requested: false,
            caps,
        }
    }

    #[test]
    fn versions_compare_by_number_and_pre_release() {
        assert_eq!(compare_versions("1.0.10", "1.0.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v1.2", "1.2.0"), Some(Ordering::Equal));
        assert_eq!(
            compare_versions("1.2.0-beta.1", "1.2.0"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_versions("1.2.0", "1.2.0-rc.1"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare_versions("1.2.0-a", "1.2.0-b"), Some(Ordering::Less));
        assert_eq!(
            compare_versions("1.2.0+build", "1.2.0"),
            Some(Ordering::Equal)
        );
        assert_eq!(compare_versions("dev", "1.0.0"), None);
        assert_eq!(compare_versions("1.0.0", "1.x"), None);
        assert_eq!(compare_versions("", "1"), None);
        assert_eq!(compare_versions("1.2.3.4.5", "1"), None);
    }

    #[test]
    fn the_users_switch_wins_over_everything_including_repair() {
        for requested in [false, true] {
            for (reg, hold) in [
                (Registration::SwitchedOff, Hold::SwitchedOff),
                (Registration::NeedsApproval, Hold::NeedsApproval),
                (
                    Registration::Unavailable("move it".into()),
                    Hold::Unavailable("move it".into()),
                ),
            ] {
                let i = Inspection {
                    registration: reg,
                    job: Job::Refused,
                    ..Inspection::absent()
                };
                let mut f = facts(&i, MAC, None);
                f.requested = requested;
                assert_eq!(plan(&f), Plan::Leave(hold));
            }
        }
    }

    #[test]
    fn nothing_registered_is_never_installed_unasked() {
        let i = Inspection::absent();
        assert_eq!(
            plan(&facts(&i, MAC, Some("0.9.0"))),
            Plan::Leave(Hold::NotInstalled)
        );
        let mut f = facts(&i, UAC, None);
        f.requested = true;
        assert_eq!(
            plan(&f),
            Plan::Replace(Reason::Requested),
            "pressing Repair is consent, also with a prompt"
        );
    }

    #[test]
    fn a_refused_job_is_repaired_only_while_the_service_is_down() {
        // R03 as the owner saw it: unlisted in Login Items, launchd holds the failing job.
        let i = Inspection {
            job: Job::Refused,
            ..Inspection::absent()
        };
        assert_eq!(
            plan(&facts(&i, MAC, Some("1.0.4"))),
            Plan::Replace(Reason::Refused)
        );
        let mut f = facts(&i, MAC, Some("1.0.4"));
        f.answers = true;
        assert_eq!(plan(&f), Plan::Leave(Hold::Healthy));
        assert_eq!(
            plan(&facts(&i, UAC, Some("1.0.4"))),
            Plan::Offer(Reason::Refused)
        );
    }

    #[test]
    fn a_new_version_replaces_a_pinned_registration_once_even_when_healthy() {
        let ok = registered(Job::Running);
        for recorded in [None, Some("1.0.3")] {
            let mut f = facts(&ok, MAC, recorded);
            f.answers = true;
            assert_eq!(plan(&f), Plan::Replace(Reason::VersionChanged));
        }
        let mut f = facts(&ok, MAC, Some("1.0.4"));
        f.answers = true;
        assert_eq!(
            plan(&f),
            Plan::Leave(Hold::Healthy),
            "once: the recorded version stops it"
        );
        // A job launchd holds without a registration counts as registered.
        let job_only = Inspection {
            job: Job::NotRunning,
            ..Inspection::absent()
        };
        assert_eq!(
            plan(&facts(&job_only, MAC, Some("1.0.3"))),
            Plan::Replace(Reason::VersionChanged)
        );
    }

    #[test]
    fn elsewhere_a_current_definition_is_only_marked() {
        let mut i = registered(Job::Running);
        i.definition_current = Some(true);
        i.points_here = Some(true);
        i.program_exists = Some(true);
        assert_eq!(plan(&facts(&i, LINUX, Some("1.0.3"))), Plan::MarkCurrent);
        assert_eq!(
            plan(&facts(&i, UAC, None)),
            Plan::MarkCurrent,
            "the installer re-registered it"
        );
        i.definition_current = Some(false);
        assert_eq!(
            plan(&facts(&i, LINUX, Some("1.0.3"))),
            Plan::Replace(Reason::VersionChanged)
        );
        assert_eq!(
            plan(&facts(&i, UAC, Some("1.0.3"))),
            Plan::Offer(Reason::VersionChanged)
        );
    }

    #[test]
    fn a_missing_or_moved_program_is_re_pointed_here() {
        let mut i = registered(Job::NotRunning);
        i.program = Some("/old/Pitwall.app".into());
        i.program_exists = Some(false);
        assert_eq!(
            plan(&facts(&i, LINUX, Some("1.0.4"))),
            Plan::Replace(Reason::ProgramMissing)
        );
        // Even a newer version's registration is taken over once its program is gone.
        assert_eq!(
            plan(&facts(&i, LINUX, Some("2.0.0"))),
            Plan::Replace(Reason::ProgramMissing)
        );
        i.program_exists = Some(true);
        i.points_here = Some(false);
        assert_eq!(
            plan(&facts(&i, LINUX, Some("1.0.4"))),
            Plan::Replace(Reason::Moved)
        );
        assert_eq!(
            plan(&facts(&i, UAC, Some("1.0.4"))),
            Plan::Offer(Reason::Moved)
        );
    }

    #[test]
    fn two_copies_installed_do_not_take_the_registration_from_each_other() {
        let mut i = registered(Job::Running);
        i.points_here = Some(false);
        i.program_exists = Some(true);
        // The older copy starts: the newer one's registration stays.
        assert_eq!(
            plan(&facts(&i, LINUX, Some("1.1.0"))),
            Plan::Leave(Hold::NewerOwner("1.1.0".into()))
        );
        assert_eq!(
            plan(&facts(&i, MAC, Some("1.1.0"))),
            Plan::Leave(Hold::NewerOwner("1.1.0".into()))
        );
        // The newer copy starts: it takes it over.
        assert_eq!(
            plan(&facts(&i, LINUX, Some("1.0.0"))),
            Plan::Replace(Reason::Moved)
        );
        // Pressing Repair in the older copy is the user's call.
        let mut f = facts(&i, LINUX, Some("1.1.0"));
        f.requested = true;
        assert_eq!(plan(&f), Plan::Replace(Reason::Requested));
        // A recorded version that is not a version does not block.
        assert_eq!(
            plan(&facts(&i, LINUX, Some("dev"))),
            Plan::Replace(Reason::Moved)
        );
    }

    #[test]
    fn reasons_have_words() {
        for r in [
            Reason::Requested,
            Reason::Refused,
            Reason::VersionChanged,
            Reason::ProgramMissing,
            Reason::Moved,
        ] {
            assert!(!r.describe().is_empty());
        }
    }
}
