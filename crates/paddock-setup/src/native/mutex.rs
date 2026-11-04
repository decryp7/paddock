//! The installer mutex and its hand-over event (native glue; the decisions are in `handoff`).

use super::handle::{self, Descriptor, Owned};
use super::wide::to_wide;
use crate::handoff::{
    self, InstallerState, MUTEX_SDDL, MutexProbe, WaitOutcome, handoff_event_name, mutex_name,
};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, WAIT_ABANDONED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, OpenMutexW, WaitForSingleObject,
};

const SYNCHRONIZE: u32 = 0x0010_0000;
const READ_CONTROL: u32 = 0x0002_0000;

/// Why a named object could not be made ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateError {
    /// The name exists and no administrator made it (or it denies us access).
    Squatted,
    /// Another Windows error.
    Os(u32),
}

/// Whether the handle's owner is trusted (`false` also when it cannot be read).
fn owned_by_administrator(h: &Owned) -> bool {
    handoff::classify(handle::owner_sid(h).as_deref()) == MutexProbe::Held
}

/// Creates `Global\<identifier>.setup` or opens the existing one when an administrator's process made it.
/// `existed` says the name was already there.
pub fn create_mutex(identifier: &str) -> Result<(Owned, bool), CreateError> {
    let sd = Descriptor::from_sddl(MUTEX_SDDL).map_err(CreateError::Os)?;
    let attrs = sd.attributes();
    let name = to_wide(&mutex_name(identifier));
    // SAFETY: NUL-terminated name; live attributes.
    let h = unsafe { CreateMutexW(&attrs, 0, name.as_ptr()) };
    let err = handle::last_error();
    if h.is_null() {
        return Err(if err == ERROR_ACCESS_DENIED {
            CreateError::Squatted
        } else {
            CreateError::Os(err)
        });
    }
    let owned = Owned(h);
    let existed = err == ERROR_ALREADY_EXISTS;
    if existed && !owned_by_administrator(&owned) {
        return Err(CreateError::Squatted);
    }
    Ok((owned, existed))
}

/// Creates the hand-over event (manual reset, not signalled) or opens the administrator's existing one.
pub fn create_event(identifier: &str) -> Result<Owned, CreateError> {
    let sd = Descriptor::from_sddl(MUTEX_SDDL).map_err(CreateError::Os)?;
    let attrs = sd.attributes();
    let name = to_wide(&handoff_event_name(identifier));
    // SAFETY: NUL-terminated name; live attributes.
    let h = unsafe { CreateEventW(&attrs, 1, 0, name.as_ptr()) };
    let err = handle::last_error();
    if h.is_null() {
        return Err(if err == ERROR_ACCESS_DENIED {
            CreateError::Squatted
        } else {
            CreateError::Os(err)
        });
    }
    let owned = Owned(h);
    if err == ERROR_ALREADY_EXISTS && !owned_by_administrator(&owned) {
        return Err(CreateError::Squatted);
    }
    Ok(owned)
}

/// Waits for a handle; `None` waits without a bound.
pub fn wait(h: &Owned, bound: Option<Duration>) -> WaitOutcome {
    let ms = bound.map_or(INFINITE, |b| {
        u32::try_from(b.as_millis()).unwrap_or(INFINITE - 1)
    });
    // SAFETY: a valid handle.
    match unsafe { WaitForSingleObject(h.0, ms) } {
        WAIT_OBJECT_0 => WaitOutcome::Released,
        WAIT_ABANDONED => WaitOutcome::Abandoned,
        WAIT_TIMEOUT => WaitOutcome::TimedOut,
        _ => WaitOutcome::Failed,
    }
}

/// Opens the mutex for a wait and reading its owner.
fn open_probe(identifier: &str) -> Result<Option<Owned>, u32> {
    let name = to_wide(&mutex_name(identifier));
    // SAFETY: NUL-terminated name.
    let h = unsafe { OpenMutexW(SYNCHRONIZE | READ_CONTROL, 0, name.as_ptr()) };
    if h.is_null() {
        let e = handle::last_error();
        return if e == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(e)
        };
    }
    Ok(Some(Owned(h)))
}

/// The app's look at the installer lock, for `paddock-updater`'s start (ADR 0053 section 4.1 step 4): is there a mutex, and did
/// an administrator make it? A name that exists but cannot be opened or is owned by anyone else is [`MutexProbe::Foreign`].
pub fn installer_probe(identifier: &str) -> MutexProbe {
    match open_probe(identifier) {
        Ok(None) => MutexProbe::Absent,
        Ok(Some(h)) => handoff::classify(handle::owner_sid(&h).as_deref()),
        Err(_) => MutexProbe::Foreign,
    }
}

/// Waits (no polling) until the installer's mutex is released or abandoned; `bound` limits the wait. Returns what the wait
/// ended with, mapped by [`handoff::after_wait`]. A missing or foreign mutex is "gone" at once.
pub fn wait_for_installer(identifier: &str, bound: Option<Duration>) -> InstallerState {
    match open_probe(identifier) {
        Ok(Some(h)) if handoff::classify(handle::owner_sid(&h).as_deref()) == MutexProbe::Held => {
            let outcome = wait(&h, bound);
            if matches!(outcome, WaitOutcome::Released | WaitOutcome::Abandoned) {
                // The wait took the mutex; give it back so the next waiter (or the next installer) can have it.
                // SAFETY: this thread acquired the mutex by the wait above.
                unsafe { windows_sys::Win32::System::Threading::ReleaseMutex(h.0) };
            }
            handoff::after_wait(outcome)
        }
        _ => InstallerState::Gone,
    }
}
