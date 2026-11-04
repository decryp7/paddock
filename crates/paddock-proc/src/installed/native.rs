//! The Windows adapter of [`Machine`](super::Machine): Restart Manager, the process table, `SeDebugPrivilege` and process
//! handles. Thin native glue: every decision lives in [`stop_all`](super::stop_all) and is tested over a fake; this file is
//! exercised by the real-process tests in `tests/it/installed_windows.rs` on the Windows VM (ER-14: only children the test started).

use super::{Bounds, Candidate, Held, Machine, OpenFailure, Request, RmProcess, StopError, StopReport};
use crate::{ProcessId, StartTime};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, ERROR_SUCCESS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::RestartManager::{
    CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources, RmStartSession,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
};

/// `ERROR_NOT_ALL_ASSIGNED`: `AdjustTokenPrivileges` succeeded but the token does not hold the privilege.
const ERROR_NOT_ALL_ASSIGNED: u32 = 1300;
/// `RM_INVALID_TS_SESSION`.
const RM_INVALID_TS_SESSION: u32 = u32::MAX;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// The real machine.
#[derive(Debug, Default)]
pub struct WindowsMachine;

/// A Restart Manager session over some files, ended on drop.
struct RmSession(u32);

impl RmSession {
    fn start(files: &[PathBuf]) -> Result<RmSession, StopError> {
        let mut handle = 0u32;
        let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
        // SAFETY: both out-pointers are valid for the sizes the API documents.
        let rc = unsafe { RmStartSession(&raw mut handle, 0, key.as_mut_ptr()) };
        if rc != ERROR_SUCCESS {
            return Err(StopError::RestartManager { call: "RmStartSession", code: rc });
        }
        let session = RmSession(handle);
        let names: Vec<Vec<u16>> = files.iter().map(|f| wide(f.as_os_str())).collect();
        let ptrs: Vec<*const u16> = names.iter().map(|n| n.as_ptr()).collect();
        let count = u32::try_from(ptrs.len()).unwrap_or(u32::MAX);
        // SAFETY: `ptrs` points at `count` NUL-terminated wide strings that outlive the call.
        let rc = unsafe { RmRegisterResources(handle, count, ptrs.as_ptr(), 0, std::ptr::null(), 0, std::ptr::null()) };
        if rc != ERROR_SUCCESS {
            return Err(StopError::RestartManager { call: "RmRegisterResources", code: rc });
        }
        Ok(session)
    }

    fn list(&self) -> Result<Vec<RmProcess>, StopError> {
        let mut needed = 0u32;
        let mut have = 0u32;
        let mut reasons = 0u32;
        // SAFETY: a size query: no buffer, the counters are valid.
        let rc = unsafe { RmGetList(self.0, &raw mut needed, &raw mut have, std::ptr::null_mut(), &raw mut reasons) };
        if rc == ERROR_SUCCESS && needed == 0 {
            return Ok(Vec::new());
        }
        if rc != ERROR_MORE_DATA && rc != ERROR_SUCCESS {
            return Err(StopError::RestartManager { call: "RmGetList", code: rc });
        }
        // The list can grow between the two calls; ask again with room to spare, a few times.
        let mut room = needed.saturating_add(8);
        for _ in 0..5 {
            // SAFETY: an all-zero RM_PROCESS_INFO is a valid (empty) value.
            let mut infos: Vec<RM_PROCESS_INFO> = vec![unsafe { std::mem::zeroed() }; room as usize];
            have = room;
            // SAFETY: `infos` has `have` writable entries.
            let rc = unsafe { RmGetList(self.0, &raw mut needed, &raw mut have, infos.as_mut_ptr(), &raw mut reasons) };
            if rc == ERROR_MORE_DATA {
                room = needed.saturating_add(8);
                continue;
            }
            if rc != ERROR_SUCCESS {
                return Err(StopError::RestartManager { call: "RmGetList", code: rc });
            }
            return Ok(infos
                .iter()
                .take(have as usize)
                .filter_map(|i| {
                    let ft = (u64::from(i.Process.ProcessStartTime.dwHighDateTime) << 32)
                        | u64::from(i.Process.ProcessStartTime.dwLowDateTime);
                    let started = crate::identity::micros_from_filetime(ft)?;
                    Some(RmProcess {
                        id: ProcessId::new(i.Process.dwProcessId, StartTime::from_micros(started)),
                        session: (i.TSSessionId != RM_INVALID_TS_SESSION).then_some(i.TSSessionId),
                        name: from_wide(&i.strAppName),
                        image: image_name_of(i.Process.dwProcessId),
                    })
                })
                .collect());
        }
        Err(StopError::RestartManager { call: "RmGetList", code: ERROR_MORE_DATA })
    }
}

impl Drop for RmSession {
    fn drop(&mut self) {
        // SAFETY: the handle came from RmStartSession and is ended once.
        unsafe { RmEndSession(self.0) };
    }
}

/// The program's file name of `pid`, read through a handle (needs the privilege for another user's process).
fn image_name_of(pid: u32) -> Option<String> {
    // SAFETY: plain values; the handle is closed below.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is writable for `len` characters.
    let ok = unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &raw mut len) } != 0;
    // SAFETY: opened above.
    unsafe { CloseHandle(h) };
    ok.then(|| from_wide(&buf[..len as usize])).and_then(|p| p.rsplit(['\\', '/']).next().map(str::to_owned))
}

/// A process held open by handle, closed on drop.
pub struct HeldProcess {
    handle: HANDLE,
    id: ProcessId,
    image: Option<PathBuf>,
    session: Option<u32>,
}

impl Held for HeldProcess {
    fn id(&self) -> ProcessId {
        self.id
    }
    fn image(&self) -> Option<&Path> {
        self.image.as_deref()
    }
    fn session(&self) -> Option<u32> {
        self.session
    }
}

impl Drop for HeldProcess {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `open` and is closed once.
        unsafe { CloseHandle(self.handle) };
    }
}

fn token_privilege(enable: bool) -> Result<(), StopError> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the current-process pseudo handle and a valid out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(StopError::Privilege(unsafe { GetLastError() }));
    }
    let name = wide(OsStr::new("SeDebugPrivilege"));
    // SAFETY: an all-zero LUID is valid; the name is NUL-terminated.
    let mut luid = unsafe { std::mem::zeroed() };
    let result = if unsafe { LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &raw mut luid) } == 0 {
        Err(StopError::Privilege(unsafe { GetLastError() }))
    } else {
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: if enable { SE_PRIVILEGE_ENABLED } else { 0 } }],
        };
        // SAFETY: `tp` is one fully initialised privilege entry; the token has TOKEN_ADJUST_PRIVILEGES.
        let ok = unsafe { AdjustTokenPrivileges(token, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) };
        let last = unsafe { GetLastError() };
        if ok == 0 {
            Err(StopError::Privilege(last))
        } else if last == ERROR_NOT_ALL_ASSIGNED {
            // Succeeded, but the token does not hold the privilege (a policy removed it).
            Err(StopError::Privilege(ERROR_NOT_ALL_ASSIGNED))
        } else {
            Ok(())
        }
    };
    // SAFETY: the token handle was opened above.
    unsafe { CloseHandle(token) };
    result
}

impl Machine for WindowsMachine {
    type Held = HeldProcess;

    fn rm_list(&mut self, files: &[PathBuf]) -> Result<Vec<RmProcess>, StopError> {
        RmSession::start(files)?.list()
    }

    fn processes(&mut self) -> Result<Vec<Candidate>, StopError> {
        // SAFETY: a plain snapshot call.
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return Err(StopError::Table(unsafe { GetLastError() }));
        }
        // SAFETY: an all-zero PROCESSENTRY32W with dwSize set is what the API wants.
        let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut out = Vec::new();
        // SAFETY: `snap` is a valid snapshot and `e` is sized.
        let mut ok = unsafe { Process32FirstW(snap, &raw mut e) };
        while ok != 0 {
            out.push(Candidate { pid: e.th32ProcessID, image: from_wide(&e.szExeFile) });
            // SAFETY: as above.
            ok = unsafe { Process32NextW(snap, &raw mut e) };
        }
        // SAFETY: the snapshot handle was created above.
        unsafe { CloseHandle(snap) };
        Ok(out)
    }

    fn enable_privilege(&mut self) -> Result<(), StopError> {
        token_privilege(true)
    }

    fn drop_privilege(&mut self) {
        if let Err(e) = token_privilege(false) {
            log::warn!("SeDebugPrivilege could not be disabled again: {e}");
        }
    }

    fn open(&mut self, pid: u32, for_end: bool) -> Result<HeldProcess, OpenFailure> {
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | if for_end { PROCESS_TERMINATE } else { 0 };
        // SAFETY: plain values; the handle is owned by the returned HeldProcess.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            let code = unsafe { GetLastError() };
            return Err(if code == ERROR_INVALID_PARAMETER { OpenFailure::Gone } else { OpenFailure::Denied(code) });
        }
        let Some(started) = crate::sys::creation_time(handle) else {
            // SAFETY: opened above.
            unsafe { CloseHandle(handle) };
            return Err(OpenFailure::Denied(unsafe { GetLastError() }));
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` is writable for `len` characters.
        let image = (unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &raw mut len) } != 0)
            .then(|| PathBuf::from(from_wide(&buf[..len as usize])));
        let mut session = 0u32;
        // SAFETY: a valid out-pointer.
        let session = (unsafe { ProcessIdToSessionId(pid, &raw mut session) } != 0).then_some(session);
        Ok(HeldProcess { handle, id: ProcessId::new(pid, started), image, session })
    }

    fn terminate(&mut self, held: &HeldProcess) -> Result<(), StopError> {
        // SAFETY: the handle was opened with PROCESS_TERMINATE.
        if unsafe { TerminateProcess(held.handle, 1) } == 0 {
            return Err(StopError::Terminate(unsafe { GetLastError() }));
        }
        Ok(())
    }

    fn wait_exit(&mut self, held: &HeldProcess, bound: Duration) -> bool {
        let ms = u32::try_from(bound.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: the handle was opened with SYNCHRONIZE.
        unsafe { WaitForSingleObject(held.handle, ms) == WAIT_OBJECT_0 }
    }
}

/// Stops every listed program that runs from `dir` in any session, as this process (which it never ends), with the real
/// Windows machine. `images` are file names in `dir`; the calling program's own file is left out of the Restart Manager
/// registration.
pub fn stop_all_here(dir: &Path, images: &[String], bounds: &Bounds) -> StopReport {
    let own = ProcessId::of(std::process::id()).unwrap_or_else(|| ProcessId::new(std::process::id(), StartTime::from_micros(0)));
    let own_image = std::env::current_exe().ok();
    let req = Request { dir, images, own, own_image: own_image.as_deref() };
    super::stop_all(&mut WindowsMachine, &req, bounds)
}
