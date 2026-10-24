//! [`Health`]: the registry of the app's services plus the repairs the shell runs for them. Supervisors report into it
//! ([`Health::set`], [`Health::down`], [`Health::ok`]); `health_repair` starts [`Health::repair`], which runs the
//! service's repair closure on a background thread and records how it ended. Nothing here derives health from error
//! text, and a repair never goes through the service it repairs (Paddock rule 19).

use crate::model::{Cause, RepairAction, ServiceHealth, ServiceState, Snapshot};
use crate::registry::{Registry, Repair, ServiceSpec, Sink, SpecError};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

/// Why a repair was not started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairRefused {
    /// No service has this id.
    Unknown(String),
    /// The service works (`label`): there is nothing to repair.
    Working(String),
    /// Nothing repairs the service from the shell (no repair action, or no repair registered).
    NothingRepairs(String),
    /// The fix is the user's choice in Settings: the UI opens the section instead.
    InSettings(String),
    /// The background thread could not start.
    NotStarted(String),
}

impl fmt::Display for RepairRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepairRefused::Unknown(id) => write!(f, "there is no service {id:?}"),
            RepairRefused::Working(label) => {
                write!(f, "{label} is working; there is nothing to repair")
            }
            RepairRefused::NothingRepairs(label) => write!(f, "nothing repairs {label} from here"),
            RepairRefused::InSettings(label) => write!(f, "{label} is fixed in Settings"),
            RepairRefused::NotStarted(why) => write!(f, "the repair could not start: {why}"),
        }
    }
}

impl std::error::Error for RepairRefused {}

/// Starts a repair's thread (a parameter so a test can make it fail).
type SpawnRepair = fn(String, Box<dyn FnOnce() + Send>) -> std::io::Result<()>;

/// The app's services and their repairs.
pub struct Health {
    registry: Registry,
    repairs: Mutex<HashMap<String, Repair>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Health {
    /// The registry of `services` (in this order) with their repairs; `sink` hears every change.
    pub fn new(services: Vec<ServiceSpec>, sink: Sink) -> Result<Arc<Health>, SpecError> {
        let registry = Registry::new(&services, sink)?;
        let repairs = services
            .into_iter()
            .filter_map(|s| s.repair.map(|r| (s.id, r)))
            .collect();
        Ok(Arc::new(Health {
            registry,
            repairs: Mutex::new(repairs),
        }))
    }

    /// Every service now (what a new window asks for: it must not assume everything is fine).
    pub fn snapshot(&self) -> Snapshot {
        self.registry.snapshot()
    }

    /// One service now.
    pub fn get(&self, service: &str) -> Option<ServiceHealth> {
        self.registry.get(service)
    }

    /// One service's state now.
    pub fn state(&self, service: &str) -> Option<ServiceState> {
        self.registry.get(service).map(|s| s.state)
    }

    /// Whether `service` is not working now (false for an unknown one). A command that failed because of an outage
    /// names the outage's cause instead of reporting the failure again.
    pub fn is_in_outage(&self, service: &str) -> bool {
        self.state(service).is_some_and(ServiceState::is_problem)
    }

    /// Sets what repairs `service` (the shell's own code: it never asks the service). False for an unknown service.
    pub fn set_repair(
        &self,
        service: &str,
        repair: impl Fn() -> Result<(), Cause> + Send + Sync + 'static,
    ) -> bool {
        if self.registry.get(service).is_none() {
            return false;
        }
        lock(&self.repairs).insert(service.to_string(), Arc::new(repair));
        true
    }

    /// Reports a service's state (see [`Registry::set`]: the same report again announces nothing).
    pub fn set(
        &self,
        service: &str,
        state: ServiceState,
        cause: Option<Cause>,
        repair: Option<RepairAction>,
    ) -> bool {
        self.registry.set(service, state, cause, repair).is_some()
    }

    /// The service works.
    pub fn ok(&self, service: &str) -> bool {
        self.registry.ok(service).is_some()
    }

    /// The service is not working, with a cause and what repairs it.
    pub fn down(&self, service: &str, cause: Cause, repair: Option<RepairAction>) -> bool {
        self.registry.down(service, cause, repair).is_some()
    }

    /// Starts the repair of `service` on a background thread (Paddock rule 10) and returns at once. The state is
    /// `repairing` until it ends, then `ok`, or `down` again with the repair's cause. Returns the snapshot as the repair
    /// starts (with the service `repairing`), whatever the thread has done by the time the caller looks.
    pub fn repair(self: &Arc<Self>, service: &str) -> Result<Snapshot, RepairRefused> {
        self.repair_with(service, |name, run| {
            std::thread::Builder::new().name(name).spawn(run).map(drop)
        })
    }

    /// [`Health::repair`] with the thread start passed in (tests: a start that fails).
    fn repair_with(
        self: &Arc<Self>,
        service: &str,
        spawn: SpawnRepair,
    ) -> Result<Snapshot, RepairRefused> {
        let id = self.begin_repair(service)?;
        let started = self.snapshot();
        let health = self.clone();
        let name = format!("repair-{id}");
        spawn(name, Box::new(move || health.finish_repair(&id)))
            .map_err(|e| {
                self.failed(
                    service,
                    Cause::new("The repair could not start.").detail(e.to_string()),
                );
                RepairRefused::NotStarted(e.to_string())
            })
            .map(|()| started)
    }

    /// Marks the repair as running and returns the service's id. Refuses an unknown service, one that works, one fixed
    /// in Settings, or one nothing repairs. Asking again while it runs is fine (and starts nothing new).
    pub fn begin_repair(&self, service: &str) -> Result<String, RepairRefused> {
        let current = self
            .registry
            .get(service)
            .ok_or_else(|| RepairRefused::Unknown(service.to_string()))?;
        match current.state {
            ServiceState::Ok | ServiceState::Starting => {
                return Err(RepairRefused::Working(current.label));
            }
            ServiceState::Repairing => return Ok(current.service),
            ServiceState::Degraded | ServiceState::Down | ServiceState::Quarantined => {}
        }
        let action = current
            .repair
            .ok_or_else(|| RepairRefused::NothingRepairs(current.label.clone()))?;
        if action == RepairAction::OpenSettings {
            return Err(RepairRefused::InSettings(current.label));
        }
        if !lock(&self.repairs).contains_key(service) {
            return Err(RepairRefused::NothingRepairs(current.label));
        }
        self.registry.set(
            service,
            ServiceState::Repairing,
            current.cause,
            Some(action),
        );
        Ok(current.service)
    }

    /// Runs the repair of `service` (blocking; a panic is contained) and records how it ended.
    pub fn finish_repair(&self, service: &str) {
        let repair = lock(&self.repairs).get(service).cloned();
        let result = match repair {
            Some(r) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r()))
                .unwrap_or_else(|_| Err(Cause::new("The repair failed unexpectedly."))),
            None => Err(Cause::new("Nothing repairs this from here.")),
        };
        let outcome = if result.is_ok() { "ok" } else { "failed" };
        log::info!(target: "paddock_health", Service = service, Outcome = outcome; "Repair finished");
        match result {
            Ok(()) => {
                self.registry.ok(service);
            }
            Err(cause) => self.failed(service, cause),
        }
    }

    /// The repair failed: down with its cause and the same repair action.
    fn failed(&self, service: &str, cause: Cause) {
        let repair = self.registry.get(service).and_then(|s| s.repair);
        self.registry.down(service, cause, repair);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Change;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    type Seen = Arc<Mutex<Vec<Change>>>;

    fn health() -> (Arc<Health>, Seen) {
        let seen: Seen = Arc::default();
        let s = seen.clone();
        let h = Health::new(
            vec![
                ServiceSpec::new("store", "the log store"),
                ServiceSpec::new("ai", "the AI worker"),
                ServiceSpec::new("browser", "the research browser"),
            ],
            Box::new(move |_, c| lock(&s).push(c.clone())),
        )
        .unwrap();
        (h, seen)
    }

    /// A repair that works (a named function: tests that are refused never run it).
    fn fine() -> Result<(), Cause> {
        Ok(())
    }

    fn count(seen: &Seen, service: &str, to: ServiceState) -> usize {
        lock(seen)
            .iter()
            .filter(|c| c.service == service && c.to == to)
            .count()
    }

    #[test]
    fn repairing_restarts_through_the_shell_and_clears_the_outage() {
        let (h, seen) = health();
        let runs = Arc::new(AtomicUsize::new(0));
        let r = runs.clone();
        assert!(h.set_repair("store", move || {
            r.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        assert!(h.down("store", Cause::new("stopped"), Some(RepairAction::Restart)));
        assert!(h.is_in_outage("store"));
        let id = h.begin_repair("store").unwrap();
        assert_eq!(h.state("store"), Some(ServiceState::Repairing));
        assert_eq!(
            h.get("store").unwrap().cause.unwrap().message,
            "stopped",
            "the cause stays while repairing"
        );
        h.finish_repair(&id);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(h.state("store"), Some(ServiceState::Ok));
        assert!(!h.is_in_outage("store"));
        assert_eq!(count(&seen, "store", ServiceState::Ok), 1);
    }

    #[test]
    fn a_repair_that_fails_leaves_the_service_down_with_the_new_cause_and_the_same_repair() {
        let (h, _) = health();
        h.set_repair("store", || {
            Err(Cause::new("still refused").hint("reinstall"))
        });
        h.set(
            "store",
            ServiceState::Quarantined,
            Some(Cause::new("x")),
            Some(RepairAction::Reset),
        );
        let id = h.begin_repair("store").unwrap();
        h.finish_repair(&id);
        let s = h.get("store").unwrap();
        assert_eq!(
            (s.state, s.repair),
            (ServiceState::Down, Some(RepairAction::Reset))
        );
        assert_eq!(s.cause.unwrap().message, "still refused");
    }

    #[test]
    fn a_repair_that_panics_is_contained() {
        let (h, _) = health();
        h.set_repair("ai", || panic!("boom"));
        h.set(
            "ai",
            ServiceState::Degraded,
            Some(Cause::new("slow")),
            Some(RepairAction::UseFallback),
        );
        let id = h.begin_repair("ai").unwrap();
        h.finish_repair(&id);
        let s = h.get("ai").unwrap();
        assert_eq!(s.state, ServiceState::Down);
        assert_eq!(s.cause.unwrap().message, "The repair failed unexpectedly.");
    }

    #[test]
    fn a_finish_without_a_repair_is_down() {
        let (h, _) = health();
        h.down("ai", Cause::new("x"), Some(RepairAction::Retry));
        h.finish_repair("ai");
        assert_eq!(
            h.get("ai").unwrap().cause.unwrap().message,
            "Nothing repairs this from here."
        );
    }

    #[test]
    fn repair_is_refused_for_a_working_unknown_or_unrepairable_service() {
        let (h, _) = health();
        let e = h.begin_repair("store").unwrap_err();
        assert_eq!(e, RepairRefused::Working("the log store".into()));
        assert!(e.to_string().contains("nothing to repair"));
        h.set("store", ServiceState::Starting, None, None);
        assert!(matches!(
            h.begin_repair("store"),
            Err(RepairRefused::Working(_))
        ));
        assert!(matches!(
            h.begin_repair("nope"),
            Err(RepairRefused::Unknown(_))
        ));
        assert!(!h.set_repair("nope", fine));
        h.down("store", Cause::new("x"), None);
        assert_eq!(
            h.begin_repair("store").unwrap_err(),
            RepairRefused::NothingRepairs("the log store".into()),
            "no repair action"
        );
        h.down("store", Cause::new("x"), Some(RepairAction::Restart));
        assert!(
            h.begin_repair("store")
                .unwrap_err()
                .to_string()
                .contains("nothing repairs"),
            "no repair registered"
        );
        h.set_repair("store", fine);
        assert_eq!(h.begin_repair("store").unwrap(), "store");
        assert_eq!(
            h.begin_repair("store").unwrap(),
            "store",
            "asking again while repairing is fine"
        );
    }

    #[test]
    fn a_service_fixed_in_settings_is_not_run_by_the_shell() {
        let (h, _) = health();
        h.set_repair("browser", fine);
        h.down(
            "browser",
            Cause::new("The research browser could not be started.").settings("browser"),
            Some(RepairAction::OpenSettings),
        );
        let e = h.begin_repair("browser").unwrap_err();
        assert_eq!(e, RepairRefused::InSettings("the research browser".into()));
        assert!(e.to_string().contains("Settings"));
        assert_eq!(h.state("browser"), Some(ServiceState::Down));
    }

    #[test]
    fn the_background_repair_ends_with_the_service_back() {
        let seen: Arc<Mutex<Option<mpsc::Sender<ServiceState>>>> = Arc::default();
        let (tx, rx) = mpsc::channel();
        *lock(&seen) = Some(tx);
        let s = seen.clone();
        let h = Health::new(
            vec![ServiceSpec::new("store", "the log store").repair(fine)],
            Box::new(move |_, c| {
                if let Some(tx) = lock(&s).as_ref() {
                    let _ = tx.send(c.to);
                }
            }),
        )
        .unwrap();
        h.down("store", Cause::new("x"), Some(RepairAction::Restart));
        let started = h.repair("store").unwrap();
        assert_eq!(
            started.service("store").map(|s| s.state),
            Some(ServiceState::Repairing)
        );
        let states: Vec<ServiceState> = (0..3)
            .map(|_| {
                rx.recv_timeout(Duration::from_secs(10))
                    .expect("the repair reports")
            })
            .collect();
        assert_eq!(
            states,
            [
                ServiceState::Down,
                ServiceState::Repairing,
                ServiceState::Ok
            ]
        );
        assert!(matches!(h.repair("store"), Err(RepairRefused::Working(_))));
    }

    #[test]
    fn a_repair_whose_thread_cannot_start_leaves_the_service_down_and_says_so() {
        let (h, _) = health();
        h.set_repair("store", fine);
        h.down("store", Cause::new("x"), Some(RepairAction::Restart));
        let e = h
            .repair_with("store", |_, _| Err(std::io::Error::other("no threads")))
            .unwrap_err();
        assert_eq!(e, RepairRefused::NotStarted("no threads".into()));
        assert!(e.to_string().contains("could not start"));
        let s = h.get("store").unwrap();
        assert_eq!(
            (s.state, s.repair),
            (ServiceState::Down, Some(RepairAction::Restart))
        );
        assert_eq!(s.cause.unwrap().message, "The repair could not start.");
        assert!(h.ok("store"));
        assert!(!h.ok("store"), "already ok");
    }

    #[test]
    fn a_poisoned_repair_list_keeps_working() {
        let (h, _) = health();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = h.repairs.lock().unwrap();
            panic!("poison the repairs");
        }));
        assert!(h.repairs.is_poisoned());
        assert!(h.set_repair("store", fine));
        h.down("store", Cause::new("x"), Some(RepairAction::Restart));
        let id = h.begin_repair("store").unwrap();
        h.finish_repair(&id);
        assert_eq!(h.state("store"), Some(ServiceState::Ok));
    }
}
