//! Keep awake in the agent (ADR 0032 sections 2 and 8). The agent holds the OS object, so the hold survives closing the window
//! and dies with the agent; the pure [`AwakeMachine`] decides, this file runs its effects and feeds it events.
//!
//! One lock guards the machine and the hold, so a request and the power thread never interleave. Live work: the thread keeps
//! the normal priority (rule 52). A marker file `keep-awake.active` in the data folder says "a hold was active"; a clean stop
//! removes it, so finding it at the next start means Dyno crashed or was restarted and the hold is not taken again (D4).
//! Logs say every decision with the OS reference and no user content (ER-27).

use crate::agent::{lock, now, Shared};
use dyno_core::metrics::Reading;
use dyno_core::ports::{IdleSignals, PowerActions};
use dyno_core::power::awake::{AwakeMachine, AwakeScope, AwakeSpec, AwakeUntil, Effect, Event, Inputs};
use dyno_core::power::energy::{PowerLogEntry, PowerLogKind, ALIVE_EVERY};
use dyno_core::power::idle::{IdleAction, IdleEffect, IdleEvent, IdleInputs, IdleMachine};
use dyno_core::power::{Capability, PowerCaps, PowerView};
use dyno_core::process::{Measured, ProcessSample};
use dyno_core::time::Timestamp;
use paddock_power::{HoldRequest, KeepAwake, PowerError, PowerEvent, PowerOs, Scope, SleepWakeSource};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The program name the OS lists the hold under.
const WHO: &str = "Dyno";
/// How often an active hold looks at its conditions (the sampler's values, no new polling).
const ACTIVE_TICK: Duration = Duration::from_secs(2);
/// How long the thread sleeps when nothing is held (a request or an OS event wakes it earlier).
const IDLE_WAIT: Duration = Duration::from_secs(5);
/// How often the idle rule looks at the computer while it is on.
const IDLE_TICK: Duration = Duration::from_secs(5);
/// The exceptions are read (an external tool may run) only this close to the rule's idle time, or during a countdown.
const SIGNALS_LEAD_SECS: u64 = 60;

struct Inner {
    machine: AwakeMachine,
    keep: KeepAwake,
    /// Set when the OS said keep awake is not available here; cleared by the next success.
    unavailable: Option<String>,
}

/// The agent's keep-awake service.
pub struct PowerCentre {
    inner: Mutex<Inner>,
    marker: PathBuf,
    events: Arc<dyn SleepWakeSource>,
    actions: Arc<dyn PowerActions>,
    signals: Arc<dyn IdleSignals>,
    idle: Mutex<IdleMachine>,
    skew_ms: std::sync::atomic::AtomicI64,
    pending: Mutex<VecDeque<PowerEvent>>,
    wake: (Mutex<bool>, Condvar),
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn scope(s: AwakeScope) -> Scope {
    match s {
        AwakeScope::System => Scope::System,
        AwakeScope::SystemAndDisplay => Scope::SystemAndDisplay,
    }
}

impl PowerCentre {
    /// A service over `os` and `events`. A marker left by an agent that did not stop cleanly becomes the "Dyno restarted" notice.
    pub fn new(
        os: Arc<dyn PowerOs>,
        events: Arc<dyn SleepWakeSource>,
        actions: Arc<dyn PowerActions>,
        signals: Arc<dyn IdleSignals>,
        marker: PathBuf,
    ) -> PowerCentre {
        let mut machine = AwakeMachine::new();
        if marker.exists() {
            log::info!(target: "dyno_agent::power", "keep-awake marker found: the last agent ended while holding; not taken again");
            machine.step(Event::Restarted, now());
            if let Err(e) = std::fs::remove_file(&marker) {
                log::warn!(target: "dyno_agent::power", "could not remove the keep-awake marker: {e}");
            }
        }
        PowerCentre {
            inner: Mutex::new(Inner {
                machine,
                keep: KeepAwake::new(os),
                unavailable: None,
            }),
            marker,
            events,
            actions,
            signals,
            idle: Mutex::new(IdleMachine::new()),
            skew_ms: std::sync::atomic::AtomicI64::new(0),
            pending: Mutex::new(VecDeque::new()),
            wake: (Mutex::new(false), Condvar::new()),
        }
    }

    /// The power view now.
    pub fn view(&self) -> PowerView {
        let (awake, keep_awake) = {
            let inner = locked(&self.inner);
            let keep_awake = match &inner.unavailable {
                Some(reason) => Capability::CantTell { reason: reason.clone() },
                None => Capability::Known,
            };
            (inner.machine.view(), keep_awake)
        };
        let idle = locked(&self.idle).view();
        let cap = |a: IdleAction| match self.actions.can(a) {
            Reading::Available(()) => Capability::Known,
            Reading::Unavailable(reason) => Capability::CantTell { reason },
        };
        PowerView {
            awake,
            idle,
            caps: PowerCaps {
                keep_awake,
                sleep: cap(IdleAction::Sleep),
                hibernate: cap(IdleAction::Hibernate),
                shut_down: cap(IdleAction::ShutDown),
            },
        }
    }

    /// Cuts the power thread's wait short.
    pub fn wake(&self) {
        *locked(&self.wake.0) = true;
        self.wake.1.notify_all();
    }

    /// Starts a hold. `enabled` is the setting; the agent validates whatever the UI sent.
    pub fn start(&self, shared: &Shared, spec: AwakeSpec, enabled: bool) -> Result<PowerView, String> {
        // Once the agent stops nothing new is held: the stop releases what is held and must not find a hold started after it.
        let Some(_pass) = shared.stop_gate.enter() else {
            return Err("Dyno is stopping; keep awake was not started.".into());
        };
        if !enabled {
            return Err("Keep awake is turned off in Settings.".into());
        }
        let mut spec = spec;
        spec.validate()?;
        let at = now();
        let ends = {
            let core = lock(&shared.core);
            // A program to follow is held by pid and start time (rule 53): when the caller gave only the pid, the start time
            // comes from the process table now, and a program that is not there is refused.
            if let AwakeUntil::WhileProcess { process } = &mut spec.until {
                if process.started.is_none() {
                    process.started = Some(
                        core.latest
                            .processes
                            .iter()
                            .find(|p| p.pid == process.pid)
                            .and_then(|p| match p.started {
                                Measured::Value(t) => Some(t),
                                Measured::Unavailable { .. } => None,
                            })
                            .ok_or("That program is not running, or its start time cannot be read; choose it again.")?,
                    );
                }
            }
            spec.deadline(at, core.zone.as_ref())
        };
        Ok(self.apply(shared, Event::Start { spec, ends }, at))
    }

    /// Releases the hold and clears a notice that it ended.
    pub fn stop(&self, shared: &Shared) -> PowerView {
        self.apply(shared, Event::Stop, now())
    }

    /// Feeds one event to the machine, runs the effects it asks for and what they lead to, pushes the view when it changed, and
    /// returns it.
    fn apply(&self, shared: &Shared, event: Event, at: Timestamp) -> PowerView {
        let before = self.view();
        {
            let mut inner = locked(&self.inner);
            self.run(&mut inner, event, at);
        }
        let after = self.view();
        if before != after {
            shared.broadcast_power(&after);
        }
        after
    }

    fn run(&self, inner: &mut Inner, event: Event, at: Timestamp) {
        let mut queue = VecDeque::from([event]);
        while let Some(event) = queue.pop_front() {
            for effect in inner.machine.step(event, at) {
                match effect {
                    Effect::Log(text) => log::info!(target: "dyno_agent::power", "{text}"),
                    Effect::Release => {
                        inner.keep.release();
                    }
                    Effect::Acquire { scope: s, reason } => {
                        let request = HoldRequest::new(scope(s), WHO, &reason);
                        match inner.keep.hold(&request) {
                            Ok(info) => {
                                inner.unavailable = None;
                                queue.push_back(Event::Acquired { os_ref: info.os_ref });
                            }
                            Err(e) => {
                                if let PowerError::Unavailable { reason } = &e {
                                    inner.unavailable = Some(reason.clone());
                                }
                                queue.push_back(Event::AcquireFailed { why: failure_words(&e) });
                            }
                        }
                    }
                }
            }
        }
        self.sync_marker(inner.machine.is_active());
    }

    fn sync_marker(&self, active: bool) {
        let result = if active {
            if self.marker.exists() {
                return;
            }
            std::fs::write(&self.marker, b"1")
        } else {
            match std::fs::remove_file(&self.marker) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                other => other,
            }
        };
        if let Err(e) = result {
            log::warn!(target: "dyno_agent::power", "could not update the keep-awake marker: {e}");
        }
    }

    /// One reading round for an active hold.
    fn tick(&self, shared: &Shared) {
        let watching = {
            let inner = locked(&self.inner);
            if !inner.machine.is_active() {
                return;
            }
            inner.machine.watching_process()
        };
        let inputs = read_inputs(shared, watching);
        self.apply(shared, Event::Tick(inputs), now());
    }

    fn drain_events(&self, shared: &Shared) {
        loop {
            let Some(event) = locked(&self.pending).pop_front() else { return };
            self.note(
                shared,
                if event == PowerEvent::Sleeping {
                    PowerLogKind::Slept
                } else {
                    PowerLogKind::Woke
                },
            );
            let (text, mapped) = match event {
                PowerEvent::Sleeping => (
                    "the computer went to sleep",
                    Event::OsEnded {
                        why: "the computer went to sleep".into(),
                    },
                ),
                PowerEvent::Woke => ("the computer woke", Event::Woke),
            };
            log::info!(target: "dyno_agent::power", "power event: {text}");
            if event == PowerEvent::Woke {
                // Volumes may have come or gone, and free space moved, while the computer slept.
                shared.disk.on_system_event("wake");
            }
            self.apply(shared, mapped, now());
        }
    }

    /// Notes a power event for the energy report (when the report is on). A store that cannot write is logged, never fatal.
    fn note(&self, shared: &Shared, kind: PowerLogKind) {
        let core = lock(&shared.core);
        if !core.settings.power.energy.enabled {
            return;
        }
        if let Err(e) = core.store.put_power_event(&PowerLogEntry { at: now(), kind }) {
            log::warn!(target: "dyno_agent::power", "could not note a power event ({kind:?}): {e}");
        }
    }

    /// The energy report for the last `days` days.
    pub fn energy(&self, shared: &Shared, days: u32) -> Result<dyno_core::power::energy::EnergyReport, String> {
        let at = now();
        let core = lock(&shared.core);
        let days = days.clamp(1, dyno_core::power::energy::MAX_DAYS);
        let since = at.minus(i64::from(days + 7) * dyno_core::time::DAY);
        let entries = core
            .store
            .power_events(since)
            .map_err(|e| format!("Dyno could not read its power log: {e}"))?;
        // The CPU energy the helper measured goes beside the estimate, never in its place (plan section 2.7).
        let (cpu_joules, cpu_via) = shared.climate.cpu_energy();
        Ok(
            dyno_core::power::energy::report(&entries, days, at, core.zone.as_ref(), &core.settings.power.energy)
                .with_cpu_energy(&cpu_joules, cpu_via),
        )
    }

    /// Releases the hold and removes the marker: a clean stop of the agent.
    fn shutdown(&self, shared: &Shared) {
        self.apply(shared, Event::Stop, now());
        self.note(shared, PowerLogKind::AgentStopped);
    }

    fn wait(&self, d: Duration) {
        let guard = locked(&self.wake.0);
        let (mut woken, _) = self
            .wake
            .1
            .wait_timeout_while(guard, d, |w| !*w)
            .unwrap_or_else(PoisonError::into_inner);
        *woken = false;
    }
}

impl PowerCentre {
    /// The idle rules' clock: the real time, plus whatever a test moved it by (rule 60: tests control time, never sleep).
    fn at(&self) -> Timestamp {
        now().plus(self.skew_ms.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// Moves this service's idle clock forward by `ms` (tests only: a 30 s countdown is never waited out).
    pub fn skew_clock(&self, ms: i64) {
        self.skew_ms.fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
    }

    /// Reads the idle and timed settings into the machine (at start and after every settings save).
    pub fn configure_idle(&self, shared: &Shared) {
        let (idle, timed_on) = {
            let core = lock(&shared.core);
            (core.settings.power.idle.clone(), core.settings.power.timed.enabled)
        };
        self.idle_apply(
            shared,
            IdleEvent::Configure {
                idle: Box::new(idle),
                timed_on,
            },
            self.at(),
        );
        self.wake();
    }

    /// The person cancelled the countdown (or the wait after the request).
    pub fn idle_cancel(&self, shared: &Shared) -> PowerView {
        self.idle_apply(shared, IdleEvent::Cancel, self.at());
        self.view()
    }

    /// The person snoozed the countdown.
    pub fn idle_snooze(&self, shared: &Shared, minutes: u32) -> PowerView {
        self.idle_apply(shared, IdleEvent::Snooze { minutes }, self.at());
        self.view()
    }

    /// Feeds one event to the idle machine, runs what it asks for and pushes the view when it changed.
    fn idle_apply(&self, shared: &Shared, event: IdleEvent, at: Timestamp) {
        let before = self.view();
        let effects = locked(&self.idle).step(event, at);
        let mut failed: Option<String> = None;
        for effect in effects {
            match effect {
                IdleEffect::Log(text) => log::info!(target: "dyno_agent::power", "{text}"),
                IdleEffect::ShowCountdown { action, deadline } => {
                    log::info!(target: "dyno_agent::power", "countdown shown: {} at {}", action.verb(), deadline.millis());
                }
                IdleEffect::HideCountdown => {}
                IdleEffect::Act { action } => match self.actions.request(action) {
                    Ok(()) => {
                        log::info!(target: "dyno_agent::power", "the system took the {} request (the normal way, not forced)", action.verb())
                    }
                    Err(e) => {
                        log::warn!(target: "dyno_agent::power", "the system refused the {} request: {e}", action.verb());
                        failed = Some(e.to_string());
                    }
                },
            }
        }
        if let Some(why) = failed {
            let more = locked(&self.idle).step(IdleEvent::ActFailed { why }, at);
            for effect in more {
                if let IdleEffect::Log(text) = effect {
                    log::info!(target: "dyno_agent::power", "{text}");
                }
            }
        }
        let after = self.view();
        if before != after {
            shared.broadcast_power(&after);
        }
    }

    /// One reading round for the idle and timed rules.
    pub fn idle_tick(&self, shared: &Shared) {
        if !locked(&self.idle).is_on() {
            return;
        }
        let at = self.at();
        // A timed action whose time has come starts its countdown (one at a time; the others stay due).
        let (timed, countdown) = {
            let core = lock(&shared.core);
            (
                core.settings.power.timed.due(at, core.zone.as_ref()),
                core.settings.power.idle.countdown_secs,
            )
        };
        if let Some((id, action)) = timed.first().copied() {
            // The machine decides whether it can take it (it ignores a due action while one is counting down or being asked).
            let accepted = locked(&self.idle)
                .step(
                    IdleEvent::TimedDue {
                        action,
                        countdown_secs: countdown,
                    },
                    at,
                )
                .iter()
                .any(|e| matches!(e, IdleEffect::ShowCountdown { .. }));
            if accepted {
                let mut core = lock(&shared.core);
                let zone = core.zone.clone();
                core.settings.power.timed.mark_run(id, at, zone.as_ref());
                core.save_settings(&shared.dirs);
                drop(core);
                let v = self.view();
                shared.broadcast_power(&v);
            }
        }
        let inputs = self.idle_inputs(shared);
        self.idle_apply(shared, IdleEvent::Tick(Box::new(inputs)), at);
    }

    fn idle_inputs(&self, shared: &Shared) -> IdleInputs {
        let idle_secs = match lock_activity(shared) {
            Some(v) => Reading::Available(v),
            None => Reading::Unavailable("this system gives Dyno no idle time".into()),
        };
        let (rule_after, counting) = {
            let m = locked(&self.idle);
            (
                u64::from(m.settings().after_secs),
                matches!(m.view(), dyno_core::power::idle::IdleView::Countdown { .. }),
            )
        };
        let near = idle_secs.value().is_some_and(|v| *v + SIGNALS_LEAD_SECS >= rule_after);
        let signals = (near || counting).then(|| self.signals.read());
        let not_yet = || Reading::Unavailable("not read yet: the computer has not been idle long enough".to_string());
        let core = lock(&shared.core);
        let sample = core.recent.back();
        fn no_sample<T>() -> Reading<T> {
            Reading::Unavailable("Dyno has not measured the computer yet".to_string())
        }
        let processes = &core.latest.processes;
        IdleInputs {
            idle_secs,
            // No battery reads as plugged in (a desktop).
            on_battery: match sample.map(|s| &s.battery) {
                Some(Reading::Available(b)) => Reading::Available(b.source == dyno_core::metrics::PowerSource::Battery),
                _ => Reading::Available(false),
            },
            audio: signals.as_ref().map_or_else(not_yet, |s| s.audio.clone()),
            full_screen: signals.as_ref().map_or_else(not_yet, |s| s.full_screen.clone()),
            stay_awake: signals
                .as_ref()
                .map_or_else(|| Reading::Unavailable("not read yet".to_string()), |s| s.stay_awake.clone()),
            cpu_pct: sample.map_or_else(no_sample::<f32>, |s| Reading::Available(s.cpu.total)),
            disk_bps: sample.map_or_else(no_sample, |s| Reading::Available(s.disk_io.read_bps + s.disk_io.write_bps)),
            net_bps: sample.map_or_else(no_sample, |s| {
                let (rx, tx) = s.network_totals();
                Reading::Available(rx + tx)
            }),
            programs: if processes.is_empty() {
                no_sample()
            } else {
                Reading::Available(processes.iter().map(|p| p.name.clone()).collect())
            },
        }
    }
}

fn lock_activity(shared: &Shared) -> Option<u64> {
    lock(&shared.ports.activity).idle_secs()
}

fn failure_words(e: &PowerError) -> String {
    match e {
        PowerError::Unavailable { reason } => format!("Keep awake is not available here: {reason}"),
        PowerError::Os { call, code, detail } => format!("The system refused to stay awake ({call}, code {code}: {detail})."),
        PowerError::Invalid(m) => format!("Dyno asked for a hold the system could not take: {m}"),
    }
}

/// The sampler's latest values as the machine's inputs.
fn read_inputs(shared: &Shared, watching: Option<dyno_core::process::ProcessRef>) -> Inputs {
    let core = lock(&shared.core);
    let sample = core.recent.back();
    let processes = &core.latest.processes;
    Inputs {
        cpu_pct: sample.map(|s| s.cpu.total),
        net_bytes_per_s: sample.map(|s| {
            let (rx, tx) = s.network_totals();
            rx + tx
        }),
        disk_bytes_per_s: sample.map(|s| s.disk_io.read_bps + s.disk_io.write_bps),
        programs: (!processes.is_empty()).then(|| processes.iter().map(|p| p.name.clone()).collect()),
        process_alive: watching.and_then(|w| alive(processes, w)),
    }
}

/// Whether the very process (pid and start time) is in the table; `None` when the table is empty or the start time of the
/// match is unknown on one side (rule 53: never act on a pid alone).
fn alive(processes: &[ProcessSample], w: dyno_core::process::ProcessRef) -> Option<bool> {
    if processes.is_empty() {
        return None;
    }
    let Some(p) = processes.iter().find(|p| p.pid == w.pid) else {
        return Some(false);
    };
    match (w.started, &p.started) {
        (Some(want), Measured::Value(have)) => Some(want == *have),
        _ => None,
    }
}

/// The power thread: OS sleep and wake notifications, a reading round every few seconds while a hold is active, and the
/// clock deadline. Ends when the agent stops, releasing the hold.
pub fn run(shared: &Arc<Shared>) {
    // Live work (rule 52): a person waits on it, so it is never lowered to a starvable level.
    dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
    let centre = shared.power.clone();
    let sink_centre = centre.clone();
    let subscription = centre.events.subscribe(Arc::new(move |e| {
        locked(&sink_centre.pending).push_back(e);
        sink_centre.wake();
    }));
    if let Err(e) = &subscription {
        log::warn!(target: "dyno_agent::power", "no sleep and wake notifications: {e}; a hold the OS ends is not re-taken on wake");
    }
    centre.note(shared, PowerLogKind::AgentStarted);
    centre.configure_idle(shared);
    let mut next_alive = std::time::Instant::now() + Duration::from_millis(ALIVE_EVERY as u64);
    while !shared.stopped() {
        if std::time::Instant::now() >= next_alive {
            centre.note(shared, PowerLogKind::Alive);
            next_alive = std::time::Instant::now() + Duration::from_millis(ALIVE_EVERY as u64);
        }
        centre.drain_events(shared);
        centre.tick(shared);
        centre.idle_tick(shared);
        let wait = {
            let inner = locked(&centre.inner);
            if inner.machine.is_active() {
                inner.machine.next_deadline().map_or(ACTIVE_TICK, |d| {
                    Duration::from_millis(u64::try_from(d.since(now())).unwrap_or(0).max(50)).min(ACTIVE_TICK)
                })
            } else {
                IDLE_WAIT
            }
        };
        let (idle_on, deadline) = {
            let m = locked(&centre.idle);
            (m.is_on(), m.next_deadline())
        };
        let wait = if idle_on {
            let until = deadline.map_or(IDLE_TICK, |d| {
                Duration::from_millis(u64::try_from(d.since(centre.at())).unwrap_or(0).max(50))
            });
            wait.min(IDLE_TICK).min(until)
        } else {
            wait
        };
        centre.wait(wait);
    }
    drop(subscription);
    centre.shutdown(shared);
}
