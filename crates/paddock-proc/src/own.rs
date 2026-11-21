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
///
/// The identities are read first, the facts after (as `processes_mentioning` does, ADR 0033): a pid handed on between the two
/// keeps the identity of the process that had it first, so acting on the row finds the new holder and leaves it alone. A row whose
/// identity no longer holds after its group was read is left out.
pub fn look(carry: Option<(&str, &str)>, names: &[&str]) -> Vec<Seen> {
    let identities = crate::usage::snapshot();
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
            let id = crate::usage::identity(&identities, pid)?;
            let same_user = same_user(p.user_id());
            let group = group_of(pid);
            // The group (and the facts above) belong to the process the identity names only if it still holds the pid.
            if ProcessId::of(pid) != Some(id) {
                return None;
            }
            Some(Seen {
                id,
                parent: p.parent().map(Pid::as_u32),
                group,
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

/// One process of this user as [`survey`] saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Its identity (pid and start time).
    pub id: ProcessId,
    /// Its parent's pid, when known.
    pub parent: Option<u32>,
    /// Whether it has ended and only waits to be collected.
    pub zombie: bool,
    /// The value of the variable [`survey`] was asked for in its environment, when it has one and the environment can be read.
    pub value: Option<String>,
    /// Its current folder, when it can be read.
    pub cwd: Option<String>,
    /// Its command line, arguments joined by a space and cut to [`ARGS_CHARS`] characters.
    pub args: String,
}

/// The longest command line [`Row::args`] keeps.
pub const ARGS_CHARS: usize = 4096;

/// Every process of this user, with what tells it from others after its parent is gone: the value of the environment variable `name`,
/// its current folder and its command line (one read of the process table). A child inherits its parent's environment, so a process a
/// job started carries what the job exported, also a detached grandchild that left the job's process tree. **macOS shows no
/// environment of a program Apple ships** (`/bin/bash`, `/bin/sh`, `/bin/sleep`: `ps eww` and `KERN_PROCARGS2` leave it out, measured
/// 2026-11-21), so the value alone finds a Homebrew or Rust program, and the folder and the command line (which are readable) find the
/// scripts: a caller matches all three. The identities are read first, as in [`look`]: a pid handed on in between keeps the first
/// process's identity, so acting on a row finds the new holder and leaves it alone. Processes of other users are left out (nothing of
/// theirs is read); on Windows the result is empty (another process's folder and environment are not read there).
pub fn survey(name: &str) -> Vec<Row> {
    let identities = crate::usage::snapshot();
    let Ok(mut sys) = System::new() else {
        return Vec::new();
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_environ(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    let prefix = format!("{name}=");
    sys.processes()
        .iter()
        .filter(|(_, p)| p.thread_kind().is_none())
        .filter_map(|(pid, p)| {
            let pid = pid.as_u32();
            if !same_user(p.user_id()) {
                return None;
            }
            let id = crate::usage::identity(&identities, pid)?;
            if ProcessId::of(pid) != Some(id) {
                return None;
            }
            let value = p.environ().iter().find_map(|e| {
                e.to_string_lossy()
                    .strip_prefix(&prefix)
                    .map(str::to_string)
            });
            let args: String = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(ARGS_CHARS)
                .collect();
            Some(Row {
                id,
                parent: p.parent().map(Pid::as_u32),
                zombie: matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead),
                value,
                cwd: p.cwd().map(|c| c.to_string_lossy().into_owned()),
                args,
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

/// Whether this process leads its own process group (Unix): true under nextest, which starts every test in a group of its own
/// and ends that group on a timeout. A test helper then never makes a new group (ADR 0103): its children stay in the group the
/// runner ends. Always false on Windows (no groups; a kill-on-close job covers it).
pub fn leads_own_group() -> bool {
    let me = std::process::id();
    group_of(me) == Some(me)
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
/// Only for a process the caller knows was re-parented (it started below another process): a child the caller started itself
/// may still be held by a `std::process::Child`, whose `wait` would then lose its exit status. Does nothing elsewhere.
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
