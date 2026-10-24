//! Ending processes: a whole tree (`kill_tree`), one process politely (`ask`) or at once (`end`), and both with a
//! grace period (`terminate`). Also liveness and leak checks.

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

/// Every process below `root` (not including it), from a fresh look at the process table.
pub fn descendants_of(root: u32) -> Vec<u32> {
    usage::descendants(&usage::snapshot(), root)
}

/// Ends `root` and everything below it. A child started by this crate leads its own process group (Unix), which is
/// signalled as a whole; helpers that left the group (setsid) are still ours by parentage and are killed one by one.
/// Windows uses `taskkill /T /F`. Safe to call for a process that is already gone.
pub fn kill_tree(root: u32) {
    let table = usage::snapshot();
    let kids = usage::descendants(&table, root);
    kill_tree_with(root, &kids);
}

fn kill_tree_with(root: u32, kids: &[u32]) {
    #[cfg(unix)]
    {
        crate::sys::signal_group(root, crate::sys::SIGKILL);
        crate::sys::signal_pid(root, crate::sys::SIGKILL);
    }
    #[cfg(windows)]
    {
        crate::sys::taskkill(root, true);
    }
    if kids.is_empty() {
        return;
    }
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&kids.iter().map(|p| Pid::from_u32(*p)).collect::<Vec<_>>()),
        true,
        ProcessRefreshKind::nothing(),
    );
    for pid in kids {
        if let Some(p) = sys.process(Pid::from_u32(*pid)) {
            p.kill();
        }
    }
}

/// Asks one process (not its group, which may contain the caller) to stop: SIGTERM, or a close request on Windows.
/// Returns whether the request was delivered.
pub fn ask(pid: u32) -> bool {
    #[cfg(unix)]
    {
        crate::sys::signal_pid(pid, crate::sys::SIGTERM)
    }
    #[cfg(windows)]
    {
        crate::sys::taskkill(pid, false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

/// Ends one process now (SIGKILL, `taskkill /F`), without touching its group or children.
pub fn end(pid: u32) {
    #[cfg(unix)]
    {
        crate::sys::signal_pid(pid, crate::sys::SIGKILL);
    }
    #[cfg(windows)]
    {
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
            true,
            ProcessRefreshKind::nothing(),
        );
        if let Some(p) = sys.process(Pid::from_u32(pid)) {
            p.kill();
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = pid;
}

/// Whether `pid` is a running process (a zombie that only waits to be reaped is not).
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

/// Asks `pid` to stop, waits up to `grace` for it, then ends it. For processes that are not our children (the caller
/// checked that the pid belongs to it); a child of ours should go through `Supervised::terminate`, which also reaps.
pub fn terminate(pid: u32, grace: Duration) -> Terminated {
    if !is_alive(pid) {
        return Terminated::Gone;
    }
    if ask(pid) {
        let end_at = Instant::now() + grace;
        while Instant::now() < end_at {
            if !is_alive(pid) {
                return Terminated::Gracefully;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    end(pid);
    Terminated::Killed
}

/// Process ids whose command line mentions `needle` (leak checks: a throwaway folder name is unique to a run).
pub fn processes_mentioning(needle: &str) -> Vec<u32> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
    );
    let table: Table = usage::snapshot();
    sys.processes()
        .iter()
        .filter(|(pid, p)| {
            table.contains_key(&pid.as_u32())
                && p.cmd().iter().any(|a| a.to_string_lossy().contains(needle))
        })
        .map(|(pid, _)| pid.as_u32())
        .collect()
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
        let pid = ch.id();
        ch.wait().unwrap();
        assert!(!is_alive(pid));
        assert_eq!(terminate(pid, Duration::from_secs(1)), Terminated::Gone);
        end(pid); // harmless
        assert!(!ask(pid));
    }

    #[test]
    fn a_polite_process_stops_within_the_grace_period() {
        let mut c = child(Mode::Polite);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        let pid = s.id().unwrap();
        // Reap concurrently: a child of ours stays a zombie until waited for, which `is_alive` does not count.
        assert_eq!(
            terminate(pid, Duration::from_secs(60)),
            Terminated::Gracefully
        );
        assert!(!is_alive(pid));
    }

    #[test]
    fn a_process_that_ignores_the_request_is_killed_after_the_grace_period() {
        let mut c = child(Mode::IgnoreTerm);
        c.stdout(Stdio::piped());
        let mut s = Supervised::spawn(c).unwrap();
        ready(&mut s);
        let pid = s.id().unwrap();
        assert_eq!(terminate(pid, Duration::ZERO), Terminated::Killed);
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
        kill_tree(s.id().unwrap());
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
        assert!(found.contains(&s.id().unwrap()), "{found:?}");
        s.kill();
        assert!(processes_mentioning(&marker).iter().all(|p| !is_alive(*p)));
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
        let root = s.id().unwrap();
        assert!(descendants_of(root).contains(&grand.unwrap()));
        assert!(descendants_of(u32::MAX - 1).is_empty());
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
            terminate(s.id().unwrap(), Duration::from_millis(60)),
            Terminated::Killed
        );
        s.kill();
    }
}
