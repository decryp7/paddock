//! What the UI and the health registry are told: one [`ServiceStatus`] with a closed [`Condition`], the words for it,
//! and whether Repair, Install or "Open Login Items" apply. The Repair never depends on the service (rule 19): it is
//! the OS registration, served by the app.

use crate::backend::{Capabilities, Inspection, Job, Mechanism, Registration};
use crate::plan::{Hold, Plan};
use crate::tracker::RepairNotice;
use paddock_health::{Cause, RepairAction, ServiceState};
use serde::Serialize;

/// The id of the launch-at-login settings section (see [`crate::login`]); health causes about the user's switch point
/// there.
pub const LOGIN_SECTION: &str = "pd.login";

/// How the background service is doing, as a closed set the UI never has to guess from text.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Condition {
    /// It answers.
    Running,
    /// A repair is working.
    Repairing,
    /// Nothing is registered.
    NotInstalled,
    /// Registered but not answering.
    NotRunning,
    /// The OS refuses to start what is registered (a stale registration after an update).
    Refused,
    /// The registered program is gone (the app was moved or deleted).
    ProgramMissing,
    /// The registration points at another copy of the app.
    Moved,
    /// The user switched it off.
    SwitchedOff,
    /// It waits for the user's approval (macOS Login Items).
    NeedsApproval,
    /// It cannot be registered from here.
    Unavailable,
    /// A newer copy of the app looks after it.
    NewerOwner,
}

/// The background service as the UI shows it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    /// How it is registered.
    pub mechanism: Mechanism,
    /// How it is doing.
    pub condition: Condition,
    /// Something is registered.
    pub installed: bool,
    /// It answers.
    pub running: bool,
    /// What is off and what to do, in words.
    pub message: Option<String>,
    /// Repair applies (re-registers it; may show an OS prompt where [`Capabilities::automatic`] is false).
    pub can_repair: bool,
    /// Install applies.
    pub can_install: bool,
    /// "Open Login Items" applies (the user's switch; the app never flips it).
    pub can_open_login_items: bool,
    /// The repair's progress.
    pub repair: Option<RepairNotice>,
}

/// What `paddock-health` is told about the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// The state.
    pub state: ServiceState,
    /// Why it is not ok.
    pub cause: Option<Cause>,
    /// What repairs it.
    pub repair: Option<RepairAction>,
}

/// What was looked at, for [`compose`].
#[derive(Debug, Clone)]
pub struct Seen<'a> {
    /// The mechanism.
    pub mechanism: Mechanism,
    /// What it allows.
    pub caps: Capabilities,
    /// The backend's look.
    pub inspection: &'a Inspection,
    /// The service answers.
    pub answers: bool,
    /// The plan for it.
    pub plan: &'a Plan,
    /// A repair's progress.
    pub repair: Option<RepairNotice>,
    /// The app's name.
    pub app: &'a str,
}

/// Puts the observations together.
pub fn compose(s: Seen<'_>) -> ServiceStatus {
    let i = s.inspection;
    let working = s
        .repair
        .as_ref()
        .is_some_and(|r| r.state == crate::tracker::RepairState::Working);
    let (condition, message) = if working {
        (Condition::Repairing, None)
    } else {
        match (&i.registration, s.plan) {
            (Registration::SwitchedOff, _) => (
                Condition::SwitchedOff,
                Some(format!(
                    "{}'s background service is switched off in the system settings. Switch it on there; {} never does it for you.",
                    s.app, s.app
                )),
            ),
            (Registration::NeedsApproval, _) => (
                Condition::NeedsApproval,
                Some(format!("Allow {} in System Settings > General > Login Items.", s.app)),
            ),
            (Registration::Unavailable(why), _) => (Condition::Unavailable, Some(why.clone())),
            _ if s.answers => (
                Condition::Running,
                (!i.present()).then(|| {
                    "It is running outside the background service (started by hand); it stops when you log out."
                        .to_string()
                }),
            ),
            _ if !i.present() => (Condition::NotInstalled, None),
            (_, Plan::Leave(Hold::NewerOwner(v))) => (
                Condition::NewerOwner,
                Some(format!(
                    "A newer copy of {} (version {v}) looks after the background service. Open that copy, or remove the older one.",
                    s.app
                )),
            ),
            _ if i.job == Job::Refused => (
                Condition::Refused,
                Some("The system refused to start the background service (the app was updated and the old registration is still held). Repair replaces it.".to_string()),
            ),
            _ if i.program_exists == Some(false) => (
                Condition::ProgramMissing,
                Some(format!(
                    "The registered program is gone (was {} moved or deleted?). Repair registers this copy.",
                    s.app
                )),
            ),
            _ if i.points_here == Some(false) => (
                Condition::Moved,
                Some(format!("The background service belongs to another copy of {}. Repair registers this copy.", s.app)),
            ),
            _ => (
                Condition::NotRunning,
                Some("The background service is registered but does not answer yet; give it a moment, or press Repair.".to_string()),
            ),
        }
    };
    let can_repair = matches!(
        condition,
        Condition::NotRunning | Condition::Refused | Condition::ProgramMissing | Condition::Moved
    ) || (condition == Condition::Running
        && i.present()
        && matches!(s.plan, Plan::Offer(_)));
    ServiceStatus {
        mechanism: s.mechanism,
        condition,
        installed: i.present(),
        running: s.answers,
        message,
        can_repair,
        can_install: condition == Condition::NotInstalled,
        can_open_login_items: s.caps.login_items
            && matches!(condition, Condition::SwitchedOff | Condition::NeedsApproval),
        repair: s.repair,
    }
}

impl ServiceStatus {
    /// The health entry for `paddock-health` (`Health::set(service, state, cause, repair)`).
    pub fn health(&self) -> HealthReport {
        let down =
            |code: &str, msg: String, hint: &str, repair: Option<RepairAction>| HealthReport {
                state: ServiceState::Down,
                cause: Some(Cause::new(msg).hint(hint).code(format!("service.{code}"))),
                repair,
            };
        let message = self.message.clone().unwrap_or_default();
        match self.condition {
            Condition::Running => HealthReport {
                state: ServiceState::Ok,
                cause: None,
                repair: None,
            },
            Condition::Repairing => HealthReport {
                state: ServiceState::Repairing,
                cause: None,
                repair: None,
            },
            Condition::NotInstalled => down(
                "notInstalled",
                "The background service is not installed.".to_string(),
                "Press Install.",
                Some(RepairAction::Install),
            ),
            Condition::NotRunning => down(
                "notRunning",
                message,
                "Press Restart.",
                Some(RepairAction::Restart),
            ),
            Condition::Refused => down(
                "refused",
                message,
                "Press Install to register it again.",
                Some(RepairAction::Install),
            ),
            Condition::ProgramMissing => down(
                "programMissing",
                message,
                "Press Install to register this copy.",
                Some(RepairAction::Install),
            ),
            Condition::Moved => down(
                "moved",
                message,
                "Press Install to register this copy.",
                Some(RepairAction::Install),
            ),
            Condition::SwitchedOff | Condition::NeedsApproval => {
                let code = if self.condition == Condition::SwitchedOff {
                    "service.switchedOff"
                } else {
                    "service.needsApproval"
                };
                // Where the OS has a page for the switch (macOS Login Items) the shell opens it; elsewhere the app's own
                // launch-at-login section is where it is switched on again.
                let (hint, repair) = if self.can_open_login_items {
                    (
                        "Press Open Login Items… and switch it on there.",
                        RepairAction::OpenLoginItems,
                    )
                } else {
                    ("Open Settings to switch it on.", RepairAction::OpenSettings)
                };
                HealthReport {
                    state: ServiceState::Down,
                    cause: Some(
                        Cause::new(message)
                            .hint(hint)
                            .code(code)
                            .settings(LOGIN_SECTION),
                    ),
                    repair: Some(repair),
                }
            }
            Condition::Unavailable => HealthReport {
                state: ServiceState::Down,
                cause: Some(Cause::new(message).code("service.unavailable")),
                repair: None,
            },
            Condition::NewerOwner => HealthReport {
                state: ServiceState::Degraded,
                cause: Some(Cause::new(message).code("service.newerOwner")),
                repair: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Reason;
    use crate::tracker::RepairState;

    const CAPS: Capabilities = Capabilities {
        automatic: true,
        pinned_identity: true,
        login_items: true,
    };

    fn seen<'a>(i: &'a Inspection, answers: bool, plan: &'a Plan) -> Seen<'a> {
        Seen {
            mechanism: Mechanism::LaunchdBundled,
            caps: CAPS,
            inspection: i,
            answers,
            plan,
            repair: None,
            app: "Pitwall",
        }
    }

    fn with(registration: Registration, job: Job) -> Inspection {
        Inspection {
            registration,
            job,
            ..Inspection::absent()
        }
    }

    #[test]
    fn every_condition_has_words_and_the_right_controls() {
        let healthy = Plan::Leave(Hold::Healthy);
        let s = compose(seen(
            &with(Registration::Registered, Job::Running),
            true,
            &healthy,
        ));
        assert_eq!(
            (s.condition, s.message.as_deref(), s.can_repair),
            (Condition::Running, None, false)
        );
        let s = compose(seen(&Inspection::absent(), true, &healthy));
        assert!(s.message.unwrap().contains("started by hand"));
        let s = compose(seen(&Inspection::absent(), false, &healthy));
        assert_eq!(
            (s.condition, s.can_install, s.installed),
            (Condition::NotInstalled, true, false)
        );
        let s = compose(seen(
            &with(Registration::Absent, Job::Refused),
            false,
            &healthy,
        ));
        assert_eq!((s.condition, s.can_repair), (Condition::Refused, true));
        let s = compose(seen(
            &with(Registration::SwitchedOff, Job::Refused),
            false,
            &healthy,
        ));
        assert_eq!(
            (s.condition, s.can_repair, s.can_open_login_items),
            (Condition::SwitchedOff, false, true)
        );
        assert!(s.message.unwrap().contains("never does it for you"));
        let s = compose(seen(
            &with(Registration::NeedsApproval, Job::Absent),
            false,
            &healthy,
        ));
        assert_eq!(
            (s.condition, s.can_open_login_items),
            (Condition::NeedsApproval, true)
        );
        let s = compose(seen(
            &with(Registration::Unavailable("move it".into()), Job::Absent),
            false,
            &healthy,
        ));
        assert_eq!(
            (s.condition, s.message.as_deref()),
            (Condition::Unavailable, Some("move it"))
        );
        let newer = Plan::Leave(Hold::NewerOwner("2.0.0".into()));
        let s = compose(seen(
            &with(Registration::Registered, Job::NotRunning),
            false,
            &newer,
        ));
        assert_eq!((s.condition, s.can_repair), (Condition::NewerOwner, false));
        let mut gone = with(Registration::Registered, Job::NotRunning);
        gone.program_exists = Some(false);
        assert_eq!(
            compose(seen(&gone, false, &healthy)).condition,
            Condition::ProgramMissing
        );
        let mut moved = with(Registration::Registered, Job::NotRunning);
        moved.points_here = Some(false);
        assert_eq!(
            compose(seen(&moved, false, &healthy)).condition,
            Condition::Moved
        );
        let s = compose(seen(
            &with(Registration::Registered, Job::NotRunning),
            false,
            &healthy,
        ));
        assert_eq!((s.condition, s.can_repair), (Condition::NotRunning, true));
        // Running but outdated where a repair needs a prompt: offered.
        let offer = Plan::Offer(Reason::VersionChanged);
        assert!(
            compose(seen(
                &with(Registration::Registered, Job::Running),
                true,
                &offer
            ))
            .can_repair
        );
        // A repair that works hides the rest.
        let refused = with(Registration::Registered, Job::Refused);
        let mut working = seen(&refused, false, &healthy);
        working.repair = Some(RepairNotice {
            state: RepairState::Working,
            message: "m".into(),
            step: None,
        });
        let s = compose(working);
        assert_eq!((s.condition, s.can_repair), (Condition::Repairing, false));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["condition"], "repairing");
        assert_eq!(json["mechanism"], "launchdBundled");
        assert_eq!(json["canOpenLoginItems"], false);
    }

    #[test]
    fn health_has_a_repair_for_every_outage_it_can_fix() {
        let status = |condition: Condition| ServiceStatus {
            mechanism: Mechanism::SystemdUser,
            condition,
            installed: true,
            running: false,
            message: Some("words".into()),
            can_repair: false,
            can_install: false,
            can_open_login_items: false,
            repair: None,
        };
        assert_eq!(status(Condition::Running).health().state, ServiceState::Ok);
        assert_eq!(
            status(Condition::Repairing).health().state,
            ServiceState::Repairing
        );
        for (c, action) in [
            (Condition::NotInstalled, RepairAction::Install),
            (Condition::NotRunning, RepairAction::Restart),
            (Condition::Refused, RepairAction::Install),
            (Condition::ProgramMissing, RepairAction::Install),
            (Condition::Moved, RepairAction::Install),
            (Condition::SwitchedOff, RepairAction::OpenSettings),
            (Condition::NeedsApproval, RepairAction::OpenSettings),
        ] {
            let h = status(c).health();
            assert_eq!(
                (h.state, h.repair),
                (ServiceState::Down, Some(action)),
                "{c:?}"
            );
            assert!(h.cause.unwrap().code.unwrap().starts_with("service."));
        }
        let off = status(Condition::SwitchedOff).health().cause.unwrap();
        assert_eq!(off.settings.as_deref(), Some(LOGIN_SECTION));
        for c in [Condition::SwitchedOff, Condition::NeedsApproval] {
            let mut mac = status(c);
            mac.can_open_login_items = true;
            let h = mac.health();
            assert_eq!(h.repair, Some(RepairAction::OpenLoginItems));
            assert!(h.cause.unwrap().hint.unwrap().contains("Open Login Items"));
        }
        let h = status(Condition::Unavailable).health();
        assert_eq!((h.state, h.repair), (ServiceState::Down, None));
        assert_eq!(
            status(Condition::NewerOwner).health().state,
            ServiceState::Degraded
        );
    }
}
