//! Windows: every plugin runs in a job object that kills it when Pitwall exits or crashes (kill on close), caps its
//! memory (a plugin past the cap fails its allocations and stops; the host restarts it) and, unless the sandbox
//! allows children, lets it start no other process. Assigned while the process is still suspended (see
//! [`crate::contain`]), so there is no window in which it runs outside the job.
#![allow(unsafe_code)]

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};

/// The limits of a plugin's job object.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobLimits {
    /// Process memory cap (committed bytes).
    pub memory: Option<u64>,
    /// No processes besides the plugin itself.
    pub no_children: bool,
}

impl JobLimits {
    /// The limits a sandbox asks for.
    pub fn of(s: &crate::sandbox::SandboxSpec) -> JobLimits {
        JobLimits {
            memory: s.memory_limit,
            no_children: !s.children,
        }
    }
}

/// An owned job handle, closed (killing the job's processes) on drop.
pub struct JobGuard(HANDLE);

// SAFETY: a job handle may be used and closed from any thread.
unsafe impl Send for JobGuard {}
// SAFETY: the calls made through a shared reference (query) are thread-safe.
unsafe impl Sync for JobGuard {}

impl JobGuard {
    /// Puts `process` into a new job with `limits`; `None` when the job cannot be created or assigned.
    pub(crate) fn assign(process: HANDLE, limits: &JobLimits) -> Option<JobGuard> {
        // SAFETY: plain Win32 calls with valid arguments; the handle is closed in `Drop` (or below on failure).
        unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // No error dialog for a crash: the process just ends and the host sees it.
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            if let Some(limit) = limits.memory {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.ProcessMemoryLimit = usize::try_from(limit).unwrap_or(usize::MAX);
            }
            if limits.no_children {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
                info.BasicLimitInformation.ActiveProcessLimit = 1;
            }
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .is_ok()
                && AssignProcessToJobObject(job, process).is_ok();
            if !ok {
                let _ = CloseHandle(job);
                return None;
            }
            Some(JobGuard(job))
        }
    }

    /// The most memory a process of the job committed (bytes), if the OS reports it.
    pub fn peak_memory(&self) -> Option<u64> {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: a valid job handle and an out structure of the right size.
        unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                None,
            )
            .ok()?;
        }
        Some(info.PeakProcessMemoryUsed as u64)
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        // SAFETY: the handle was created by `assign` and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
