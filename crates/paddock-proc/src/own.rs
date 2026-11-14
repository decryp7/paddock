//! What a process can prove it started (ADR 0103, rule 53): the facts a test kit's reaper needs to end only the processes
//! of its own test, and nothing another agent, a user or the OS runs.
//!
//! - [`look`]: one read of the process table with, per process, its identity, parent, process group, whether it belongs to
//!   this user, whether it is a zombie, whether its environment carries a given `NAME=value` and whether its command line
//!   names a given text. A process of another user is never "carrying" anything: its environment cannot be read.
//! - [`own_group`]: makes a `Command` lead a process group of its own (Unix), so the group is one the caller created.
//! - [`become_child_subreaper`] (Linux): orphans of this process's descendants are re-parented to it instead of to init,
//!   so they stay in its tree.
//! - [`reap_zombie`] (Linux): collects an ended orphan that was re-parented to this process.
//! - [`KillOnClose`] (Windows): a kill-on-close job that a child, and everything it starts, joins.
//!
//! Nothing here signals a process except [`KillOnClose::terminate`], which acts through a job handle the caller owns.

use crate::identity::{ProcessId, StartTime};
use std::process::Command;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

/// One process as [`look`] saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// Its identity (pid and start time).
    pub id: ProcessId,
    /// Its parent's pid, when known.
    pub parent: Option<u32>,
    /// Its process group (Unix), when known.
    pub group: Option<u32>,
    /// Whether it runs as this process's effective user (always false on Windows, where the reaper relies on its job).
    pub same_user: bool,
    /// Whether it has ended and only waits to be collected.
    pub zombie: bool,
    /// Whether its environment carries the `NAME=value` that [`look`] was given (only readable for this user's processes).
    pub carries: bool,
    /// Whether its command line names one of the texts that [`look`] was given.
    pub names: bool,
}

/// Reads the process table once. `carry` is the `(NAME, value)` an environment must hold for [`Seen::carries`]; `names` the
/// texts one of which a command line must contain for [`Seen::names`]. A process whose start time cannot be read is left out (nothing
/// could act on it by identity). Empty when the table cannot be read.
pub fn look(carry: Option<(&str, &str)>, names: &[&str]) -> Vec<Seen> {
    let Ok(mut sys) = System::new() else {
        return Vec::new();
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_environ(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    let wanted = carry.map(|(k, v)| format!("{k}={v}"));
    sys.processes()
        .iter()
        .filter(|(_, p)| p.thread_kind().is_none())
        .filter_map(|(pid, p)| {
            let pid = pid.as_u32();
            let id = ProcessId::of(pid)?;
            let same_user = same_user(p.user_id());
            Some(Seen {
                id,
                parent: p.parent().map(Pid::as_u32),
                group: group_of(pid),
                same_user,
                zombie: matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead),
                carries: same_user
                    && wanted
                        .as_ref()
                        .is_some_and(|w| p.environ().iter().any(|e| e.to_string_lossy() == *w)),
                names: names
                    .iter()
                    .any(|n| p.cmd().iter().any(|a| a.to_string_lossy().contains(n))),
            })
        })
        .collect()
}

/// The start time of a process starting now, for "started after" comparisons with [`ProcessId::started`].
pub fn now() -> StartTime {
    let micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX));
    StartTime::from_micros(micros)
}

#[cfg(unix)]
fn same_user(uid: Option<&sysinfo::Uid>) -> bool {
    uid.is_some_and(|u| **u == crate::effective_uid())
}

#[cfg(not(unix))]
fn same_user(_: Option<&sysinfo::Uid>) -> bool {
    false
}

/// The process group of `pid` (Unix), when it can be read.
#[cfg(unix)]
pub fn group_of(pid: u32) -> Option<u32> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: getpgid takes a plain value and reads nothing of ours.
    let g = unsafe { libc::getpgid(pid) };
    u32::try_from(g).ok().filter(|g| *g > 0)
}

/// Windows has no process groups.
#[cfg(not(unix))]
pub fn group_of(_: u32) -> Option<u32> {
    None
}

/// Makes `cmd` lead a process group of its own when it starts (Unix; nothing on Windows), so the group is one the caller made.
pub fn own_group(cmd: &mut Command) {
    crate::sys::own_group(cmd);
}

/// Linux: marks this process as a child subreaper (`PR_SET_CHILD_SUBREAPER`), so an orphan among its descendants is re-parented
/// to it and stays in its tree. Returns whether the flag is set. Other systems have no such flag: `false`.
#[cfg(target_os = "linux")]
pub fn become_child_subreaper() -> bool {
    // SAFETY: prctl with these plain arguments only sets a flag on this process.
    unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) == 0 }
}

/// Only Linux has a child-subreaper flag.
#[cfg(not(target_os = "linux"))]
pub fn become_child_subreaper() -> bool {
    false
}

/// Linux: collects `id` if it is an ended child of this process (an orphan re-parented to a subreaper), so it leaves the table.
/// Never for a child whose `std::process::Child` someone still holds: its `wait` would then fail. Does nothing elsewhere.
pub fn reap_zombie(id: ProcessId) {
    #[cfg(target_os = "linux")]
    if crate::sys::is_zombie(id.pid())
        && let Ok(pid) = libc::pid_t::try_from(id.pid())
    {
        let mut status = 0;
        // SAFETY: waitpid on one pid with WNOHANG; a pid that is not our child returns ECHILD and changes nothing.
        let _ = unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) };
    }
    #[cfg(not(target_os = "linux"))]
    let _: ProcessId = id;
}

/// Whether `id` has ended: gone, or a zombie that only waits to be collected.
pub fn has_ended(id: ProcessId) -> bool {
    id.has_ended()
        || (crate::sys::start_time(id.pid()) == Some(id.started())
            && crate::sys::is_zombie(id.pid()))
}

/// Windows: a kill-on-close job. A child assigned to it, and every process it starts afterwards, ends when the job is
/// terminated or this value is dropped.
#[cfg(windows)]
pub struct KillOnClose(crate::job::Job);

#[cfg(windows)]
impl KillOnClose {
    /// Puts `child` into a new kill-on-close job.
    pub fn assign(child: &std::process::Child) -> std::io::Result<KillOnClose> {
        crate::job::Job::assign(child, None).map(KillOnClose)
    }

    /// Ends every process in the job now, through the job's handle.
    pub fn terminate(&self) {
        self.0.terminate();
    }
}
