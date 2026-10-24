//! Pitwall in the app's service health (`paddock-health`, ADR 0010): an optional service that is *degraded*, never down, and
//! always has a Repair that does not depend on Pitwall (rule 19: "Use the built-in engine", or the Settings section when the
//! app has no engine of its own). [`report`] turns an [`AiState`] into the one health entry, announced once per change.

use crate::status::AiState;
use paddock_health::{Cause, Health, RepairAction, ServiceSpec, ServiceState};

/// The id apps register Pitwall under (they may use another).
pub const SERVICE_ID: &str = "ai";

/// What the Repair does when Pitwall is wanted but unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// The app has its own engine: the repair switches the session to it.
    BuiltIn,
    /// The app has none: the repair opens this Settings section, where the user connects or turns the feature off.
    Settings(String),
}

/// The registration of the Pitwall service. `use_fallback` is the app's switch to its own engine (it never asks Pitwall).
pub fn service_spec(
    id: &str,
    label: &str,
    use_fallback: impl Fn() -> Result<(), Cause> + Send + Sync + 'static,
) -> ServiceSpec {
    ServiceSpec::new(id, label).repair(use_fallback)
}

/// Reports `state` into `health` under `service`. `wanted`: the user's setting needs Pitwall (Pitwall, or Automatic after it
/// served this session); when Pitwall is merely optional and absent, the entry stays `ok`. Returns whether anything
/// changed (the same report twice announces nothing).
pub fn report(
    health: &Health,
    service: &str,
    state: &AiState,
    wanted: bool,
    app: &str,
    fallback: &Fallback,
) -> bool {
    if !wanted || matches!(state, AiState::Disabled | AiState::Ready { .. }) {
        return health.ok(service);
    }
    let code = match state {
        AiState::NotInstalled => "pitwall.notInstalled",
        AiState::NotRunning => "pitwall.notRunning",
        AiState::Incompatible { .. } => "pitwall.incompatible",
        AiState::NeedsPairing => "pitwall.needsPairing",
        AiState::Paused { .. } => "pitwall.paused",
        AiState::NoSuitableModel { .. } => "pitwall.noModel",
        AiState::Unavailable { .. } | AiState::Disabled | AiState::Ready { .. } => {
            "pitwall.unavailable"
        }
    };
    let (hint, repair) = match fallback {
        Fallback::BuiltIn => (
            "Choose Use the built-in engine to carry on without Pitwall.".to_string(),
            RepairAction::UseFallback,
        ),
        Fallback::Settings(_) => (
            "Open the settings to connect Pitwall or turn the feature off.".to_string(),
            RepairAction::OpenSettings,
        ),
    };
    let mut cause = Cause::new(state.message(app)).hint(hint).code(code);
    if let Fallback::Settings(section) = fallback {
        cause = cause.settings(section.clone());
    }
    health.set(service, ServiceState::Degraded, Some(cause), Some(repair))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn health() -> (Arc<Health>, Arc<Mutex<Vec<ServiceState>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let h = Health::new(
            vec![service_spec(SERVICE_ID, "Pitwall", || Ok(()))],
            Box::new(move |_, c| s.lock().unwrap().push(c.to)),
        )
        .unwrap();
        (h, seen)
    }

    #[test]
    fn an_absent_optional_pitwall_is_not_a_problem() {
        let (h, seen) = health();
        assert!(!report(
            &h,
            SERVICE_ID,
            &AiState::NotInstalled,
            false,
            "Debrief",
            &Fallback::BuiltIn
        ));
        assert_eq!(h.state(SERVICE_ID), Some(ServiceState::Ok));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_wanted_pitwall_that_is_unusable_is_degraded_with_a_repair_that_does_not_need_it() {
        let (h, seen) = health();
        for (state, code) in [
            (AiState::NotInstalled, "pitwall.notInstalled"),
            (AiState::NotRunning, "pitwall.notRunning"),
            (AiState::NeedsPairing, "pitwall.needsPairing"),
            (AiState::Paused { stopped: false }, "pitwall.paused"),
            (
                AiState::NoSuitableModel {
                    needs: crate::Caps::chat(8192),
                },
                "pitwall.noModel",
            ),
            (
                AiState::Incompatible {
                    message: "m".into(),
                    update_app: true,
                },
                "pitwall.incompatible",
            ),
            (
                AiState::Unavailable {
                    message: "m".into(),
                },
                "pitwall.unavailable",
            ),
        ] {
            assert!(
                report(&h, SERVICE_ID, &state, true, "Debrief", &Fallback::BuiltIn),
                "{state:?}"
            );
            let entry = h.get(SERVICE_ID).unwrap();
            assert_eq!(
                entry.state,
                ServiceState::Degraded,
                "never down: Pitwall is optional"
            );
            assert_eq!(entry.repair, Some(RepairAction::UseFallback));
            assert_eq!(
                entry.cause.as_ref().and_then(|c| c.code.as_deref()),
                Some(code)
            );
            // Saying it again announces nothing.
            assert!(!report(
                &h,
                SERVICE_ID,
                &state,
                true,
                "Debrief",
                &Fallback::BuiltIn
            ));
        }
        // The repair works whatever Pitwall does.
        assert!(h.repair(SERVICE_ID).is_ok());
        assert!(!seen.lock().unwrap().contains(&ServiceState::Down));
    }

    #[test]
    fn an_app_without_an_engine_of_its_own_repairs_through_settings_and_recovery_clears_it() {
        let (h, _) = health();
        let fallback = Fallback::Settings("pitwall".into());
        assert!(report(
            &h,
            SERVICE_ID,
            &AiState::NeedsPairing,
            true,
            "Pacenotes",
            &fallback
        ));
        let e = h.get(SERVICE_ID).unwrap();
        assert_eq!(
            (
                e.repair,
                e.cause.unwrap().settings.as_deref().map(str::to_string)
            ),
            (
                Some(RepairAction::OpenSettings),
                Some("pitwall".to_string())
            )
        );
        let ready = AiState::Ready {
            model: "m".into(),
            vision: false,
            context_tokens: 4096,
        };
        assert!(report(&h, SERVICE_ID, &ready, true, "Pacenotes", &fallback));
        assert_eq!(h.state(SERVICE_ID), Some(ServiceState::Ok));
        assert!(report(
            &h,
            SERVICE_ID,
            &AiState::NeedsPairing,
            true,
            "Pacenotes",
            &fallback
        ));
        assert!(report(
            &h,
            SERVICE_ID,
            &AiState::Disabled,
            true,
            "Pacenotes",
            &fallback
        ));
        assert_eq!(h.state(SERVICE_ID), Some(ServiceState::Ok));
    }
}
