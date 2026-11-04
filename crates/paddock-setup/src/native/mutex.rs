//! The installer lock, the hand-over event and the app-facing signal (native glue; the decisions are in `handoff`).
//!
//! The lock and the hand-over event live in a private namespace whose boundary is Administrators, so only an elevated
//! process can reach (or squat) them. The signal is an event in `Global\` that Authenticated Users may only wait on.

use super::handle::{self, Descriptor, Owned};
use super::wide::to_wide;
use crate::handoff::{
    self, InstallerState, LOCK_SDDL, NAMESPACE_SDDL, SIGNAL_SDDL, SignalProbe, WaitOutcome,
    handoff_name, lock_name, namespace_alias, signal_name,
};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, HANDLE, LocalFree,
    WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::System::Threading::{
    AddSIDToBoundaryDescriptor, ClosePrivateNamespace, CreateBoundaryDescriptorW, CreateEventW,
    CreateMutexW, CreatePrivateNamespaceW, DeleteBoundaryDescriptor, INFINITE, OpenEventW,
    OpenPrivateNamespaceW, ResetEvent, SetEvent, WaitForSingleObject,
};

const SYNCHRONIZE: u32 = 0x0010_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const EVENT_MODIFY_STATE: u32 = 0x0002;

/// Why a named object could not be made ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateError {
    /// The name exists and no administrator made it (or it denies us access).
    Squatted,
    /// Another Windows error.
    Os(u32),
}

/// The private namespace: closed (and its boundary descriptor deleted) on drop.
pub struct Namespace {
    ns: HANDLE,
    boundary: HANDLE,
}

impl Drop for Namespace {
    fn drop(&mut self) {
        // SAFETY: both handles were made by the calls in `enter` and are released once.
        unsafe {
            ClosePrivateNamespace(self.ns, 0);
            DeleteBoundaryDescriptor(self.boundary);
        }
    }
}

/// Creates (or, when another elevated process made it, opens) the private namespace of the app. Only a process whose token
/// has Administrators enabled can do either, which is what keeps a standard user from taking the names first.
pub fn enter(identifier: &str) -> Result<Namespace, CreateError> {
    // Create-or-open races with the last holder closing the namespace (it exists for the create, then is gone for the open):
    // one more try settles it.
    match enter_once(identifier) {
        Err(CreateError::Os(_)) => enter_once(identifier),
        other => other,
    }
}

fn enter_once(identifier: &str) -> Result<Namespace, CreateError> {
    let alias = namespace_alias(identifier);
    let wide_alias = to_wide(&alias);
    // SAFETY: NUL-terminated name.
    let boundary = unsafe { CreateBoundaryDescriptorW(wide_alias.as_ptr(), 0) };
    if boundary.is_null() {
        return Err(CreateError::Os(handle::last_error()));
    }
    let sid_text = to_wide(handoff::BOUNDARY_SID);
    let mut sid: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: NUL-terminated SID text; the SID is freed below.
    if unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) } == 0 {
        let e = handle::last_error();
        // SAFETY: made above.
        unsafe { DeleteBoundaryDescriptor(boundary) };
        return Err(CreateError::Os(e));
    }
    let mut boundary_mut = boundary;
    // SAFETY: a valid descriptor handle and SID.
    let added = unsafe { AddSIDToBoundaryDescriptor(&mut boundary_mut, sid) };
    // SAFETY: allocated by ConvertStringSidToSidW; the descriptor copied what it needs.
    unsafe { LocalFree(sid) };
    if added == 0 {
        let e = handle::last_error();
        // SAFETY: made above.
        unsafe { DeleteBoundaryDescriptor(boundary_mut) };
        return Err(CreateError::Os(e));
    }
    let sd = Descriptor::from_sddl(NAMESPACE_SDDL).map_err(CreateError::Os)?;
    let attrs = sd.attributes();
    // SAFETY: valid boundary, live attributes, NUL-terminated alias.
    let mut ns = unsafe { CreatePrivateNamespaceW(&attrs, boundary_mut, wide_alias.as_ptr()) };
    if ns.is_null() && handle::last_error() == ERROR_ALREADY_EXISTS {
        // SAFETY: valid boundary, NUL-terminated alias.
        ns = unsafe { OpenPrivateNamespaceW(boundary_mut, wide_alias.as_ptr()) };
    }
    if ns.is_null() {
        let e = handle::last_error();
        // SAFETY: made above.
        unsafe { DeleteBoundaryDescriptor(boundary_mut) };
        return Err(if e == ERROR_ACCESS_DENIED {
            CreateError::Squatted
        } else {
            CreateError::Os(e)
        });
    }
    Ok(Namespace {
        ns,
        boundary: boundary_mut,
    })
}

/// Creates the lock (a mutex) in the namespace; `existed` says another process already made it.
pub fn create_lock(_ns: &Namespace, identifier: &str) -> Result<(Owned, bool), CreateError> {
    let sd = Descriptor::from_sddl(LOCK_SDDL).map_err(CreateError::Os)?;
    let attrs = sd.attributes();
    let name = to_wide(&lock_name(identifier));
    // SAFETY: NUL-terminated name; live attributes.
    let h = unsafe { CreateMutexW(&attrs, 0, name.as_ptr()) };
    let err = handle::last_error();
    if h.is_null() {
        return Err(CreateError::Os(err));
    }
    Ok((Owned(h), err == ERROR_ALREADY_EXISTS))
}

/// Creates (or opens) the hand-over event in the namespace, then resets it: a stale signalled one never counts.
pub fn create_handoff(_ns: &Namespace, identifier: &str) -> Result<Owned, CreateError> {
    let sd = Descriptor::from_sddl(LOCK_SDDL).map_err(CreateError::Os)?;
    let attrs = sd.attributes();
    let name = to_wide(&handoff_name(identifier));
    // SAFETY: NUL-terminated name; live attributes. Manual reset, not signalled.
    let h = unsafe { CreateEventW(&attrs, 1, 0, name.as_ptr()) };
    if h.is_null() {
        return Err(CreateError::Os(handle::last_error()));
    }
    let event = Owned(h);
    reset(&event);
    Ok(event)
}

/// Whether a hand-over event exists in the namespace and is not set: `apply-update` has started an installer that has not
/// yet taken the lock.
pub fn handoff_pending(_ns: &Namespace, identifier: &str) -> bool {
    let name = to_wide(&handoff_name(identifier));
    // SAFETY: NUL-terminated name.
    let h = unsafe { OpenEventW(SYNCHRONIZE, 0, name.as_ptr()) };
    if h.is_null() {
        return false;
    }
    wait(&Owned(h), Some(Duration::ZERO)) == WaitOutcome::TimedOut
}

/// Opens an existing hand-over event for setting it (`None` when there is none: an install started by hand).
pub fn open_handoff(_ns: &Namespace, identifier: &str) -> Option<Owned> {
    let name = to_wide(&handoff_name(identifier));
    // SAFETY: NUL-terminated name.
    let h = unsafe { OpenEventW(EVENT_MODIFY_STATE | SYNCHRONIZE, 0, name.as_ptr()) };
    (!h.is_null()).then(|| Owned(h))
}

/// Creates or opens the app-facing signal, then resets it (not signalled = an install is running). A name taken by anyone
/// but an administrator is [`CreateError::Squatted`]: the caller goes on without a signal.
pub fn create_signal(identifier: &str) -> Result<Owned, CreateError> {
    let sd = Descriptor::from_sddl(SIGNAL_SDDL).map_err(CreateError::Os)?;
    let attrs = sd.attributes();
    let name = to_wide(&signal_name(identifier));
    // SAFETY: NUL-terminated name; live attributes. Manual reset, not signalled.
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
    if err == ERROR_ALREADY_EXISTS && !trusted_owner(&owned) {
        return Err(CreateError::Squatted);
    }
    reset(&owned);
    Ok(owned)
}

fn trusted_owner(h: &Owned) -> bool {
    matches!(
        handoff::signal_state(handle::owner_sid(h).as_deref(), false),
        SignalProbe::Active
    )
}

fn reset(event: &Owned) {
    // SAFETY: a valid event handle opened with modify access.
    unsafe { ResetEvent(event.0) };
}

/// Sets an event (the install is over, or the hand-over happened).
pub fn set(event: &Owned) {
    // SAFETY: a valid event handle opened with modify access.
    unsafe { SetEvent(event.0) };
}

/// Waits for a handle; `None` waits without a bound (only the installer-side processes do that, never the app).
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

/// Opens the signal for a wait and reading its owner.
fn open_signal(identifier: &str) -> Result<Option<Owned>, u32> {
    let name = to_wide(&signal_name(identifier));
    // SAFETY: NUL-terminated name.
    let h = unsafe { OpenEventW(SYNCHRONIZE | READ_CONTROL, 0, name.as_ptr()) };
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

fn probe_handle(h: &Owned) -> SignalProbe {
    let signalled = wait(h, Some(Duration::ZERO)) == WaitOutcome::Released;
    handoff::signal_state(handle::owner_sid(h).as_deref(), signalled)
}

/// The signal as an administrator-side process sees it before an update.
pub fn signal_probe(identifier: &str) -> SignalProbe {
    match open_signal(identifier) {
        Ok(None) => SignalProbe::Absent,
        Ok(Some(h)) => probe_handle(&h),
        Err(_) => SignalProbe::Foreign,
    }
}

/// The app's look at the installer signal, for `paddock-updater`'s start (ADR 0053 section 4.1 step 4). A name that exists
/// but cannot be opened or is owned by anyone else is [`SignalProbe::Foreign`]. Nothing here can hold anything.
pub fn installer_probe(identifier: &str) -> SignalProbe {
    signal_probe(identifier)
}

/// Waits (no polling) until the install is over, for at most `bound`. A missing, finished or foreign signal is "gone" at
/// once; a timeout is "still installing" (unknown), and the hand-off record settles it at the next start. The bound is
/// required: the app never waits without one.
pub fn wait_for_installer(identifier: &str, bound: Duration) -> InstallerState {
    match open_signal(identifier) {
        Ok(Some(h)) if probe_handle(&h) == SignalProbe::Active => {
            handoff::after_wait(wait(&h, Some(bound)))
        }
        _ => InstallerState::Gone,
    }
}
