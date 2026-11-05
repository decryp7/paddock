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

#[cfg(test)]
use crate::backend::LoginItemFacts;
use crate::backend::{Capabilities, DisabledMark, Inspection, Job, Registration};
use crate::port::AppServiceStatus;
use crate::record::{Phase, Recorded, State};
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
    /// A replace was interrupted (a crash, a quit) after the old registration was removed: register again.
    Interrupted,
    /// The service was stopped on request and is brought back.
    Stopped,
    /// The user's switch is on and nothing is registered.
    Wanted,
    /// The user's switch is off and the service is still registered.
    Unwanted,
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
            Reason::Interrupted => "an earlier replacement was interrupted after the old registration was removed",
            Reason::Stopped => "the service was stopped on request",
            Reason::Wanted => "the switch is on and nothing is registered",
            Reason::Unwanted => "the switch is off and the service is still registered",
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
    /// This version registered it this many times and the service never answered: held, not replaced again.
    Unanswered(u32),
    /// This version registered it this many times and the OS refused to start it every time: held, not replaced again.
    Refused(u32),
    /// This version called `register` this many times and the OS said it failed every time: held, not retried by itself.
    RegisterFailed(u32),
    /// The record cannot be written, so nothing was changed (no OS call without a durable write); the text says why.
    RecordUnavailable(String),
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
    /// This version already registered it and the service has not answered yet: give it time without removing anything,
    /// and replace only if it never comes (the reason is why it was registered).
    Await(Reason),
    /// A replacement is needed but needs an OS prompt: offer Repair.
    Offer(Reason),
    /// An uninstall was begun and not finished: finish removing, never register.
    Remove,
}

/// What [`plan`] looks at.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// The backend's look at the registration.
    pub inspection: &'a Inspection,
    /// The service itself answers (its own health check; never the app's own agent).
    pub answers: bool,
    /// The app's own agent holds the data folder: it only keeps a refused job from being replaced.
    pub covered: bool,
    /// The version that saw the service come up last, from the state file.
    pub recorded_version: Option<&'a str>,
    /// What is in progress, from the state file.
    pub state: &'a State,
    /// This app's version.
    pub current_version: &'a str,
    /// What the backend allows.
    pub caps: Capabilities,
    /// The wait for an accepted registration ran out.
    pub waited: bool,
    /// The service was stopped on request and is being brought back.
    pub resuming: bool,
    /// The user's switch as the app stores it (`None`: it could not be read, so nothing is registered unattended).
    pub wanted: Option<bool>,
    /// The first run has been done (so a switch that is on with nothing registered is put right).
    pub first_run_done: bool,
    /// The app runs from a place it can register from (not a disk image or a translocated copy).
    pub location_ok: bool,
    /// The state file is there but cannot be used (damaged, unreadable, a newer app's): a service that answers is adopted.
    pub record_damaged: bool,
}

/// How many register calls are allowed per version before the service is held.
pub const MAX_ATTEMPTS: u32 = 3;

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

/// Why a request to repair or install is refused: only the user's switch or the OS stop one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The user switched it off.
    SwitchedOff,
    /// Waiting for the user's approval in Login Items.
    NeedsApproval,
    /// It cannot be registered from here.
    Unavailable(String),
}

impl From<Refusal> for Hold {
    fn from(r: Refusal) -> Hold {
        match r {
            Refusal::SwitchedOff => Hold::SwitchedOff,
            Refusal::NeedsApproval => Hold::NeedsApproval,
            Refusal::Unavailable(why) => Hold::Unavailable(why),
        }
    }
}

/// What a request (Repair, Install, launch at login switched on) does: replace the registration ([`Reason::Requested`]),
/// whatever else holds (a newer copy's registration, a healthy one, nothing installed: pressing it is consent), unless
/// the user's switch or the OS refuses.
pub fn request(i: &Inspection) -> Result<Reason, Refusal> {
    match &i.registration {
        Registration::SwitchedOff => Err(Refusal::SwitchedOff),
        Registration::NeedsApproval => Err(Refusal::NeedsApproval),
        Registration::Unavailable(why) => Err(Refusal::Unavailable(why.clone())),
        Registration::Absent | Registration::Registered => Ok(Reason::Requested),
    }
}

/// Decides what happens without the user asking (at start, first run, resume); see the module documentation. Every
/// unattended action goes through here, and a hold is derived: `attempts >= MAX_ATTEMPTS` for this version.
pub fn plan(f: &Facts<'_>) -> Plan {
    let i = f.inspection;
    if let Err(r) = request(i) {
        return Plan::Leave(r.into());
    }
    let remove = || {
        if !i.present() {
            Plan::Leave(Hold::NotInstalled)
        } else if f.caps.automatic {
            Plan::Remove
        } else {
            Plan::Offer(Reason::Unwanted)
        }
    };
    if matches!(f.state, State::Removing) {
        return remove();
    }
    if !f.location_ok {
        return Plan::Leave(Hold::Unavailable(
            "the app runs from a place it cannot register the background service from; move it to the Applications folder and open it there"
                .to_string(),
        ));
    }
    if f.wanted == Some(false) {
        return remove();
    }
    // A record that cannot be used says nothing about the version: a service that is up is adopted, never replaced.
    if f.record_damaged && (f.answers || i.job == Job::Running) {
        return Plan::MarkCurrent;
    }
    let (tries, phase) = match f.state {
        State::Attempting {
            version,
            attempts,
            phase,
        } if version == f.current_version => (*attempts, Some(*phase)),
        _ => (0, None),
    };
    let capped = tries >= MAX_ATTEMPTS;
    let held = || {
        if i.job == Job::Refused {
            Hold::Refused(tries)
        } else {
            Hold::Unanswered(tries)
        }
    };
    // Accepted by the OS (or, for a removal that began on this version's own settled registration, still ours).
    let accepted = matches!(
        phase,
        Some(Phase::Registering | Phase::Awaiting | Phase::RemovingOld { ours: true })
    );
    let fix = |r: Reason| {
        if !f.caps.automatic {
            Plan::Offer(r)
        } else if capped {
            Plan::Leave(held())
        } else {
            Plan::Replace(r)
        }
    };
    if i.registration == Registration::Absent {
        // macOS has no record of the item, yet a job is held (the app was opened from a place it cannot register from,
        // such as a disk image): replacing would remove the job and fail to register it again. Left alone.
        if i.login_item
            .is_some_and(|f| f.status == AppServiceStatus::NotFound)
            && !matches!(i.job, Job::Absent | Job::Unknown)
        {
            return Plan::Leave(Hold::Unavailable(
                "macOS has no record of the background item although a job runs; open the app from the Applications folder"
                    .to_string(),
            ));
        }
        if f.wanted.is_none() {
            return Plan::Leave(Hold::NotInstalled);
        }
        // A replace that was interrupted after the old registration went (the job may linger): finish it, whoever began
        // it, because the user wanted the service. `Registering` adds no attempt; the bound is the attempts counted.
        if let State::Attempting {
            phase: Phase::RemovingOld { .. } | Phase::Registering,
            ..
        } = f.state
        {
            return if f.caps.automatic {
                Plan::Replace(Reason::Interrupted)
            } else {
                Plan::Offer(Reason::Interrupted)
            };
        }
        // A register that failed is retried only while attempts are left; at the cap it is a hold.
        if let State::Attempting {
            phase: Phase::RegisterFailed(_),
            ..
        } = f.state
        {
            return if !f.caps.automatic {
                Plan::Offer(Reason::Interrupted)
            } else if capped {
                Plan::Leave(Hold::RegisterFailed(tries))
            } else {
                Plan::Replace(Reason::Interrupted)
            };
        }
        if matches!(i.job, Job::Absent | Job::Unknown) {
            // The switch is on, the first run is done and nothing is registered: put it right.
            if f.wanted == Some(true) && f.first_run_done {
                return fix(Reason::Wanted);
            }
            // Nothing registered, and the OS either holds no job or could not be asked: not installed, never install unasked.
            return Plan::Leave(Hold::NotInstalled);
        }
    }
    // The registration of this version was accepted and the service is up: settled, whatever the attempts were.
    if accepted && (f.answers || i.job == Job::Running) {
        return Plan::MarkCurrent;
    }
    if i.program_exists == Some(false) {
        return fix(Reason::ProgramMissing);
    }
    // A job the OS refuses while the service is silent is repaired whatever version registered it: leaving it would keep
    // the service down (a downgrade, or a recorded version nobody can verify). The app's own agent covers a refused job.
    if i.job == Job::Refused && !f.answers && !f.covered {
        return fix(Reason::Refused);
    }
    // Another copy's registration is left to that copy only when its program is known to exist; a newer version that is
    // part-way through registering counts as the newer owner.
    let newer = |v: &str| compare_versions(v, f.current_version) == Some(Ordering::Greater);
    if let Some(recorded) = f.recorded_version
        && newer(recorded)
        && i.points_here == Some(false)
        && i.program_exists == Some(true)
    {
        return Plan::Leave(Hold::NewerOwner(recorded.to_string()));
    }
    if let State::Attempting { version, .. } = f.state
        && newer(version)
        && i.program_exists != Some(false)
    {
        return Plan::Leave(Hold::NewerOwner(version.clone()));
    }
    if i.points_here == Some(false) {
        return fix(Reason::Moved);
    }
    if f.recorded_version != Some(f.current_version) {
        if accepted && f.caps.pinned_identity && f.caps.automatic {
            // This version registered it already: removing a job that is only slow to start would replace a working
            // service (and announce it again). Given time first; replaced only if the wait ran out, and held at the cap.
            return if f.waited {
                fix(Reason::VersionChanged)
            } else {
                Plan::Await(Reason::VersionChanged)
            };
        }
        let current =
            i.definition_current == Some(true) && i.registration == Registration::Registered;
        return if !f.caps.pinned_identity && current {
            Plan::MarkCurrent
        } else {
            fix(Reason::VersionChanged)
        };
    }
    if f.resuming && !(f.answers || i.job == Job::Running) {
        return fix(Reason::Stopped);
    }
    Plan::Leave(Hold::Healthy)
}

/// How the service comes back after "Quit and stop measuring" (a stop keeps the registration, so a job that launchd no
/// longer holds is loaded again by registering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumePath {
    /// Leave it: the user switched the item off in Login Items, the OS has no record of it, or what the user switched could
    /// not be read. That switch is theirs.
    Nothing,
    /// Register the enabled item again (no removal first). Only while this version registered it: registering records the
    /// version, so a version change must go through the full repair first, which is what notices it.
    Light,
    /// The full repair ([`Reason::Requested`]): the registration is replaced.
    Repair,
}

/// Decides the path of a resume before anything is called (pure, like [`plan`]). What cannot be read is left alone. A
/// version that registered the service and is waiting for it counts as having registered it.
pub fn resume_path(i: &Inspection, rec: &Recorded, current_version: &str) -> ResumePath {
    let (enabled, left_alone) = match &i.login_item {
        Some(f) => (
            f.status == AppServiceStatus::Enabled,
            f.status == AppServiceStatus::NotFound
                || f.mark == DisabledMark::Unreadable
                || f.switched_off(),
        ),
        None => (
            i.registration == Registration::Registered,
            matches!(
                i.registration,
                Registration::SwitchedOff
                    | Registration::NeedsApproval
                    | Registration::Unavailable(_)
            ),
        ),
    };
    let registered_here = rec.registered_version.as_deref() == Some(current_version)
        || matches!(
            rec.phase_of(current_version),
            Some(Phase::Registering | Phase::Awaiting)
        );
    if left_alone {
        ResumePath::Nothing
    } else if !registered_here {
        ResumePath::Repair
    } else if enabled {
        ResumePath::Light
    } else {
        ResumePath::Repair
    }
}

/// Whether the app has registered the service, whatever the user did with it since: registered or waiting for approval
/// (macOS reports an item the user switched off as waiting). A switched-off item that was never registered is not installed.
pub fn installed(i: &Inspection) -> bool {
    match &i.login_item {
        Some(f) => matches!(
            f.status,
            AppServiceStatus::Enabled | AppServiceStatus::RequiresApproval
        ),
        None => !matches!(
            i.registration,
            Registration::Absent | Registration::Unavailable(_)
        ),
    }
}

/// Whether the OS starts the job now and restarts it (launchd `KeepAlive`, systemd `Restart`), so the app must wait for it
/// instead of running its own: the item is registered and allowed, the user did not switch it off and the OS still holds the
/// job. Anything less supervises nothing, and waiting for it would wait for a service that never comes.
pub fn supervises(i: &Inspection) -> bool {
    let allowed = match &i.login_item {
        Some(f) => f.status == AppServiceStatus::Enabled && !f.switched_off(),
        None => i.registration == Registration::Registered,
    };
    allowed && !matches!(i.job, Job::Absent | Job::Unknown | Job::Refused)
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

    fn rec_of(v: Option<&str>) -> Recorded {
        Recorded {
            registered_version: v.map(str::to_string),
            ..Recorded::default()
        }
    }

    const IDLE: State = State::Idle;

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
            covered: false,
            recorded_version: recorded,
            state: &IDLE,
            current_version: "1.0.4",
            caps,
            waited: false,
            resuming: false,
            wanted: Some(true),
            first_run_done: false,
            location_ok: true,
            record_damaged: false,
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
        {
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
                assert_eq!(plan(&facts(&i, MAC, None)), Plan::Leave(hold.clone()));
                assert_eq!(request(&i).map_err(Hold::from), Err(hold));
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
        assert_eq!(
            request(&i),
            Ok(Reason::Requested),
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
        assert_eq!(request(&i), Ok(Reason::Requested));
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

    // ---- resume after a stop on request (ported from Dyno's service_repair.rs, blocker B4 and the 2026-11-05 Quit rules)

    fn item(status: AppServiceStatus, mark: DisabledMark, job: Job) -> Inspection {
        let registration = match status {
            AppServiceStatus::RequiresApproval => Registration::NeedsApproval,
            _ if mark == DisabledMark::Disabled => Registration::SwitchedOff,
            AppServiceStatus::Enabled => Registration::Registered,
            _ => Registration::Absent,
        };
        Inspection {
            registration,
            job,
            login_item: Some(LoginItemFacts { status, mark }),
            ..Inspection::absent()
        }
    }

    #[test]
    fn a_resume_never_touches_an_item_the_user_switched_off_or_cannot_be_read_and_never_hides_a_version_change()
     {
        use AppServiceStatus as S;
        use DisabledMark as D;
        let path = |s, d, rec: &Recorded, cur| resume_path(&item(s, d, Job::Absent), rec, cur);
        assert_eq!(
            path(S::RequiresApproval, D::NotListed, &rec_of(Some("1")), "1"),
            ResumePath::Nothing
        );
        assert_eq!(
            path(S::Enabled, D::Disabled, &rec_of(Some("1")), "1"),
            ResumePath::Nothing
        );
        assert_eq!(
            path(S::NotRegistered, D::Disabled, &rec_of(Some("1")), "1"),
            ResumePath::Nothing
        );
        assert_eq!(
            path(S::NotFound, D::Enabled, &rec_of(Some("1")), "1"),
            ResumePath::Nothing
        );
        assert_eq!(
            path(S::Enabled, D::Unreadable, &rec_of(Some("1")), "1"),
            ResumePath::Nothing,
            "a failed look is not 'switched on'"
        );
        assert_eq!(
            path(S::Enabled, D::Enabled, &rec_of(Some("0")), "1"),
            ResumePath::Repair
        );
        assert_eq!(
            path(S::Enabled, D::NotListed, &Recorded::default(), "1"),
            ResumePath::Repair
        );
        assert_eq!(
            path(S::Enabled, D::NotListed, &rec_of(Some("1")), "1"),
            ResumePath::Light
        );
        assert_eq!(
            path(S::Enabled, D::Enabled, &rec_of(Some("1")), "1"),
            ResumePath::Light
        );
        assert_eq!(
            path(S::NotRegistered, D::NotListed, &rec_of(Some("1")), "1"),
            ResumePath::Repair
        );
    }

    #[test]
    fn a_resume_of_a_backend_without_login_item_facts_follows_the_registration() {
        let with = |registration| Inspection {
            registration,
            ..Inspection::absent()
        };
        assert_eq!(
            resume_path(&with(Registration::Registered), &rec_of(Some("1")), "1"),
            ResumePath::Light
        );
        assert_eq!(
            resume_path(&with(Registration::Registered), &rec_of(Some("0")), "1"),
            ResumePath::Repair
        );
        assert_eq!(
            resume_path(&with(Registration::Absent), &rec_of(Some("1")), "1"),
            ResumePath::Repair
        );
        for held in [
            Registration::SwitchedOff,
            Registration::NeedsApproval,
            Registration::Unavailable("x".into()),
        ] {
            assert_eq!(
                resume_path(&with(held), &rec_of(Some("1")), "1"),
                ResumePath::Nothing
            );
        }
    }

    #[test]
    fn installed_means_registered_or_waiting_for_approval() {
        use AppServiceStatus as S;
        use DisabledMark as D;
        for (status, want) in [
            (S::Enabled, true),
            (S::RequiresApproval, true),
            (S::NotRegistered, false),
            (S::NotFound, false),
        ] {
            assert_eq!(
                installed(&item(status, D::NotListed, Job::Absent)),
                want,
                "{status:?}"
            );
        }
        // The two views disagree (enabled, but launchd's database says off): still registered.
        assert!(installed(&item(S::Enabled, D::Disabled, Job::Absent)));
        // Not registered and switched off in launchd's database is not installed.
        assert!(!installed(&item(
            S::NotRegistered,
            D::Disabled,
            Job::Absent
        )));
        let plain = |registration| Inspection {
            registration,
            ..Inspection::absent()
        };
        assert!(installed(&plain(Registration::Registered)));
        assert!(installed(&plain(Registration::SwitchedOff)));
        assert!(!installed(&plain(Registration::Absent)));
    }

    #[test]
    fn launchd_supervises_only_an_enabled_item_the_user_did_not_switch_off_whose_job_is_loaded() {
        use AppServiceStatus as S;
        use DisabledMark as D;
        let loaded = Job::NotRunning;
        assert!(supervises(&item(S::Enabled, D::NotListed, loaded)));
        assert!(supervises(&item(S::Enabled, D::Enabled, loaded)));
        assert!(supervises(&item(S::Enabled, D::Unreadable, Job::Running)));
        // Enabled for SMAppService but off in Login Items (launchd's override database): nothing runs the agent.
        assert!(!supervises(&item(S::Enabled, D::Disabled, loaded)));
        // Waiting for approval, not registered, not found: launchd runs nothing.
        assert!(!supervises(&item(
            S::RequiresApproval,
            D::NotListed,
            loaded
        )));
        assert!(!supervises(&item(S::NotRegistered, D::NotListed, loaded)));
        assert!(!supervises(&item(S::NotFound, D::NotListed, loaded)));
        // Enabled, but launchd dropped the job (or `launchctl print` could not be read): it starts nothing.
        assert!(!supervises(&item(S::Enabled, D::NotListed, Job::Absent)));
        assert!(!supervises(&item(S::Enabled, D::NotListed, Job::Unknown)));
        // Without macOS facts the registration decides.
        let plain = Inspection {
            registration: Registration::Registered,
            job: Job::Running,
            ..Inspection::absent()
        };
        assert!(supervises(&plain));
    }

    #[test]
    fn nothing_is_installed_unasked_when_the_os_could_not_be_asked_or_has_no_record() {
        use AppServiceStatus as S;
        use DisabledMark as D;
        fn facts<'a>(i: &'a Inspection, rec: Option<&'a str>) -> Facts<'a> {
            Facts {
                inspection: i,
                answers: false,
                recorded_version: rec,
                current_version: "1.1.0",
                caps: MAC,
                covered: false,
                state: &IDLE,
                waited: false,
                resuming: false,
                wanted: Some(true),
                first_run_done: false,
                location_ok: true,
                record_damaged: false,
            }
        }
        // (a) not registered, `launchctl print` timed out, no record: not installed.
        let a = item(S::NotRegistered, D::NotListed, Job::Unknown);
        assert_eq!(plan(&facts(&a, None)), Plan::Leave(Hold::NotInstalled));
        // (b) neither the switch list nor the job could be read: still not installed (never re-enables a switched-off item).
        let b = item(S::NotRegistered, D::Unreadable, Job::Unknown);
        assert_eq!(
            plan(&facts(&b, Some("1.0.0"))),
            Plan::Leave(Hold::NotInstalled)
        );
        // (c) macOS has no record but a job runs: left alone, not replaced.
        for job in [Job::Running, Job::NotRunning, Job::Refused] {
            let c = item(S::NotFound, D::NotListed, job);
            assert!(
                matches!(
                    plan(&facts(&c, Some("1.0.0"))),
                    Plan::Leave(Hold::Unavailable(_))
                ),
                "{job:?}"
            );
        }
        // A fresh install (no record, no job) stays "not installed".
        let fresh = item(S::NotFound, D::NotListed, Job::Absent);
        assert_eq!(plan(&facts(&fresh, None)), Plan::Leave(Hold::NotInstalled));
    }

    #[test]
    fn a_refused_job_is_repaired_after_a_downgrade_and_a_recorded_version_nobody_can_verify() {
        let refused = Inspection {
            registration: Registration::Registered,
            job: Job::Refused,
            ..Inspection::absent()
        };
        for recorded in ["1.2.0", "999.0.0"] {
            assert_eq!(
                plan(&facts(&refused, MAC, Some(recorded))),
                Plan::Replace(Reason::Refused),
                "{recorded}"
            );
        }
        // A healthy job registered by a newer version, with no other copy's program known, is not pinned either way:
        // the bundled registration belongs to whichever copy runs.
        let healthy = registered(Job::Running);
        assert_eq!(
            plan(&facts(&healthy, MAC, Some("1.2.0"))),
            Plan::Replace(Reason::VersionChanged)
        );
    }

    #[test]
    fn a_version_that_registered_it_already_is_given_time_not_replaced_and_held_after_a_few_tries() {
        let reg = |job| Inspection {
            registration: Registration::Registered,
            job,
            ..Inspection::absent()
        };
        let attempting = |version: &str, attempts, phase| State::Attempting {
            version: version.to_string(),
            attempts,
            phase,
        };
        let with = |i: &Inspection, answers, state: State, waited| {
            plan(&Facts {
                inspection: i,
                answers,
                covered: false,
                recorded_version: Some("1.0.0"),
                state: &state,
                current_version: "1.0.4",
                caps: MAC,
                waited,
                resuming: false,
                wanted: Some(true),
                first_run_done: false,
                location_ok: true,
                record_damaged: false,
            })
        };
        let awaiting = |n| attempting("1.0.4", n, Phase::Awaiting);
        // It runs, or answers: settled, nothing removed.
        assert_eq!(with(&reg(Job::Running), false, awaiting(1), false), Plan::MarkCurrent);
        assert_eq!(with(&reg(Job::NotRunning), true, awaiting(2), false), Plan::MarkCurrent);
        // Not up yet: wait, no removal; once the wait ran out: replaced while attempts are left, held at the cap.
        assert_eq!(
            with(&reg(Job::NotRunning), false, awaiting(1), false),
            Plan::Await(Reason::VersionChanged)
        );
        assert_eq!(
            with(&reg(Job::NotRunning), false, awaiting(1), true),
            Plan::Replace(Reason::VersionChanged)
        );
        assert_eq!(
            with(&reg(Job::NotRunning), false, awaiting(MAX_ATTEMPTS), true),
            Plan::Leave(Hold::Unanswered(MAX_ATTEMPTS))
        );
        // A call that may have landed is waited for, never removed; a removal that began is carried on, and an old job
        // that runs does not settle a version whose replace has not registered anything yet.
        let phase = |ph| attempting("1.0.4", 1, ph);
        assert_eq!(
            with(&reg(Job::NotRunning), false, phase(Phase::Registering), false),
            Plan::Await(Reason::VersionChanged)
        );
        assert_eq!(
            with(&reg(Job::Running), false, phase(Phase::RemovingOld { ours: false }), false),
            Plan::Replace(Reason::VersionChanged)
        );
        // Another version's attempts do not count.
        assert_eq!(
            with(&reg(Job::NotRunning), false, attempting("1.0.3", 9, Phase::Awaiting), false),
            Plan::Replace(Reason::VersionChanged)
        );
        // A refusal is replaced while attempts are left and held at the cap (never without limit).
        assert_eq!(
            with(&reg(Job::Refused), false, awaiting(1), false),
            Plan::Replace(Reason::Refused)
        );
        assert_eq!(
            with(&reg(Job::Refused), false, awaiting(MAX_ATTEMPTS), false),
            Plan::Leave(Hold::Refused(MAX_ATTEMPTS))
        );
        // A crash between the removal and the registration finishes the job, at the cap too.
        for ph in [Phase::RemovingOld { ours: true }, Phase::RemovingOld { ours: false }, Phase::Registering] {
            assert_eq!(
                with(&Inspection::absent(), false, attempting("1.0.4", MAX_ATTEMPTS, ph), false),
                Plan::Replace(Reason::Interrupted)
            );
        }
        // A register that failed is retried under the cap and held at it.
        let failed = |n| attempting("1.0.4", n, Phase::RegisterFailed(crate::record::FailKind::Other));
        assert_eq!(
            with(&Inspection::absent(), false, failed(1), false),
            Plan::Replace(Reason::Interrupted)
        );
        assert_eq!(
            with(&Inspection::absent(), false, failed(MAX_ATTEMPTS), false),
            Plan::Leave(Hold::RegisterFailed(MAX_ATTEMPTS))
        );
    }

    #[test]
    fn a_mechanism_that_needs_a_prompt_or_has_no_pinned_identity_never_awaits_or_replaces_unattended() {
        let reg = Inspection {
            registration: Registration::Registered,
            job: Job::NotRunning,
            definition_current: Some(true),
            ..Inspection::absent()
        };
        let state = State::Attempting {
            version: "1.0.4".into(),
            attempts: 1,
            phase: Phase::Awaiting,
        };
        let with = |caps| {
            plan(&Facts {
                inspection: &reg,
                answers: false,
                covered: false,
                recorded_version: Some("1.0.0"),
                state: &state,
                current_version: "1.0.4",
                caps,
                waited: true,
                resuming: false,
                wanted: Some(true),
                first_run_done: false,
                location_ok: true,
                record_damaged: false,
            })
        };
        assert_eq!(with(UAC), Plan::MarkCurrent, "a current definition needs no replacement");
        assert_eq!(with(LINUX), Plan::MarkCurrent, "a current definition needs no replacement");
        let stale = Inspection {
            definition_current: Some(false),
            ..reg.clone()
        };
        let stale_with = |caps| {
            plan(&Facts {
                inspection: &stale,
                answers: false,
                covered: false,
                recorded_version: Some("1.0.0"),
                state: &State::Idle,
                current_version: "1.0.4",
                caps,
                waited: true,
                resuming: false,
                wanted: Some(true),
                first_run_done: false,
                location_ok: true,
                record_damaged: false,
            })
        };
        assert_eq!(stale_with(UAC), Plan::Offer(Reason::VersionChanged));
        assert_eq!(stale_with(LINUX), Plan::Replace(Reason::VersionChanged));
    }

    #[test]
    fn a_record_that_cannot_be_used_adopts_a_service_that_answers_and_replaces_nothing() {
        let up = registered(Job::Running);
        let damaged = |i: &Inspection, answers| {
            plan(&Facts {
                record_damaged: true,
                answers,
                ..facts(i, MAC, None)
            })
        };
        assert_eq!(damaged(&up, false), Plan::MarkCurrent);
        assert_eq!(damaged(&registered(Job::NotRunning), true), Plan::MarkCurrent);
        assert_eq!(
            damaged(&registered(Job::NotRunning), false),
            Plan::Replace(Reason::VersionChanged),
            "nothing is up: replaced once, as with no record"
        );
    }

    #[test]
    fn the_app_s_own_decisions_are_inputs_to_the_plan() {
        let reg = registered(Job::Running);
        let with = |wanted, first_run_done, location_ok, i: &Inspection| {
            plan(&Facts {
                wanted,
                first_run_done,
                location_ok,
                ..facts(i, MAC, Some("1.0.4"))
            })
        };
        assert_eq!(with(Some(false), true, true, &reg), Plan::Remove);
        assert_eq!(with(Some(false), true, true, &Inspection::absent()), Plan::Leave(Hold::NotInstalled));
        assert!(matches!(with(Some(true), true, false, &reg), Plan::Leave(Hold::Unavailable(_))));
        // Nothing registered with the switch on after the first run: put it right; before it: never installed unasked.
        assert_eq!(with(Some(true), true, true, &Inspection::absent()), Plan::Replace(Reason::Wanted));
        assert_eq!(with(Some(true), false, true, &Inspection::absent()), Plan::Leave(Hold::NotInstalled));
        // An unreadable switch registers nothing unattended.
        assert_eq!(with(None, true, true, &Inspection::absent()), Plan::Leave(Hold::NotInstalled));
    }

    #[test]
    fn a_newer_record_with_the_program_unknown_and_the_registration_elsewhere_is_taken_over() {
        let mut i = registered(Job::Running);
        i.points_here = Some(false);
        i.program_exists = None;
        assert_eq!(
            plan(&facts(&i, MAC, Some("1.1.0"))),
            Plan::Replace(Reason::Moved)
        );
    }
}
