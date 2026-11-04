//! `hold-installing`: creates and holds `Global\<identifier>.setup` until the parent process (the installer) exits
//! (native glue). Started by the installer include from `$PLUGINSDIR`, so `stop-all` (which ends images in `$INSTDIR`) never
//! ends it.

use super::handle::{self, Descriptor, Owned};
use super::wide::to_wide;
use crate::handoff::{MUTEX_SDDL, mutex_name};
use windows_sys::Win32::Foundation::{FILETIME, INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, ReleaseMutex,
    WaitForSingleObject,
};

const SYNCHRONIZE: u32 = 0x0010_0000;

fn creation_time(process: windows_sys::Win32::Foundation::HANDLE) -> Option<u64> {
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
    // SAFETY: valid process handle and out pointers.
    let ok = unsafe { GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) };
    (ok != 0).then(|| (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

fn parent_pid() -> Option<u32> {
    // SAFETY: a snapshot of the process table, walked with correctly sized entries.
    unsafe {
        let snap = Owned(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if snap.0 == INVALID_HANDLE_VALUE {
            return None;
        }
        let me = GetCurrentProcessId();
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

/// Holds the mutex until the parent exits. `Err` carries the sentence for the log.
pub fn hold(identifier: &str) -> Result<(), String> {
    let pid = parent_pid().ok_or("the parent process could not be found")?;
    // SAFETY: plain open of the parent for waiting and its start time.
    let parent = Owned(unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
    if parent.0.is_null() {
        return Err(format!("the parent process cannot be opened (error {})", handle::last_error()));
    }
    // The parent must have started before this process (a reused pid would be younger).
    // SAFETY: the current process pseudo-handle.
    let mine = creation_time(unsafe { GetCurrentProcess() });
    match (creation_time(parent.0), mine) {
        (Some(p), Some(m)) if p <= m => {}
        _ => return Err("the parent process is not the one that started this program".into()),
    }
    let sd = Descriptor::from_sddl(MUTEX_SDDL).map_err(|e| format!("mutex descriptor error {e}"))?;
    let attrs = sd.attributes();
    let name = to_wide(&mutex_name(identifier));
    // SAFETY: NUL-terminated name; live attributes.
    let m = Owned(unsafe { CreateMutexW(&attrs, 0, name.as_ptr()) });
    if m.0.is_null() {
        return Err(format!("the mutex could not be created (error {})", handle::last_error()));
    }
    // SAFETY: valid mutex handle.
    let got = unsafe { WaitForSingleObject(m.0, 0) };
    if got != WAIT_OBJECT_0 && got != WAIT_ABANDONED {
        return Err("another install holds the mutex".into());
    }
    // A wait on the parent's handle, not a poll: returns when the installer exits.
    // SAFETY: valid handle.
    unsafe { WaitForSingleObject(parent.0, INFINITE) };
    // SAFETY: owned by this thread.
    unsafe { ReleaseMutex(m.0) };
    Ok(())
}
