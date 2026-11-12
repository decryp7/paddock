//! Starting a process that outlives its starter without carrying the starter's handles ([`spawn_detached`]).
//!
//! A detached child that keeps a pipe the starter's parent gave it blocks that parent: the Gitea runner waits 10 s for a step's
//! pipe after the step ends, then ends every process of the step's job object (gitea/runner `internal/pkg/process/treekill.go`),
//! the detached child with it. Seen twice in `tools/paddock-dev` (the standard handles, then every handle: run 8019, jobs 37039
//! and 37040, where a second job took a slot whose holder had been ended). This is the one place that does it (rule 55).
//!
//! - **Unix:** `spawn_detached` leads a process group of its own (no Ctrl+C from the starter's terminal, a `killpg` reaches its
//!   tree). It does not close descriptors in the child: that needs a `pre_exec` hook, which forces fork+exec, and a fork carries
//!   copies of the process's descriptors until it execs. Rust opens its own descriptors close-on-exec; what is left are
//!   descriptors inherited from the starter's parent. The child closes those itself, first thing, with [`close_inherited`]
//!   (`close_range(3, ~0)` on Linux; elsewhere the open descriptors are read from `/dev/fd`; only when that fails does it count
//!   up to the descriptor limit). That also covers pipes a sibling thread's fork leaked on macOS (a pipe is not created
//!   close-on-exec atomically).
//! - **Windows:** `CreateProcessW` is called with handle inheritance on, so a child gets every inheritable handle of its parent,
//!   not only its standard handles. The runner's step pipe reaches a Rust starter as a plain inherited handle when it was started
//!   by another Rust program (std starts children with inheritance on, so the parent's own standard handles travel along beside
//!   the child's). `spawn_detached` therefore clears the inherit flag of EVERY handle of this process
//!   ([`keep_handles_from_children`]); std makes the child's own stdio inheritable at the spawn, after that. The change is
//!   process-wide and stays: call it from a program that starts no other child relying on an inherited handle. The documented
//!   alternative, `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, is unstable in std (`CommandExt::spawn_with_attributes`, issue 114854).
//!   The flags are `DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`.

use std::process::{Child, Command};

/// Why [`spawn_detached`] did not start the child.
#[derive(Debug, thiserror::Error)]
pub enum DetachError {
    /// Not every handle of this process could be kept from the child (Windows): starting it would leak one, and a leaked pipe
    /// ends the child with the step that started it. Nothing was started.
    #[error(
        "this process's handles could not be kept from the detached child ({0}), so it would be ended with its starter's step"
    )]
    Handles(#[source] std::io::Error),
    /// The operating system refused to start the program.
    #[error("{program} could not be started: {source}")]
    Spawn {
        /// The program that was to start.
        program: String,
        /// What the operating system said.
        #[source]
        source: std::io::Error,
    },
}

/// Windows process creation flags of a detached child: no console at all, no window, its own process group.
#[cfg(windows)]
const DETACHED_FLAGS: u32 = 0x0000_0008 | crate::CREATE_NO_WINDOW | 0x0000_0200;

/// Starts `cmd` as a process that outlives this one and shares nothing with it but what `cmd` passes explicitly (its standard
/// streams): its own process group (Unix) or process group and no console (Windows), and on Windows no inherited handle but its
/// standard streams. Fail closed: when the handles cannot all be kept from it, nothing is started ([`DetachError::Handles`]).
///
/// Build `cmd` with [`crate::command`]; set its arguments, working directory and standard streams first (a `stdout` pipe is
/// the usual announcement channel). The flags set here replace any creation flags already on `cmd`. On Unix the started
/// program should call [`close_inherited`] before it opens anything of its own.
///
/// # Errors
/// [`DetachError::Handles`] on Windows when a handle could not be made non-inheritable; [`DetachError::Spawn`] when the
/// program could not be started.
pub fn spawn_detached(cmd: &mut Command) -> Result<Child, DetachError> {
    keep_handles_from_children().map_err(DetachError::Handles)?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(DETACHED_FLAGS);
    }
    cmd.spawn().map_err(|source| DetachError::Spawn {
        program: cmd.get_program().to_string_lossy().into_owned(),
        source,
    })
}

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
    let listed = std::fs::read_dir("/dev/fd").map(|d| {
        parse_fd_listing(
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned()),
        )
    });
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
    if reported < 0 {
        1024
    } else {
        reported.min(1 << 20) as i32
    }
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
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };
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
fn scan_handles(
    count: u32,
    limit: usize,
    mut probe: impl FnMut(usize) -> std::io::Result<Option<bool>>,
) -> std::io::Result<usize> {
    let (mut seen, mut cleared) = (0u32, 0usize);
    let mut value = 4usize;
    while seen < count && value <= limit {
        match probe(value) {
            Ok(None) => {}
            Ok(Some(was_inheritable)) => {
                seen += 1;
                cleared += usize::from(was_inheritable);
            }
            Err(e) => {
                return Err(std::io::Error::new(
                    e.kind(),
                    format!("handle {value:#x} stays inheritable: {e}"),
                ));
            }
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
        assert_eq!(
            parse_fd_listing(["0", "1", "2", "3", "17", "4", "x", "-1", "17", ""]),
            [3, 4, 17]
        );
        assert!(parse_fd_listing(Vec::<String>::new()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_descriptor_limit_is_bounded_and_has_a_default() {
        assert_eq!(
            (
                descriptor_limit(-1),
                descriptor_limit(256),
                descriptor_limit(i64::MAX)
            ),
            (1024, 256, 1 << 20)
        );
    }

    /// The child half of `the_last_resort_closes_inherited_descriptors`: opens a file, closes everything above 2 and checks.
    #[cfg(unix)]
    #[test]
    fn last_resort_child() {
        use std::os::fd::AsRawFd;
        if std::env::var_os("PADDOCK_PROC_DETACH_CHILD").is_none() {
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
            .args([
                "--exact",
                "detach::tests::last_resort_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("PADDOCK_PROC_DETACH_CHILD", "1")
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
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
        assert_eq!(
            looked.last(),
            Some(&16),
            "the scan ends once both handles were seen"
        );
        let short = scan_handles(3, 64, table).unwrap_err();
        assert!(
            short
                .to_string()
                .contains("only 2 of this process's 3 handles"),
            "{short}"
        );
        assert!(
            scan_handles(2, 12, table).is_err(),
            "the limit came before the second handle"
        );
        let stuck = scan_handles(2, 64, |v| {
            if v == 8 {
                Err(std::io::Error::other("denied"))
            } else {
                table(v)
            }
        })
        .unwrap_err();
        assert!(
            stuck
                .to_string()
                .contains("handle 0x8 stays inheritable: denied"),
            "{stuck}"
        );
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
        use windows_sys::Win32::Foundation::{
            GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation,
        };
        let (_reader, writer) = std::io::pipe().unwrap();
        let h = writer.as_raw_handle();
        // SAFETY: the test's own open pipe handle.
        assert_ne!(
            unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
            0
        );
        assert!(keep_handles_from_children().unwrap() >= 1);
        let mut flags = 0u32;
        // SAFETY: as above; one u32 written.
        assert_ne!(unsafe { GetHandleInformation(h, &mut flags) }, 0);
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
    }

    /// A detached child leads a process group of its own (Unix) and is a live child the caller can end.
    #[cfg(unix)]
    #[test]
    #[allow(unsafe_code)] // getpgid on the pid of a child this test holds unreaped
    fn a_detached_child_leads_its_own_process_group() {
        let mut cmd = crate::command("sleep");
        cmd.arg("30")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null());
        let mut child = spawn_detached(&mut cmd).unwrap();
        let pid = child.id() as libc::pid_t;
        // SAFETY: getpgid reads the group of a pid; the child is held and not yet reaped, so the pid is its own.
        let group = unsafe { libc::getpgid(pid) };
        // SAFETY: getpgid(0) is this process's own group.
        let ours = unsafe { libc::getpgid(0) };
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(group, pid, "the child leads its own group");
        assert_ne!(group, ours);
    }

    /// The started program's standard output reaches the caller through the pipe `cmd` asked for.
    #[test]
    fn a_detached_child_keeps_the_streams_it_was_given() {
        use std::io::Read;
        let mut cmd = crate::command(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "detach::tests::detached_child_half",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("PADDOCK_PROC_DETACH_SAY", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
        let mut child = spawn_detached(&mut cmd).unwrap();
        let mut said = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut said)
            .unwrap();
        child.wait().unwrap();
        assert!(said.contains("DETACHED-OK"), "{said}");
    }

    /// The child half of `a_detached_child_keeps_the_streams_it_was_given`.
    #[test]
    fn detached_child_half() {
        if std::env::var_os("PADDOCK_PROC_DETACH_SAY").is_some() {
            println!("DETACHED-OK");
        }
    }

    /// A program that does not exist is a typed error that names it; nothing is left running.
    #[test]
    fn a_missing_program_is_a_spawn_error_naming_it() {
        let mut cmd = crate::command("paddock-proc-no-such-program");
        match spawn_detached(&mut cmd) {
            Err(DetachError::Spawn { program, .. }) => {
                assert_eq!(program, "paddock-proc-no-such-program")
            }
            other => panic!("expected a spawn error, got {other:?}"),
        }
    }
}
