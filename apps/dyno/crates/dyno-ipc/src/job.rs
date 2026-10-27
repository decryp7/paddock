//! Windows: a job object per child that kills it when the host's handle closes (the host exited or crashed) and can
//! cap its committed memory (an allocation past the cap fails, and Rust aborts the child).
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;
use std::ptr::null;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};

/// The job; closing it (on drop) kills the child.
pub(crate) struct Job(#[allow(dead_code)] OwnedHandle);

impl Job {
    /// Puts `child` into a new job with kill-on-close and an optional memory cap in bytes.
    pub(crate) fn assign(child: &Child, memory_limit: Option<u64>) -> io::Result<Job> {
        // SAFETY: no attributes, no name; the handle is owned below.
        let h = unsafe { CreateJobObjectW(null(), null()) };
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a fresh, valid handle nobody else owns.
        let job = unsafe { OwnedHandle::from_raw_handle(h) };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(limit) = memory_limit {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = usize::try_from(limit).unwrap_or(usize::MAX);
        }
        // SAFETY: `info` is a valid structure of the size passed, for the class passed.
        let set = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both handles are valid; the child's stays open while `child` lives.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Job(job))
    }
}
