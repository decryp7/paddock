//! `hold-installing`: creates and holds `Global\<identifier>.setup` until the parent process (the installer) exits
//! (native glue). Started by the installer include from `$PLUGINSDIR`, so `stop-all` (which ends images in `$INSTDIR`) never
//! ends it.
//!
//! When `apply-update` handed the lock over, this process takes it (waiting for the release, bounded by the parent's life)
//! and sets the hand-over event so `apply-update` can end. The parent is identified by pid and start time (rule 53) and
//! waited on through a handle opened before the identity is checked again.

use super::handle::{self, Owned};
use super::mutex::{self, CreateError};
use super::wide::to_wide;
use crate::handoff::{self, MutexProbe, handoff_event_name};
use crate::log::Sink;
use paddock_proc::ProcessId;
use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    INFINITE, OpenEventW, OpenProcess, ReleaseMutex, SetEvent, WaitForMultipleObjects,
};

const SYNCHRONIZE: u32 = 0x0010_0000;
const READ_CONTROL: u32 = 0x0002_0000;
const EVENT_MODIFY_STATE: u32 = 0x0002;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

fn parent_pid() -> Option<u32> {
    // SAFETY: a snapshot of the process table, walked with correctly sized entries.
    unsafe {
        let snap = Owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if snap.0 == INVALID_HANDLE_VALUE {
            return None;
        }
        let me = std::process::id();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap.0, &mut entry);
        while more != 0 {
            if entry.th32ProcessID == me {
                return Some(entry.th32ParentProcessID);
            }
            more = Process32NextW(snap.0, &mut entry);
        }
        None
    }
}

/// Sets the hand-over event when an administrator's `apply-update` made one.
fn signal_handover(identifier: &str, log: &mut dyn Sink) {
    let name = to_wide(&handoff_event_name(identifier));
    // SAFETY: NUL-terminated name.
    let h = unsafe { OpenEventW(EVENT_MODIFY_STATE | READ_CONTROL, 0, name.as_ptr()) };
    if h.is_null() {
        log.line(
            "hold-installing",
            "no hand-over event: this install was started by hand",
        );
        return;
    }
    let event = Owned(h);
    if handoff::classify(handle::owner_sid(&event).as_deref()) != MutexProbe::Held {
        log.line(
            "hold-installing",
            "a hand-over event exists but no administrator made it; not used",
        );
        return;
    }
    // SAFETY: a valid event handle opened with EVENT_MODIFY_STATE.
    unsafe { SetEvent(event.0) };
    log.line(
        "hold-installing",
        "told apply-update that the installer holds the lock",
    );
}

/// Holds the mutex until the parent exits. `Err` carries the sentence for the log and stderr.
pub fn hold(identifier: &str, log: &mut dyn Sink) -> Result<(), String> {
    let ppid = parent_pid().ok_or("the parent process could not be found")?;
    let me = ProcessId::of(std::process::id()).ok_or("this process could not be identified")?;
    let parent = ProcessId::of(ppid).ok_or("the parent process could not be identified")?;
    // A parent that started after this process is a recycled pid, not the installer that started us.
    if parent.started().as_micros() > me.started().as_micros() {
        return Err("the parent process is not the one that started this program".into());
    }
    // SAFETY: plain open of the parent for waiting.
    let parent_handle =
        Owned(unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, ppid) });
    if parent_handle.0.is_null() {
        return Err(format!(
            "the parent process cannot be opened (error {})",
            handle::last_error()
        ));
    }
    // The identity again, now that the handle is held: the pid still names the same process.
    if ProcessId::of(ppid) != Some(parent) {
        return Err("the parent process ended while it was being opened".into());
    }
    let (m, existed) = mutex::create_mutex(identifier).map_err(|e| match e {
        CreateError::Squatted => {
            "the lock name is taken by an object no administrator made".to_string()
        }
        CreateError::Os(code) => format!("the mutex could not be created (error {code})"),
    })?;
    log.line(
        "hold-installing",
        if existed {
            "the lock exists; waiting for its holder or the installer to end"
        } else {
            "the lock was made"
        },
    );
    // Take the lock: wait for the mutex or for the parent to end, whichever is first (no clock).
    let handles = [m.0, parent_handle.0];
    // SAFETY: both handles are valid for the length of the call.
    let r = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
    if r != WAIT_OBJECT_0 && r != WAIT_ABANDONED {
        log.line(
            "hold-installing",
            "the installer ended before it could take the lock",
        );
        return Ok(());
    }
    log.line(
        "hold-installing",
        "holding the lock until the installer ends",
    );
    signal_handover(identifier, log);
    // A wait on the parent's handle, not a poll: returns when the installer exits.
    // SAFETY: valid handle.
    unsafe { WaitForMultipleObjects(1, &parent_handle.0, 0, INFINITE) };
    // SAFETY: owned by this thread.
    unsafe { ReleaseMutex(m.0) };
    log.line("hold-installing", "the installer ended; lock released");
    Ok(())
}
