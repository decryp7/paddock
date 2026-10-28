//! Descriptors a detached process inherited.
//!
//! The slot holder outlives the step that started it. Any pipe the runner gave the step (or that a sibling thread's fork
//! leaked into it: on macOS a pipe is not created close-on-exec atomically) that the holder still has open would keep the
//! runner waiting for the end of the step's output. The holder therefore closes every descriptor above the standard three
//! before it opens its own. On Windows, std passes only the standard handles, so there is nothing to close.

/// Closes every inherited descriptor above 2.
#[cfg(unix)]
#[allow(unsafe_code)] // close(2) on numbers this process does not use; the one unsafe block of the tool
pub fn close_inherited() {
    // SAFETY: sysconf has no preconditions.
    let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    let limit = if limit < 0 { 1024 } else { limit.min(65_536) };
    for fd in 3..limit {
        // SAFETY: closing a number that is not open fails with EBADF; at this point the process has opened no file of its own.
        unsafe {
            libc::close(fd as libc::c_int);
        }
    }
}

/// Nothing to close.
#[cfg(not(unix))]
pub fn close_inherited() {}
