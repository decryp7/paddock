//! What a service-health change means for the notification centre: a service that stops working raises one app-wide
//! message with the cause `health:<service>` (updated in place while the outage lasts), and a service that works again
//! resolves it. The chip and the message say the same thing; the message is never raised twice for one outage.

use super::centre::{NewNotice, NoticeAction, NoticeScope, NoticeSeverity, NoticeSource};
use crate::model::{Change, RepairAction, ServiceState, Snapshot, cause_key};

/// What the centre should do for one health change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthNotice {
    /// Post (or update in place) the outage's message.
    Post(Box<NewNotice>),
    /// The service works again: resolve the message of this cause.
    Resolve(String),
    /// Nothing (a repair in progress keeps the message; starting is not an outage).
    Nothing,
}

/// The centre's part of `change`.
pub fn for_change(snapshot: &Snapshot, change: &Change) -> HealthNotice {
    if change.to == ServiceState::Ok {
        return HealthNotice::Resolve(cause_key(&change.service));
    }
    match notice_for(snapshot, change) {
        Some(n) => HealthNotice::Post(Box::new(n)),
        None => HealthNotice::Nothing,
    }
}

/// The message of a service that is not working, or nothing (working, starting, being repaired, or no cause given).
/// Degraded is a warning, anything worse an error; the button repairs from the shell, or opens Settings when that is
/// where it is fixed.
pub fn notice_for(snapshot: &Snapshot, change: &Change) -> Option<NewNotice> {
    let entry = snapshot.service(&change.service)?;
    // The repair in progress keeps the message; the outcome (ok / down again) changes it.
    if !entry.state.is_problem() || entry.state == ServiceState::Repairing {
        return None;
    }
    let cause = entry.cause.as_ref()?;
    let severity = if entry.state == ServiceState::Degraded {
        NoticeSeverity::Warning
    } else {
        NoticeSeverity::Error
    };
    let mut n = NewNotice::new(
        cause_key(&entry.service),
        NoticeSource::health(),
        severity,
        cause.message.clone(),
        NoticeScope::App,
    )
    .body(cause.hint.clone().unwrap_or_default());
    if let Some(code) = &cause.code {
        n = n.code(code.clone());
    }
    match (entry.repair, &cause.settings) {
        (Some(RepairAction::OpenSettings), Some(section)) => {
            n = n.primary(NoticeAction::open_settings(section.to_string()));
        }
        (Some(RepairAction::OpenSettings), None) | (None, _) => {}
        (Some(_), _) => {
            n = n.primary(NoticeAction::repair_service(entry.service.clone()));
        }
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cause, ServiceHealth};
    use crate::notify::centre::{Centre, NoticeId};

    fn snapshot_with(
        service: &str,
        state: ServiceState,
        cause: Option<Cause>,
        repair: Option<RepairAction>,
    ) -> (Snapshot, Change) {
        let snapshot = Snapshot {
            revision: 1,
            services: vec![ServiceHealth {
                service: service.into(),
                label: "the log store".into(),
                state,
                cause,
                repair,
            }],
        };
        let change = Change {
            service: service.into(),
            from: ServiceState::Ok,
            to: state,
        };
        (snapshot, change)
    }

    #[test]
    fn an_outage_is_one_error_notification_with_the_services_cause_key_and_a_repair_button() {
        let (s, c) = snapshot_with(
            "store",
            ServiceState::Down,
            Some(
                Cause::new("The log store stopped unexpectedly.")
                    .hint("Choose Try again.")
                    .code("store.crashed"),
            ),
            Some(RepairAction::Restart),
        );
        let n = notice_for(&s, &c).unwrap();
        assert_eq!(
            (n.cause.as_str(), n.severity, n.source.as_str(), n.scope),
            (
                "health:store",
                NoticeSeverity::Error,
                "health",
                NoticeScope::App
            )
        );
        assert_eq!(n.title, "The log store stopped unexpectedly.");
        assert_eq!(n.body, "Choose Try again.");
        assert_eq!(n.code.as_deref(), Some("store.crashed"));
        assert_eq!(n.primary, Some(NoticeAction::repair_service("store")));
        assert_eq!(for_change(&s, &c), HealthNotice::Post(Box::new(n)));
    }

    #[test]
    fn the_same_outage_three_times_is_one_notification_and_ok_resolves_it() {
        let (s, c) = snapshot_with(
            "store",
            ServiceState::Down,
            Some(Cause::new("stopped")),
            Some(RepairAction::Restart),
        );
        let mut centre = Centre::new(10);
        for i in 0..3u64 {
            centre.post(notice_for(&s, &c).unwrap(), NoticeId(i + 1), 100 + i as i64);
        }
        assert_eq!(centre.notices().len(), 1, "same cause: updated in place");
        assert_eq!(centre.notices()[0].count, 3);
        let (ok, back) = snapshot_with("store", ServiceState::Ok, None, None);
        assert_eq!(
            for_change(&ok, &back),
            HealthNotice::Resolve("health:store".into())
        );
        assert!(centre.resolve_cause("health:store").is_some());
        assert!(centre.notices().is_empty());
    }

    #[test]
    fn a_repair_in_progress_starting_and_a_missing_cause_post_nothing() {
        for (state, cause) in [
            (ServiceState::Repairing, Some(Cause::new("stopped"))),
            (ServiceState::Starting, None),
            (ServiceState::Down, None),
        ] {
            let (s, c) = snapshot_with("store", state, cause, Some(RepairAction::Restart));
            assert!(notice_for(&s, &c).is_none(), "{state:?}");
            assert_eq!(for_change(&s, &c), HealthNotice::Nothing);
        }
        let (s, _) = snapshot_with("store", ServiceState::Down, Some(Cause::new("x")), None);
        let other = Change {
            service: "nope".into(),
            from: ServiceState::Ok,
            to: ServiceState::Down,
        };
        assert!(
            notice_for(&s, &other).is_none(),
            "a service that is not in the snapshot"
        );
    }

    #[test]
    fn a_degraded_service_is_a_warning_without_a_button_when_nothing_repairs_it() {
        let (s, c) = snapshot_with("ai", ServiceState::Degraded, Some(Cause::new("slow")), None);
        let n = notice_for(&s, &c).unwrap();
        assert_eq!(n.severity, NoticeSeverity::Warning);
        assert!(n.primary.is_none());
    }

    #[test]
    fn a_service_fixed_in_settings_opens_that_section_and_without_one_offers_nothing() {
        let (s, c) = snapshot_with(
            "browser",
            ServiceState::Down,
            Some(Cause::new("The research browser could not be started.").settings("browser")),
            Some(RepairAction::OpenSettings),
        );
        assert_eq!(
            notice_for(&s, &c).unwrap().primary,
            Some(NoticeAction::open_settings("browser"))
        );
        let (s, c) = snapshot_with(
            "browser",
            ServiceState::Down,
            Some(Cause::new("x")),
            Some(RepairAction::OpenSettings),
        );
        assert!(notice_for(&s, &c).unwrap().primary.is_none());
    }
}
