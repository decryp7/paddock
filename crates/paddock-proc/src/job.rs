//! Windows: every supervised child goes into a job object of its own with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (plan
//! X54). The job's handle lives in the [`Supervised`](crate::Supervised) value, so when it is dropped, or when the app
//! exits or crashes and Windows closes its handles, every process in the job ends: the child and the helpers it started,
//! even those that left its process tree (an orphan's parent pid no longer leads back to the child, so `taskkill /T`
//! cannot find it, but the job still holds it). From Dyno's `dyno-ipc` `job.rs`.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]
#![allow(unsafe_code)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;
use std::ptr::null;
use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};

/// A kill-on-close job; closing its handle (on drop) ends every process in it.
pub(crate) struct Job(OwnedHandle);

/// How one query of the job's id list ended.
pub(crate) enum Query {
    /// The list fit the buffer.
    Done,
    /// More processes than the buffer holds (the buffer has the first ones).
    MoreData,
    /// The job could not be queried.
    Failed,
}

/// Reads the id list through `query`, which fills the given buffer (header, then ids). A full buffer is not a failure: the
/// ids that were filled count (a tree of more than [`crate::MAX_TREE`] processes is measured by its first ones).
pub(crate) fn pids_with(query: impl FnOnce(&mut [u64]) -> Query) -> Option<Vec<u32>> {
    // The list header is two u32 and then the ids (usize); a u64 buffer is aligned for it.
    let mut buf = vec![0u64; 1 + crate::MAX_TREE];
    match query(&mut buf) {
        Query::Failed => return None,
        Query::Done | Query::MoreData => {}
    }
    let list = buf.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
    // SAFETY: the buffer is at least as large and as aligned as the list header; the query wrote the count of ids it filled,
    // and the count is clamped to the room the buffer has.
    unsafe {
        let n = ((*list).NumberOfProcessIdsInList as usize).min(crate::MAX_TREE);
        let ids = (&raw const (*list).ProcessIdList).cast::<usize>();
        Some((0..n).map(|i| *ids.add(i) as u32).collect())
    }
}

impl Job {
    /// The ids of the processes in the job (the child and every helper it started, even orphans), read from the job itself:
    /// no walk of the machine's process list. At most [`crate::MAX_TREE`] ids; `None` when the job cannot be queried.
    pub(crate) fn pids(&self) -> Option<Vec<u32>> {
        let handle = self.0.as_raw_handle();
        pids_with(|buf| {
            // SAFETY: `buf` is a live u64 buffer of the size passed; the job handle is valid while `self` lives.
            let ok = unsafe {
                QueryInformationJobObject(
                    handle,
                    JobObjectBasicProcessIdList,
                    buf.as_mut_ptr().cast(),
                    std::mem::size_of_val(buf) as u32,
                    std::ptr::null_mut(),
                )
            };
            if ok != 0 {
                Query::Done
            } else if io::Error::last_os_error().raw_os_error() == Some(ERROR_MORE_DATA as i32) {
                Query::MoreData
            } else {
                Query::Failed
            }
        })
    }

    /// The members of the job by identity: each id of [`Job::pids`] whose process, opened by handle, is still in the job
    /// (a member that ended and whose pid was handed on is left out). `None` when the job cannot be queried.
    pub(crate) fn members(&self) -> Option<Vec<crate::ProcessId>> {
        let job = self.0.as_raw_handle();
        Some(
            self.pids()?
                .into_iter()
                .filter_map(|pid| crate::sys::identity_in_job(pid, job))
                .collect(),
        )
    }

    /// Puts `child` into a new kill-on-close job. Processes it starts afterwards join the job too (nested jobs: Windows 8
    /// and later, also when the app itself runs in a job).
    pub(crate) fn assign(child: &Child, commit_limit: Option<u64>) -> io::Result<Job> {
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
        if let Some(limit) = commit_limit {
            // An allocation past the limit fails in the child at once (Rust aborts it), however fast it allocates.
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = usize::try_from(limit).unwrap_or(usize::MAX);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a list header with `count` ids that are `1..=count` (as the kernel does) into `buf`.
    fn fill(buf: &mut [u64], count: usize) {
        buf[0] = (count as u64) << 32 | count as u64;
        for (i, slot) in buf.iter_mut().skip(1).take(count).enumerate() {
            *slot = i as u64 + 1;
        }
    }

    #[test]
    fn a_full_buffer_gives_the_ids_that_were_filled_and_a_failure_gives_none() {
        let ids = pids_with(|b| {
            fill(b, crate::MAX_TREE);
            Query::MoreData
        })
        .unwrap();
        assert_eq!(ids.len(), crate::MAX_TREE);
        assert_eq!(ids.first(), Some(&1));
        let few = pids_with(|b| {
            fill(b, 3);
            Query::Done
        });
        assert_eq!(few, Some(vec![1, 2, 3]));
        assert_eq!(pids_with(|_| Query::Failed), None);
    }
}
