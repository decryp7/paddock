//! The start guard's thread (the pure [`StartGuard`](crate::start_guard::StartGuard) decides). The shell begins it first
//! in its plugin's set-up; from there to `RunEvent::Ready` it logs a start that has not got that far in time, with its
//! phase, and again while it stays stuck, from its own thread, so the log says where even when the main thread is blocked
//! (ADR 0029). It does not cover what runs before the shell plugin's set-up (Tauri's build, earlier plugins' set-ups such
//! as single instance). It ends at [`ready`] (a start that fails ends the process instead); it never ends, restarts or
//! reloads anything (ER-19). It logs warnings only: a slow start on a busy machine is not an error.

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

/// What one tick decided, taken out of the lock before anything is logged.
enum Tick {
    Done,
    Quiet,
    Stuck(StuckStart),
}

fn watch(guard: &Weak<Mutex<StartGuard>>, timing: StartTiming) {
    loop {
        std::thread::sleep(timing.tick);
        let decided = match guard.upgrade() {
            None => Tick::Done,
            Some(shared) => {
                let mut g = lock(&shared);
                if g.is_done() {
                    Tick::Done
                } else {
                    g.tick().map_or(Tick::Quiet, Tick::Stuck)
                }
            }
        };
        match decided {
            Tick::Done => return,
            Tick::Quiet => {}
            Tick::Stuck(stuck) => report(stuck),
        }
    }
}

fn report(stuck: StuckStart) {
    let (phase, ticks) = (stuck.phase.describe(), stuck.ticks);
    if stuck.first {
        log::warn!(
            Phase = phase, Ticks = ticks;
            "The start has not opened its window after {ticks} ticks; it is in {phase} (still waiting)"
        );
    } else {
        log::warn!(Phase = phase, Ticks = ticks; "The start is still in {phase} after {ticks} ticks");
    }
}

/// The start moved on to `phase` (nothing without [`begin`]).
pub fn phase<R: Runtime>(app: &AppHandle<R>, phase: StartPhase) {
    if let Some(g) = app.try_state::<Guarded>() {
        lock(&g.0).enter(phase);
    }
}

/// The phase the start is in (`None` without [`begin`]).
pub fn current<R: Runtime>(app: &AppHandle<R>) -> Option<StartPhase> {
    let g = app.try_state::<Guarded>()?;
    let phase = lock(&g.0).phase();
    Some(phase)
}

/// The start got to `RunEvent::Ready`: the guard ends (logged when the start had been reported stuck).
pub fn ready<R: Runtime>(app: &AppHandle<R>) {
    let Some(g) = app.try_state::<Guarded>() else {
        return;
    };
    let took = lock(&g.0).ready(Instant::now());
    if let Some(took) = took {
        log::warn!(Waited = took.as_secs(); "The start went on after {} s", took.as_secs());
    }
}
