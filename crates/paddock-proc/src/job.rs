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
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
};

/// A kill-on-close job; closing its handle (on drop) ends every process in it.
pub(crate) struct Job(OwnedHandle);

impl Job {
    /// The ids of the processes in the job (the child and every helper it started, even orphans), read from the job itself:
    /// no walk of the machine's process list. At most [`crate::MAX_TREE`] ids.
    pub(crate) fn pids(&self) -> Vec<u32> {
        // The list header is two u32 and then the ids (usize); a u64 buffer is aligned for it.
        let words = 1 + crate::MAX_TREE;
        let mut buf = vec![0u64; words];
        let list = buf.as_mut_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        // SAFETY: the buffer is `words * 8` bytes, the size passed; the job handle is valid while `self` lives. On success
        // the kernel filled `NumberOfProcessIdsInList` ids, which fit the buffer (it never writes more than the size given).
        unsafe {
            let ok = QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicProcessIdList,
                list.cast(),
                (words * std::mem::size_of::<u64>()) as u32,
                std::ptr::null_mut(),
            );
            if ok == 0 {
                return Vec::new();
            }
            let n = ((*list).NumberOfProcessIdsInList as usize).min(crate::MAX_TREE);
            let ids = (&raw const (*list).ProcessIdList).cast::<usize>();
            (0..n).map(|i| *ids.add(i) as u32).collect()
        }
    }

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
