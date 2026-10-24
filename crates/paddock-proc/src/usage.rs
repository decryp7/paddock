//! Process memory. Linux lists every thread as a process that reports its process's whole resident size again (a browser
//! with 400 threads would count 400 times over), so thread entries are skipped (rule 26); browser processes also share
//! large mappings, so on Linux the proportional set size (PSS, from `smaps_rollup`) is used, which divides each page
//! between the processes that map it. Everywhere else the resident size is the best figure there is.

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

#[cfg(not(target_os = "linux"))]
fn proportional(_pid: u32) -> Option<u64> {
    None
}

/// What `pid` really uses, in bytes: PSS on Linux (resident size if it cannot be read), the resident size elsewhere.
/// `None` when there is no such process (or it is a thread id).
pub fn process_memory(pid: u32) -> Option<u64> {
    let table = snapshot();
    proportional(pid).or_else(|| table.get(&pid).map(|e| e.1))
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
}
