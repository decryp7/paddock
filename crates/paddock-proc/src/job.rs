//! Windows: every supervised child goes into a job object of its own with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (plan
//! X54). The job's handle lives in the [`Supervised`](crate::Supervised) value, so when it is dropped, or when the app
//! exits or crashes and Windows closes its handles, every process in the job ends: the child and the helpers it started,
//! even those that left its process tree (an orphan's parent pid no longer leads back to the child, so `taskkill /T`
//! cannot find it, but the job still holds it). From Dyno's `dyno-ipc` `job.rs`.
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;
use std::ptr::null;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};

/// A kill-on-close job; closing its handle (on drop) ends every process in it.
pub(crate) struct Job(OwnedHandle);

impl Job {
    /// Puts `child` into a new kill-on-close job. Processes it starts afterwards join the job too (nested jobs: Windows 8
    /// and later, also when the app itself runs in a job).
    pub(crate) fn assign(child: &Child) -> io::Result<Job> {
        // SAFETY: no attributes, no name; the handle is owned below.
        let h = unsafe { CreateJobObjectW(null(), null()) };
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a fresh, valid handle nobody else owns.
        let job = unsafe { OwnedHandle::from_raw_handle(h) };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // A crash in the child ends it rather than waiting on a Windows Error Reporting dialog nobody sees.
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        // SAFETY: `info` is a valid structure of the size passed, for the class passed.
        let set = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
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

impl Job {
    /// Ends every process in the job now, through the job's handle: no pid is looked up, so none can have been handed on
    /// (ADR 0033). The job stays open; closing it later ends whatever joined since.
    pub(crate) fn terminate(&self) {
        // SAFETY: a valid job handle owned by `self`.
        // conformance: pid-kill — a job handle this value owns: no pid is looked up
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            log::warn!(
                "Could not end the job of a child: {}",
                io::Error::last_os_error()
            );
        }
    }
}
