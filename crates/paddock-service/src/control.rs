//! The service as the app's commands and health registry use it: [`ServiceControl`]. It owns the [`Manager`] behind a
//! lock, runs every slow step on its own thread (rule 10: a command returns at once and the outcome arrives as a
//! [`ServiceUpdate`]), answers status from the last look while a repair holds the manager, keeps `paddock-health`'s
//! entry current ([`ServiceStatus::health`]) and is the health registry's repair (rule 19: it is the OS registration,
//! never the service). No Tauri here: the adapter (`tauri` feature) only turns its calls into commands and its updates
//! into events, so everything is tested without a runtime.

use crate::error::ServiceError;
use crate::manager::{Manager, Started};
use crate::repair::RepairOutcome;
use crate::status::{Condition, ServiceStatus};
use paddock_health::{Cause, Health};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Why a service command was refused or failed. On the wire `{ code, message, retryable }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ServiceCommandError {
    /// What kind.
    pub code: ServiceErrorCode,
    /// Words for the user, with the next step.
    pub message: String,
    /// Whether trying again can help.
    pub retryable: bool,
}

/// The kinds of [`ServiceCommandError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ServiceErrorCode {
    /// The app has no background service set up.
    NotSetUp,
    /// A registration or repair is running.
    Busy,
    /// The user switched it off (Login Items, Startup, Services).
    SwitchedOff,
    /// macOS waits for the user's approval in Login Items.
    NeedsApproval,
    /// Administrator rights are needed.
    NeedsElevation,
    /// The OS prompt was declined.
    Declined,
    /// It cannot be registered from here.
    Unavailable,
    /// A newer copy of the app looks after it.
    NewerOwner,
    /// It did not work.
    Failed,
}

impl From<&ServiceError> for ServiceCommandError {
    fn from(e: &ServiceError) -> Self {
        let (code, retryable) = match e {
            ServiceError::Busy => (ServiceErrorCode::Busy, true),
            ServiceError::SwitchedOff => (ServiceErrorCode::SwitchedOff, false),
            ServiceError::NeedsApproval => (ServiceErrorCode::NeedsApproval, false),
            ServiceError::NeedsElevation => (ServiceErrorCode::NeedsElevation, true),
            ServiceError::Declined => (ServiceErrorCode::Declined, true),
            ServiceError::Unavailable(_) | ServiceError::NoHome => {
                (ServiceErrorCode::Unavailable, false)
            }
            ServiceError::NewerOwner(_) => (ServiceErrorCode::NewerOwner, false),
            ServiceError::Failed(_) | ServiceError::Io { .. } => (ServiceErrorCode::Failed, true),
        };
        ServiceCommandError {
            code,
            message: e.to_string(),
            retryable,
        }
    }
}

impl ServiceCommandError {
    fn busy() -> Self {
        Self::from(&ServiceError::Busy)
    }
}

/// What every window hears after the service changed (a repair's step or end, a status look).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ServiceUpdate {
    /// The service now.
    pub status: ServiceStatus,
    /// Why the request that ended here did not work, if it did not.
    pub error: Option<ServiceCommandError>,
}

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Repair.
    Repair,
    /// Install (consent).
    Install,
}

/// Where updates go (the adapter emits them to every window).
pub type Emit = Arc<dyn Fn(&ServiceUpdate) + Send + Sync>;

/// The app's background service, shared by its commands, its health entry and the start-up checks.
pub struct ServiceControl {
    manager: Mutex<Manager>,
    tracker: Arc<crate::tracker::RepairTracker>,
    last: Mutex<Option<ServiceStatus>>,
    health: Option<(Arc<Health>, String)>,
    emit: Emit,
    cancel: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ServiceControl {
    /// Controls `manager`; updates go to `emit`.
    pub fn new(manager: Manager, emit: Emit) -> Arc<Self> {
        Self::build(manager, emit, None)
    }

    /// Also keeps the health entry `service` of `health` current and makes this its repair (the service must be declared
    /// in the health registry; returns `None` when it is not).
    pub fn with_health(
        manager: Manager,
        emit: Emit,
        health: Arc<Health>,
        service: &str,
    ) -> Option<Arc<Self>> {
        let control = Self::build(manager, emit, Some((health.clone(), service.to_string())));
        Self::wire(&control, &health, service).then_some(control)
    }

    /// [`ServiceControl::with_health`], or without health when `service` is not declared (logged).
    pub fn with_health_or_plain(
        manager: Manager,
        emit: Emit,
        health: Arc<Health>,
        service: &str,
    ) -> Arc<Self> {
        let control = Self::build(manager, emit, Some((health.clone(), service.to_string())));
        if !Self::wire(&control, &health, service) {
            log::error!(service; "the service's health entry is not declared in the app's health services");
        }
        control
    }

    fn wire(control: &Arc<Self>, health: &Arc<Health>, service: &str) -> bool {
        let weak = Arc::downgrade(control);
        health.set_repair(service, move || match weak.upgrade() {
            Some(c) => c.health_repair(),
            None => Err(Cause::new("The app is closing.")),
        })
    }

    fn build(manager: Manager, emit: Emit, health: Option<(Arc<Health>, String)>) -> Arc<Self> {
        Arc::new(Self {
            tracker: manager.tracker(),
            manager: Mutex::new(manager),
            last: Mutex::new(None),
            health,
            emit,
            cancel: AtomicBool::new(false),
        })
    }

    /// Asks a running repair to stop where nothing is left half-done (at app exit).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn publish(&self, status: &ServiceStatus, error: Option<ServiceCommandError>) {
        *lock(&self.last) = Some(status.clone());
        if let Some((health, id)) = &self.health {
            let r = status.health();
            health.set(id, r.state, r.cause, r.repair);
        }
        (self.emit)(&ServiceUpdate {
            status: status.clone(),
            error,
        });
    }

    /// The service now. While a repair holds the manager, the last look with the repair's live progress (bounded: the
    /// OS is never asked while it works). Blocking (bounded helper calls): call it off the IPC thread.
    pub fn status(&self) -> Option<ServiceStatus> {
        match self.manager.try_lock() {
            Ok(mut m) => {
                let s = m.status();
                drop(m);
                self.publish(&s, None);
                Some(s)
            }
            Err(std::sync::TryLockError::Poisoned(p)) => {
                let s = p.into_inner().status();
                self.publish(&s, None);
                Some(s)
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                let mut s = lock(&self.last).clone()?;
                s.repair = self.tracker.current(Instant::now());
                if s.repair
                    .as_ref()
                    .is_some_and(|r| r.state == crate::tracker::RepairState::Working)
                {
                    s.condition = Condition::Repairing;
                }
                Some(s)
            }
        }
    }

    /// Runs `what` now (blocking, bounded; see [`Manager::repair`]) and publishes the result.
    pub fn run(&self, what: Request) -> Result<RepairOutcome, ServiceError> {
        let mut m = lock(&self.manager);
        let result = match what {
            Request::Repair => m.repair(&self.cancel),
            Request::Install => m.install(&self.cancel),
        };
        let status = m.status();
        drop(m);
        self.publish(
            &status,
            result.as_ref().err().map(ServiceCommandError::from),
        );
        result
    }

    /// Starts `what` on its own thread and returns at once (the outcome arrives as a [`ServiceUpdate`]). Refused while
    /// another runs.
    pub fn request(self: &Arc<Self>, what: Request) -> Result<(), ServiceCommandError> {
        if self.tracker.working() {
            return Err(ServiceCommandError::busy());
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("paddock-service".into())
            .spawn(move || {
                let _ = me.run(what);
            })
            .map(drop)
            .map_err(|e| {
                ServiceCommandError::from(&ServiceError::Failed(format!(
                    "The repair could not start: {e}"
                )))
            })
    }

    /// Applies the launch-at-login switch to the service and the app's own login item (blocking: call it on a background
    /// thread, as [`ServiceControl::spawn_launch_at_login`] does) and publishes the result.
    pub fn apply_launch_at_login(&self, on: bool, autostart: &mut dyn crate::login::Autostart) {
        let mut m = lock(&self.manager);
        let result = crate::login::apply_launch_at_login(on, Some(&mut m), autostart, &self.cancel);
        let status = m.status();
        drop(m);
        let error = result.err().map(|message| ServiceCommandError {
            code: ServiceErrorCode::Failed,
            message,
            retryable: true,
        });
        self.publish(&status, error);
    }

    /// [`ServiceControl::apply_launch_at_login`] on its own thread.
    pub fn spawn_launch_at_login(
        self: &Arc<Self>,
        on: bool,
        autostart: Arc<Mutex<Box<dyn crate::login::Autostart>>>,
    ) -> std::io::Result<()> {
        let me = self.clone();
        std::thread::Builder::new()
            .name("paddock-service-login".into())
            .spawn(move || me.apply_launch_at_login(on, lock(&autostart).as_mut()))
            .map(drop)
    }

    /// Opens Login Items (macOS); `false` where there is none. Refused while a repair holds the manager.
    pub fn open_login_items(&self) -> Result<bool, ServiceCommandError> {
        match self.manager.try_lock() {
            Ok(mut m) => Ok(m.open_login_items()),
            Err(_) => Err(ServiceCommandError::busy()),
        }
    }

    /// The health registry's repair (on its repair thread): the user's switch opens Login Items (the switch stays theirs),
    /// anything else is a repair.
    pub fn health_repair(&self) -> Result<(), Cause> {
        let condition = lock(&self.manager).status().condition;
        if matches!(condition, Condition::SwitchedOff | Condition::NeedsApproval) {
            let opened = lock(&self.manager).open_login_items();
            let status = lock(&self.manager).status();
            self.publish(&status, None);
            return Err(Cause::new(if opened {
                "Switch the app's background item on in Login Items; it shows as running once you do."
            } else {
                "Switch the background service on in Settings."
            })
            .code("service.switchOn"));
        }
        self.run(Request::Repair)
            .map(drop)
            .map_err(|e| Cause::new(e.to_string()).code("service.repairFailed"))
    }

    /// Rule 24 at start: [`Manager::on_start`], published.
    pub fn on_start(&self) -> Started {
        let mut m = lock(&self.manager);
        let started = m.on_start(&self.cancel);
        let status = m.status();
        drop(m);
        let error = match &started {
            Started::Repaired(_, o) if !o.registered() && *o != RepairOutcome::Cancelled => {
                Some(ServiceCommandError {
                    code: ServiceErrorCode::Failed,
                    message: self
                        .tracker
                        .current(Instant::now())
                        .map(|n| n.message)
                        .unwrap_or_default(),
                    retryable: true,
                })
            }
            _ => None,
        };
        self.publish(&status, error);
        started
    }

    /// Runs the start-up checks on their own thread: after `first`, then every `every`, `times` in all (the service may
    /// only fail once it restarts itself onto the new binary). Dropping or stopping the handle ends them.
    pub fn spawn_start_checks(
        self: &Arc<Self>,
        first: Duration,
        every: Duration,
        times: u32,
    ) -> std::io::Result<StartChecks> {
        let (tx, rx) = channel();
        let me = Arc::downgrade(self);
        let thread = std::thread::Builder::new()
            .name("paddock-service-checks".into())
            .spawn(move || {
                run_checks(&rx, first, every, times, &mut || {
                    me.upgrade().map(|c| c.on_start())
                });
            })?;
        Ok(StartChecks(tx, Some(thread)))
    }
}

/// The check loop: waits on `stop` (a message or a dropped sender ends it), then checks; stops when the control is gone.
fn run_checks(
    stop: &Receiver<()>,
    first: Duration,
    every: Duration,
    times: u32,
    check: &mut dyn FnMut() -> Option<Started>,
) {
    for n in 0..times {
        let wait = if n == 0 { first } else { every };
        if !matches!(stop.recv_timeout(wait), Err(RecvTimeoutError::Timeout)) {
            return;
        }
        if check().is_none() {
            return;
        }
    }
}

/// Stops the start-up checks (and waits for their thread, which wakes at once).
pub struct StartChecks(Sender<()>, Option<std::thread::JoinHandle<()>>);

impl StartChecks {
    /// Stops them.
    pub fn stop(mut self) {
        let _ = self.0.send(());
        if let Some(t) = self.1.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{LaunchdBundled, Target, WindowsRun};
    use crate::plan::Hold;
    use crate::port::{AppServiceStatus, Output};
    use crate::spec::ServiceSpec;
    use crate::testing::{Call, FakeAppService, FakeSystem};
    use paddock_health::{RepairAction, ServiceSpec as HealthSpec, ServiceState};

    type Seen = Arc<Mutex<Vec<ServiceUpdate>>>;

    fn emitter() -> (Emit, Seen) {
        let seen: Seen = Arc::default();
        let s = seen.clone();
        (
            Arc::new(move |u: &ServiceUpdate| lock(&s).push(u.clone())),
            seen,
        )
    }

    fn dyno(sys: &FakeSystem, dir: &std::path::Path, up: bool) -> Manager {
        let spec = ServiceSpec::builder("net.decryptology.dyno.agent", "Dyno", "dyno-agent")
            .args(["run"])
            .build()
            .unwrap();
        sys.put_program("C:\\D\\dyno-agent.exe");
        let b = WindowsRun::new(
            spec,
            Target::exe("C:\\D\\dyno-agent.exe"),
            Box::new(sys.clone()),
        );
        Manager::new(
            "Dyno",
            Box::new(b),
            dir,
            "net.decryptology.dyno.agent",
            "1.0.0",
            move || up,
        )
    }

    fn health() -> Arc<Health> {
        Health::new(
            vec![HealthSpec::new("agent", "the Dyno agent")],
            Box::new(|_, _| {}),
        )
        .unwrap()
    }

    /// Waits (bounded, no sleep) for an update that matches.
    fn wait_for(seen: &Seen, f: impl Fn(&ServiceUpdate) -> bool) -> ServiceUpdate {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(u) = lock(seen).iter().find(|u| f(u)).cloned() {
                return u;
            }
            assert!(
                Instant::now() < deadline,
                "no such update: {:?}",
                lock(seen)
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn install_runs_off_the_callers_thread_and_publishes_the_status_and_health() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, seen) = emitter();
        let h = health();
        let c = ServiceControl::with_health(dyno(&sys, d.path(), true), emit, h.clone(), "agent")
            .unwrap();
        sys.emulate_run_registry();
        let s = c.status().unwrap();
        assert_eq!(
            s.condition,
            Condition::Running,
            "running by hand counts as running"
        );
        c.request(Request::Install).unwrap();
        let u = wait_for(&seen, |u| u.status.repair.is_some() || u.error.is_some());
        assert_eq!(u.error, None);
        assert!(sys.commands().iter().any(|c| c.starts_with("reg add")));
        assert_eq!(h.state("agent"), Some(ServiceState::Ok));
        let json = serde_json::to_value(&u).unwrap();
        assert!(json["status"]["condition"].is_string() && json["error"].is_null());
    }

    #[test]
    fn a_failed_repair_reaches_the_ui_and_health_with_its_words() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, seen) = emitter();
        let h = health();
        let c = ServiceControl::with_health(dyno(&sys, d.path(), false), emit, h.clone(), "agent")
            .unwrap();
        sys.answer("reg add", Output::code(1));
        let r = c.run(Request::Repair);
        assert!(matches!(r, Err(ServiceError::Failed(_))));
        let u = lock(&seen).last().cloned().unwrap();
        let e = u.error.unwrap();
        assert_eq!((e.code, e.retryable), (ServiceErrorCode::Failed, true));
        assert_eq!(h.state("agent"), Some(ServiceState::Down));
        assert_eq!(h.get("agent").unwrap().repair, Some(RepairAction::Install));
        // The health registry's Repair runs the same repair (it fails the same way, with a cause).
        let cause = c.health_repair().unwrap_err();
        assert!(cause.code.as_deref() == Some("service.repairFailed"));
    }

    #[test]
    fn while_a_repair_holds_the_manager_status_answers_from_the_last_look_and_requests_are_busy() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, _) = emitter();
        let c = ServiceControl::new(dyno(&sys, d.path(), true), emit);
        assert!(c.status().is_some());
        let held = lock(&c.manager);
        assert!(c.tracker.start(Instant::now()));
        let s = c.status().unwrap();
        assert_eq!(s.condition, Condition::Repairing);
        assert_eq!(
            c.request(Request::Repair).unwrap_err().code,
            ServiceErrorCode::Busy
        );
        assert_eq!(
            c.open_login_items().unwrap_err().code,
            ServiceErrorCode::Busy
        );
        drop(held);
        c.tracker.finish(&RepairOutcome::Recovered, Instant::now());
        assert_eq!(
            c.open_login_items(),
            Ok(false),
            "Windows has no Login Items"
        );
        // Nothing looked yet and the manager is held: no status to give.
        let (emit2, _) = emitter();
        let fresh = ServiceControl::new(dyno(&sys, d.path(), true), emit2);
        let _held = lock(&fresh.manager);
        assert_eq!(fresh.status(), None);
    }

    #[test]
    fn a_poisoned_manager_lock_still_answers() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, _) = emitter();
        sys.emulate_run_registry();
        let c = ServiceControl::new(dyno(&sys, d.path(), true), emit);
        let c2 = c.clone();
        let _ = std::thread::spawn(move || {
            let _g = c2.manager.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(c.status().is_some());
        assert!(c.run(Request::Repair).is_ok());
    }

    #[test]
    fn switched_off_opens_login_items_from_health_and_never_switches_it_on() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let app = FakeAppService::new(&sys);
        app.set_status(AppServiceStatus::RequiresApproval);
        let spec = ServiceSpec::builder(
            "net.decryptology.pitwall.service",
            "Pitwall",
            "pitwall-server",
        )
        .build()
        .unwrap();
        let b = LaunchdBundled::new(spec, 501, Box::new(app), Box::new(sys.clone()));
        let m = Manager::new(
            "Pitwall",
            Box::new(b),
            d.path(),
            "net.decryptology.pitwall.service",
            "1.0.0",
            || false,
        );
        let (emit, _) = emitter();
        let h = Health::new(
            vec![HealthSpec::new("server", "the server")],
            Box::new(|_, _| {}),
        )
        .unwrap();
        let c = ServiceControl::with_health(m, emit, h.clone(), "server").unwrap();
        c.status();
        assert_eq!(
            h.get("server").unwrap().repair,
            Some(RepairAction::OpenLoginItems)
        );
        let cause = c.health_repair().unwrap_err();
        assert!(cause.message.contains("Login Items"), "{cause:?}");
        assert!(sys.calls().contains(&Call::Os("openLoginItems".into())));
        assert!(!sys.calls().contains(&Call::Os("register".into())));
        assert_eq!(c.open_login_items(), Ok(true));
    }

    #[test]
    fn health_wiring_needs_a_declared_service_and_a_closed_app_refuses_the_repair() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, _) = emitter();
        assert!(
            ServiceControl::with_health(dyno(&sys, d.path(), true), emit.clone(), health(), "nope")
                .is_none()
        );
        let h = health();
        let c = ServiceControl::with_health(dyno(&sys, d.path(), true), emit, h.clone(), "agent")
            .unwrap();
        c.cancel();
        drop(c);
        h.down("agent", Cause::new("x"), Some(RepairAction::Install));
        h.finish_repair("agent");
        assert!(
            h.get("agent")
                .unwrap()
                .cause
                .unwrap()
                .message
                .contains("closing")
        );
    }

    #[test]
    fn start_checks_run_on_their_schedule_publish_and_stop() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, seen) = emitter();
        let c = ServiceControl::new(dyno(&sys, d.path(), true), emit);
        // The loop itself: three checks with no waiting, then done; a stop or a gone control ends it.
        let (tx, rx) = channel::<()>();
        let mut n = 0;
        run_checks(&rx, Duration::ZERO, Duration::ZERO, 3, &mut || {
            n += 1;
            Some(c.on_start())
        });
        assert_eq!(n, 3);
        assert_eq!(lock(&seen).len(), 3);
        tx.send(()).unwrap();
        run_checks(&rx, Duration::ZERO, Duration::ZERO, 3, &mut || {
            panic!("stopped")
        });
        let (_tx, rx) = channel::<()>();
        let mut asked = 0;
        run_checks(&rx, Duration::ZERO, Duration::ZERO, 3, &mut || {
            asked += 1;
            None
        });
        assert_eq!(asked, 1, "the control is gone");
        // The thread: stopped before its first check.
        let checks = c
            .spawn_start_checks(Duration::from_secs(3600), Duration::from_secs(3600), 8)
            .unwrap();
        checks.stop();
        assert_eq!(lock(&seen).len(), 3);
        // A failed start check carries the tracker's words.
        let (emit2, seen) = emitter();
        let c = ServiceControl::new(dyno(&sys, d.path(), false), emit2);
        sys.answer(
            "reg query",
            Output::ok("    Dyno    REG_SZ    \"C:\\Old\\dyno-agent.exe\" run\r\n"),
        );
        sys.answer("reg add", Output::code(1));
        let started = c.on_start();
        assert!(
            matches!(started, Started::Repaired(_, RepairOutcome::Failed(_))),
            "{started:?}"
        );
        let last = lock(&seen).last().cloned().unwrap();
        assert!(last.error.unwrap().message.contains("Registering failed"));
    }

    #[test]
    fn the_launch_at_login_switch_registers_publishes_and_reports_a_failure() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, seen) = emitter();
        sys.emulate_run_registry();
        let c = ServiceControl::new(dyno(&sys, d.path(), true), emit);
        let auto: Arc<Mutex<Box<dyn crate::login::Autostart>>> =
            Arc::new(Mutex::new(Box::new(crate::login::NoAutostart)));
        c.spawn_launch_at_login(true, auto).unwrap();
        let u = wait_for(&seen, |u| u.status.repair.is_some() || u.error.is_some());
        assert_eq!(u.error, None);
        assert!(sys.commands().iter().any(|c| c.starts_with("reg add")));
        struct Refuses;
        impl crate::login::Autostart for Refuses {
            fn set(&mut self, _: bool) -> Result<(), String> {
                Err("denied".into())
            }
        }
        c.apply_launch_at_login(false, &mut Refuses);
        let e = lock(&seen).last().cloned().unwrap().error.unwrap();
        assert!(e.message.contains("denied"), "{e:?}");
    }

    #[test]
    fn every_service_error_has_a_wire_code() {
        for (e, code) in [
            (ServiceError::Busy, ServiceErrorCode::Busy),
            (ServiceError::SwitchedOff, ServiceErrorCode::SwitchedOff),
            (ServiceError::NeedsApproval, ServiceErrorCode::NeedsApproval),
            (
                ServiceError::NeedsElevation,
                ServiceErrorCode::NeedsElevation,
            ),
            (ServiceError::Declined, ServiceErrorCode::Declined),
            (
                ServiceError::Unavailable("x".into()),
                ServiceErrorCode::Unavailable,
            ),
            (ServiceError::NoHome, ServiceErrorCode::Unavailable),
            (
                ServiceError::NewerOwner("2".into()),
                ServiceErrorCode::NewerOwner,
            ),
            (ServiceError::Failed("x".into()), ServiceErrorCode::Failed),
            (
                ServiceError::Io {
                    op: "write",
                    path: "/x".into(),
                    source: std::io::Error::other("full"),
                },
                ServiceErrorCode::Failed,
            ),
        ] {
            assert_eq!(ServiceCommandError::from(&e).code, code);
        }
        assert_eq!(
            serde_json::to_value(ServiceErrorCode::NeedsApproval).unwrap(),
            serde_json::json!("needsApproval")
        );
    }

    #[test]
    fn a_service_the_app_did_not_declare_is_logged_and_the_control_still_works() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, seen) = emitter();
        let h = health();
        let c = ServiceControl::with_health_or_plain(
            dyno(&sys, d.path(), true),
            emit.clone(),
            h.clone(),
            "nope",
        );
        assert!(
            h.get("agent").unwrap().repair.is_none(),
            "nothing was wired"
        );
        c.status();
        assert!(
            !lock(&seen).is_empty(),
            "the control publishes without health"
        );
        // A declared one is wired as with_health does.
        let c = ServiceControl::with_health_or_plain(
            dyno(&sys, d.path(), true),
            emit,
            h.clone(),
            "agent",
        );
        drop(c);
    }

    #[test]
    fn the_health_registry_repairs_through_the_control_and_the_service_comes_back() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, _) = emitter();
        let h = health();
        let c = ServiceControl::with_health(dyno(&sys, d.path(), true), emit, h.clone(), "agent")
            .unwrap();
        h.down("agent", Cause::new("stopped"), Some(RepairAction::Install));
        h.repair("agent").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while h.get("agent").unwrap().state == ServiceState::Repairing {
            assert!(Instant::now() < deadline, "the repair never ended");
            std::thread::yield_now();
        }
        assert!(
            sys.calls()
                .iter()
                .any(|c| matches!(c, Call::Os(_) | Call::Run(..))),
            "the repair reached the system"
        );
        drop(c);
    }

    #[test]
    fn a_repair_pressed_while_the_start_waits_is_refused_at_once_and_says_why() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, _) = emitter();
        let h = health();
        let c = ServiceControl::with_health(dyno(&sys, d.path(), true), emit, h, "agent").unwrap();
        c.status().unwrap();
        // `on_start` holds the manager through the whole wait; the health registry's Repair and `run` must not queue behind it.
        let held = lock(&c.manager);
        assert!(c.tracker.start(Instant::now()));
        let (tx, rx) = channel();
        let c2 = c.clone();
        let t = std::thread::spawn(move || {
            let run = c2.run(Request::Repair).map(drop);
            let cause = c2.health_repair().unwrap_err();
            let _ = tx.send((run, cause));
        });
        let got = rx.recv_timeout(Duration::from_secs(10));
        drop(held);
        let (run, cause) = got.expect("Repair queued behind the start's wait");
        t.join().unwrap();
        assert!(matches!(run, Err(ServiceError::Busy)), "{run:?}");
        assert_eq!(cause.code.as_deref(), Some("service.busy"));
        assert!(cause.message.contains("already"), "{cause:?}");
        c.tracker.finish(&RepairOutcome::Cancelled, Instant::now());
    }

    #[test]
    fn a_start_that_cannot_save_its_state_tells_the_ui_why_and_what_to_do() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, seen) = emitter();
        sys.answer(
            "reg query",
            Output::ok("    Dyno    REG_SZ    \"C:\\Old\\dyno-agent.exe\" run\r\n"),
        );
        std::fs::write(
            d.path().join("service-net.decryptology.dyno.agent.json"),
            r#"{"version": 99}"#,
        )
        .unwrap();
        let c = ServiceControl::new(dyno(&sys, d.path(), false), emit);
        let started = c.on_start();
        assert!(
            matches!(started, Started::Left(Hold::RecordUnavailable(_))),
            "{started:?}"
        );
        let e = lock(&seen)
            .last()
            .cloned()
            .unwrap()
            .error
            .expect("the UI is told");
        assert_eq!(e.code, ServiceErrorCode::Failed);
        assert!(
            e.message.contains("changed nothing") && e.message.contains("Repair"),
            "{e:?}"
        );
    }

    #[test]
    fn start_checks_run_the_check_on_their_own_thread() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let (emit, seen) = emitter();
        let c = ServiceControl::new(dyno(&sys, d.path(), true), emit);
        let checks = c
            .spawn_start_checks(Duration::ZERO, Duration::ZERO, 1)
            .unwrap();
        wait_for(&seen, |_| true);
        checks.stop();
    }
}
