//! Ending processes: a whole tree (`kill_tree`), one process politely (`ask`) or at once (`end`), and both with a
//! grace period (`terminate`). Also liveness and leak checks.

use crate::identity::{Ending, Holder, ProcessId, StartTime, holder, warn_not_ours};
use crate::usage::{self, Table};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// How a process ended under [`terminate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminated {
    /// It was not running.
    Gone,
    /// It stopped within the grace period after the polite request.
    Gracefully,
    /// It ignored the request and was killed.
    Killed,
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
        let _ = root;
    }

    fn kill_pid(&self, pid: u32) -> bool {
        #[cfg(unix)]
        {
            crate::sys::signal_pid(pid, crate::sys::SIGKILL)
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
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

/// Whether `pid` is a running process (a zombie that only waits to be reaped is not). Says nothing about which process:
/// use [`ProcessId::is_alive`] for that.
pub fn is_alive(pid: u32) -> bool {
    let mut sys = System::new();
    let id = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(id).is_some_and(|p| {
        !matches!(
            p.status(),
            sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead
        )
    })
}

/// Asks `id` to stop, waits up to `grace` for it, then ends it. For processes that are not our children (the caller
/// checked that the process belongs to it); a child of ours should go through `Supervised::terminate`, which also reaps.
pub fn terminate(id: ProcessId, grace: Duration) -> Terminated {
    if !id.is_alive() {
        return Terminated::Gone;
    }
    if ask(id) {
        let end_at = Instant::now() + grace;
        while Instant::now() < end_at {
            if !id.is_alive() {
                return Terminated::Gracefully;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    end(id);
    Terminated::Killed
}

/// The processes whose command line mentions `needle` (leak checks: a throwaway folder name is unique to a run). The
/// identities are read first and the command lines after: a pid handed on between the two keeps the identity of the
/// process that had it first, so ending the result ([`end`]) finds the new holder and leaves it alone.
pub fn processes_mentioning(needle: &str) -> Vec<ProcessId> {
    let table: Table = usage::snapshot();
    let mut sys = System::new();
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

    /// Regression (ADR 0033), the suspected cause of the Windows `ERR_ABORTED` flake: ending one browser could kill the
    /// network service of a browser another test had just started, because a pid from the snapshot was killed without asking
    /// who held it now.
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::Supervised;
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
}
