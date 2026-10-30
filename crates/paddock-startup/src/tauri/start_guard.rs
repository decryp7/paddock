//! The start guard's thread (the pure [`StartGuard`](crate::start_guard::StartGuard) decides): started first in the
//! shell plugin's set-up, it logs a start that has not reached the event loop in time, with its phase, and again while it
//! stays stuck, from its own thread, so the log says where even when the main thread is blocked (ADR 0029). It ends at
//! [`ready`] (a start that fails ends the process instead); it never ends, restarts or reloads anything (ER-19).

use crate::start_guard::{StartGuard, StartPhase, StartTiming, StuckStart};
use ::tauri::{AppHandle, Manager, Runtime};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::Instant;

/// The managed guard; the thread holds it weakly (it never keeps the guard alive on its own).
struct Guarded(Arc<Mutex<StartGuard>>);

fn lock(m: &Mutex<StartGuard>) -> MutexGuard<'_, StartGuard> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts guarding the start (once per app; a second call does nothing and returns `None`). Call it first in the
/// set-up that may block. Returns the thread's handle.
pub fn begin<R: Runtime>(app: &AppHandle<R>, timing: StartTiming) -> Option<JoinHandle<()>> {
    if app.try_state::<Guarded>().is_some() {
        return None;
    }
    let shared = Arc::new(Mutex::new(StartGuard::new(timing, Instant::now())));
    let weak = Arc::downgrade(&shared);
    app.manage(Guarded(shared));
    let spawned = std::thread::Builder::new()
        .name("paddock-start-guard".into())
        .spawn(move || watch(&weak, timing));
    spawned
        .map_err(|e| log::warn!("The start guard could not start: {e}"))
        .ok()
}

fn watch(guard: &Weak<Mutex<StartGuard>>, timing: StartTiming) {
    loop {
        std::thread::sleep(timing.tick);
        let Some(shared) = guard.upgrade() else {
            return;
        };
        let mut g = lock(&shared);
        if g.is_done() {
            return;
        }
        if let Some(stuck) = g.due(Instant::now()) {
            report(stuck);
        }
    }
}

fn report(stuck: StuckStart) {
    let (phase, secs) = (stuck.phase.describe(), stuck.waited.as_secs());
    if stuck.first {
        log::error!(
            Phase = phase, Waited = secs;
            "The start has not reached the event loop after {secs} s; it is in {phase} (still waiting)"
        );
    } else {
        log::warn!(Phase = phase, Waited = secs; "The start is still in {phase} after {secs} s");
    }
}

/// The start moved on to `phase` (nothing without [`begin`]).
pub fn phase<R: Runtime>(app: &AppHandle<R>, phase: StartPhase) {
    if let Some(g) = app.try_state::<Guarded>() {
        lock(&g.0).enter(phase);
    }
}

/// The event loop runs: the guard ends (logged when the start had been reported stuck).
pub fn ready<R: Runtime>(app: &AppHandle<R>) {
    let Some(g) = app.try_state::<Guarded>() else {
        return;
    };
    if let Some(took) = lock(&g.0).ready(Instant::now()) {
        log::warn!(Waited = took.as_secs(); "The start reached the event loop after {} s", took.as_secs());
    }
}
