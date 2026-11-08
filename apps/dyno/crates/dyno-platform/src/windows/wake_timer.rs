//! Scheduled wakes the agent sets itself on Windows (ADR 0032 section 5): a waitable timer with the resume flag, which wakes the
//! computer from sleep when it fires. It lives as long as the agent does (the page says so), needs "Allow wake timers" in the power
//! plan, and on Modern Standby it is not reliable: Dyno cannot tell, and says that. One thread per pending wake waits on its timer and
//! on a cancel event; the thread owns and closes both handles, so a late cancel never touches a recycled handle.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use dyno_core::ports::PlatformError;
use dyno_core::wake::{WakeEntry, WakeScheduler, WakeTag};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, CreateWaitableTimerW, SetEvent, SetWaitableTimer, WaitForMultipleObjects, INFINITE};

/// One pending wake: when, and the cancel event while the waiting thread lives.
struct Pending {
    at_utc: i64,
    cancel: Arc<Mutex<Option<isize>>>,
}

/// The agent's wake timers.
#[derive(Default)]
pub struct WakeTimers {
    pending: Arc<Mutex<HashMap<String, Pending>>>,
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn failed(call: &str, e: windows::core::Error) -> PlatformError {
    PlatformError::Failed(format!(
        "{call} failed ({e}): Windows would not set a wake timer. Check that the power plan allows wake timers."
    ))
}

impl WakeScheduler for WakeTimers {
    fn schedule(&self, tag: &WakeTag, at_utc: i64) -> Result<(), PlatformError> {
        self.cancel(tag)?;
        let wait_secs = at_utc - now_secs();
        if wait_secs <= 0 {
            return Err(PlatformError::Failed("That time is not in the future.".into()));
        }
        // A negative due time is relative, in 100 ns units.
        let due: i64 = -(wait_secs.saturating_mul(10_000_000));
        // SAFETY: plain handle creation; the handles are closed by the waiting thread below (or here on failure).
        let (timer, cancel) = unsafe {
            let timer = CreateWaitableTimerW(None, true, PCWSTR::null()).map_err(|e| failed("CreateWaitableTimerW", e))?;
            let cancel = match CreateEventW(None, true, false, PCWSTR::null()) {
                Ok(h) => h,
                Err(e) => {
                    paddock_proc::cleanup::logged("closing the timer after CreateEventW failed", CloseHandle(timer));
                    return Err(failed("CreateEventW", e));
                }
            };
            // SAFETY of the call: `timer` is a valid waitable timer; `due` outlives the call; fResume = true wakes the computer.
            if let Err(e) = SetWaitableTimer(timer, &due, 0, None, None, true) {
                paddock_proc::cleanup::logged("closing the timer after SetWaitableTimer failed", CloseHandle(timer));
                paddock_proc::cleanup::logged("closing the cancel event after SetWaitableTimer failed", CloseHandle(cancel));
                return Err(failed("SetWaitableTimer", e));
            }
            (timer, cancel)
        };
        let slot = Arc::new(Mutex::new(Some(cancel.0 as isize)));
        let (timer_raw, key, map, thread_slot) = (timer.0 as isize, tag.as_str().to_string(), self.pending.clone(), slot.clone());
        locked(&self.pending).insert(key.clone(), Pending { at_utc, cancel: slot });
        let spawned = std::thread::Builder::new().name("dyno-wake-timer".into()).spawn(move || {
            let handles = [HANDLE(timer_raw as *mut _), HANDLE(locked(&thread_slot).unwrap_or(0) as *mut _)];
            // SAFETY: both handles are valid until this thread closes them below; nobody else closes them.
            unsafe {
                WaitForMultipleObjects(&handles, false, INFINITE);
            }
            // Take the slot first: after this a cancel finds nothing to signal.
            let event = locked(&thread_slot).take();
            if let Some(raw) = event {
                // SAFETY: the handles are still ours; closed exactly once.
                unsafe {
                    paddock_proc::cleanup::logged("closing the cancel event", CloseHandle(HANDLE(raw as *mut _)));
                    paddock_proc::cleanup::logged("closing the timer", CloseHandle(HANDLE(timer_raw as *mut _)));
                }
            }
            let mut p = locked(&map);
            if p.get(&key).is_some_and(|e| Arc::ptr_eq(&e.cancel, &thread_slot)) {
                p.remove(&key);
            }
        });
        if let Err(e) = spawned {
            locked(&self.pending).remove(tag.as_str());
            // SAFETY: the thread never started, so the handles are still ours.
            unsafe {
                paddock_proc::cleanup::logged("closing the timer after the thread did not start", CloseHandle(timer));
                paddock_proc::cleanup::logged("closing the cancel event after the thread did not start", CloseHandle(cancel));
            }
            return Err(PlatformError::Failed(format!("could not start the wake timer's thread: {e}")));
        }
        Ok(())
    }

    fn cancel(&self, tag: &WakeTag) -> Result<bool, PlatformError> {
        let Some(p) = locked(&self.pending).remove(tag.as_str()) else {
            return Ok(false);
        };
        if let Some(raw) = *locked(&p.cancel) {
            // SAFETY: the slot still holds the event, so the waiting thread has not closed it (it takes the slot first).
            unsafe {
                paddock_proc::cleanup::logged("signalling the wake timer's cancel event", SetEvent(HANDLE(raw as *mut _)));
            }
        }
        Ok(true)
    }

    fn list(&self) -> Result<Vec<WakeEntry>, PlatformError> {
        let mut all: Vec<WakeEntry> = locked(&self.pending)
            .iter()
            .filter_map(|(k, p)| WakeTag::parse(k).ok().map(|tag| WakeEntry { tag, at_utc: p.at_utc }))
            .collect();
        all.sort_by_key(|w| w.at_utc);
        Ok(all)
    }
}
