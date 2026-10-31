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

/// A CPU time and the start time of the process it was read from, when the OS gave both in one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuSample {
    /// User + system CPU time in microseconds.
    pub micros: u64,
    /// When the process started, from the same call; `None` when the OS call did not carry it.
    pub started: Option<StartTime>,
    /// Whether the process is stopped (by a signal, App Tamer), from the same call; `None` when the OS call did not say.
    pub stopped: Option<bool>,
}

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

/// The CPU time in microseconds from the text of a Linux `/proc/<pid>/stat`: `utime` + `stime` (fields 14 and 15, counted
/// after the last `)` because the name may hold spaces and parentheses) over `hz` ticks a second. `None` when malformed.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn cpu_micros_from_stat(stat: &str, hz: u64) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut f = rest.split_ascii_whitespace().skip(11);
    let user: u64 = f.next()?.parse().ok()?;
    let system: u64 = f.next()?.parse().ok()?;
    user.checked_add(system)?
        .checked_mul(1_000_000)?
        .checked_div(hz)
}

/// Mach absolute time units to microseconds with the timebase `numer / denom` (nanoseconds per unit).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn mach_to_micros(units: u64, numer: u32, denom: u32) -> u64 {
    u64::try_from(u128::from(units) * u128::from(numer) / u128::from(denom.max(1)) / 1000)
        .unwrap_or(u64::MAX)
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
    /// When the process that holds `pid` now started; `None` when there is none (ADR 0033: a walk follows a parent link
    /// only to a child that started no earlier than its parent).
    fn started(&self, pid: u32) -> Option<StartTime>;
    /// The CPU time (user + system) `pid` has used so far, with the start time of the process it was read from, taken in
    /// the same call where the OS gives both (rule 53: a reading is only that process's if the identity came with it, not
    /// from a look before or after). `None` when there is no such process or the OS did not say. Reads that pid only (a
    /// start judge reads it every beat: ADR 0031 section 4, about 0.1% of a core).
    fn cpu_sample(&self, _pid: u32) -> Option<CpuSample> {
        None
    }
    /// [`ProcessSource::cpu_sample`]'s CPU time in microseconds, without the identity.
    fn cpu_micros(&self, pid: u32) -> Option<u64> {
        self.cpu_sample(pid).map(|s| s.micros)
    }
    /// The whole process table (every process of the machine: the expensive read).
    fn table(&self) -> Table;
    /// The latest start time a child listed by [`ProcessSource::children`] may have: the time of the scan its listing came
    /// from, when the listing is a scan taken earlier in the walk (Windows without a job). A pid in that scan that names a
    /// process started after it was handed on since and is not the scanned child. `None` (the default) for a live listing.
    fn listed_by(&self) -> Option<StartTime> {
        None
    }
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
struct LazyScan(std::cell::OnceCell<(Table, StartTime)>);

impl LazyScan {
    /// The children of `pid` from the scan, which `scan` makes the first time only.
    fn children(&self, pid: u32, scan: impl FnOnce() -> Table) -> Vec<u32> {
        direct_children(
            &self.0.get_or_init(|| (scan(), crate::identity::now())).0,
            pid,
        )
    }

    /// When the scan was taken (just after it), if it was.
    fn taken(&self) -> Option<StartTime> {
        self.0.get().map(|s| s.1)
    }
}

/// The parents of every process, for the fallback scan: on Windows a light id snapshot (no memory is read), elsewhere the
/// ordinary table.
fn scan_table() -> Table {
    #[cfg(windows)]
    if let Some(list) = crate::sys::process_parents() {
        return list
            .into_iter()
            .map(|(pid, parent)| {
                let row = Row {
                    parent: Some(parent),
                    rss: 0,
                    started: None,
                };
                (pid, row)
            })
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

    fn started(&self, pid: u32) -> Option<StartTime> {
        crate::sys::start_time(pid)
    }

    fn cpu_sample(&self, pid: u32) -> Option<CpuSample> {
        crate::sys::cpu_sample(pid)
    }

    fn table(&self) -> Table {
        snapshot()
    }

    fn listed_by(&self) -> Option<StartTime> {
        self.scan.taken()
    }
}

/// `listed` when the platform can list the children of one pid; otherwise `scanned` (the fallback only).
fn children_or_scan(listed: Option<Vec<u32>>, scanned: impl FnOnce() -> Vec<u32>) -> Vec<u32> {
    listed.unwrap_or_else(scanned)
}

fn direct_children(table: &Table, pid: u32) -> Vec<u32> {
    table
        .iter()
        .filter(|(kid, row)| **kid != pid && row.parent == Some(pid))
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

/// Memory of `root` and everything below it, reading one pid at a time through `source` and never its whole table. By
/// identity (ADR 0033): a root whose pid now names another process counts nothing; a child counts only if it started no
/// earlier than its parent, and a parent's children count only if the parent still held its pid after they were listed.
pub fn tree_memory_from(source: &impl ProcessSource, root: ProcessId) -> u64 {
    tree_fold(source, root, |p, _| source.memory(p)).unwrap_or(0)
}

/// The CPU time (user + system) of `root` and everything below it, reading one pid at a time through `source` and never
/// its whole table, by identity like [`tree_memory_from`]: each reading carries the start time of the process it came from
/// when the OS gives it in the same call, and a reading whose start differs from the one expected counts nothing. `None`
/// when `root` is gone or the OS did not report its time. A child whose time cannot be read counts nothing.
///
/// The sum grows while the tree runs, but it can also fall: a child that exits takes its time out of the sum. Two readings
/// therefore tell that the tree ran in between only when the second is larger; an equal or smaller one says nothing more
/// (the start judge counts it as no growth). Load does not change the sum (a starved process uses none, a busy one does).
pub fn tree_cpu_time_from(
    source: &impl ProcessSource,
    root: ProcessId,
) -> Option<std::time::Duration> {
    tree_fold(source, root, |p, expected| {
        source
            .cpu_sample(p)
            .filter(|s| s.started.is_none_or(|t| t == expected))
            .map(|s| s.micros)
    })
    .map(std::time::Duration::from_micros)
}

/// Adds `read(pid, expected start)` over `root` and its descendants (the walk of [`tree_memory_from`]); `None` when the root is not that
/// process any more or `read` has nothing for it.
fn tree_fold(
    source: &impl ProcessSource,
    root: ProcessId,
    read: impl Fn(u32, StartTime) -> Option<u64>,
) -> Option<u64> {
    if source.started(root.pid()) != Some(root.started()) {
        return None;
    }
    let mut total = read(root.pid(), root.started())?;
    let mut seen = std::collections::HashSet::from([root.pid()]);
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        let kids = source.children(p.pid());
        // The listing is of whoever holds the pid now: if that is no longer `p`, the children are a stranger's.
        if source.started(p.pid()) != Some(p.started()) {
            continue;
        }
        for kid in kids {
            if seen.len() >= MAX_TREE {
                return Some(total);
            }
            if !seen.insert(kid) {
                continue;
            }
            // A child that is gone, older than its parent, or younger than the scan that listed it counts nothing and is not
            // walked.
            let listed_by = source.listed_by();
            let Some(started) = source
                .started(kid)
                .filter(|s| *s >= p.started() && listed_by.is_none_or(|t| *s <= t))
            else {
                continue;
            };
            if let Some(m) = read(kid, started) {
                total = total.saturating_add(m);
                frontier.push(ProcessId::new(kid, started));
            }
        }
    }
    Some(total)
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
    ProcessId::of(root).map_or(0, |id| tree_memory_from(&OsSource::default(), id))
}

/// The CPU time of the tree at `root` (see [`tree_cpu_time_from`]) on this OS, reading the pids of the tree only.
pub fn tree_cpu_time(root: ProcessId) -> Option<std::time::Duration> {
    tree_cpu_time_from(&OsSource::default(), root)
}

/// The CPU time of the listed processes (a job object's members), `None` when none could be read.
pub fn tree_cpu_time_of(source: &impl ProcessSource, pids: &[u32]) -> Option<std::time::Duration> {
    let mut any = false;
    let sum = pids
        .iter()
        .filter_map(|&p| source.cpu_sample(p).map(|s| s.micros))
        .inspect(|_| any = true)
        .fold(0u64, u64::saturating_add);
    any.then(|| std::time::Duration::from_micros(sum))
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

    /// The real OS reader against a process started here: a busy loop's CPU time grows, an idle one's does not move much,
    /// and a gone process has none (this runs on macOS and Linux; Windows is checked by its CI run).
    #[cfg(unix)]
    #[test]
    fn the_os_reads_the_cpu_time_of_a_busy_child_and_of_its_tree() {
        let mut cmd = crate::command("sh");
        cmd.args(["-c", "while :; do :; done"]);
        let mut child = crate::Supervised::spawn(cmd).expect("a busy child");
        let id = child.process().expect("its identity");
        let os = OsSource::default();
        let sample = os.cpu_sample(id.pid()).expect("the OS reports a sample");
        assert_eq!(
            sample.started,
            Some(id.started()),
            "the reading carries the process's own start time"
        );
        let first = os.cpu_micros(id.pid()).expect("the OS reports a CPU time");
        let t = std::time::Instant::now();
        // Busy until the OS has charged some CPU (a condition, not a sleep); bounded only as a safety net.
        while os
            .cpu_micros(id.pid())
            .is_some_and(|now| now <= first + 20_000)
            && t.elapsed() < std::time::Duration::from_secs(60)
        {
            std::thread::yield_now();
        }
        let later = os.cpu_micros(id.pid()).expect("still reported");
        assert!(later > first + 20_000, "{first} -> {later}");
        let tree = tree_cpu_time_from(&os, id).expect("the tree's time");
        assert!(
            tree.as_micros() >= u128::from(later),
            "the tree includes the root"
        );
        child.kill();
        assert_eq!(os.cpu_micros(u32::MAX - 1), None, "no such process");
    }

    #[test]
    fn cpu_time_is_read_from_the_stat_line_after_the_last_parenthesis() {
        let stat = "1234 (a (weird) name) S 1 1234 1234 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 1 0 98765 1000000 50";
        assert_eq!(
            cpu_micros_from_stat(stat, 100),
            Some(80_000),
            "5 + 3 ticks at 100 Hz"
        );
        assert_eq!(cpu_micros_from_stat("1 (x) S 1 2", 100), None);
        assert_eq!(cpu_micros_from_stat("no parenthesis", 100), None);
        assert_eq!(
            cpu_micros_from_stat("1 (x) S 1 1 1 0 -1 0 0 0 0 0 a 3", 100),
            None
        );
        assert_eq!(cpu_micros_from_stat(stat, 0), None, "no division by zero");
        let huge = "1 (x) S 1 1 1 0 -1 0 0 0 0 0 18446744073709551615 1";
        assert_eq!(cpu_micros_from_stat(huge, 100), None, "no overflow");
    }

    #[test]
    fn mach_units_convert_with_the_timebase() {
        assert_eq!(
            mach_to_micros(24_000_000, 125, 3),
            1_000_000,
            "Apple silicon: 125/3 ns per unit"
        );
        assert_eq!(mach_to_micros(5_000, 1, 1), 5);
        assert_eq!(mach_to_micros(u64::MAX, u32::MAX, 1), u64::MAX);
        assert_eq!(
            mach_to_micros(1_000, 1, 0),
            1,
            "a zero denominator is not divided by"
        );
    }

    /// A tree 1 -> 2 -> 3 whose members used 10, 20 and 30 ms; every read is counted. `mute`: the OS says nothing.
    /// `child_start`: the start time process 2's readings carry (`None`: none), as when its pid was handed on.
    struct Fake {
        reads: std::cell::Cell<u32>,
        tables: std::cell::Cell<u32>,
        mute: bool,
        child_start: Option<StartTime>,
    }
    impl Fake {
        fn new() -> Fake {
            Fake {
                reads: 0.into(),
                tables: 0.into(),
                mute: false,
                child_start: Some(StartTime::from_micros(2)),
            }
        }
    }
    impl ProcessSource for Fake {
        fn memory(&self, pid: u32) -> Option<u64> {
            Some(u64::from(pid))
        }
        fn children(&self, pid: u32) -> Vec<u32> {
            match pid {
                1 => vec![2],
                2 => vec![3],
                _ => vec![],
            }
        }
        fn started(&self, pid: u32) -> Option<StartTime> {
            (1..=3)
                .contains(&pid)
                .then(|| StartTime::from_micros(u64::from(pid)))
        }
        fn cpu_sample(&self, pid: u32) -> Option<CpuSample> {
            self.reads.set(self.reads.get() + 1);
            if self.mute || !(1..=3).contains(&pid) {
                return None;
            }
            let started = if pid == 2 {
                self.child_start
            } else {
                self.started(pid)
            };
            Some(CpuSample {
                micros: u64::from(pid) * 10_000,
                started,
                stopped: None,
            })
        }
        fn table(&self) -> Table {
            self.tables.set(self.tables.get() + 1);
            Table::new()
        }
    }

    #[test]
    fn tree_cpu_time_sums_the_tree_one_pid_at_a_time_and_by_identity() {
        let src = Fake::new();
        let root = ProcessId::new(1, StartTime::from_micros(1));
        assert_eq!(
            tree_cpu_time_from(&src, root),
            Some(std::time::Duration::from_millis(60))
        );
        assert_eq!(src.reads.get(), 3, "one read per process of the tree");
        assert_eq!(src.tables.get(), 0, "never the whole table");
        let other = ProcessId::new(1, StartTime::from_micros(99));
        assert_eq!(
            tree_cpu_time_from(&src, other),
            None,
            "a pid held by someone else is not that process"
        );
        let gone = ProcessId::new(7, StartTime::from_micros(7));
        assert_eq!(tree_cpu_time_from(&src, gone), None);
        // The fake's other answers, which the memory walk uses.
        assert_eq!(tree_memory_from(&src, root), 6);
        assert!(src.table().is_empty());
    }

    #[test]
    fn a_reading_from_another_process_than_the_expected_one_counts_nothing() {
        let root = ProcessId::new(1, StartTime::from_micros(1));
        let ms = std::time::Duration::from_millis;
        assert_eq!(tree_cpu_time_from(&Fake::new(), root), Some(ms(60)));
        let handed = Fake {
            child_start: Some(StartTime::from_micros(99)),
            ..Fake::new()
        };
        assert_eq!(
            tree_cpu_time_from(&handed, root),
            Some(ms(10)),
            "the child's reading is dropped, and with it its subtree"
        );
        let unknown = Fake {
            child_start: None,
            ..Fake::new()
        };
        assert_eq!(
            tree_cpu_time_from(&unknown, root),
            Some(ms(60)),
            "no start in the sample: accepted, as documented"
        );
        let wrong_root = ProcessId::new(2, StartTime::from_micros(2));
        assert_eq!(
            tree_cpu_time_from(&handed, wrong_root),
            None,
            "the root's own reading came from someone else"
        );
    }

    #[test]
    fn a_source_that_cannot_say_has_no_cpu_time() {
        let mute = Fake {
            mute: true,
            ..Fake::new()
        };
        let root = ProcessId::new(1, StartTime::from_micros(1));
        assert_eq!(tree_cpu_time_from(&mute, root), None);
        assert_eq!(tree_cpu_time_of(&mute, &[1, 2]), None);
        assert_eq!(mute.cpu_micros(1), None);
        let src = Fake::new();
        assert_eq!(
            src.cpu_micros(1),
            Some(10_000),
            "the bare reading is the sample's time"
        );
        let ms = std::time::Duration::from_millis;
        assert_eq!(
            tree_cpu_time_of(&src, &[1, 2, 9]),
            Some(ms(30)),
            "a member that is gone counts nothing"
        );
    }

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
    fn tree_cpu_time_grows_with_work_and_is_none_for_a_gone_process() {
        let me = ProcessId::of(std::process::id()).expect("own identity");
        let before = tree_cpu_time(me).expect("own cpu time");
        let t = std::time::Instant::now();
        let mut x = 0u64;
        // Busy until the OS has charged some CPU (a condition, not a sleep); bounded by the clock only as a safety net.
        while tree_cpu_time(me).is_some_and(|now| now <= before)
            && t.elapsed() < std::time::Duration::from_secs(60)
        {
            for i in 0..2_000_000u64 {
                x = x.wrapping_add(std::hint::black_box(i));
            }
        }
        assert!(tree_cpu_time(me).unwrap() > before, "{x}");
        let gone = ProcessId::new(std::process::id(), StartTime::from_micros(1));
        assert_eq!(
            tree_cpu_time(gone),
            None,
            "a pid held by someone else is not that process"
        );
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
        /// Every process started at its pid in µs, so a higher pid is younger (the cycle 2 -> 1 is "older than its parent").
        fn started(&self, pid: u32) -> Option<StartTime> {
            self.tree
                .contains_key(&pid)
                .then(|| StartTime::from_micros(u64::from(pid)))
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
        // A source that does not override the CPU reading says nothing (the trait's default).
        let plain = Counting::new(&[(1, &[], 1)]);
        assert_eq!(plain.cpu_sample(1), None);
        assert_eq!(plain.cpu_micros(1), None);
        let src = Counting::new(&[
            (1, &[2, 3], 10),
            (2, &[4], 20),
            (3, &[], 30),
            (4, &[], 40),
            (9, &[], 999),
        ]);
        assert_eq!(tree_memory_from(&src, id(1)), 100);
        assert_eq!(src.table_reads.get(), 0, "a whole-table read per tick");
        assert_eq!(src.memory_reads.get(), 4);
        assert_eq!(src.children_reads.get(), 4);
        let single = Counting::new(&[(5, &[], 7)]);
        assert_eq!(tree_memory_from(&single, id(5)), 7);
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
        assert_eq!(tree_memory_from(&src, id(1)), 0);
        assert_eq!((src.children_reads.get(), src.table_reads.get()), (0, 0));
        assert!(
            src.table().is_empty() && src.table_reads.get() == 1,
            "the counter counts"
        );
        // 1 -> 2 -> 1 (a cycle) and a child that vanished between the listing and the read: its children are not walked.
        let src = Counting::new(&[(1, &[2, 8], 10), (2, &[1], 20)]);
        assert_eq!(tree_memory_from(&src, id(1)), 30);
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
        assert_eq!(tree_memory_from(&src, id(1)), MAX_TREE as u64);
    }

    /// ADR 0033: a per-pid walk counts no stranger. A child older than its parent is the orphan of an earlier holder of the
    /// parent's pid, and a root whose pid now names another process counts nothing at all.
    #[test]
    fn a_per_pid_walk_skips_strangers_and_a_replaced_root() {
        // 5 lists 3 as a child, but 3 started before 5: not 5's child.
        let src = Counting::new(&[(5, &[3, 7], 10), (3, &[], 100), (7, &[], 1)]);
        assert_eq!(tree_memory_from(&src, id(5)), 11);
        let replaced = ProcessId::new(5, StartTime::from_micros(4));
        assert_eq!(
            tree_memory_from(&src, replaced),
            0,
            "pid 5 belongs to another process now"
        );
        assert_eq!(
            src.memory_reads.get(),
            2,
            "the replaced root was not measured"
        );
    }

    /// A root whose memory cannot be read counts nothing; a parent whose pid was handed on between the listing and the
    /// re-check contributes none of the listed children (they are the new holder's).
    #[test]
    fn a_parent_handed_on_during_its_listing_contributes_no_children() {
        struct Flaky {
            asked: std::cell::Cell<u32>,
        }
        impl ProcessSource for Flaky {
            fn memory(&self, pid: u32) -> Option<u64> {
                (pid != 9).then_some(5)
            }
            fn children(&self, _pid: u32) -> Vec<u32> {
                vec![2]
            }
            fn started(&self, pid: u32) -> Option<StartTime> {
                // pid 1 is itself at the first look and someone else (started later) at the second.
                let n = self.asked.get();
                self.asked.set(n + 1);
                Some(StartTime::from_micros(if pid == 1 && n > 0 {
                    99
                } else {
                    u64::from(pid)
                }))
            }
            fn table(&self) -> Table {
                Table::new()
            }
        }
        let src = Flaky { asked: 0.into() };
        assert_eq!(tree_memory_from(&src, id(1)), 5, "only the root counts");
        assert_eq!(
            tree_memory_from(&Flaky { asked: 0.into() }, id(9)),
            0,
            "no memory for the root"
        );
        assert_eq!(src.table().len(), 0);
    }

    /// Without a job on Windows the children come from a scan taken earlier in the walk: a pid of that scan whose process
    /// started after the scan was handed on since and is not counted.
    #[test]
    fn a_child_younger_than_the_scan_that_listed_it_is_not_counted() {
        struct Scanned;
        impl ProcessSource for Scanned {
            fn memory(&self, _pid: u32) -> Option<u64> {
                Some(5)
            }
            fn children(&self, pid: u32) -> Vec<u32> {
                if pid == 1 { vec![2, 3] } else { vec![] }
            }
            fn started(&self, pid: u32) -> Option<StartTime> {
                // 3 started after the scan (at 50): its pid was handed on.
                Some(StartTime::from_micros(if pid == 3 {
                    60
                } else {
                    u64::from(pid)
                }))
            }
            fn table(&self) -> Table {
                Table::new()
            }
            fn listed_by(&self) -> Option<StartTime> {
                Some(StartTime::from_micros(50))
            }
        }
        assert_eq!(
            tree_memory_from(&Scanned, id(1)),
            10,
            "the root and 2, not 3"
        );
        assert!(Scanned.table().is_empty());
        let lazy = LazyScan::default();
        assert_eq!(lazy.taken(), None);
        let _ = lazy.children(1, Table::new);
        assert!(lazy.taken().is_some());
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
                    started: None,
                    is_thread: false,
                };
                table_from([
                    row(std::process::id(), None),
                    row(KID.with(std::cell::Cell::get), Some(std::process::id())),
                ])
            },
        };
        SCANS.with(|s| s.set(0));
        assert!(tree_memory_from(&src, ProcessId::of(me).unwrap()) >= process_memory(kid).unwrap());
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
        #[cfg(not(windows))]
        assert!(OsSource::default().children(pid).is_empty());
        // On Windows a CREATE_NO_WINDOW child owns one conhost.exe and nothing else (CI run 6603).
        #[cfg(windows)]
        {
            let kids = OsSource::default().children(pid);
            assert!(kids.len() <= 1, "{kids:?}");
            for kid in kids {
                let id = Pid::from_u32(kid);
                let mut sys = System::new();
                sys.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[id]),
                    true,
                    ProcessRefreshKind::nothing(),
                );
                let name = sys
                    .process(id)
                    .map(|p| p.name().to_string_lossy().to_lowercase());
                assert_eq!(name.as_deref(), Some("conhost.exe"), "pid {kid}");
            }
        }
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
