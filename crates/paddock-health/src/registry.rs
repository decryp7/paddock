//! The registry: one entry per service the app registered, updated by that service's supervisor, announcing every real
//! change once. Pure: no clock, no threads, no I/O; the sink (an event to every window, a notification) is passed in.

use crate::model::{
    Cause, Change, MAX_SERVICE_ID, RepairAction, ServiceHealth, ServiceState, Snapshot,
    is_service_id,
};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// Told after each change, outside the registry's lock.
pub type Sink = Box<dyn Fn(&Snapshot, &Change) + Send + Sync>;

/// Brings a service back. Runs on a background thread, in the shell: it must not need the broken service to work
/// (Paddock rule 19). `Err` is why it is still down.
pub type Repair = Arc<dyn Fn() -> Result<(), Cause> + Send + Sync>;

/// A service the app registers: its id, what the user calls it, how it starts and what repairs it.
#[derive(Clone)]
pub struct ServiceSpec {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) initial: ServiceState,
    pub(crate) repair: Option<Repair>,
}

impl ServiceSpec {
    /// A service that starts `ok`, with no repair yet.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> ServiceSpec {
        ServiceSpec {
            id: id.into(),
            label: label.into(),
            initial: ServiceState::Ok,
            repair: None,
        }
    }

    /// The service starts in `starting` (its supervisor reports `ok` when it runs).
    pub fn starting(mut self) -> ServiceSpec {
        self.initial = ServiceState::Starting;
        self
    }

    /// What repairs it (also settable later with [`crate::Health::set_repair`], when the closure needs the app).
    pub fn repair(
        mut self,
        repair: impl Fn() -> Result<(), Cause> + Send + Sync + 'static,
    ) -> ServiceSpec {
        self.repair = Some(Arc::new(repair));
        self
    }

    /// The id.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Debug for ServiceSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceSpec")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("initial", &self.initial)
            .field("repair", &self.repair.is_some())
            .finish()
    }
}

/// Why a list of services cannot be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// Not 1 to 64 ASCII letters, digits, `-`, `_` or `.`, starting with a letter.
    InvalidId(String),
    /// Two services have this id.
    Duplicate(String),
    /// The service has no label.
    EmptyLabel(String),
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecError::InvalidId(id) => write!(
                f,
                "{id:?} is not a service id (1 to {MAX_SERVICE_ID} ASCII letters, digits, '-', '_' or '.', starting with a letter)"
            ),
            SpecError::Duplicate(id) => write!(f, "the service {id:?} is registered twice"),
            SpecError::EmptyLabel(id) => write!(f, "the service {id:?} has no label"),
        }
    }
}

impl std::error::Error for SpecError {}

/// Checks a list of services: valid, unique ids and a label each.
pub(crate) fn validate(services: &[ServiceSpec]) -> Result<(), SpecError> {
    for (i, s) in services.iter().enumerate() {
        if !is_service_id(&s.id) {
            return Err(SpecError::InvalidId(s.id.clone()));
        }
        if s.label.trim().is_empty() {
            return Err(SpecError::EmptyLabel(s.id.clone()));
        }
        if services[..i].iter().any(|o| o.id == s.id) {
            return Err(SpecError::Duplicate(s.id.clone()));
        }
    }
    Ok(())
}

struct Inner {
    revision: u64,
    services: Vec<ServiceHealth>,
}

/// Every registered service's state, with one announcement per real change.
pub struct Registry {
    inner: Mutex<Inner>,
    sink: Sink,
}

impl Registry {
    /// A registry of `services`, each in its initial state, in this order.
    pub fn new(services: &[ServiceSpec], sink: Sink) -> Result<Registry, SpecError> {
        validate(services)?;
        Ok(Registry {
            inner: Mutex::new(Inner {
                revision: 0,
                services: services
                    .iter()
                    .map(|s| ServiceHealth {
                        service: s.id.clone(),
                        label: s.label.clone(),
                        state: s.initial,
                        cause: None,
                        repair: None,
                    })
                    .collect(),
            }),
            sink,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Every service now.
    pub fn snapshot(&self) -> Snapshot {
        let g = self.lock();
        Snapshot {
            revision: g.revision,
            services: g.services.clone(),
        }
    }

    /// One service now.
    pub fn get(&self, service: &str) -> Option<ServiceHealth> {
        self.lock()
            .services
            .iter()
            .find(|s| s.service == service)
            .cloned()
    }

    /// Sets a service's state. Saying the same thing again changes nothing and announces nothing (that is the dedupe);
    /// an unknown service is ignored; `ok` drops a stray cause and repair. Returns the change, if there was one.
    pub fn set(
        &self,
        service: &str,
        state: ServiceState,
        cause: Option<Cause>,
        repair: Option<RepairAction>,
    ) -> Option<Change> {
        let (snapshot, change) = {
            let mut g = self.lock();
            let revision = g.revision + 1;
            let (cause, repair) = if state == ServiceState::Ok {
                (None, None)
            } else {
                (cause, repair)
            };
            let entry = g.services.iter_mut().find(|s| s.service == service)?;
            if entry.state == state && entry.cause == cause && entry.repair == repair {
                return None;
            }
            let change = Change {
                service: service.into(),
                from: entry.state,
                to: state,
            };
            entry.state = state;
            entry.cause = cause;
            entry.repair = repair;
            g.revision = revision;
            (
                Snapshot {
                    revision,
                    services: g.services.clone(),
                },
                change,
            )
        };
        (self.sink)(&snapshot, &change);
        Some(change)
    }

    /// The service works.
    pub fn ok(&self, service: &str) -> Option<Change> {
        self.set(service, ServiceState::Ok, None, None)
    }

    /// The service is not working, with a cause and what repairs it.
    pub fn down(
        &self,
        service: &str,
        cause: Cause,
        repair: Option<RepairAction>,
    ) -> Option<Change> {
        self.set(service, ServiceState::Down, Some(cause), repair)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    type Seen = Arc<Mutex<Vec<(u64, Change)>>>;

    /// A sink that ignores changes (a named function: lists that are refused never call it).
    fn ignore(_: &Snapshot, _: &Change) {}

    fn fine() -> Result<(), Cause> {
        Ok(())
    }

    fn registry() -> (Registry, Seen) {
        let seen: Seen = Arc::default();
        let s = seen.clone();
        let r = Registry::new(
            &[
                ServiceSpec::new("store", "the log store"),
                ServiceSpec::new("ai", "the AI worker"),
            ],
            Box::new(move |snap, c| s.lock().unwrap().push((snap.revision, c.clone()))),
        )
        .unwrap();
        (r, seen)
    }

    #[test]
    fn every_service_starts_ok_in_the_given_order() {
        let (r, _) = registry();
        let s = r.snapshot();
        assert_eq!(s.revision, 0);
        assert_eq!(
            s.services
                .iter()
                .map(|s| s.service.as_str())
                .collect::<Vec<_>>(),
            ["store", "ai"]
        );
        assert!(
            s.services
                .iter()
                .all(|s| s.state == ServiceState::Ok && s.cause.is_none())
        );
    }

    #[test]
    fn a_service_can_start_in_starting() {
        let r = Registry::new(
            &[ServiceSpec::new("llm", "the model").starting()],
            Box::new(ignore),
        )
        .unwrap();
        assert_eq!(r.get("llm").unwrap().state, ServiceState::Starting);
        assert_eq!(r.ok("llm").map(|c| c.from), Some(ServiceState::Starting));
    }

    #[test]
    fn the_same_failure_three_times_is_announced_once() {
        let (r, seen) = registry();
        for _ in 0..3 {
            r.down(
                "store",
                Cause::new("The log store stopped."),
                Some(RepairAction::Restart),
            );
        }
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(r.snapshot().revision, 1);
        let h = r.get("store").unwrap();
        assert_eq!(
            (h.state, h.repair),
            (ServiceState::Down, Some(RepairAction::Restart))
        );
    }

    #[test]
    fn a_new_cause_updates_in_place_and_ok_clears_the_cause() {
        let (r, seen) = registry();
        r.down("store", Cause::new("a"), Some(RepairAction::Restart));
        r.down(
            "store",
            Cause::new("b").hint("do x"),
            Some(RepairAction::Restart),
        );
        assert_eq!(r.get("store").unwrap().cause.unwrap().message, "b");
        let c = r.ok("store").unwrap();
        assert_eq!((c.from, c.to), (ServiceState::Down, ServiceState::Ok));
        assert!(r.get("store").unwrap().cause.is_none());
        assert_eq!(seen.lock().unwrap().len(), 3);
        assert!(r.ok("store").is_none(), "already ok");
        assert_eq!(
            seen.lock()
                .unwrap()
                .iter()
                .map(|(rev, _)| *rev)
                .collect::<Vec<_>>(),
            [1, 2, 3],
            "the revision grows with each change"
        );
    }

    #[test]
    fn an_unknown_service_is_ignored_and_ok_ignores_a_stray_cause() {
        let (r, seen) = registry();
        assert!(r.down("nope", Cause::new("x"), None).is_none());
        assert!(
            r.set(
                "ai",
                ServiceState::Ok,
                Some(Cause::new("stale")),
                Some(RepairAction::Retry)
            )
            .is_none()
        );
        assert!(seen.lock().unwrap().is_empty());
        assert!(r.get("nope").is_none());
    }

    #[test]
    fn states_are_problems_or_not() {
        use ServiceState::*;
        assert!([Ok, Starting].iter().all(|s| s.is_ok()));
        assert!(
            [Degraded, Down, Quarantined, Repairing]
                .iter()
                .all(|s| s.is_problem())
        );
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let (r, _) = registry();
        r.set(
            "ai",
            ServiceState::Quarantined,
            Some(
                Cause::new("m")
                    .detail(" ")
                    .code("ai.switched")
                    .raise_cap("ai", 2048),
            ),
            Some(RepairAction::LiftQuarantine),
        );
        let v = serde_json::to_value(r.snapshot()).unwrap();
        assert_eq!(v["services"][1]["state"], "quarantined");
        assert_eq!(v["services"][1]["repair"], "liftQuarantine");
        let cause = &v["services"][1]["cause"];
        assert_eq!(cause["detail"], serde_json::Value::Null);
        assert_eq!(cause["code"], "ai.switched");
        assert_eq!(
            cause["raiseCap"],
            serde_json::json!({"target": "ai", "suggestedMb": 2048})
        );
        assert_eq!(cause["settings"], serde_json::Value::Null);
        // A cause written before `code`, `raiseCap` and `settings` existed still reads.
        let old: Cause = serde_json::from_value(
            serde_json::json!({"message": "m", "hint": null, "detail": null}),
        )
        .unwrap();
        assert_eq!(old, Cause::new("m"));
        assert_eq!(Cause::new("plain").to_string(), "plain");
        assert_eq!(
            r.snapshot().service("ai").map(|s| s.state),
            Some(ServiceState::Quarantined)
        );
        assert!(r.snapshot().service("x").is_none());
    }

    #[test]
    fn service_lists_are_checked() {
        let sink = || -> Sink { Box::new(ignore) };
        let bad = |specs: &[ServiceSpec]| Registry::new(specs, sink()).err().unwrap();
        assert_eq!(
            bad(&[ServiceSpec::new("", "x")]),
            SpecError::InvalidId(String::new())
        );
        assert_eq!(
            bad(&[ServiceSpec::new("1st", "x")]),
            SpecError::InvalidId("1st".into())
        );
        assert_eq!(
            bad(&[ServiceSpec::new("a b", "x")]),
            SpecError::InvalidId("a b".into())
        );
        assert!(matches!(
            bad(&[ServiceSpec::new("a".repeat(65), "x")]),
            SpecError::InvalidId(_)
        ));
        assert_eq!(
            bad(&[ServiceSpec::new("a", " ")]),
            SpecError::EmptyLabel("a".into())
        );
        assert_eq!(
            bad(&[ServiceSpec::new("a", "x"), ServiceSpec::new("a", "y")]),
            SpecError::Duplicate("a".into())
        );
        assert!(Registry::new(&[ServiceSpec::new("plugin.x-1_a", "x")], sink()).is_ok());
        assert!(Registry::new(&[], sink()).is_ok());
        for e in [
            SpecError::InvalidId("1".into()),
            SpecError::Duplicate("a".into()),
            SpecError::EmptyLabel("a".into()),
        ] {
            assert!(e.to_string().contains("service"), "{e}");
        }
        let spec = ServiceSpec::new("a", "x").repair(fine);
        assert_eq!(spec.id(), "a");
        assert!(format!("{spec:?}").contains("repair: true"));
        assert_eq!(spec.repair.as_ref().map(|r| r()), Some(Ok(())));
    }

    #[test]
    fn a_poisoned_registry_keeps_working() {
        let (r, seen) = registry();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = r.inner.lock().unwrap();
            panic!("poison the registry");
        }));
        assert!(r.inner.is_poisoned());
        assert!(r.down("store", Cause::new("x"), None).is_some());
        assert_eq!(r.snapshot().revision, 1);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
