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
//!   not only its standard handles. The runner's step pipe reaches `acquire` as a plain inherited handle when `acquire` was
//!   started by another Rust program (`./dev ci isolation acquire`: std starts children with inheritance on, so the parent's own
//!   standard handles travel along beside the child's). `acquire` therefore clears the inherit flag of EVERY handle it has
//!   before it starts the holder ([`keep_handles_from_children`]); std makes the holder's own stdio inheritable at the spawn,
//!   after that. Run 8019 (jobs 37039, 37040): with only the standard handles cleared, the holder kept the step's pipe; the
//!   Gitea runner waits 10 s for a step's pipe after the step ends, then ends every process of the step's job object
//!   (gitea/runner `internal/pkg/process/treekill.go`), the holder with it, and a second job took the slot. The documented
//!   alternative, `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, is unstable in std (`CommandExt::spawn_with_attributes`, issue 114854).

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

/// The highest handle value [`keep_handles_from_children`] looks at. Handle values are multiples of 4 and a process's table
/// fills from the bottom; a holder's starter is a small process, so its inherited handles are far below this.
#[cfg(windows)]
const HANDLE_SCAN_LIMIT: usize = 1 << 20;

/// Stops every handle of this process from being inherited by children it starts later (Windows), and says how many were
/// inheritable. A child std starts afterwards still gets the stdio handles std makes for it at the spawn. `Err` when a handle
/// could not be changed, or not every handle was found below the scan limit ([`scan_handles`]): the caller must not start a
/// detached child then (it would keep that handle, a pipe the runner waits on).
#[cfg(windows)]
#[allow(unsafe_code)] // Win32 calls on this process's own handle table
pub fn keep_handles_from_children() -> std::io::Result<usize> {
    use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut count = 0u32;
    // SAFETY: GetCurrentProcess returns a pseudo handle; GetProcessHandleCount writes one u32.
    if unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    scan_handles(count, HANDLE_SCAN_LIMIT, |value| {
        let handle = value as HANDLE;
        let mut flags = 0u32;
        // SAFETY: GetHandleInformation only reads the entry; a value that names no handle fails and is skipped.
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            return Ok(None);
        }
        if flags & HANDLE_FLAG_INHERIT == 0 {
            return Ok(Some(false));
        }
        // SAFETY: the handle is this process's own; clearing its inherit flag changes nothing else.
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Some(true))
    })
}

/// The scan of [`keep_handles_from_children`] over handle values 4, 8, ... up to `limit`, until `count` handles were seen.
/// `probe(value)` answers `None` for a value that names no handle, `Some(true)` for an inheritable handle it made non-inheritable,
/// `Some(false)` for one that was not inheritable. Fails closed: an error when a handle could not be changed, and when the limit is
/// reached before all `count` handles were seen (one of the missing ones may still be inheritable).
#[cfg_attr(not(windows), allow(dead_code))] // the scan is Windows-only; its tests run everywhere
fn scan_handles(count: u32, limit: usize, mut probe: impl FnMut(usize) -> std::io::Result<Option<bool>>) -> std::io::Result<usize> {
    let (mut seen, mut cleared) = (0u32, 0usize);
    let mut value = 4usize;
    while seen < count && value <= limit {
        match probe(value) {
            Ok(None) => {}
            Ok(Some(was_inheritable)) => {
                seen += 1;
                cleared += usize::from(was_inheritable);
            }
            Err(e) => return Err(std::io::Error::new(e.kind(), format!("handle {value:#x} stays inheritable: {e}"))),
        }
        value += 4;
    }
    if seen < count {
        return Err(std::io::Error::other(format!(
            "only {seen} of this process's {count} handles were found up to {limit:#x}; one of the others may still be inheritable"
        )));
    }
    Ok(cleared)
}

/// Nothing to do: Unix children get only the descriptors they are given (the holder closes the rest, [`close_inherited`]).
#[cfg(not(windows))]
pub fn keep_handles_from_children() -> std::io::Result<usize> {
    Ok(0)
}

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

    /// The scan stops once every handle was seen, counts the inheritable ones, and fails closed when the limit comes first or a
    /// handle cannot be changed (the count is injected; no handle table is read).
    #[test]
    fn the_handle_scan_fails_closed_when_it_cannot_see_or_change_every_handle() {
        // Handles at 8 (inheritable) and 16 (not); nothing else.
        let table = |v: usize| -> std::io::Result<Option<bool>> {
            Ok(match v {
                8 => Some(true),
                16 => Some(false),
                _ => None,
            })
        };
        assert_eq!(scan_handles(2, 64, table).unwrap(), 1);
        let mut looked = Vec::new();
        scan_handles(2, 64, |v| {
            looked.push(v);
            table(v)
        })
        .unwrap();
        assert_eq!(looked.last(), Some(&16), "the scan ends once both handles were seen");
        let short = scan_handles(3, 64, table).unwrap_err();
        assert!(short.to_string().contains("only 2 of this process's 3 handles"), "{short}");
        assert!(scan_handles(2, 12, table).is_err(), "the limit came before the second handle");
        let stuck = scan_handles(2, 64, |v| if v == 8 { Err(std::io::Error::other("denied")) } else { table(v) }).unwrap_err();
        assert!(stuck.to_string().contains("handle 0x8 stays inheritable: denied"), "{stuck}");
    }

    /// Changes the inherit flag of every handle of the test process: it needs nextest's one process per test (another test's
    /// child started meanwhile would get no inherited handles).
    #[test]
    fn handles_can_be_marked_non_inheritable_at_any_time() {
        keep_handles_from_children().unwrap();
    }

    /// An inheritable handle this process opened is no longer inheritable afterwards, and is counted. Changes every handle of the
    /// test process: it needs nextest's one process per test.
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code)] // Win32 calls on the test's own pipe handle
    fn an_inheritable_handle_is_no_longer_inheritable() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation};
        let (_reader, writer) = std::io::pipe().unwrap();
        let h = writer.as_raw_handle();
        // SAFETY: the test's own open pipe handle.
        assert_ne!(unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) }, 0);
        assert!(keep_handles_from_children().unwrap() >= 1);
        let mut flags = 0u32;
        // SAFETY: as above; one u32 written.
        assert_ne!(unsafe { GetHandleInformation(h, &mut flags) }, 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
    }
}
