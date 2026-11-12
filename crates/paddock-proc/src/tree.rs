//! Ending processes: a whole tree (`kill_tree`), one process politely (`ask`) or at once (`end`), and both with a
//! grace period (`terminate`). Also liveness and leak checks.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::identity::{Ending, Holder, ProcessId, StartTime, holder, warn_not_ours};
use crate::usage::{self, Table};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate};

/// How a process ended under [`terminate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminated {
    /// It was not running.
    Gone,
    /// It stopped within the grace period after the polite request.
    Gracefully,
    /// It ignored the request and was killed.
    Killed,
    /// The OS process table (or the process's start time) could not be read: nothing was asked and nothing was ended, and
    /// whether the process runs is not known. Not the same as [`Terminated::Gone`].
    Unknown,
}

/// What ending a tree needs from the OS: the real one ([`RealOs`]), or a process table the tests drive.
pub(crate) trait Os {
    /// A fresh look at the process table.
    fn table(&self) -> Table;
    /// When the process that holds `pid` now started; `None` when no process has it.
    fn start_of(&self, pid: u32) -> Option<StartTime>;
    /// Ends the process group `root` led (Unix). Called while the root is still its own process, or gone with no new holder
    /// of its pid: POSIX does not hand a pid on while a process group with that id exists, so a group by that id is still
    /// the root's (helpers that outlived it included). Nothing on Windows, where every member is ended by identity.
    fn end_group(&self, root: u32);
    /// Ends `pid` at once, whoever holds it. Only [`Os::end`] calls it, right after the identity check.
    fn kill_pid(&self, pid: u32) -> bool;
    /// Ends `id` if its pid still belongs to it. Windows overrides this with a check and a kill on one process handle,
    /// which leaves no gap between the two.
    fn end(&self, id: ProcessId) -> Ending {
        check_then_kill(self, id)
    }
}

/// Ends `id` if the start time of its pid's holder is still its own: the check, then the kill (a gap of microseconds on
/// macOS; Linux closes it with a pidfd, Windows with a handle).
fn check_then_kill<O: Os + ?Sized>(os: &O, id: ProcessId) -> Ending {
    let h = holder(id, os.start_of(id.pid()));
    let sent = h == Holder::Same && os.kill_pid(id.pid());
    Ending::of(h, sent)
}

/// The operating system.
pub(crate) struct RealOs;

impl Os for RealOs {
    fn table(&self) -> Table {
        usage::snapshot()
    }

    fn start_of(&self, pid: u32) -> Option<StartTime> {
        crate::sys::start_time(pid)
    }

    fn end_group(&self, root: u32) {
        #[cfg(unix)]
        crate::sys::signal_group(root, crate::sys::SIGKILL);
        #[cfg(not(unix))]
        let _: u32 = root;
    }

    fn kill_pid(&self, pid: u32) -> bool {
        #[cfg(unix)]
        {
            crate::sys::signal_pid(pid, crate::sys::SIGKILL)
        }
        #[cfg(not(unix))]
        {
            let _: u32 = pid;
            false
        }
    }

    #[cfg(windows)]
    fn end(&self, id: ProcessId) -> Ending {
        crate::sys::end_if(id)
    }

    #[cfg(target_os = "linux")]
    fn end(&self, id: ProcessId) -> Ending {
        crate::sys::signal_by_pidfd(id, crate::sys::SIGKILL)
            .unwrap_or_else(|| check_then_kill(self, id))
    }
}

/// What ending one process of a tree found, root first.
pub type TreeEnding = Vec<(ProcessId, Ending)>;

/// Ends `root` and the processes below it, each by identity: every process is ended only if its pid still belongs to it
/// when its turn comes (ending the root ends its children too, and Windows hands their pids on at once; ADR 0033). A pid
/// that names another process by then is skipped, and a kill the OS refused is reported; both are logged.
///
/// The tree is `members` when given (a Windows job's members, identified through a handle each: no parent pid is followed)
/// and otherwise read from one look at the process table, following parent pids by age ([`usage::descendants`]).
/// `root_is_ours`: the root is a child of this process that has not been reaped, so its pid cannot have been handed on
/// (its start time may no longer be readable once it exited).
pub(crate) fn kill_tree_on(
    os: &dyn Os,
    root: ProcessId,
    root_is_ours: bool,
    members: Option<Vec<ProcessId>>,
) -> TreeEnding {
    let kids = match members {
        Some(m) => m.into_iter().filter(|p| p.pid() != root.pid()).collect(),
        None => usage::descendants(&os.table(), root),
    };
    let mut out = Vec::with_capacity(kids.len() + 1);
    let root_holder = if root_is_ours {
        Holder::Same
    } else {
        holder(root, os.start_of(root.pid()))
    };
    match root_holder {
        Holder::Same => {
            os.end_group(root.pid());
            out.push((root, os.end(root)));
        }
        Holder::Gone => {
            os.end_group(root.pid());
            out.push((root, Ending::Gone));
        }
        h @ Holder::Other(_) => out.push((root, Ending::of(h, false))),
    }
    out.extend(kids.into_iter().map(|kid| (kid, os.end(kid))));
    for (id, e) in &out {
        match e {
            Ending::NotOurs(now) => warn_not_ours(*id, *now),
            Ending::Refused => {
                log::warn!("Could not end {id} of the tree of {root}: the system refused")
            }
            Ending::Ended | Ending::Gone => {}
        }
    }
    out
}

/// Ends the process group `group` while `keeper` (a process that joined the group only to hold its id, see
/// [`crate::guardian`]) is still the process it was and still in it: the id cannot then have been handed to another group
/// (rule 53). True when the group was signalled.
#[cfg(unix)]
pub(crate) fn end_pinned_group(group: u32, keeper: ProcessId) -> bool {
    keeper.is_alive()
        && crate::sys::group_of(keeper.pid()) == Some(group)
        && crate::sys::signal_group(group, crate::sys::SIGKILL)
}

/// Whether the process group `group` still has a member that runs. Only valid once the group cannot gain members, that is
/// after it was SIGKILLed (the guardian's use): a zombie leader whose other threads still run counts as running, but a
/// live group may fork between the look and the answer.
///
/// A zombie, which only waits to be reaped, is not a member that runs: Linux's `kill(-group, 0)` counts it, and where
/// nothing reaps the orphans (a container whose init does not) the answer would stay "yes" for ever. macOS does not count
/// zombies, so there the signal alone answers. On Linux, when the signal says yes, `/proc/*/stat` is read once per pid
/// (group and state together, no sysinfo); an unreadable `/proc` leaves the signal's answer.
#[cfg(unix)]
pub(crate) fn group_exists(group: u32) -> bool {
    if !crate::sys::signal_group(group, 0) {
        return false;
    }
    #[cfg(target_os = "linux")]
    return group_runs_in_proc(std::path::Path::new("/proc"), group).unwrap_or(true);
    #[cfg(not(target_os = "linux"))]
    true
}

/// What `/proc/<pid>/stat` says of a process: its group (field 5), whether it runs, and whether it is a zombie that still
/// has running threads (field 20, `num_threads`, above 1: the leader's zombie state hides them) and so counts as running.
/// Fields are read after the last `)`, since the name (field 2) may hold spaces and parentheses.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn parse_group_and_running(stat: &[u8]) -> Option<(u32, bool)> {
    // Bytes, not text: the name (field 2) can be any bytes, and only the tail after its closing `)` is ASCII.
    let rest = stat.get(stat.iter().rposition(|&b| b == b')')? + 1..)?;
    let rest = std::str::from_utf8(rest).ok()?;
    let f: Vec<&str> = rest.split_whitespace().collect();
    // After the name: state (3), ppid (4), pgrp (5), ... num_threads (20) are f[0], f[1], f[2], ... f[17].
    let state = *f.first()?;
    let group: u32 = f.get(2)?.parse().ok()?;
    let threads: u32 = f.get(17)?.parse().ok()?;
    let zombie = matches!(state, "Z" | "X" | "x");
    Some((group, !zombie || threads > 1))
}

/// Whether a process under `proc_root` is in `group` and runs. `None` when nothing is known: `proc_root` cannot be listed
/// or lists no pid at all. A process whose stat is gone (`NotFound`, or `ESRCH` as `/proc` says for a process that ended
/// while it was read) ended and is not in the group; a stat that exists but cannot be read or parsed is a process of unknown
/// state and counts as running (unknown is never read as gone, ADR 0081).
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn group_runs_in_proc(proc_root: &std::path::Path, group: u32) -> Option<bool> {
    let mut seen = false;
    let mut runs = false;
    for e in std::fs::read_dir(proc_root).ok()?.flatten() {
        let numeric = e
            .file_name()
            .to_str()
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !numeric {
            continue;
        }
        seen = true;
        match std::fs::read(e.path().join("stat")) {
            Ok(bytes) => match parse_group_and_running(&bytes) {
                Some((g, r)) => runs |= g == group && r,
                None => runs = true,
            },
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    || err.raw_os_error() == Some(libc::ESRCH) => {}
            Err(_) => runs = true,
        }
    }
    seen.then_some(runs)
}

/// Ends `root` and everything below it. A child started by this crate leads its own process group (Unix), which is
/// signalled as a whole while the root is still the process `root` names (or is gone with no new holder of its pid); every
/// process of the tree is then ended by identity, so a pid handed on since the tree was read is left alone (ADR 0033).
/// Windows ends each process through a handle after checking its creation time (never `taskkill /T`, which follows parent
/// pids). Safe to call for a process that is already gone. Returns what it found for each process, root first.
pub fn kill_tree(root: ProcessId) -> TreeEnding {
    kill_tree_on(&RealOs, root, false, None)
}

/// [`kill_tree`] for a child of this process that has not been reaped, with its job's members on Windows.
pub(crate) fn kill_child_tree(root: ProcessId, members: Option<Vec<ProcessId>>) -> TreeEnding {
    kill_tree_on(&RealOs, root, true, members)
}

/// Ends the process group led by `pid`, a child of this process that has not been reaped and whose start time could not
/// be read (so no [`ProcessId`] exists): an unreaped child is ours by definition, its pid cannot have been handed on, and
/// its group is what it started (ADR 0033 decision 4). Not for a child that may have been reaped: its pid may belong to
/// another group by now, and with an identity [`kill_tree`] is the call. Windows has no groups: a child there ends through
/// its handle or job, so this does nothing.
pub fn end_unreaped_group(pid: u32) {
    #[cfg(unix)]
    crate::sys::signal_group(pid, crate::sys::SIGKILL);
    #[cfg(not(unix))]
    let _: u32 = pid;
}

/// Every process below `root` (not including it), from a fresh look at the process table.
pub fn descendants_of(root: ProcessId) -> Vec<ProcessId> {
    usage::descendants(&usage::snapshot(), root)
}

/// Asks one process (not its group, which may contain the caller) to stop: SIGTERM, or a close request on Windows. Only
/// while its pid still belongs to it. Returns whether the request was delivered.
pub fn ask(id: ProcessId) -> bool {
    #[cfg(target_os = "linux")]
    let (found, sent) = match crate::sys::signal_by_pidfd(id, crate::sys::SIGTERM) {
        Some(Ending::Ended) => (Holder::Same, true),
        Some(Ending::Refused) => (Holder::Same, false),
        Some(Ending::Gone) => (Holder::Gone, false),
        Some(Ending::NotOurs(now)) => (Holder::Other(now), false),
        None => ask_checked(id),
    };
    #[cfg(windows)]
    let (found, sent) = crate::sys::close_request(id);
    #[cfg(not(any(target_os = "linux", windows)))]
    let (found, sent) = ask_checked(id);
    if let Holder::Other(now) = found {
        warn_not_ours(id, now);
    }
    sent
}

/// SIGTERM after the identity check (macOS, and Linux without pidfds).
#[cfg(unix)]
fn ask_checked(id: ProcessId) -> (Holder, bool) {
    let found = holder(id, crate::sys::start_time(id.pid()));
    let sent = found == Holder::Same && crate::sys::signal_pid(id.pid(), crate::sys::SIGTERM);
    (found, sent)
}

#[cfg(not(any(unix, windows)))]
fn ask_checked(id: ProcessId) -> (Holder, bool) {
    (holder(id, crate::sys::start_time(id.pid())), false)
}

/// Ends one process now (SIGKILL, `TerminateProcess`), without touching its group or children, if its pid still belongs
/// to it; a pid handed on is skipped with a warning.
pub fn end(id: ProcessId) -> Ending {
    let e = RealOs.end(id);
    if let Ending::NotOurs(now) = e {
        warn_not_ours(id, now);
    }
    e
}

/// What is known of a pid: the answer is three-valued because "could not look" is not "gone" (rule 53).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// A running process holds the pid.
    Running,
    /// Nothing runs under the pid (a zombie that only waits to be reaped counts as gone).
    Gone,
    /// The OS process table could not be read, so nothing is known.
    Unknown,
}

/// Whether `pid` is a running process (a zombie that only waits to be reaped is not). Says nothing about which process:
/// use [`ProcessId::is_alive`] for that. **Unknown counts as not running**: this is the answer for code that ends or waits
/// for a process (`terminate`: an unreadable table never makes it kill or wait). Code that deletes anything on "gone" (a
/// profile folder, a discovery file) must use [`may_be_alive`] or [`presence`] instead.
pub fn is_alive(pid: u32) -> bool {
    // conformance: pid-liveness — this is the raw-pid function itself; callers hold a ProcessId
    presence(pid) == Presence::Running
}

/// Whether `pid` might be a running process: false only when the OS said it is gone. **Unknown counts as alive**, so a
/// sweep that removes the files of dead processes removes nothing when the process table cannot be read.
pub fn may_be_alive(pid: u32) -> bool {
    // conformance: pid-liveness — this is the raw-pid function itself; callers hold a ProcessId
    presence(pid).may_be_alive()
}

impl Presence {
    /// Whether the process might run: false only for [`Presence::Gone`]; `Unknown` counts as running. The decision of every
    /// caller that deletes or starts something on "gone".
    pub fn may_be_alive(self) -> bool {
        self != Presence::Gone
    }
}

/// The three-valued look at `pid` (see [`Presence`]).
pub fn presence(pid: u32) -> Presence {
    presence_with(pid, sysinfo::System::new)
}

/// [`presence`] over a constructor of the sysinfo `System` (tests give one that fails).
pub(crate) fn presence_with(
    pid: u32,
    make: impl FnOnce() -> Result<sysinfo::System, sysinfo::Error>,
) -> Presence {
    let Some(mut sys) = usage::new_system_with(make) else {
        return Presence::Unknown;
    };
    let id = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        true,
        ProcessRefreshKind::nothing(),
    );
    let seen = sys.process(id).map(|p| (p.start_time(), p.status()));
    classify(
        seen,
        || crate::sys::pid_exists(pid),
        || crate::sys::is_zombie(pid),
    )
}

/// What sysinfo's per-pid refresh means. A process it lists with start time 0 or status Unknown was one it could not read
/// (macOS lists a root process to another user that way): nothing is known of it. One it does not list is gone only when an
/// OS primitive that tells "no such process" from "refused" (`exists`, asked only then) says so: a refused look (Windows
/// drops a process it cannot snapshot) is not an absent process.
pub(crate) fn classify(
    seen: Option<(u64, sysinfo::ProcessStatus)>,
    exists: impl FnOnce() -> crate::sys::Exists,
    zombie: impl FnOnce() -> bool,
) -> Presence {
    match seen {
        Some((_, sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead)) => Presence::Gone,
        Some((0, _) | (_, sysinfo::ProcessStatus::Unknown(_))) => Presence::Unknown,
        Some(_) => Presence::Running,
        None => match exists() {
            crate::sys::Exists::No => Presence::Gone,
            crate::sys::Exists::Refused => Presence::Unknown,
            // Listed nowhere yet answering: sysinfo can drop a live process (macOS), so it is gone only when the OS's own
            // state read says it is a zombie that waits to be reaped. Every other case is not known.
            crate::sys::Exists::Yes if zombie() => Presence::Gone,
            crate::sys::Exists::Yes => Presence::Unknown,
        },
    }
}

/// Asks `id` to stop, waits up to `grace` for it, then ends it. For processes that are not our children (the caller
/// checked that the process belongs to it); a child of ours should go through `Supervised::terminate`, which also reaps.
pub fn terminate(id: ProcessId, grace: Duration) -> Terminated {
    terminate_by(id, grace, &ProcessId::presence, &ask)
}

/// [`terminate`] over the look at the process, first and during the grace period, and the polite request (tests give a look
/// that cannot read the process table).
pub(crate) fn terminate_by(
    id: ProcessId,
    grace: Duration,
    look: &dyn Fn(ProcessId) -> Presence,
    ask: &dyn Fn(ProcessId) -> bool,
) -> Terminated {
    match look(id) {
        Presence::Gone => return Terminated::Gone,
        Presence::Unknown => return Terminated::Unknown,
        Presence::Running => {}
    }
    if ask(id) {
        let end_at = Instant::now() + grace;
        let mut last = None;
        while Instant::now() < end_at {
            let seen = look(id);
            // A process that is dying is for a moment listed nowhere yet still answers (macOS): not known, so look again
            // until the grace ends rather than report either way on one look.
            if seen == Presence::Gone {
                return Terminated::Gracefully;
            }
            last = Some(seen);
            std::thread::sleep(Duration::from_millis(25));
        }
        // The request may have outlasted the grace, so the loop never looked: take one look now. Nothing is ended without a
        // real Running look.
        match last.unwrap_or_else(|| look(id)) {
            Presence::Gone => return Terminated::Gracefully,
            // Still not known when the grace ran out: nothing is ended, and nothing is claimed.
            Presence::Unknown => return Terminated::Unknown,
            Presence::Running => {}
        }
    }
    end(id);
    Terminated::Killed
}

/// The processes whose command line mentions `needle` (leak checks: a throwaway folder name is unique to a run). The
/// identities are read first and the command lines after: a pid handed on between the two keeps the identity of the
/// process that had it first, so ending the result ([`end`]) finds the new holder and leaves it alone.
pub fn processes_mentioning(needle: &str) -> Vec<ProcessId> {
    processes_mentioning_with(needle, sysinfo::System::new)
}

/// [`processes_mentioning`] over a constructor; empty when it fails (nothing is known, so nothing is ended).
pub(crate) fn processes_mentioning_with(
    needle: &str,
    make: impl Fn() -> Result<sysinfo::System, sysinfo::Error>,
) -> Vec<ProcessId> {
    let table: Table = usage::snapshot_with(&make);
    let Some(mut sys) = usage::new_system_with(&make) else {
        return Vec::new();
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .filter(|(_, p)| p.cmd().iter().any(|a| a.to_string_lossy().contains(needle)))
        .filter_map(|(pid, _)| usage::identity(&table, pid.as_u32()))
        .collect()
}

/// Every process whose command line can be read, as its identity (pid and start time) and its command line (the arguments
/// joined by a space). The identities come from one look at the table and the command lines from the next, so a pid handed on
/// between the two keeps the identity of the process that had it first: ending the result ([`end`], [`terminate`]) finds the
/// new holder and leaves it alone. Code that matches command lines and then ends the matches takes this list and nothing else,
/// so it never holds a bare pid (ADR 0033); empty when the table cannot be read.
pub fn processes_with_command_lines() -> Vec<(ProcessId, String)> {
    processes_with_command_lines_with(sysinfo::System::new)
}

/// [`processes_with_command_lines`] over a constructor.
pub(crate) fn processes_with_command_lines_with(
    make: impl Fn() -> Result<sysinfo::System, sysinfo::Error>,
) -> Vec<(ProcessId, String)> {
    let table: Table = usage::snapshot_with(&make);
    let Some(mut sys) = usage::new_system_with(&make) else {
        return Vec::new();
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
    );
    sys.processes()
        .iter()
        .filter_map(|(pid, p)| {
            let line = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            (!line.is_empty())
                .then(|| usage::identity(&table, pid.as_u32()).map(|id| (id, line)))
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod port_tests {
    use super::*;
    use crate::usage::Row;
    use std::sync::Mutex;

    const fn at(us: u64) -> StartTime {
        StartTime::from_micros(us)
    }

    /// A process table the test drives: `on_group_end` runs when the root's group is ended, the moment the OS hands the
    /// freed pids on.
    struct FakeOs {
        now: Mutex<Table>,
        killed: Mutex<Vec<u32>>,
        groups: Mutex<Vec<u32>>,
        on_group_end: fn(&mut Table),
    }

    fn unchanged(_: &mut Table) {}

    impl FakeOs {
        fn new(rows: &[(u32, Option<u32>, u64)], on_group_end: fn(&mut Table)) -> FakeOs {
            let table = rows
                .iter()
                .map(|&(pid, parent, started)| {
                    (
                        pid,
                        Row {
                            parent,
                            rss: 1,
                            started: Some(at(started)),
                        },
                    )
                })
                .collect();
            FakeOs {
                now: Mutex::new(table),
                killed: Mutex::new(Vec::new()),
                groups: Mutex::new(Vec::new()),
                on_group_end,
            }
        }

        fn running(&self, pid: u32) -> Option<StartTime> {
            self.now.lock().unwrap().get(&pid).and_then(|r| r.started)
        }
    }

    impl Os for FakeOs {
        fn table(&self) -> Table {
            self.now.lock().unwrap().clone()
        }
        fn start_of(&self, pid: u32) -> Option<StartTime> {
            self.running(pid)
        }
        fn end_group(&self, root: u32) {
            self.groups.lock().unwrap().push(root);
            (self.on_group_end)(&mut self.now.lock().unwrap());
        }
        fn kill_pid(&self, pid: u32) -> bool {
            self.killed.lock().unwrap().push(pid);
            self.now.lock().unwrap().remove(&pid).is_some()
        }
    }

    /// The browser (10), its network service (11) and a renderer (12); ending the browser's group ends all three, and the
    /// OS hands pid 11 to another test's network service (started later) before the tree's own pids are ended one by one.
    fn browser_whose_pid_is_handed_on() -> FakeOs {
        FakeOs::new(
            &[
                (1, None, 1),
                (10, Some(1), 100),
                (11, Some(10), 200),
                (12, Some(10), 300),
            ],
            |t| {
                for pid in [10, 11, 12] {
                    t.remove(&pid);
                }
                t.insert(
                    11,
                    Row {
                        parent: Some(50),
                        rss: 1,
                        started: Some(at(900)),
                    },
                );
            },
        )
    }

    /// Regression (ADR 0033): ending one browser could kill the network service of a browser another test had just started,
    /// because a pid from the snapshot was killed without asking who held it now. (Once suspected of causing the Windows
    /// `ERR_ABORTED` flake; disproved by run 7158.)
    #[test]
    fn a_stale_snapshot_entry_never_kills_the_process_that_holds_its_pid_now() {
        let os = browser_whose_pid_is_handed_on();
        let out = kill_tree_on(&os, ProcessId::new(10, at(100)), true, None);
        assert_eq!(
            os.running(11),
            Some(at(900)),
            "the bystander that now holds pid 11 still runs"
        );
        assert!(
            !os.killed.lock().unwrap().contains(&11),
            "pid 11 was never signalled"
        );
        assert!(
            out.contains(&(ProcessId::new(11, at(200)), Ending::NotOurs(at(900)))),
            "{out:?}"
        );
        assert!(
            out.contains(&(ProcessId::new(12, at(300)), Ending::Gone)),
            "{out:?}"
        );
    }

    #[test]
    fn a_live_tree_is_ended_by_identity() {
        let os = FakeOs::new(
            &[
                (1, None, 1),
                (10, Some(1), 100),
                (11, Some(10), 200),
                (12, Some(11), 300),
            ],
            unchanged,
        );
        let out = kill_tree_on(&os, ProcessId::new(10, at(100)), false, None);
        assert!(out.iter().all(|(_, e)| *e == Ending::Ended), "{out:?}");
        assert_eq!(out.len(), 3);
        assert_eq!(os.running(10).or(os.running(11)).or(os.running(12)), None);
    }

    #[test]
    fn a_root_whose_pid_was_handed_on_is_left_alone_with_its_new_children() {
        // The recorded root (started 100) is gone; pid 10 now belongs to a process started at 500 with a child of its own.
        // Only the orphan the old root left (started 200, before the new holder) is ours.
        let os = FakeOs::new(
            &[(10, Some(1), 500), (11, Some(10), 200), (13, Some(10), 600)],
            unchanged,
        );
        let out = kill_tree_on(&os, ProcessId::new(10, at(100)), false, None);
        assert!(
            os.groups.lock().unwrap().is_empty(),
            "the new holder's group is never signalled"
        );
        assert_eq!(
            out[0],
            (ProcessId::new(10, at(100)), Ending::NotOurs(at(500)))
        );
        assert_eq!(out[1..], [(ProcessId::new(11, at(200)), Ending::Ended)]);
        assert_eq!(
            (os.running(10), os.running(13)),
            (Some(at(500)), Some(at(600)))
        );
        // Gone with no new holder: its group (helpers that outlived it) is still its own and is ended.
        let gone = FakeOs::new(&[(11, Some(10), 200), (20, Some(1), 50)], |t| {
            t.remove(&20);
        });
        let out = kill_tree_on(&gone, ProcessId::new(10, at(100)), false, None);
        assert_eq!(
            out,
            [
                (ProcessId::new(10, at(100)), Ending::Gone),
                (ProcessId::new(11, at(200)), Ending::Ended)
            ]
        );
        assert_eq!(gone.running(20), None, "the group was signalled");
    }

    /// Windows: the job's member list is the tree; no table is read and no parent pid followed, so a stranger that claims the
    /// root as its parent is left alone.
    #[test]
    fn a_job_member_list_is_the_tree() {
        let os = FakeOs::new(
            &[
                (1, None, 1),
                (10, Some(1), 100),
                (11, Some(10), 200),
                (12, Some(10), 300),
            ],
            unchanged,
        );
        let members = vec![ProcessId::new(10, at(100)), ProcessId::new(11, at(200))];
        let out = kill_tree_on(&os, ProcessId::new(10, at(100)), true, Some(members));
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(os.running(12), Some(at(300)), "not a member: not ended");
        assert_eq!(os.running(11), None);
    }

    #[test]
    fn a_refused_kill_is_reported() {
        struct Stubborn;
        impl Os for Stubborn {
            fn table(&self) -> Table {
                Table::new()
            }
            fn start_of(&self, _: u32) -> Option<StartTime> {
                Some(at(1))
            }
            fn end_group(&self, _: u32) {}
            fn kill_pid(&self, _: u32) -> bool {
                false
            }
        }
        assert_eq!(
            kill_tree_on(&Stubborn, ProcessId::new(5, at(1)), false, None),
            [(ProcessId::new(5, at(1)), Ending::Refused)]
        );
    }
}

#[cfg(test)]
mod presence_tests {
    use super::*;
    use crate::sys::{self, Exists};
    use std::process::{Child, Command, Stdio};

    #[test]
    fn what_sysinfo_could_not_read_is_unknown_and_only_proof_of_absence_is_gone() {
        use sysinfo::ProcessStatus::{Run, Unknown as Unk, Zombie};
        let never = || -> Exists { panic!("a listed process needs no second look") };
        let nor = || -> bool { panic!("a listed process needs no second look") };
        assert_eq!(classify(Some((5, Run)), never, nor), Presence::Running);
        assert_eq!(
            classify(Some((0, Run)), never, nor),
            Presence::Unknown,
            "start 0"
        );
        assert_eq!(
            classify(Some((5, Unk(0))), never, nor),
            Presence::Unknown,
            "status Unknown"
        );
        assert_eq!(classify(Some((0, Unk(0))), never, nor), Presence::Unknown);
        assert_eq!(classify(Some((5, Zombie)), never, nor), Presence::Gone);
        assert_eq!(
            classify(None, || Exists::No, nor),
            Presence::Gone,
            "no such process"
        );
        assert_eq!(classify(None, || Exists::Refused, nor), Presence::Unknown);
    }

    /// sysinfo can drop a live process (macOS): a pid it does not list that still answers is Gone only when its own state is
    /// a zombie, never because it answered.
    #[test]
    fn a_live_process_that_sysinfo_does_not_list_is_not_gone() {
        assert_eq!(
            classify(None, || Exists::Yes, || false),
            Presence::Unknown,
            "answers, not a zombie"
        );
        assert_eq!(
            classify(None, || Exists::Yes, || true),
            Presence::Gone,
            "answers, a zombie"
        );
    }

    #[test]
    fn the_state_of_a_zombie_is_read_from_the_os_records() {
        // Linux /proc/<pid>/stat: the command may hold spaces and parentheses; the state follows the last ')'.
        assert!(sys::zombie_from_stat("12 (a (b) c) Z 1 2 3"));
        assert!(sys::zombie_from_stat("12 (x) X 1"));
        assert!(
            !sys::zombie_from_stat("12 (a) Z) S 1"),
            "the state is after the last )"
        );
        assert!(!sys::zombie_from_stat("12 (x) S 1"));
        assert!(!sys::zombie_from_stat("garbage"));
        // macOS kinfo_proc: p_stat is byte 36, SZOMB is 5.
        let mut buf = [0u64; 128];
        assert!(!sys::zombie_from_kinfo(0, 648, &buf));
        buf[4] = u64::from_ne_bytes([0, 0, 0, 0, 5, 0, 0, 0]); // byte 36 of the buffer
        assert!(sys::zombie_from_kinfo(0, 648, &buf), "byte 36 is SZOMB");
        assert!(
            !sys::zombie_from_kinfo(1, 648, &buf),
            "a failed sysctl says nothing"
        );
        assert!(
            !sys::zombie_from_kinfo(0, 36, &buf),
            "too short to hold p_stat"
        );
    }

    fn exiting_child() -> Child {
        #[cfg(unix)]
        let mut c = Command::new("true");
        #[cfg(windows)]
        let mut c = {
            let mut c = Command::new("cmd");
            c.args(["/C", "exit", "0"]);
            c
        };
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c.spawn().expect("a child that exits at once")
    }

    /// Blocks until the child has exited without reaping it (Unix: `waitid(WNOWAIT)`, so it stays a zombie; Windows: the
    /// process object lives on while the `Child` holds its handle).
    fn wait_exited_keeping_it(child: &mut Child) {
        #[cfg(unix)]
        {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: a valid siginfo_t and a pid of our own child; WNOWAIT leaves the child waitable.
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    libc::id_t::from(child.id()),
                    &raw mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            assert_eq!(rc, 0, "waitid: {}", std::io::Error::last_os_error());
        }
        #[cfg(windows)]
        {
            child.wait().expect("the child exits");
        }
    }

    /// An exited child whose handle (Windows) or zombie entry (Unix) is still there is gone: it runs nothing.
    #[test]
    fn an_exited_child_that_is_not_yet_reaped_is_gone() {
        let mut child = exiting_child();
        let pid = child.id();
        wait_exited_keeping_it(&mut child);
        assert_eq!(presence(pid), Presence::Gone);
        // The same look through the primitives alone: sysinfo hiding it must not change the answer.
        assert_eq!(
            classify(None, || sys::pid_exists(pid), || sys::is_zombie(pid)),
            Presence::Gone
        );
        child.wait().expect("reaped");
    }

    /// After the child is reaped, the OS says no such process (ESRCH; Windows: invalid parameter).
    #[cfg(unix)]
    #[test]
    fn a_reaped_child_is_gone_by_no_such_process() {
        let mut child = exiting_child();
        let pid = child.id();
        child.wait().expect("reaped");
        assert_eq!(sys::pid_exists(pid), Exists::No);
        assert_eq!(presence(pid), Presence::Gone);
    }

    /// A live child is running, and a seam that hides it from sysinfo does not make it gone.
    #[cfg(unix)]
    #[test]
    fn a_live_child_hidden_from_sysinfo_is_not_gone() {
        let mut child = Command::new("sleep").arg("30").spawn().expect("sleep");
        let pid = child.id();
        assert_eq!(presence(pid), Presence::Running);
        assert_ne!(
            classify(None, || sys::pid_exists(pid), || sys::is_zombie(pid)),
            Presence::Gone,
            "listed by the OS, not a zombie"
        );
        child.kill().expect("kill");
        child.wait().expect("reaped");
    }

    /// Positive control: a process this user may not signal (pid 1, root's) is never reported gone, and on macOS, where sysinfo
    /// lists it with start time 0 and status Unknown, it is not reported running either: nothing is known of it.
    #[cfg(unix)]
    #[test]
    fn a_process_of_another_user_is_not_gone() {
        // SAFETY: geteuid has no arguments and cannot fail.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let seen = presence(1);
        assert_ne!(seen, Presence::Gone, "pid 1 runs");
        #[cfg(target_os = "macos")]
        assert_eq!(
            seen,
            Presence::Unknown,
            "start time 0 and status Unknown: not known"
        );
    }

    /// Positive control (Windows): the System process (pid 4) runs and may not be opened; it is never reported gone.
    #[cfg(windows)]
    #[test]
    fn the_system_process_is_never_gone() {
        assert_ne!(presence(4), Presence::Gone);
        assert_ne!(sys::pid_exists(4), Exists::No);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::Supervised;
    use std::process::Command;

    #[test]
    fn an_unreadable_process_table_is_not_a_process_that_was_gone() {
        let id = ProcessId::new(4242, StartTime::from_micros(1));
        assert_eq!(
            terminate_by(id, Duration::ZERO, &|_| Presence::Unknown, &|_| false),
            Terminated::Unknown
        );
        assert_eq!(
            terminate_by(id, Duration::ZERO, &|_| Presence::Gone, &|_| false),
            Terminated::Gone
        );
    }

    #[test]
    fn a_table_that_becomes_unreadable_during_the_grace_period_is_not_a_graceful_stop() {
        let id = ProcessId::new(4242, StartTime::from_micros(1));
        let looks = std::cell::Cell::new(0);
        let look = |_: ProcessId| {
            looks.set(looks.get() + 1);
            if looks.get() == 1 {
                Presence::Running
            } else {
                Presence::Unknown
            }
        };
        assert_eq!(
            terminate_by(id, Duration::from_millis(200), &look, &|_| true),
            Terminated::Unknown
        );
    }

    /// A dying process is for a moment listed nowhere yet still answers: one unknown look inside the grace is not the answer.
    #[test]
    fn an_unknown_look_that_clears_inside_the_grace_period_is_still_a_graceful_stop() {
        let id = ProcessId::new(4242, StartTime::from_micros(1));
        let looks = std::cell::Cell::new(0);
        let look = |_: ProcessId| {
            looks.set(looks.get() + 1);
            match looks.get() {
                1 => Presence::Running,
                2 => Presence::Unknown,
                _ => Presence::Gone,
            }
        };
        assert_eq!(
            terminate_by(id, Duration::from_secs(5), &look, &|_| true),
            Terminated::Gracefully
        );
    }

    /// A polite request that outlasts the grace period (the closure stands in for a slow `ask`, so the loop never looks): the
    /// process is judged by a look taken after it, and is never ended on an assumed Running.
    #[test]
    fn a_request_that_outlasts_the_grace_period_is_followed_by_a_real_look_before_any_kill() {
        let id = ProcessId::new(4242, StartTime::from_micros(1));
        let after = |second: Presence| {
            let looks = std::cell::Cell::new(0);
            let look = move |_: ProcessId| {
                looks.set(looks.get() + 1);
                if looks.get() == 1 {
                    Presence::Running
                } else {
                    second
                }
            };
            terminate_by(id, Duration::ZERO, &look, &|_| true)
        };
        assert_eq!(
            after(Presence::Unknown),
            Terminated::Unknown,
            "not known: nothing is ended"
        );
        assert_eq!(
            after(Presence::Gone),
            Terminated::Gracefully,
            "it stopped while the request was made"
        );
    }

    #[test]
    fn a_process_seen_gone_during_the_grace_period_stopped_gracefully() {
        let id = ProcessId::new(4242, StartTime::from_micros(1));
        let looks = std::cell::Cell::new(0);
        let look = |_: ProcessId| {
            looks.set(looks.get() + 1);
            if looks.get() == 1 {
                Presence::Running
            } else {
                Presence::Gone
            }
        };
        assert_eq!(
            terminate_by(id, Duration::from_secs(5), &look, &|_| true),
            Terminated::Gracefully
        );
    }

    fn broken() -> Result<sysinfo::System, sysinfo::Error> {
        Err(sysinfo::Error::Unsupported)
    }

    #[test]
    fn an_unreadable_process_table_is_unknown_and_unknown_is_not_gone() {
        let me = std::process::id();
        assert_eq!(presence_with(me, broken), Presence::Unknown);
        assert_eq!(presence_with(me, sysinfo::System::new), Presence::Running);
        assert_eq!(
            presence_with(u32::MAX - 1, sysinfo::System::new),
            Presence::Gone
        );
        assert!(
            Presence::Unknown.may_be_alive(),
            "unknown counts as alive for a sweep"
        );
        assert!(Presence::Running.may_be_alive());
        assert!(!Presence::Gone.may_be_alive());
        assert!(may_be_alive(me));
        assert!(!may_be_alive(u32::MAX - 1));
        assert!(processes_mentioning_with("anything", broken).is_empty());
    }

    use crate::testchild::{Mode, child, lines};
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    fn ready(s: &mut Supervised) {
        let out = s.take_stdout().unwrap();
        let mut r = BufReader::new(out);
        let mut l = String::new();
        loop {
            l.clear();
            assert!(
                r.read_line(&mut l).unwrap() > 0,
                "child ended before it was ready"
            );
            if lines(&l) == ["ready"] {
                return;
            }
        }
    }

    #[test]
    fn a_dead_pid_is_gone() {
        let mut c = child(Mode::Exit3);
        c.stdout(Stdio::null());
        let mut ch = c.spawn().unwrap();
        let id = ProcessId::of(ch.id()).expect("an unreaped child has an identity");
        ch.wait().unwrap();
        assert!(!is_alive(id.pid()) && !id.is_alive());
        assert_eq!(terminate(id, Duration::from_secs(1)), Terminated::Gone);
        assert_eq!(end(id), Ending::Gone); // harmless
        assert!(!ask(id));
    }

    /// The checked ask (macOS, and Linux without pidfds) delivers SIGTERM to the process it names, and the plain kill ends it.
    #[cfg(unix)]
    #[test]
    fn the_checked_ask_and_the_plain_kill_reach_the_process_they_name() {
        let mut c = child(Mode::Polite);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        let id = s.process().unwrap();
        assert_eq!(ask_checked(id), (Holder::Same, true));
        let gone =
            paddock_testkit::wait::poll_until_within(paddock_testkit::wait::HANG_GUARD, || {
                crate::run_state(id) != crate::RunState::Running
            });
        assert!(gone, "a polite process stops when asked");
        // A pid that was handed on is not signalled, and a process that is gone cannot be killed.
        let other = ProcessId::new(id.pid(), crate::StartTime::from_micros(1));
        assert!(!ask_checked(other).1);
        let mut c = child(Mode::Polite);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        let victim = s.process().unwrap();
        assert!(RealOs.kill_pid(victim.pid()));
        assert!(!RealOs.kill_pid(0));
    }

    #[test]
    fn a_polite_process_stops_within_the_grace_period() {
        let mut c = child(Mode::Polite);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        let id = s.process().unwrap();
        // Reap concurrently: a child of ours stays a zombie until waited for, which `is_alive` does not count.
        assert_eq!(
            terminate(id, Duration::from_secs(60)),
            Terminated::Gracefully
        );
        assert!(!is_alive(id.pid()));
    }

    #[test]
    fn a_process_that_ignores_the_request_is_killed_after_the_grace_period() {
        let mut c = child(Mode::IgnoreTerm);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        let id = s.process().unwrap();
        assert_eq!(terminate(id, Duration::ZERO), Terminated::Killed);
        let mut rest = String::new();
        let _ = s.take_stdout().map(|mut o| o.read_to_string(&mut rest));
    }

    #[test]
    fn kill_tree_ends_a_child_and_its_grandchild() {
        let mut c = child(Mode::Tree);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let mut grand = None;
        let mut l = String::new();
        while grand.is_none() {
            l.clear();
            assert!(r.read_line(&mut l).unwrap() > 0);
            grand = lines(&l)
                .first()
                .and_then(|p| p.strip_prefix("pid "))
                .and_then(|p| p.trim().parse::<u32>().ok());
        }
        let grand = grand.unwrap();
        assert!(is_alive(grand) && is_alive(s.id().unwrap()));
        kill_tree(s.process().unwrap());
        // Both ends of the shared stdout pipe are closed only when the whole tree is dead.
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert!(crate::testchild::is_gone(grand), "the grandchild survived");
        assert!(s.wait().is_some());
    }

    #[test]
    fn processes_are_found_by_their_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut c = child(Mode::Polite);
        c.arg(&marker).stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        let found = processes_mentioning(&marker);
        assert!(found.contains(&s.process().unwrap()), "{found:?}");
        s.kill();
        assert!(processes_mentioning(&marker).iter().all(|p| !p.is_alive()));
    }

    #[test]
    fn descendants_are_found_by_walking_the_process_table() {
        let mut c = child(Mode::Tree);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let mut grand = None;
        let mut l = String::new();
        while grand.is_none() {
            l.clear();
            assert!(r.read_line(&mut l).unwrap() > 0);
            grand = lines(&l)
                .first()
                .and_then(|p| p.strip_prefix("pid "))
                .and_then(|p| p.trim().parse::<u32>().ok());
        }
        let root = s.process().unwrap();
        let grand = ProcessId::of(grand.unwrap()).unwrap();
        assert!(descendants_of(root).contains(&grand));
        assert!(descendants_of(ProcessId::new(u32::MAX - 1, StartTime::from_micros(1))).is_empty());
        let stale = ProcessId::new(
            grand.pid(),
            StartTime::from_micros(grand.started().as_micros() + 1),
        );
        assert_eq!(
            end(stale),
            Ending::NotOurs(grand.started()),
            "another start time is another process"
        );
        assert!(!ask(stale));
        assert!(grand.is_alive(), "the grandchild was left alone");
        s.kill();
    }

    #[test]
    fn a_process_that_ignores_the_request_is_waited_for_until_the_grace_period_ends() {
        let mut c = child(Mode::IgnoreTerm);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        // It cannot stop gracefully, so the outcome does not depend on how long the wait takes.
        assert_eq!(
            terminate(s.process().unwrap(), Duration::from_millis(60)),
            Terminated::Killed
        );
        s.kill();
    }

    /// `kill(-group, 0)` counts a zombie as a member; the guardian waits for a group to empty, so a group whose members
    /// only wait to be reaped (a container whose init does not reap) must read as empty, and one with a running member not.
    #[cfg(unix)]
    #[test]
    fn a_group_of_zombies_has_no_member_that_runs() {
        use std::os::unix::process::CommandExt;
        let mut dead = Command::new("/bin/sh");
        dead.args(["-c", "exit 0"]).process_group(0);
        let mut dead = dead.spawn().unwrap();
        let zombie = dead.id();
        // Not waited for: it stays a zombie until this test reaps it.
        paddock_testkit::wait::wait_until("the child to exit and wait to be reaped", || {
            crate::sys::is_zombie(zombie)
        });
        // Linux counts the zombie; macOS does not (killpg fails there), so the check below holds either way.
        #[cfg(target_os = "linux")]
        assert!(
            crate::sys::signal_group(zombie, 0),
            "the OS still counts the zombie as a member of its group"
        );
        assert!(!group_exists(zombie), "a zombie is not a member that runs");
        dead.wait().unwrap();

        let mut live = Command::new("/bin/sh");
        live.args(["-c", "exec sleep 60"]).process_group(0);
        let mut live = live.spawn().unwrap();
        assert!(group_exists(live.id()));
        live.kill().unwrap();
        // Between the kill and the reap the child is a zombie: not a running member either.
        paddock_testkit::wait::wait_until("the killed group to have no running member", || {
            !group_exists(live.id())
        });
        live.wait().unwrap();
    }

    /// stat lines as Linux writes them: pid (name) state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt utime
    /// stime cutime cstime priority nice num_threads ...
    fn stat(name: &str, state: &str, pgrp: u32, threads: u32) -> String {
        format!("42 ({name}) {state} 1 {pgrp} 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 {threads} 0 5 0 0")
    }

    #[test]
    fn a_stat_line_gives_its_group_and_whether_it_runs() {
        assert_eq!(
            parse_group_and_running(stat("sh", "S", 7, 1).as_bytes()),
            Some((7, true))
        );
        assert_eq!(
            parse_group_and_running(stat("sh", "Z", 7, 1).as_bytes()),
            Some((7, false))
        );
        assert_eq!(
            parse_group_and_running(stat("sh", "X", 7, 1).as_bytes()),
            Some((7, false))
        );
        // A zombie leader whose other threads still run is running.
        assert_eq!(
            parse_group_and_running(stat("sh", "Z", 7, 3).as_bytes()),
            Some((7, true))
        );
        // A name with spaces and parentheses does not shift the fields.
        assert_eq!(
            parse_group_and_running(stat("a) S (b c", "S", 9, 1).as_bytes()),
            Some((9, true))
        );
        assert_eq!(parse_group_and_running(b"42 (sh) S 1"), None);
        assert_eq!(parse_group_and_running(b"garbage"), None);
    }

    #[test]
    fn a_group_runs_while_a_member_that_is_not_a_zombie_is_in_it() {
        let root = tempfile::tempdir().unwrap();
        let put = |pid: &str, text: String| {
            let d = root.path().join(pid);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("stat"), text).unwrap();
        };
        put("10", stat("a", "Z", 7, 1));
        put("11", stat("b", "S", 7, 1));
        put("12", stat("c", "S", 99, 1));
        std::fs::create_dir_all(root.path().join("self")).unwrap(); // not a pid
        std::fs::create_dir_all(root.path().join("13")).unwrap(); // ended: no stat
        assert_eq!(group_runs_in_proc(root.path(), 7), Some(true));
        put("11", stat("b", "Z", 7, 1));
        assert_eq!(
            group_runs_in_proc(root.path(), 7),
            Some(false),
            "only zombies, and another group's process"
        );
        assert_eq!(group_runs_in_proc(root.path(), 5), Some(false));
        assert_eq!(group_runs_in_proc(&root.path().join("missing"), 7), None);
    }

    /// A name that is not UTF-8 is still a process that runs (unknown is never read as gone, ADR 0081); an empty /proc, or a
    /// stat that cannot be parsed, is not a reason to call a group empty either.
    #[test]
    fn a_stat_that_is_not_text_or_a_proc_without_pids_is_not_read_as_gone() {
        let root = tempfile::tempdir().unwrap();
        let d = root.path().join("11");
        std::fs::create_dir_all(&d).unwrap();
        let mut bytes = b"11 (a\xff) b) S 1 7 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 5 0 0".to_vec();
        bytes.extend_from_slice(b"\n");
        std::fs::write(d.join("stat"), &bytes).unwrap();
        assert_eq!(group_runs_in_proc(root.path(), 7), Some(true));
        assert_eq!(group_runs_in_proc(root.path(), 5), Some(false));
        // A stat that exists but cannot be parsed: running, as far as is known.
        std::fs::write(d.join("stat"), b"\xff\xfe").unwrap();
        assert_eq!(group_runs_in_proc(root.path(), 7), Some(true));
        // No pid entry at all: nothing is known.
        let empty = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(empty.path().join("self")).unwrap();
        assert_eq!(group_runs_in_proc(empty.path(), 7), None);
    }

    #[test]
    fn a_table_that_cannot_be_read_lists_nothing_and_this_process_is_listed_with_its_identity() {
        assert!(processes_with_command_lines_with(|| Err(sysinfo::Error::Unsupported)).is_empty());
        let own = std::process::id();
        let me = ProcessId::of(own).unwrap();
        let listed = processes_with_command_lines();
        assert!(
            listed
                .iter()
                .any(|(id, line)| *id == me && !line.is_empty()),
            "this test process is in the list with the identity ProcessId::of gives"
        );
    }
}
