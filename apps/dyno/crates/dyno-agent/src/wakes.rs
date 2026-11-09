//! Scheduled wake in the agent (ADR 0032 section 5, D9). A schedule is armed for its next occurrence; the OS wake is set through the
//! route (Windows: the agent's own timer; macOS and the Linux package: the administrator helper's `ScheduleWake`), and when the time
//! comes the agent acts: stay awake, wake the chosen Wake-on-LAN computers, run one job from the fixed list. Off until the person
//! turns it on in Settings. A wake that passed while the computer was off or the agent was not running is "missed", never run late.
//!
//! Logs (ER-27): schedule id, when it was due, each action and its outcome, each OS wake set or refused with the OS's words. Never a
//! name of a schedule or of a computer.

use crate::agent::{lock, now, Shared};
use dyno_core::jobs::JobSpec;
use dyno_core::ports::PlatformError;
use dyno_core::power::awake::{AwakeScope, AwakeSpec, AwakeUntil};
use dyno_core::power::schedule::{
    advance, armed, fire, tag_of, validate, DynoJob, Fire, LastRun, RowState, WakeInput, WakeRoute, WakeRouteState, WakeRow, WakeSchedule,
    WakesView,
};
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_core::time::Timestamp;
use dyno_core::wake::{WakeEntry, WakeScheduler, WakeTag};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::time::Duration;

/// How often the thread looks at the schedules (a due wake is acted on within this).
const TICK: Duration = Duration::from_secs(15);

/// The administrator helper as the wake route sees it.
pub trait HelperRoute: Send + Sync {
    /// `Ok` when the helper answers; else why not, in words with the next step.
    fn ready(&self) -> Result<(), HelperNeed>;
    /// The helper's wake requests.
    fn wakes(&self) -> &dyn WakeScheduler;
}

/// Why the helper cannot set a wake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperNeed {
    /// This build has no helper at all.
    NotIncluded(String),
    /// The helper can be turned on or repaired from Settings.
    TurnOn(String),
}

fn bad(m: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, m)
}

fn internal(m: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::Internal, m)
}

/// The scheduled wakes.
pub struct WakeCentre {
    local: Option<Arc<dyn WakeScheduler>>,
    helper: Arc<dyn HelperRoute>,
    failures: Mutex<HashMap<u32, String>>,
    /// Held by every change of the saved schedules and every sync of the OS wakes, from the first read to the last write: a sync that
    /// listed the OS wakes after a change had been saved, with the schedules it read before, cancelled the changed schedule's wake.
    exclusive: Mutex<()>,
    wait: (Mutex<bool>, Condvar),
}

impl WakeCentre {
    /// A centre: `local` is the agent's own wake timer where the OS has one, else the helper route is used.
    pub fn new(local: Option<Arc<dyn WakeScheduler>>, helper: Arc<dyn HelperRoute>) -> WakeCentre {
        WakeCentre {
            local,
            helper,
            failures: Mutex::new(HashMap::new()),
            exclusive: Mutex::new(()),
            wait: (Mutex::new(false), Condvar::new()),
        }
    }

    fn exclusive(&self) -> std::sync::MutexGuard<'_, ()> {
        self.exclusive.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Cuts the thread's wait short.
    pub fn wake(&self) {
        *self.wait.0.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.wait.1.notify_all();
    }

    fn pause(&self, d: Duration) {
        let guard = self.wait.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut woken, _) = self
            .wait
            .1
            .wait_timeout_while(guard, d, |w| !*w)
            .unwrap_or_else(PoisonError::into_inner);
        *woken = false;
    }

    fn enabled(shared: &Shared) -> bool {
        lock(&shared.core).settings.power.wake.enabled
    }

    fn schedules(&self, shared: &Shared) -> Result<Vec<WakeSchedule>, AgentError> {
        lock(&shared.core)
            .store
            .wake_schedules()
            .map_err(|e| internal(format!("The scheduled wakes could not be read: {e}")))
    }

    /// Saves `s`, unless the agent has been asked to stop: the wake thread may be inside a helper call the stop does not wait for,
    /// and writes nothing after the stop.
    fn save(&self, shared: &Shared, s: &mut WakeSchedule) -> Result<(), AgentError> {
        shared
            .write_unless_stopped(|core| core.store.put_wake_schedule(s))
            .ok_or_else(|| internal("Dyno is stopping; the schedule was not saved.".to_string()))?
            .map(|_| ())
            .map_err(|e| internal(format!("The schedule could not be saved: {e}")))
    }

    /// Who sets the OS wake, and whether it can be done now.
    pub fn route_state(&self) -> WakeRouteState {
        if self.local.is_some() {
            return WakeRouteState::Agent {
                note: "Windows wakes the computer for a timer that Dyno sets, so a scheduled wake works while Dyno's agent is running. A computer that is shut down is not woken."
                    .into(),
                cant_tell: vec![
                    "Allow wake timers: Dyno can't tell. Check Power Options, Change advanced settings, Sleep.".into(),
                    "Modern Standby: Dyno can't tell whether wake timers fire on this computer.".into(),
                ],
            };
        }
        match self.helper.ready() {
            Ok(()) => WakeRouteState::HelperReady,
            Err(HelperNeed::NotIncluded(r)) => WakeRouteState::Unavailable {
                reason: format!("Setting a wake needs the administrator helper, and this build has none. {r} The macOS app and the Linux .deb package include it; an AppImage does not."),
            },
            Err(HelperNeed::TurnOn(r)) => WakeRouteState::NeedsHelper { reason: format!("Setting a wake needs the administrator helper. {r}") },
        }
    }

    fn scheduler(&self) -> Option<(&dyn WakeScheduler, WakeRoute)> {
        match &self.local {
            Some(l) => Some((&**l, WakeRoute::Agent)),
            None => self.helper.ready().ok().map(|()| (self.helper.wakes(), WakeRoute::Helper)),
        }
    }

    /// The Schedule tab's data.
    pub fn view(&self, shared: &Shared) -> Result<WakesView, AgentError> {
        let route = self.route_state();
        let paused = !Self::enabled(shared);
        let failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let rows = self
            .schedules(shared)?
            .into_iter()
            .map(|entry| {
                let state = if !entry.enabled {
                    RowState::Off
                } else if paused {
                    RowState::Paused
                } else if let Some(why) = failures.get(&entry.id) {
                    RowState::Failed { why: why.clone() }
                } else {
                    match &route {
                        WakeRouteState::Agent { .. } => RowState::Armed { route: WakeRoute::Agent },
                        WakeRouteState::HelperReady => RowState::Armed { route: WakeRoute::Helper },
                        WakeRouteState::NeedsHelper { .. } => RowState::NeedsHelper,
                        WakeRouteState::Unavailable { reason } => RowState::Failed { why: reason.clone() },
                    }
                };
                WakeRow { entry, state }
            })
            .collect();
        Ok(WakesView { rows, route })
    }

    /// Adds or changes a schedule, arms it and sets its OS wake.
    pub fn set(&self, shared: &Shared, input: WakeInput) -> Result<WakesView, AgentError> {
        if !Self::enabled(shared) {
            return Err(bad("Scheduled wake is turned off in Settings."));
        }
        let _one_at_a_time = self.exclusive();
        let saved = self.schedules(shared)?;
        let devices = shared.wol.device_ids(shared)?;
        let valid = validate(&input, &saved, &devices).map_err(bad)?;
        let zone = lock(&shared.core).zone.clone();
        let mut s = armed(&valid, now(), &*zone);
        let adding = s.id == 0;
        self.save(shared, &mut s)?;
        log::info!(target: "dyno_agent::wakes", "schedule {} {}: next wake {:?}", s.id, if adding { "added" } else { "changed" }, s.next_at.map(|t| t.0 / 1000));
        self.sync_one(&s);
        self.changed(shared)
    }

    /// Cancels a schedule and its OS wake.
    pub fn cancel(&self, shared: &Shared, id: u32) -> Result<WakesView, AgentError> {
        let _one_at_a_time = self.exclusive();
        lock(&shared.core)
            .store
            .delete_wake_schedule(id)
            .map_err(|e| internal(format!("The schedule could not be removed: {e}")))?;
        self.cancel_os(id);
        self.failures.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
        log::info!(target: "dyno_agent::wakes", "schedule {id} cancelled");
        self.changed(shared)
    }

    /// The view after a change, pushed to every connected app.
    fn changed(&self, shared: &Shared) -> Result<WakesView, AgentError> {
        let view = self.view(shared)?;
        shared.broadcast_wakes(&view);
        Ok(view)
    }

    fn cancel_os(&self, id: u32) {
        let Some((sched, _)) = self.scheduler() else { return };
        let Ok(tag) = WakeTag::parse(&tag_of(id)) else { return };
        if let Err(e) = sched.cancel(&tag) {
            log::warn!(target: "dyno_agent::wakes", "the OS wake of schedule {id} could not be cancelled: {e}");
        }
    }

    /// Sets (or clears) the OS wake of one schedule through the route; a refusal is kept for the row.
    fn sync_one(&self, s: &WakeSchedule) {
        let id = s.id;
        let Some((sched, route)) = self.scheduler() else {
            self.failures.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            return;
        };
        let Ok(tag) = WakeTag::parse(&tag_of(id)) else { return };
        let result: Result<(), PlatformError> = match (s.enabled, s.next_at) {
            (true, Some(at)) => sched.schedule(&tag, at.0.div_euclid(1000)),
            _ => sched.cancel(&tag).map(|_| ()),
        };
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        match result {
            Ok(()) => {
                failures.remove(&id);
                log::info!(target: "dyno_agent::wakes", "OS wake of schedule {id} synced through the {route:?} route");
            }
            Err(e) => {
                log::warn!(target: "dyno_agent::wakes", "OS wake of schedule {id} refused through the {route:?} route: {e}");
                failures.insert(id, e.to_string());
            }
        }
    }

    /// Sets every OS wake from the saved schedules and cancels Dyno's wakes that no schedule owns (after a restart, a removal).
    pub fn sync_all(&self, shared: &Shared) {
        let _one_at_a_time = self.exclusive();
        let Ok(all) = self.schedules(shared) else { return };
        let on = Self::enabled(shared);
        let zone = lock(&shared.core).zone.clone();
        for s in &all {
            if shared.stopped() {
                return;
            }
            let mut s = s.clone();
            let newly_armed = on && s.enabled && s.next_at.is_none();
            if newly_armed {
                s = armed(&s, now(), &*zone);
            }
            let mut for_os = s.clone();
            if !on {
                for_os.enabled = false;
            }
            // The OS wake first, then the saved time: a stop between the two leaves the schedule unarmed in the store, so the next
            // start arms it again, never a saved time the OS does not have.
            self.sync_one(&for_os);
            if newly_armed {
                let _ = self.save(shared, &mut s);
            }
        }
        if shared.stopped() {
            return;
        }
        if let Some((sched, _)) = self.scheduler() {
            let owned: Vec<String> = all.iter().filter(|s| on && s.enabled).map(|s| tag_of(s.id)).collect();
            match sched.list() {
                Ok(pending) => {
                    for WakeEntry { tag, .. } in pending.into_iter().filter(|w| !owned.iter().any(|o| o == w.tag.as_str())) {
                        if shared.stopped() {
                            return;
                        }
                        let _ = sched.cancel(&tag);
                    }
                }
                Err(e) => log::warn!(target: "dyno_agent::wakes", "the pending OS wakes could not be listed: {e}"),
            }
        }
    }

    /// One look at the schedules at `at`: acts on what is due, skips what was missed, and arms the next occurrence.
    pub fn tick(&self, shared: &Arc<Shared>, at: Timestamp) {
        if !Self::enabled(shared) {
            return;
        }
        let _one_at_a_time = self.exclusive();
        let Ok(all) = self.schedules(shared) else { return };
        let zone = lock(&shared.core).zone.clone();
        let mut acted = false;
        for s in all {
            if shared.stopped() {
                return;
            }
            let what = fire(&s, at);
            if what == Fire::NotYet {
                continue;
            }
            let mut next = advance(&s, at, &*zone);
            // The OS wake for the next time first (a helper call the stop does not wait for). A stop during it leaves the schedule
            // as it was, still due, so the next start acts on it and arms it again.
            self.sync_one(&next);
            // Then the actions and the saved next time together, behind the stop gate: a stop either waits for both or finds
            // neither started.
            let Some(_pass) = shared.stop_gate.enter() else { return };
            if shared.stopped() {
                return;
            }
            let summary = match what {
                Fire::Run => self.run_actions(shared, &s),
                _ => "Missed: the computer was off or Dyno was not running then.".to_string(),
            };
            log::info!(target: "dyno_agent::wakes", "schedule {} due: {}", s.id, summary);
            next.last_run = Some(LastRun { at, summary });
            if self.save(shared, &mut next).is_ok() {
                acted = true;
            }
        }
        if acted {
            let _ = self.changed(shared);
        }
    }

    /// The actions of a due schedule, each outcome in words.
    fn run_actions(&self, shared: &Arc<Shared>, s: &WakeSchedule) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(minutes) = s.stay_awake_minutes {
            let enabled = lock(&shared.core).settings.power.keep_awake.enabled;
            let spec = AwakeSpec {
                scope: AwakeScope::System,
                until: AwakeUntil::For { minutes },
            };
            parts.push(match shared.power.start(shared, spec, enabled) {
                Ok(_) => format!("staying awake {minutes} min"),
                Err(e) => format!("could not stay awake ({e})"),
            });
        }
        let (mut woken, mut refused) = (0, 0);
        for id in &s.wake_devices {
            let started = shared.wol.check_wake(shared, *id).map_err(|e| e.message).and_then(|()| {
                shared
                    .jobs
                    .start(shared, JobSpec::WolWake { id: *id, test: false })
                    .map(|_| ())
                    .map_err(|e| e.message)
            });
            match started {
                Ok(()) => woken += 1,
                Err(e) => {
                    refused += 1;
                    log::warn!(target: "dyno_agent::wakes", "schedule {}: the wake of device {id} was not started: {e}", s.id);
                }
            }
        }
        if !s.wake_devices.is_empty() {
            parts.push(if refused == 0 {
                format!("started the wake of {woken} computer(s)")
            } else {
                format!("started the wake of {woken}, {refused} could not be woken")
            });
        }
        if let Some(job) = s.job {
            let spec = match job {
                DynoJob::SoftwareInventory => JobSpec::SoftwareInventory,
            };
            parts.push(match shared.jobs.start(shared, spec) {
                Ok(_) => "started its job".to_string(),
                Err(e) => format!("its job did not start ({})", e.message),
            });
        }
        parts.join("; ")
    }
}

/// The helper as the real agent reaches it: through `Shared`'s helper centre (looked up at each call, so it is never stale).
pub struct SharedHelper {
    shared: Weak<Shared>,
    wakes: LinkWakes,
}

impl SharedHelper {
    /// A route over the agent's own state.
    pub fn new(shared: Weak<Shared>) -> SharedHelper {
        SharedHelper {
            wakes: LinkWakes(shared.clone()),
            shared,
        }
    }
}

/// The helper's wake requests as a [`WakeScheduler`].
pub struct LinkWakes(Weak<Shared>);

fn gone() -> PlatformError {
    PlatformError::Failed("Dyno is shutting down.".into())
}

impl WakeScheduler for LinkWakes {
    fn schedule(&self, tag: &WakeTag, at_utc: i64) -> Result<(), PlatformError> {
        self.0.upgrade().ok_or_else(gone)?.helper.link().schedule_wake(tag, at_utc)
    }
    fn cancel(&self, tag: &WakeTag) -> Result<bool, PlatformError> {
        self.0.upgrade().ok_or_else(gone)?.helper.link().cancel_wake(tag)
    }
    fn list(&self) -> Result<Vec<WakeEntry>, PlatformError> {
        self.0.upgrade().ok_or_else(gone)?.helper.link().list_wakes()
    }
}

impl HelperRoute for SharedHelper {
    fn ready(&self) -> Result<(), HelperNeed> {
        use dyno_core::helper_wire::HelperState;
        let Some(shared) = self.shared.upgrade() else {
            return Err(HelperNeed::NotIncluded("Dyno is shutting down.".into()));
        };
        let status = shared.helper.status();
        match status.state {
            HelperState::Ready => Ok(()),
            HelperState::NotIncluded => Err(HelperNeed::NotIncluded(status.message)),
            _ => Err(HelperNeed::TurnOn(status.message)),
        }
    }

    fn wakes(&self) -> &dyn WakeScheduler {
        &self.wakes
    }
}

/// The scheduled-wake thread: arms and syncs at the start, then looks at the schedules every few seconds until the agent stops.
pub fn run(shared: &Arc<Shared>) {
    // Live work (rule 52): a wake is acted on at its time.
    dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
    shared.wakes.sync_all(shared);
    while !shared.stopped() {
        shared.wakes.pause(TICK);
        if shared.stopped() {
            break;
        }
        shared.wakes.tick(shared, now());
    }
}
