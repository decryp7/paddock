//! Process memory, measured as real memory per OS and never as the resident size (rule 26). Linux lists every thread as a
//! process that reports its process's whole resident size again (a browser with 400 threads would count 400 times over), so
//! thread entries are skipped; browser processes also share large mappings, so on Linux the proportional set size (PSS,
//! from `smaps_rollup`) is used, which divides each page between the processes that map it. On macOS it is the physical
//! footprint (`ri_phys_footprint`, Activity Monitor's "Memory"): the resident size counts pages mimalloc freed but the
//! kernel has not reclaimed (a worker with a 535 MB footprint showed 1,965 MB). On Windows it is the private working set.
//! The resident size is only the fallback for a process whose real memory cannot be read.

use std::collections::HashMap;
use std::path::Path;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// `pid -> (parent, resident bytes)` for real processes only.
pub type Table = HashMap<u32, (Option<u32>, u64)>;

/// One row of the OS process list, before the thread entries are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcEntry {
    /// Process or thread id.
    pub pid: u32,
    /// Parent process, if known.
    pub parent: Option<u32>,
    /// Resident bytes.
    pub rss: u64,
    /// Linux thread entry (shows up in the list beside the real processes).
    pub is_thread: bool,
}

/// The table of the real processes in `entries`: thread entries are skipped.
pub fn table_from(entries: impl IntoIterator<Item = ProcEntry>) -> Table {
    entries
        .into_iter()
        .filter(|e| !e.is_thread)
        .map(|e| (e.pid, (e.parent, e.rss)))
        .collect()
}

/// A look at the OS process table: parents and resident memory only (command lines, environments and working folders
/// are slow to read on Windows and nothing here needs them).
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
        is_thread: p.thread_kind().is_some(),
    }))
}

/// Every process below `root` (not including it), in no particular order.
pub fn descendants(table: &Table, root: u32) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&pid, &(parent, _)) in table {
        if let Some(p) = parent
            && pid != p
        {
            children.entry(p).or_default().push(pid);
        }
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::from([root]);
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        for &kid in children.get(&p).into_iter().flatten() {
            if seen.insert(kid) {
                out.push(kid);
                frontier.push(kid);
            }
        }
    }
    out
}

/// Memory of `root` and its descendants, in bytes; `better` may give a process a more exact figure than its resident
/// size (its PSS), a process it returns `None` for counts by resident size.
pub fn tree_memory_with(table: &Table, root: u32, better: impl Fn(u32) -> Option<u64>) -> u64 {
    std::iter::once(root)
        .chain(descendants(table, root))
        .filter_map(|p| better(p).or_else(|| table.get(&p).map(|e| e.1)))
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

/// Where the figures of a process come from. A watcher tick asks only [`ProcessSource::memory`] and
/// [`ProcessSource::children`], which read one pid each; [`ProcessSource::table`] reads the whole machine's process list
/// and belongs to one-off work (ending a tree, leak checks), never to a per-tick path (ADR 0024 rule 4).
pub trait ProcessSource {
    /// What `pid` really uses (see [`process_memory`]); `None` when there is no such process.
    fn memory(&self, pid: u32) -> Option<u64>;
    /// The direct children of `pid`.
    fn children(&self, pid: u32) -> Vec<u32>;
    /// The whole process table (every process of the machine: the expensive read).
    fn table(&self) -> Table;
}

/// The operating system as a [`ProcessSource`]. Make one per tree walk ([`tree_memory`] does): on a platform that cannot
/// list the children of one pid it scans the machine's process list at most once, lazily, and shares that scan between
/// the nodes of the walk.
#[derive(Debug)]
pub struct OsSource {
    scan: LazyScan,
    /// Lists the children of one pid where the platform can (`None`: it cannot).
    list: fn(u32) -> Option<Vec<u32>>,
    /// Scans the machine for the fallback; a seam for tests.
    scan_with: fn() -> Table,
}

impl Default for OsSource {
    fn default() -> OsSource {
        OsSource {
            scan: LazyScan::default(),
            list: children_of,
            scan_with: scan_table,
        }
    }
}

/// The process list scanned on first use, for the platforms without a children-of-one-pid call.
#[derive(Debug, Default)]
struct LazyScan(std::cell::OnceCell<Table>);

impl LazyScan {
    /// The children of `pid` from the scan, which `scan` makes the first time only.
    fn children(&self, pid: u32, scan: impl FnOnce() -> Table) -> Vec<u32> {
        direct_children(self.0.get_or_init(scan), pid)
    }
}

/// The parents of every process, for the fallback scan: on Windows a light id snapshot (no memory is read), elsewhere the
/// ordinary table.
fn scan_table() -> Table {
    #[cfg(windows)]
    if let Some(list) = crate::sys::process_parents() {
        return list
            .into_iter()
            .map(|(pid, parent)| (pid, (Some(parent), 0)))
            .collect();
    }
    snapshot()
}

impl ProcessSource for OsSource {
    fn memory(&self, pid: u32) -> Option<u64> {
        proportional(pid).or_else(|| resident(pid))
    }

    fn children(&self, pid: u32) -> Vec<u32> {
        children_or_scan((self.list)(pid), || self.scan.children(pid, self.scan_with))
    }

    fn table(&self) -> Table {
        snapshot()
    }
}

/// `listed` when the platform can list the children of one pid; otherwise `scanned` (the fallback only).
fn children_or_scan(listed: Option<Vec<u32>>, scanned: impl FnOnce() -> Vec<u32>) -> Vec<u32> {
    listed.unwrap_or_else(scanned)
}

fn direct_children(table: &Table, pid: u32) -> Vec<u32> {
    table
        .iter()
        .filter(|(kid, (parent, _))| **kid != pid && *parent == Some(pid))
        .map(|(kid, _)| *kid)
        .collect()
}

/// The resident size of one process (the last resort when its real memory cannot be read), from a refresh of that pid
/// only; `None` when it is gone or is a thread entry.
fn resident(pid: u32) -> Option<u64> {
    let id = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    sys.process(id)
        .filter(|p| p.thread_kind().is_none())
        .map(sysinfo::Process::memory)
}

/// The most processes one tree walk visits (a parent cycle or a fork bomb cannot keep a watcher busy).
pub const MAX_TREE: usize = 4096;

/// Memory of `root` and everything below it, reading one pid at a time through `source` and never its whole table.
pub fn tree_memory_from(source: &impl ProcessSource, root: u32) -> u64 {
    let Some(first) = source.memory(root) else {
        return 0;
    };
    let mut total = first;
    let mut seen = std::collections::HashSet::from([root]);
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        for kid in source.children(p) {
            if seen.len() >= MAX_TREE {
                return total;
            }
            // A child that is gone (or whose pid was reused after the listing) counts nothing and is not walked.
            if seen.insert(kid)
                && let Some(m) = source.memory(kid)
            {
                total = total.saturating_add(m);
                frontier.push(kid);
            }
        }
    }
    total
}

/// What `pid` really uses, in bytes: PSS on Linux, the physical footprint on macOS, the private working set on Windows (the
/// resident size if that cannot be read). `None` when there is no such process (or it is a thread id). Reads that pid only.
pub fn process_memory(pid: u32) -> Option<u64> {
    OsSource::default().memory(pid)
}

/// The memory of the tree at `root` (see [`process_memory`]), 0 when it is gone. Reads the pids of the tree only; the
/// children of each come from the OS per pid (`proc_listchildpids` on macOS, `/proc/<pid>/task/*/children` on Linux). Windows
/// has no per-parent call: one snapshot of the machine's process and thread list (ids and parents only) is taken for the
/// whole walk. A supervised child on Windows is measured through its job object instead ([`tree_memory_of`]), with no scan.
pub fn tree_memory(root: u32) -> u64 {
    tree_memory_from(&OsSource::default(), root)
}

/// Memory of the listed processes (a job object's members), 0 for those gone.
pub fn tree_memory_of(source: &impl ProcessSource, pids: &[u32]) -> u64 {
    pids.iter()
        .take(MAX_TREE)
        .filter_map(|p| source.memory(*p))
        .fold(0, u64::saturating_add)
}

/// The direct children of `pid` read for that pid only, `None` when the platform has no such call.
#[cfg(target_os = "macos")]
fn children_of(pid: u32) -> Option<Vec<u32>> {
    crate::sys::child_pids(pid)
}

#[cfg(target_os = "linux")]
fn children_of(pid: u32) -> Option<Vec<u32>> {
    read_children(Path::new("/proc"), pid)
}

#[cfg(windows)]
fn children_of(_pid: u32) -> Option<Vec<u32>> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn children_of(_pid: u32) -> Option<Vec<u32>> {
    None
}

/// The children of `pid` from `<proc_root>/<pid>/task/<tid>/children` (every thread lists the children it forked).
/// `Some(empty)` when the process is gone, `None` when the kernel has no `children` files (CONFIG_PROC_CHILDREN off).
pub fn read_children(proc_root: &Path, pid: u32) -> Option<Vec<u32>> {
    let Ok(tasks) = std::fs::read_dir(proc_root.join(pid.to_string()).join("task")) else {
        return Some(Vec::new());
    };
    let mut kids = Vec::new();
    for task in tasks.flatten() {
        match std::fs::read_to_string(task.path().join("children")) {
            Ok(text) => kids.extend(
                text.split_whitespace()
                    .filter_map(|t| t.parse::<u32>().ok()),
            ),
            // No `children` file in a task that is still there: the kernel does not offer them. A thread that ended while
            // listing, or an unreadable entry, is skipped.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && task.path().exists() => {
                return None;
            }
            Err(_) => {}
        }
    }
    Some(kids)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLLUP: &str = "55d0c0a00000-7ffd5a7ff000 ---p 00000000 00:00 0                          [rollup]\n\
Rss:              219040 kB\nPss:              131633 kB\nPss_Anon:          90000 kB\nShared_Clean:      70000 kB\n";
    const MB: u64 = 1 << 20;

    fn entry(pid: u32, parent: Option<u32>, rss: u64, is_thread: bool) -> ProcEntry {
        ProcEntry {
            pid,
            parent,
            rss,
            is_thread,
        }
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
        let mut d = descendants(&table, 1);
        d.sort();
        assert_eq!(d, vec![2, 3]);
        let mut d = descendants(&table, 6);
        d.sort();
        assert_eq!(d, vec![7], "a parent cycle terminates");
        assert!(
            descendants(&table, 5).is_empty(),
            "self-parent is not its own child"
        );
        assert_eq!(tree_memory_with(&table, 1, |_| None).saturating_add(0), 3);
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

    /// A process source that counts what a tick reads: the tree is `pid -> (children, memory)`.
    struct Counting {
        tree: HashMap<u32, (Vec<u32>, u64)>,
        table_reads: std::cell::Cell<usize>,
        memory_reads: std::cell::Cell<usize>,
        children_reads: std::cell::Cell<usize>,
    }

    impl Counting {
        fn new(rows: &[(u32, &[u32], u64)]) -> Counting {
            Counting {
                tree: rows
                    .iter()
                    .map(|(p, k, m)| (*p, (k.to_vec(), *m)))
                    .collect(),
                table_reads: 0.into(),
                memory_reads: 0.into(),
                children_reads: 0.into(),
            }
        }
    }

    impl ProcessSource for Counting {
        fn memory(&self, pid: u32) -> Option<u64> {
            self.memory_reads.set(self.memory_reads.get() + 1);
            self.tree.get(&pid).map(|e| e.1)
        }
        fn children(&self, pid: u32) -> Vec<u32> {
            self.children_reads.set(self.children_reads.get() + 1);
            self.tree.get(&pid).map(|e| e.0.clone()).unwrap_or_default()
        }
        fn table(&self) -> Table {
            self.table_reads.set(self.table_reads.get() + 1);
            Table::new()
        }
    }

    /// ADR 0024 rule 4: one watcher tick for one child reads no whole process table, and one memory read and one children
    /// read per process of its tree.
    #[test]
    fn a_tick_reads_no_whole_table_and_one_read_per_process_of_the_tree() {
        let src = Counting::new(&[
            (1, &[2, 3], 10),
            (2, &[4], 20),
            (3, &[], 30),
            (4, &[], 40),
            (9, &[], 999),
        ]);
        assert_eq!(tree_memory_from(&src, 1), 100);
        assert_eq!(src.table_reads.get(), 0, "a whole-table read per tick");
        assert_eq!(src.memory_reads.get(), 4);
        assert_eq!(src.children_reads.get(), 4);
        let single = Counting::new(&[(5, &[], 7)]);
        assert_eq!(tree_memory_from(&single, 5), 7);
        assert_eq!(
            (
                single.table_reads.get(),
                single.memory_reads.get(),
                single.children_reads.get()
            ),
            (0, 1, 1)
        );
    }

    #[test]
    fn a_gone_root_reads_nothing_more_and_cycles_and_unreadable_children_are_bounded() {
        let src = Counting::new(&[(2, &[3], 5)]);
        assert_eq!(tree_memory_from(&src, 1), 0);
        assert_eq!((src.children_reads.get(), src.table_reads.get()), (0, 0));
        assert!(
            src.table().is_empty() && src.table_reads.get() == 1,
            "the counter counts"
        );
        // 1 -> 2 -> 1 (a cycle) and a child that vanished between the listing and the read: its children are not walked.
        let src = Counting::new(&[(1, &[2, 8], 10), (2, &[1], 20)]);
        assert_eq!(tree_memory_from(&src, 1), 30);
        assert_eq!(
            src.children_reads.get(),
            2,
            "the gone child 8 is not asked for its children"
        );
        // A very wide tree stops at the bound.
        let kids: Vec<u32> = (2..(MAX_TREE as u32 + 100)).collect();
        let mut rows: Vec<(u32, &[u32], u64)> = vec![(1, &kids, 1)];
        rows.extend(kids.iter().map(|k| (*k, &[][..], 1)));
        let src = Counting::new(&rows);
        assert_eq!(tree_memory_from(&src, 1), MAX_TREE as u64);
    }

    #[test]
    fn children_are_read_from_the_task_folders_of_a_proc_fixture() {
        let proc = tempfile::tempdir().unwrap();
        for (tid, kids) in [("10", "11 12 \n"), ("14", "13\n"), ("15", "\n")] {
            let dir = proc.path().join("10/task").join(tid);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("children"), kids).unwrap();
        }
        let mut kids = read_children(proc.path(), 10).unwrap();
        kids.sort();
        assert_eq!(kids, vec![11, 12, 13]);
        assert_eq!(
            read_children(proc.path(), 99),
            Some(vec![]),
            "a process that is gone has no children"
        );
        // A kernel without the files.
        std::fs::create_dir_all(proc.path().join("20/task/20")).unwrap();
        assert_eq!(read_children(proc.path(), 20), None);
        // A thread that ended while listing, and an unreadable entry, are skipped.
        std::fs::create_dir_all(proc.path().join("30/task/30/children")).unwrap();
        assert_eq!(read_children(proc.path(), 30), Some(vec![]));
    }

    /// Without a children-of-one-pid call, a whole walk scans the machine once, however many nodes it has.
    #[test]
    fn a_walk_without_a_per_pid_call_scans_once() {
        let table = table_from([
            entry(1, None, 1, false),
            entry(2, Some(1), 1, false),
            entry(3, Some(2), 1, false),
        ]);
        let scans = std::cell::Cell::new(0);
        let lazy = LazyScan::default();
        let scan = || {
            scans.set(scans.get() + 1);
            table.clone()
        };
        assert_eq!(
            scans.get(),
            0,
            "lazy: nothing is scanned before the first node"
        );
        for pid in [1, 2, 3, 1, 9] {
            let _ = lazy.children(pid, scan);
        }
        assert_eq!(scans.get(), 1);
        assert_eq!(lazy.children(1, scan), vec![2]);
    }

    thread_local! {
        static KID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// The real source on a platform without a per-pid call: a whole tree walk scans the machine once, through its own
    /// scan and not through `table()`.
    #[test]
    fn the_os_source_scans_once_per_walk_when_it_cannot_list_children() {
        use crate::testchild::{Mode, child};
        let me = std::process::id();
        let c = crate::Supervised::spawn(child(Mode::Sleep)).unwrap();
        let kid = c.id().unwrap();
        KID.with(|k| k.set(kid));
        let src = OsSource {
            scan: LazyScan::default(),
            list: |_| None,
            scan_with: || {
                SCANS.with(|s| s.set(s.get() + 1));
                let row = |pid, parent| ProcEntry {
                    pid,
                    parent,
                    rss: 1,
                    is_thread: false,
                };
                table_from([
                    row(std::process::id(), None),
                    row(KID.with(std::cell::Cell::get), Some(std::process::id())),
                ])
            },
        };
        SCANS.with(|s| s.set(0));
        assert!(tree_memory_from(&src, me) >= process_memory(kid).unwrap());
        assert_eq!(
            SCANS.with(std::cell::Cell::get),
            1,
            "one scan for the root and the child together"
        );
    }

    #[test]
    fn the_members_of_a_job_are_summed_and_the_gone_ones_count_nothing() {
        let src = Counting::new(&[(1, &[], 10), (2, &[], 20)]);
        assert_eq!(tree_memory_of(&src, &[1, 2, 3]), 30);
        assert_eq!((src.table_reads.get(), src.children_reads.get()), (0, 0));
    }

    #[test]
    fn direct_children_come_from_a_table() {
        let table = table_from([
            entry(1, None, 1, false),
            entry(2, Some(1), 1, false),
            entry(3, Some(1), 1, false),
            entry(4, Some(2), 1, false),
            entry(5, Some(5), 1, false),
        ]);
        let mut k = direct_children(&table, 1);
        k.sort();
        assert_eq!(k, vec![2, 3]);
        assert!(direct_children(&table, 5).is_empty());
        assert_eq!(
            children_or_scan(Some(vec![7]), || panic!("no scan when the platform lists")),
            vec![7]
        );
        assert_eq!(children_or_scan(None, || vec![8]), vec![8]);
        assert!(
            OsSource::default()
                .table()
                .contains_key(&std::process::id())
        );
    }

    #[test]
    fn the_resident_size_of_one_pid_is_read_and_a_missing_pid_has_none() {
        assert!(resident(std::process::id()).is_some_and(|m| m > 0));
        assert_eq!(resident(u32::MAX - 1), None);
    }

    #[test]
    fn the_os_source_lists_the_children_of_one_pid_without_a_table() {
        use crate::testchild::{Mode, child};
        let me = std::process::id();
        let c = crate::Supervised::spawn(child(Mode::Sleep)).unwrap();
        let pid = c.id().unwrap();
        assert!(OsSource::default().children(me).contains(&pid));
        // On Windows a CREATE_NO_WINDOW child owns a conhost.exe, so it is never childless there (CI run 6603).
        #[cfg(not(windows))]
        assert!(OsSource::default().children(pid).is_empty());
        #[cfg(windows)]
        assert!(!OsSource::default().children(pid).contains(&pid));
        assert!(OsSource::default().children(u32::MAX - 1).is_empty());
        assert!(tree_memory(me) >= process_memory(pid).unwrap());
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
        let own = table.get(&me).map_or(0, |e| e.1);
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
        let rss = snapshot().get(&pid).map_or(0, |e| e.1);
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
