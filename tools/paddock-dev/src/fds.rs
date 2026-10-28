//! Descriptors and handles a detached process must not carry.
//!
//! The slot holder outlives the step that started it. A pipe the runner gave the step that the holder still has open keeps the
//! runner waiting for the end of the step's output, until the job folder is gone and the holder ends.
//!
//! - **Unix:** the holder closes every inherited descriptor above 2 before it opens its own ([`close_inherited`]). That covers
//!   the runner's pipes and, on macOS, pipes a sibling thread's fork leaked (a pipe is not created close-on-exec atomically).
//!   Linux uses `close_range(3, ~0)`; elsewhere the open descriptors are read from `/dev/fd`; only when that fails does it count
//!   up to the descriptor limit.
//! - **Windows:** `CreateProcessW` is called with handle inheritance on, so a child gets every inheritable handle of its parent,
//!   including the runner's stdout pipe. `acquire` clears the inherit flag of its own standard handles before it starts the
//!   holder ([`keep_standard_handles_from_children`]); the holder's own stdout pipe is passed explicitly and is not affected.

/// The descriptor numbers (3 and above) in a listing of file names such as the entries of `/dev/fd`.
#[cfg(unix)]
pub fn parse_fd_listing<I, S>(names: I) -> Vec<i32>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut fds: Vec<i32> = names
        .into_iter()
        .filter_map(|n| n.as_ref().parse::<i32>().ok())
        .filter(|fd| *fd >= 3)
        .collect();
    fds.sort_unstable();
    fds.dedup();
    fds
}

/// Closes every inherited descriptor above 2.
#[cfg(unix)]
#[allow(unsafe_code)] // close(2) and close_range(2) on descriptors this process did not open itself
pub fn close_inherited() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: close_range takes plain integers and closes only descriptors of this process.
        let done = unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) };
        if done == 0 {
            return;
        }
    }
    let listed = std::fs::read_dir("/dev/fd").map(|d| parse_fd_listing(d.flatten().map(|e| e.file_name().to_string_lossy().into_owned())));
    close_from_listing(listed);
}

/// Closes the listed descriptors, or counts up to the limit when there is no listing.
#[cfg(unix)]
fn close_from_listing(listed: std::io::Result<Vec<i32>>) {
    match listed {
        Ok(fds) => fds.into_iter().for_each(close),
        Err(_) => close_up_to_the_limit(),
    }
}

/// The last resort when no listing of the open descriptors exists: every number up to the descriptor limit.
#[cfg(unix)]
#[allow(unsafe_code)] // see close_inherited
fn close_up_to_the_limit() {
    // SAFETY: sysconf has no preconditions.
    let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    (3..descriptor_limit(limit)).for_each(close);
}

/// How far to count: the system's limit, 1024 when it cannot say, never more than a million.
#[cfg(unix)]
pub fn descriptor_limit(reported: i64) -> i32 {
    if reported < 0 { 1024 } else { reported.min(1 << 20) as i32 }
}

#[cfg(unix)]
#[allow(unsafe_code)] // see close_inherited
fn close(fd: i32) {
    // SAFETY: closing a number that is not open fails with EBADF; this process has opened no file of its own yet.
    unsafe {
        libc::close(fd as libc::c_int);
    }
}

/// Nothing to close.
#[cfg(not(unix))]
pub fn close_inherited() {}

/// Stops the standard handles of this process from being inherited by children it starts (Windows).
#[cfg(windows)]
#[allow(unsafe_code)] // two Win32 calls on this process's own standard handles
pub fn keep_standard_handles_from_children() {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle has no preconditions; a null or invalid result is skipped.
        let handle = unsafe { GetStdHandle(which) };
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            // SAFETY: the handle is this process's own; clearing its inherit flag changes nothing else.
            unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Nothing to do: Unix children get only the descriptors they are given.
#[cfg(not(windows))]
pub fn keep_standard_handles_from_children() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_listing_gives_the_open_descriptors_above_the_standard_three() {
        assert_eq!(parse_fd_listing(["0", "1", "2", "3", "17", "4", "x", "-1", "17", ""]), [3, 4, 17]);
        assert!(parse_fd_listing(Vec::<String>::new()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_descriptor_limit_is_bounded_and_has_a_default() {
        assert_eq!(
            (descriptor_limit(-1), descriptor_limit(256), descriptor_limit(i64::MAX)),
            (1024, 256, 1 << 20)
        );
    }

    /// The child half of `the_last_resort_closes_inherited_descriptors`: opens a file, closes everything above 2 and checks.
    #[cfg(unix)]
    #[test]
    fn last_resort_child() {
        use std::os::fd::AsRawFd;
        if std::env::var_os("PADDOCK_DEV_FDS_CHILD").is_none() {
            return;
        }
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd = file.as_raw_fd();
        assert!(fd >= 3);
        close_from_listing(Err(std::io::Error::other("no listing")));
        std::mem::forget(file); // already closed under it
        // SAFETY: fcntl(F_GETFD) only reads the descriptor's flags.
        #[allow(unsafe_code)]
        let still_open = unsafe { libc::fcntl(fd, libc::F_GETFD) } != -1;
        assert!(!still_open, "descriptor {fd} survived");
        println!("CHILD-OK");
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::disallowed_methods)] // a test child of this very test binary
    fn the_last_resort_closes_inherited_descriptors() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "fds::tests::last_resort_child", "--nocapture", "--test-threads=1"])
            .env("PADDOCK_DEV_FDS_CHILD", "1")
            .output()
            .unwrap();
        let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(said.contains("CHILD-OK"), "{said}");
    }

    #[cfg(unix)]
    #[test]
    fn the_system_listing_of_open_descriptors_is_readable_and_includes_the_standard_ones() {
        let names: Vec<String> = std::fs::read_dir("/dev/fd")
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n == "0") && names.iter().any(|n| n == "2"));
    }

    #[test]
    fn standard_handles_can_be_marked_non_inheritable_at_any_time() {
        keep_standard_handles_from_children();
    }
}
