use super::*;
use crate::health::model::{Change, ServiceState};
use std::sync::atomic::{AtomicUsize, Ordering};

type Seen = Arc<Mutex<Vec<Change>>>;

fn health() -> (Arc<Health>, Seen) {
    let seen: Seen = Arc::default();
    let s = seen.clone();
    (Health::new(Box::new(move |_, c| lock(&s).push(c.clone()))), seen)
}

fn count(seen: &Seen, service: &str, to: ServiceState) -> usize {
    lock(seen).iter().filter(|c| c.service == service && c.to == to).count()
}

#[test]
fn the_store_worker_dying_is_one_outage_however_many_calls_meet_it() {
    let (h, seen) = health();
    for _ in 0..3 {
        h.store_failed(StoreFailure::Crashed, "the store worker exited (signal: 6)");
    }
    assert_eq!(lock(&seen).len(), 1, "one announcement");
    let s = h.snapshot().services.into_iter().find(|s| s.service == STORE).unwrap();
    assert_eq!((s.state, s.repair), (ServiceState::Down, Some(RepairAction::Restart)));
    let cause = s.cause.unwrap();
    assert_eq!(cause.message, StoreFailure::Crashed.message());
    assert!(cause.detail.unwrap().contains("signal: 6"));
}

#[test]
fn a_quarantined_store_is_reset_and_a_missing_library_is_told_apart() {
    let (h, _) = health();
    h.store_failed(StoreFailure::Quarantined, "was stopped; reset it");
    let s = h.registry.get(STORE).unwrap();
    assert_eq!((s.state, s.repair), (ServiceState::Quarantined, Some(RepairAction::Reset)));
    h.store_failed(StoreFailure::LibraryMissing, "image not found");
    assert_eq!(h.registry.get(STORE).unwrap().state, ServiceState::Down);
}

#[test]
fn repairing_the_store_restarts_it_through_the_shell_and_clears_the_outage() {
    let (h, seen) = health();
    let runs = Arc::new(AtomicUsize::new(0));
    let r = runs.clone();
    h.set_repair(
        STORE,
        Arc::new(move || {
            r.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    h.store_failed(StoreFailure::Crashed, "x");
    let id = h.begin_repair("store").unwrap();
    assert_eq!(h.state(STORE), Some(ServiceState::Repairing));
    h.finish_repair(id);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(h.state(STORE), Some(ServiceState::Ok));
    assert!(h.store_failure().is_none());
    assert_eq!(count(&seen, STORE, ServiceState::Ok), 1);
}

#[test]
fn a_repair_that_fails_leaves_the_service_down_with_the_new_cause_and_the_same_repair() {
    let (h, _) = health();
    h.set_repair(STORE, Arc::new(|| Err(Cause::new("still refused").hint("reinstall"))));
    h.store_failed(StoreFailure::LibraryRefused, "x");
    let id = h.begin_repair("store").unwrap();
    h.finish_repair(id);
    let s = h.registry.get(STORE).unwrap();
    assert_eq!((s.state, s.repair), (ServiceState::Down, Some(RepairAction::Restart)));
    assert_eq!(s.cause.unwrap().message, "still refused");
}

#[test]
fn a_repair_that_panics_is_contained() {
    let (h, _) = health();
    h.set_repair(AI, Arc::new(|| panic!("boom")));
    h.ai(AiHealth::EngineLost, Some("stopped"));
    let id = h.begin_repair("ai").unwrap();
    h.finish_repair(id);
    assert_eq!(h.state(AI), Some(ServiceState::Down));
}

#[test]
fn repair_is_refused_for_a_working_unknown_or_unrepairable_service() {
    let (h, _) = health();
    assert!(h.begin_repair("store").unwrap_err().contains("nothing to repair"));
    assert!(h.begin_repair("nope").is_err());
    h.store_failed(StoreFailure::Crashed, "x");
    assert!(
        h.begin_repair("store").unwrap_err().contains("nothing repairs"),
        "no repair registered"
    );
    h.set_repair(STORE, Arc::new(|| Ok(())));
    assert!(h.begin_repair("store").is_ok());
    assert!(h.begin_repair("store").is_ok(), "asking again while repairing is fine");
}

#[test]
fn the_background_repair_ends_with_the_service_back() {
    let (h, _) = health();
    h.set_repair(STORE, Arc::new(|| Ok(())));
    h.store_failed(StoreFailure::Crashed, "x");
    h.repair("store").unwrap();
    paddock_testkit::wait_until("the repair to finish", || h.state(STORE) == Some(ServiceState::Ok));
}

#[test]
fn a_pitwall_that_cannot_serve_is_one_degraded_service_with_a_retry_and_clears_when_it_serves() {
    let (h, seen) = health();
    h.ai(AiHealth::EngineLost, Some("Pitwall stopped."));
    h.ai(AiHealth::EngineLost, Some("Pitwall stopped."));
    let s = h.registry.get(AI).unwrap();
    assert_eq!((s.state, s.repair), (ServiceState::Degraded, Some(RepairAction::Retry)));
    assert_eq!(lock(&seen).len(), 1, "the same state is announced once");
    h.ai(AiHealth::Fine, None);
    assert_eq!(h.state(AI), Some(ServiceState::Ok));
    // A failure of another kind (the library is missing) replaces it.
    h.store_failed(StoreFailure::LibraryMissing, "image not found");
    assert!(h.registry.get(STORE).unwrap().cause.unwrap().raise_cap.is_none());
}

fn stop(service: &'static str, plugin: Option<&str>, target: &str) -> crate::performance::watch::Stop {
    crate::performance::watch::Stop {
        service,
        plugin: plugin.map(str::to_string),
        cause: Cause::new("It was stopped: it used more than its cap")
            .hint("Raise the cap")
            .raise_cap(target, 3328),
    }
}

#[test]
fn trying_again_after_a_cap_stop_restarts_the_service_and_drops_the_advice() {
    let (h, _) = health();
    h.set_repair(STORE, Arc::new(|| Ok(())));
    h.memory_stopped(STORE, None, stop(STORE, None, "storeWorker").cause);
    let id = h.begin_repair(STORE).unwrap();
    h.finish_repair(id);
    assert_eq!(h.state(STORE), Some(ServiceState::Ok));
}

#[test]
fn a_plugin_stopped_by_its_memory_cap_joins_the_plugins_service_with_the_advice_until_allowed_again() {
    let (h, _) = health();
    h.memory_stopped(PLUGINS, Some("net.x"), stop(PLUGINS, Some("net.x"), "plugin:net.x").cause);
    let s = h.registry.get(PLUGINS).unwrap();
    assert_eq!((s.state, s.repair), (ServiceState::Quarantined, Some(RepairAction::LiftQuarantine)));
    assert_eq!(s.cause.unwrap().raise_cap.map(|c| c.target), Some("plugin:net.x".to_string()));
    h.plugin_reset("net.x");
    assert_eq!(h.state(PLUGINS), Some(ServiceState::Ok));
}
