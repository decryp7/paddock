//! Process memory, measured as real memory per OS and never as the resident size (rule 26). Linux lists every thread as a
//! process that reports its process's whole resident size again (a browser with 400 threads would count 400 times over), so
//! thread entries are skipped; browser processes also share large mappings, so on Linux the proportional set size (PSS,
//! from `smaps_rollup`) is used, which divides each page between the processes that map it. On macOS it is the physical
//! footprint (`ri_phys_footprint`, Activity Monitor's "Memory"): the resident size counts pages mimalloc freed but the
//! kernel has not reclaimed (a worker with a 535 MB footprint showed 1,965 MB). On Windows it is the private working set.
//! The resident size is only the fallback for a process whose real memory cannot be read.

use crate::identity::{ProcessId, StartTime};
use std::collections::HashMap;
use std::path::Path;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// `pid -> row` for real processes only.
pub type Table = HashMap<u32, Row>;

/// One real process in a [`Table`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// Parent process, if known. Only a pid: the parent may have ended and its pid been handed on (see [`descendants`]).
    pub parent: Option<u32>,
    /// Resident bytes.
    pub rss: u64,
    /// When it started, if the OS said.
    pub started: Option<StartTime>,
}

/// One row of the OS process list, before the thread entries are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcEntry {
    /// Process or thread id.
    pub pid: u32,
    /// Parent process, if known.
    pub parent: Option<u32>,
    /// Resident bytes.
    pub rss: u64,
    /// When it started, if the OS said.
    pub started: Option<StartTime>,
    /// Linux thread entry (shows up in the list beside the real processes).
    pub is_thread: bool,
}

/// The table of the real processes in `entries`: thread entries are skipped.
pub fn table_from(entries: impl IntoIterator<Item = ProcEntry>) -> Table {
    entries
        .into_iter()
        .filter(|e| !e.is_thread)
        .map(|e| {
            (
                e.pid,
                Row {
                    parent: e.parent,
                    rss: e.rss,
                    started: e.started,
                },
            )
        })
        .collect()
}

/// A look at the OS process table: parents, start times and resident memory only (command lines, environments and
/// working folders are slow to read on Windows and nothing here needs them).
pub fn snapshot() -> Table {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    table_from(sys.processes().iter().map(|(pid, p)| ProcEntry {
        pid: pid.as_u32(),
        parent: p.parent().map(Pid::as_u32),
        rss: p.memory(),
        started: crate::sys::start_time(pid.as_u32()),
        is_thread: p.thread_kind().is_some(),
    }))
}

/// The process in `table` with this pid, by identity.
pub fn identity(table: &Table, pid: u32) -> Option<ProcessId> {
    table.get(&pid)?.started.map(|s| ProcessId::new(pid, s))
}

/// Every process below `root` (not including it), in no particular order. A parent link is only a pid, so a process
/// counts as a child only if it started no earlier than its parent: a process whose parent ended has a parent pid that
/// may since name a newer, unrelated process (Windows hands pids on at once), and that process's "children" would be
/// strangers. When `root` itself is gone and its pid names a newer process, only the processes that started between
/// the two are `root`'s (orphans on Windows keep their parent pid). A process whose start time is unknown is not counted.
pub fn descendants(table: &Table, root: ProcessId) -> Vec<ProcessId> {
    let mut children: HashMap<u32, Vec<ProcessId>> = HashMap::new();
    for (&pid, row) in table {
        if let (Some(p), Some(s)) = (row.parent, row.started)
            && pid != p
        {
            children.entry(p).or_default().push(ProcessId::new(pid, s));
        }
    }
    // The root's children end where a newer holder of its pid began.
    let root_until = match table.get(&root.pid()).and_then(|r| r.started) {
        Some(s) if s != root.started() => Some(s),
        _ => None,
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::from([root.pid()]);
    let mut frontier = vec![(root, root_until)];
    while let Some((p, until)) = frontier.pop() {
        for &kid in children.get(&p.pid()).into_iter().flatten() {
            let after_parent = kid.started() >= p.started();
            let before_successor = until.is_none_or(|u| kid.started() < u);
            if after_parent && before_successor && seen.insert(kid.pid()) {
                out.push(kid);
                frontier.push((kid, None));
            }
        }
    }
    out
}

/// Memory of `root` and its descendants, in bytes; `better` may give a process a more exact figure than its resident
/// size (its PSS), a process it returns `None` for counts by resident size.
pub fn tree_memory_with(table: &Table, root: u32, better: impl Fn(u32) -> Option<u64>) -> u64 {
    let Some(id) = identity(table, root) else {
        // Gone, or its start time is unknown: only its own figure.
        return table
            .get(&root)
            .map_or(0, |r| better(root).unwrap_or(r.rss));
    };
    std::iter::once(root)
        .chain(descendants(table, id).into_iter().map(ProcessId::pid))
        .filter_map(|p| better(p).or_else(|| table.get(&p).map(|e| e.rss)))
        .fold(0u64, u64::saturating_add)
}

/// The proportional set size in bytes from the text of `smaps_rollup` (the `Pss:` line, in kB).
pub fn parse_pss(smaps_rollup: &str) -> Option<u64> {
    let line = smaps_rollup.lines().find_map(|l| l.strip_prefix("Pss:"))?;
    let kb: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kb.checked_mul(1024)
}

/// The PSS of `pid` read from `<proc_root>/<pid>/smaps_rollup` (`/proc` on Linux; a folder of fixtures in tests).
pub fn read_pss(proc_root: &Path, pid: u32) -> Option<u64> {
    parse_pss(&std::fs::read_to_string(proc_root.join(pid.to_string()).join("smaps_rollup")).ok()?)
}

#[cfg(target_os = "linux")]
fn proportional(pid: u32) -> Option<u64> {
    read_pss(Path::new("/proc"), pid)
}

#[cfg(target_os = "macos")]
fn proportional(pid: u32) -> Option<u64> {
    crate::sys::phys_footprint(pid)
}

#[cfg(windows)]
fn proportional(pid: u32) -> Option<u64> {
    crate::sys::private_working_set(pid)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn proportional(_pid: u32) -> Option<u64> {
    None
}

/// What `pid` really uses, in bytes: PSS on Linux, the physical footprint on macOS, the private working set on Windows (the
/// resident size if that cannot be read). `None` when there is no such process (or it is a thread id).
pub fn process_memory(pid: u32) -> Option<u64> {
    let table = snapshot();
    proportional(pid).or_else(|| table.get(&pid).map(|e| e.rss))
}

/// The memory of the tree at `root` (see [`process_memory`]), 0 when it is gone.
pub fn tree_memory(root: u32) -> u64 {
    tree_memory_with(&snapshot(), root, proportional)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLUP: &str = "55d0c0a00000-7ffd5a7ff000 ---p 00000000 00:00 0                          [rollup]\n\
Rss:              219040 kB\nPss:              131633 kB\nPss_Anon:          90000 kB\nShared_Clean:      70000 kB\n";
    const MB: u64 = 1 << 20;

    /// A row that started at `pid` µs, so a child (a higher pid in these tables) starts after its parent.
    fn entry(pid: u32, parent: Option<u32>, rss: u64, is_thread: bool) -> ProcEntry {
        ProcEntry {
            pid,
            parent,
            rss,
            started: Some(StartTime::from_micros(u64::from(pid))),
            is_thread,
        }
    }

    fn id(pid: u32) -> ProcessId {
        ProcessId::new(pid, StartTime::from_micros(u64::from(pid)))
    }

    fn pids(mut v: Vec<ProcessId>) -> Vec<u32> {
        v.sort();
        v.into_iter().map(ProcessId::pid).collect()
    }

    #[test]
    fn pss_is_read_from_smaps_rollup_in_bytes() {
        assert_eq!(parse_pss(ROLLUP), Some(131_633 * 1024));
        assert_eq!(parse_pss("Rss: 5 kB\n"), None);
        assert_eq!(parse_pss("Pss: lots kB\n"), None);
        assert_eq!(parse_pss("Pss: 12\n"), None);
        assert_eq!(parse_pss(""), None);
        assert_eq!(parse_pss(&format!("Pss: {} kB\n", u64::MAX)), None);
    }

    #[test]
    fn pss_is_read_from_a_fixture_proc_folder() {
        let proc = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proc.path().join("77")).unwrap();
        std::fs::write(proc.path().join("77/smaps_rollup"), ROLLUP).unwrap();
        std::fs::create_dir_all(proc.path().join("78")).unwrap();
        std::fs::write(proc.path().join("78/smaps_rollup"), "garbage").unwrap();
        assert_eq!(read_pss(proc.path(), 77), Some(131_633 * 1024));
        assert_eq!(read_pss(proc.path(), 78), None, "unparsable");
        assert_eq!(read_pss(proc.path(), 79), None, "missing or not readable");
    }

    #[test]
    fn thread_entries_are_skipped() {
        // A browser with three renderers and 400 thread entries that each repeat its resident size.
        let mut rows = vec![entry(1, None, 100 * MB, false)];
        rows.extend((2..=4).map(|p| entry(p, Some(1), 100 * MB, false)));
        rows.extend((1000..1400).map(|t| entry(t, Some(1), 100 * MB, true)));
        let table = table_from(rows);
        assert_eq!(table.len(), 4);
        assert_eq!(tree_memory_with(&table, 1, |_| None), 400 * MB);
    }

    #[test]
    fn shared_pages_are_not_counted_once_per_process() {
        // A zygote and three renderers that all map the same 100 MB: resident sizes add it four times, PSS once.
        let table = table_from((1..=4).map(|p| entry(p, (p > 1).then_some(1), 110 * MB, false)));
        assert_eq!(tree_memory_with(&table, 1, |_| None), 440 * MB);
        assert_eq!(tree_memory_with(&table, 1, |_| Some(35 * MB)), 140 * MB);
    }

    #[test]
    fn an_unreadable_process_falls_back_to_its_resident_size() {
        let table = table_from([entry(1, None, 50, false), entry(2, Some(1), 70, false)]);
        assert_eq!(tree_memory_with(&table, 1, |p| (p == 1).then_some(20)), 90);
        assert_eq!(tree_memory_with(&table, 1, |_| None), 120);
        assert_eq!(
            tree_memory_with(&table, 99, |_| None),
            0,
            "a missing root is nothing"
        );
    }

    #[test]
    fn descendants_follow_parents_and_survive_cycles() {
        let table = table_from([
            entry(1, None, 1, false),
            entry(2, Some(1), 1, false),
            entry(3, Some(2), 1, false),
            entry(4, Some(9), 1, false),
            entry(5, Some(5), 1, false),
            entry(6, Some(7), 1, false),
            entry(7, Some(6), 1, false),
        ]);
        assert_eq!(pids(descendants(&table, id(1))), vec![2, 3]);
        assert_eq!(
            pids(descendants(&table, id(6))),
            vec![7],
            "a parent cycle terminates"
        );
        assert!(
            descendants(&table, id(5)).is_empty(),
            "self-parent is not its own child"
        );
        assert_eq!(tree_memory_with(&table, 1, |_| None).saturating_add(0), 3);
    }

    fn row(pid: u32, parent: u32, started: Option<u64>) -> ProcEntry {
        ProcEntry {
            pid,
            parent: Some(parent),
            rss: 1,
            started: started.map(StartTime::from_micros),
            is_thread: false,
        }
    }

    /// Regression (ADR 0033): on Windows a parent pid outlives its process. A process that started before its "parent" is
    /// the orphan of an earlier holder of that pid, not a child, and must not be counted or ended with the tree.
    #[test]
    fn a_child_older_than_its_parent_is_a_stranger() {
        let root = ProcessId::new(10, StartTime::from_micros(100));
        let table = table_from([
            row(10, 1, Some(100)),
            row(11, 10, Some(200)),
            row(12, 10, Some(50)),
            row(13, 11, Some(150)),
            row(14, 10, None),
            row(15, 10, Some(100)),
        ]);
        assert_eq!(
            pids(descendants(&table, root)),
            vec![11, 15],
            "12 and 13 are older than their parents, 14 is unknown"
        );
        assert_eq!(tree_memory_with(&table, 10, |_| None), 3);
    }

    #[test]
    fn a_gone_root_keeps_only_the_orphans_older_than_the_new_holder_of_its_pid() {
        let root = ProcessId::new(10, StartTime::from_micros(100));
        let table = table_from([
            row(10, 1, Some(500)),
            row(11, 10, Some(200)),
            row(16, 10, Some(600)),
            row(17, 11, Some(300)),
        ]);
        assert_eq!(pids(descendants(&table, root)), vec![11, 17]);
        let gone = table_from([row(11, 10, Some(200)), row(16, 10, Some(90))]);
        assert_eq!(pids(descendants(&gone, root)), vec![11]);
        assert_eq!(identity(&gone, 10), None);
        assert_eq!(
            tree_memory_with(&gone, 10, |_| None),
            0,
            "a gone root counts nothing"
        );
        let unknown = table_from([row(10, 1, None), row(11, 10, Some(200))]);
        assert_eq!(
            tree_memory_with(&unknown, 10, |_| Some(7)),
            7,
            "an unknown start counts the root alone"
        );
    }

    #[test]
    fn sums_saturate() {
        let table = table_from([entry(1, None, u64::MAX, false), entry(2, Some(1), 5, false)]);
        assert_eq!(tree_memory_with(&table, 1, |_| None), u64::MAX);
    }

    #[test]
    fn this_process_has_memory_and_a_missing_one_has_none() {
        assert!(process_memory(std::process::id()).is_some_and(|m| m > 0));
        assert!(tree_memory(std::process::id()) > 0);
        assert_eq!(process_memory(u32::MAX - 1), None);
        assert_eq!(tree_memory(u32::MAX - 1), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn threads_do_not_multiply_this_process_memory() {
        let (stop, wait) = std::sync::mpsc::channel::<()>();
        let wait = std::sync::Arc::new(std::sync::Mutex::new(wait));
        let threads: Vec<_> = (0..40)
            .map(|_| {
                let w = wait.clone();
                std::thread::spawn(move || {
                    let _ = w.lock().map(|r| r.recv());
                })
            })
            .collect();
        let table = snapshot();
        let me = std::process::id();
        let own = table.get(&me).map_or(0, |e| e.rss);
        let total = tree_memory_with(&table, me, |_| None);
        drop(stop);
        for t in threads {
            let _ = t.join();
        }
        assert!(
            own > 0 && total < own * 4,
            "own {own}, tree {total}: threads were counted as processes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_real_linux_process_has_a_readable_pss() {
        assert!(proportional(std::process::id()).is_some_and(|m| m > 0));
    }

    /// Regression: mimalloc's freed pages stay resident until the kernel needs them, and a store worker with a footprint of
    /// 535 MB showed 1,965 MB resident and was killed at its 2 GiB cap. Real memory is the physical footprint.
    #[cfg(target_os = "macos")]
    #[test]
    fn freed_pages_that_stay_resident_are_not_real_memory() {
        use crate::testchild::{Mode, child, lines};
        use std::io::BufRead;
        let mut cmd = child(Mode::Freed);
        cmd.stdout(std::process::Stdio::piped());
        // Ends with the test, whatever it asserts.
        let mut c = crate::Supervised::spawn(cmd).unwrap();
        let mut r = std::io::BufReader::new(c.take_stdout().unwrap());
        let mut l = String::new();
        loop {
            l.clear();
            assert!(
                r.read_line(&mut l).unwrap() > 0,
                "the child ended before it was ready"
            );
            if lines(&l).first().is_some_and(|p| *p == "ready") {
                break;
            }
        }
        let pid = c.id().unwrap();
        let rss = snapshot().get(&pid).map_or(0, |e| e.rss);
        let real = process_memory(pid).unwrap();
        let tree = tree_memory(pid);
        assert!(
            rss >= (crate::testchild::FREED_BYTES as u64) / 2,
            "the freed pages are still resident: {rss}"
        );
        assert!(
            real < rss / 4,
            "real memory {real} must be far below the resident size {rss}"
        );
        assert!(
            tree < rss / 4,
            "the tree of a child without children: {tree}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_footprint_of_this_process_is_readable_and_a_missing_process_has_none() {
        assert!(crate::sys::phys_footprint(std::process::id()).is_some_and(|m| m > 0));
        assert_eq!(crate::sys::phys_footprint(u32::MAX - 1), None);
        assert_eq!(
            crate::sys::phys_footprint(u32::MAX),
            None,
            "a pid that is not a c_int"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_private_working_set_of_this_process_is_readable() {
        assert!(crate::sys::private_working_set(std::process::id()).is_some_and(|m| m > 0));
        assert_eq!(crate::sys::private_working_set(u32::MAX - 1), None);
    }
}
