//! Runner-scoped resource use: the CPU and resident memory of a runner process and everything it started (the job's
//! shells, compilers, test processes, browsers), as opposed to the whole machine (ADR 0007).
//!
//! Everything here is pure. The service reads a process table from the OS ([`ProcessInfo`]) on every probe tick,
//! [`tree_members`] picks the runner's processes, [`UsageTracker`] turns the change in their cumulative CPU time into
//! a share of the machine's capacity, and [`UsageAcc`] averages the ticks of one sample period (and later the
//! buckets of the history tiers).
//!
//! # What counts as "the runner's processes"
//!
//! The runner process, every descendant by parent pid, and every process in the runner's process group (Pitcrew
//! starts runners as group leaders on macOS and Linux, so a job process that was reparented to `init` when its
//! parent exited is still counted). A process that leaves the group *and* loses its parent (a daemon that calls
//! `setsid`) is not counted: it is no longer the job's, and following it would mean guessing.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Most processes counted for one runner; a fork bomb must not make the sampler slow or the result huge.
pub const MAX_TREE_PROCESSES: usize = 4096;

/// One row of the OS process table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessInfo {
    /// Process id.
    pub pid: u32,
    /// Parent process id, when known.
    pub ppid: Option<u32>,
    /// Process group id (Unix), when known.
    pub pgid: Option<u32>,
    /// Cumulative CPU time (user + system) in milliseconds.
    pub cpu_time_ms: u64,
    /// Resident memory in bytes.
    pub rss_bytes: u64,
}

/// The processes that belong to the runner with process id `root`: the root, its descendants and its process group,
/// at most [`MAX_TREE_PROCESSES`]. Empty when `root` is not in the table (the runner is gone) or is a system pid.
// ADR 0033 WP-6: a tree by parent pids without start times; skip a child older than its parent (paddock_proc::descendants)
pub fn tree_members(root: u32, table: &[ProcessInfo]) -> Vec<ProcessInfo> {
    if root <= 1 || !table.iter().any(|p| p.pid == root) {
        return Vec::new();
    }
    let by_pid: HashMap<u32, usize> = table.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, p) in table.iter().enumerate() {
        if let Some(parent) = p.ppid {
            if parent != p.pid {
                children.entry(parent).or_default().push(i);
            }
        }
    }
    let mut seen: HashSet<u32> = HashSet::new();
    let mut out: Vec<ProcessInfo> = Vec::new();
    let mut queue: Vec<u32> = vec![root];
    queue.extend(table.iter().filter(|p| p.pgid == Some(root)).map(|p| p.pid));
    while let Some(pid) = queue.pop() {
        if out.len() >= MAX_TREE_PROCESSES {
            break;
        }
        if !seen.insert(pid) {
            continue;
        }
        if let Some(&i) = by_pid.get(&pid) {
            out.push(table[i]);
        }
        if let Some(kids) = children.get(&pid) {
            queue.extend(kids.iter().map(|&i| table[i].pid));
        }
    }
    out
}

/// What a set of processes used. As a tick it is instantaneous (peak = value); as a sample or a bucket it is the
/// average with the peak next to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    /// CPU time used, per mille of the whole machine's capacity (all cores = 1000), averaged.
    pub cpu_permille: u32,
    /// The highest CPU share seen.
    pub cpu_peak_permille: u32,
    /// Resident memory, averaged.
    pub mem_bytes: u64,
    /// The highest resident memory seen.
    pub mem_peak_bytes: u64,
    /// Most processes counted at one moment.
    pub processes: u32,
}

impl Usage {
    /// An instantaneous reading.
    pub fn tick(cpu_permille: u32, mem_bytes: u64, processes: u32) -> Self {
        Usage {
            cpu_permille,
            cpu_peak_permille: cpu_permille,
            mem_bytes,
            mem_peak_bytes: mem_bytes,
            processes,
        }
    }

    /// The readings of several runners at the same moment added up.
    pub fn sum<'a>(items: impl IntoIterator<Item = &'a Usage>) -> Usage {
        let mut t = Usage::default();
        for u in items {
            t.cpu_permille = t.cpu_permille.saturating_add(u.cpu_permille);
            t.cpu_peak_permille = t.cpu_peak_permille.saturating_add(u.cpu_peak_permille);
            t.mem_bytes = t.mem_bytes.saturating_add(u.mem_bytes);
            t.mem_peak_bytes = t.mem_peak_bytes.saturating_add(u.mem_peak_bytes);
            t.processes = t.processes.saturating_add(u.processes);
        }
        t
    }
}

/// Accumulates [`Usage`] values into their average and peaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UsageAcc {
    n: u64,
    cpu: u64,
    cpu_peak: u32,
    mem: u128,
    mem_peak: u64,
    processes: u32,
}

impl UsageAcc {
    /// Adds a value.
    pub fn add(&mut self, u: &Usage) {
        self.n += 1;
        self.cpu += u64::from(u.cpu_permille);
        self.cpu_peak = self.cpu_peak.max(u.cpu_peak_permille).max(u.cpu_permille);
        self.mem += u128::from(u.mem_bytes);
        self.mem_peak = self.mem_peak.max(u.mem_peak_bytes).max(u.mem_bytes);
        self.processes = self.processes.max(u.processes);
    }

    /// Whether nothing was added.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The average with the peaks; all zero when empty.
    pub fn mean(&self) -> Usage {
        let n = self.n.max(1);
        Usage {
            cpu_permille: u32::try_from(self.cpu / n).unwrap_or(u32::MAX),
            cpu_peak_permille: self.cpu_peak,
            mem_bytes: u64::try_from(self.mem / u128::from(n)).unwrap_or(u64::MAX),
            mem_peak_bytes: self.mem_peak,
            processes: self.processes,
        }
    }

    /// The average, and a fresh accumulator in its place.
    pub fn take(&mut self) -> Option<Usage> {
        let out = (!self.is_empty()).then(|| self.mean());
        *self = UsageAcc::default();
        out
    }
}

/// Turns cumulative CPU times into a share of the machine: one tracker per runner.
#[derive(Debug, Clone, Default)]
pub struct UsageTracker {
    previous: HashMap<u32, u64>,
    at_ms: Option<u64>,
    /// Each process's share of the machine at the last reading, per mille.
    shares: HashMap<u32, u32>,
}

impl UsageTracker {
    /// A tracker with no history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the history (the runner stopped); the next reading has no CPU share.
    pub fn reset(&mut self) {
        self.previous.clear();
        self.at_ms = None;
        self.shares.clear();
    }

    /// The CPU share (per mille of the machine) of one process at the last reading; 0 when unknown.
    pub fn share_of(&self, pid: u32) -> u32 {
        self.shares.get(&pid).copied().unwrap_or(0)
    }

    /// The use of `members` at `now_ms` on a machine with `cpus` logical cores.
    ///
    /// CPU is the CPU time the members gained since the previous reading, divided by `elapsed × cpus`. A process
    /// seen for the first time counts all of its CPU time (it started within the interval), except in the very
    /// first reading, which only sets the baseline. Processes that exited between readings take their last
    /// interval's time with them: that is a small under-count, preferred to guessing.
    pub fn observe(&mut self, now_ms: u64, members: &[ProcessInfo], cpus: usize) -> Usage {
        let mem: u64 = members.iter().fold(0, |a, p| a.saturating_add(p.rss_bytes));
        let processes = u32::try_from(members.len()).unwrap_or(u32::MAX);
        let cpu = match self.at_ms {
            Some(before) if now_ms > before => {
                let gained: u64 = members.iter().fold(0u64, |a, p| {
                    a.saturating_add(p.cpu_time_ms.saturating_sub(self.previous.get(&p.pid).copied().unwrap_or(0)))
                });
                let capacity = u128::from(now_ms - before) * cpus.max(1) as u128;
                u32::try_from((u128::from(gained) * 1000 / capacity).min(1000)).unwrap_or(1000)
            }
            _ => 0,
        };
        self.shares = match self.at_ms {
            Some(before) if now_ms > before => {
                let capacity = u128::from(now_ms - before) * cpus.max(1) as u128;
                members
                    .iter()
                    .map(|p| {
                        let gained = p.cpu_time_ms.saturating_sub(self.previous.get(&p.pid).copied().unwrap_or(0));
                        (
                            p.pid,
                            u32::try_from((u128::from(gained) * 1000 / capacity).min(1000)).unwrap_or(1000),
                        )
                    })
                    .collect()
            }
            _ => HashMap::new(),
        };
        if self.at_ms.is_none_or(|before| now_ms > before) {
            self.at_ms = Some(now_ms);
            self.previous = members.iter().map(|p| (p.pid, p.cpu_time_ms)).collect();
        }
        Usage::tick(cpu, mem, processes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: Option<u32>, pgid: Option<u32>, cpu: u64, rss: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid,
            pgid,
            cpu_time_ms: cpu,
            rss_bytes: rss,
        }
    }

    fn pids(v: &[ProcessInfo]) -> Vec<u32> {
        let mut x: Vec<u32> = v.iter().map(|p| p.pid).collect();
        x.sort_unstable();
        x
    }

    #[test]
    fn the_tree_is_descendants_and_group_members() {
        let table = [
            p(1, None, None, 0, 0),
            p(100, Some(1), Some(100), 0, 10),
            p(101, Some(100), Some(100), 0, 10),
            p(102, Some(101), Some(102), 0, 10), // grandchild in its own group
            p(103, Some(1), Some(100), 0, 10),   // reparented to init, still in the runner's group
            p(104, Some(103), Some(104), 0, 10), // child of that orphan
            p(200, Some(1), Some(200), 0, 10),   // another runner
            p(201, Some(200), Some(200), 0, 10),
        ];
        assert_eq!(pids(&tree_members(100, &table)), vec![100, 101, 102, 103, 104]);
        assert_eq!(pids(&tree_members(200, &table)), vec![200, 201]);
    }

    #[test]
    fn a_missing_or_system_root_has_no_members() {
        let table = [p(1, None, None, 0, 0), p(5, Some(1), None, 0, 0)];
        assert!(tree_members(999, &table).is_empty());
        assert!(tree_members(1, &table).is_empty());
        assert!(tree_members(0, &table).is_empty());
        assert!(tree_members(5, &[]).is_empty());
    }

    #[test]
    fn cycles_and_self_parents_terminate() {
        let table = [
            p(10, Some(10), None, 0, 0),
            p(11, Some(12), None, 0, 0),
            p(12, Some(11), None, 0, 0),
        ];
        assert_eq!(pids(&tree_members(10, &table)), vec![10]);
        let loop_table = [
            p(10, Some(12), None, 0, 0),
            p(11, Some(10), None, 0, 0),
            p(12, Some(11), None, 0, 0),
        ];
        assert_eq!(pids(&tree_members(10, &loop_table)), vec![10, 11, 12]);
    }

    #[test]
    fn the_tree_is_bounded() {
        let mut table = vec![p(10, Some(1), None, 0, 0)];
        for i in 0..(MAX_TREE_PROCESSES as u32 + 100) {
            table.push(p(1000 + i, Some(10), None, 0, 0));
        }
        assert_eq!(tree_members(10, &table).len(), MAX_TREE_PROCESSES);
    }

    #[test]
    fn cpu_is_a_share_of_the_machine() {
        let mut t = UsageTracker::new();
        let first = t.observe(0, &[p(10, None, None, 5_000, 100), p(11, None, None, 1_000, 50)], 4);
        assert_eq!((first.cpu_permille, first.mem_bytes, first.processes), (0, 150, 2), "baseline only");
        // 5 s later: process 10 gained 5 s (one core of four = 25%), process 11 gained 1 s (5%).
        let u = t.observe(5_000, &[p(10, None, None, 10_000, 120), p(11, None, None, 2_000, 60)], 4);
        assert_eq!(u.cpu_permille, 300);
        assert_eq!(u.cpu_peak_permille, 300);
        assert_eq!(u.mem_bytes, 180);
        assert_eq!(u.mem_peak_bytes, 180);
    }

    #[test]
    fn new_processes_count_and_exited_ones_drop_out() {
        let mut t = UsageTracker::new();
        t.observe(0, &[p(10, None, None, 100, 1)], 2);
        // A compiler appears with 4 s of CPU in a 5 s interval on 2 cores: 40%, plus 0 for the runner.
        let u = t.observe(5_000, &[p(10, None, None, 100, 1), p(20, None, None, 4_000, 500)], 2);
        assert_eq!((u.cpu_permille, u.processes, u.mem_bytes), (400, 2, 501));
        // It exits: gone from the count, and a new process reusing nothing else starts from zero again.
        let u = t.observe(10_000, &[p(10, None, None, 100, 1)], 2);
        assert_eq!((u.cpu_permille, u.processes, u.mem_bytes), (0, 1, 1));
    }

    #[test]
    fn cpu_is_clamped_and_time_going_backwards_is_ignored() {
        let mut t = UsageTracker::new();
        t.observe(10_000, &[p(10, None, None, 0, 1)], 1);
        let u = t.observe(11_000, &[p(10, None, None, 50_000, 1)], 1);
        assert_eq!(u.cpu_permille, 1000, "never above the machine");
        let same = t.observe(11_000, &[p(10, None, None, 99_000, 1)], 1);
        assert_eq!(same.cpu_permille, 0, "no time passed");
        let back = t.observe(5_000, &[p(10, None, None, 99_000, 1)], 1);
        assert_eq!(back.cpu_permille, 0, "clock went back");
        // A counter that went backwards (pid reuse) is not negative CPU.
        let after = t.observe(12_000, &[p(10, None, None, 10, 1)], 1);
        assert_eq!(after.cpu_permille, 0);
        // Zero cores is treated as one.
        let mut z = UsageTracker::new();
        z.observe(0, &[p(1, None, None, 0, 0)], 0);
        assert_eq!(z.observe(1_000, &[p(1, None, None, 500, 0)], 0).cpu_permille, 500);
    }

    #[test]
    fn reset_forgets_the_baseline() {
        let mut t = UsageTracker::new();
        t.observe(0, &[p(10, None, None, 0, 1)], 1);
        t.reset();
        assert_eq!(t.observe(5_000, &[p(10, None, None, 4_000, 1)], 1).cpu_permille, 0);
    }

    #[test]
    fn accumulation_averages_and_keeps_peaks() {
        let mut a = UsageAcc::default();
        assert!(a.is_empty());
        assert_eq!(a.mean(), Usage::default());
        assert_eq!(a.take(), None);
        a.add(&Usage::tick(100, 1000, 2));
        a.add(&Usage::tick(300, 3000, 5));
        let u = a.mean();
        assert_eq!(
            (u.cpu_permille, u.cpu_peak_permille, u.mem_bytes, u.mem_peak_bytes, u.processes),
            (200, 300, 2000, 3000, 5)
        );
        assert_eq!(a.take(), Some(u));
        assert!(a.is_empty(), "taking starts a new period");
    }

    #[test]
    fn sums_add_up_and_saturate() {
        let a = Usage::tick(100, 10, 1);
        let b = Usage::tick(250, 20, 3);
        assert_eq!(Usage::sum([&a, &b]), Usage::tick(350, 30, 4));
        assert_eq!(Usage::sum([]), Usage::default());
        let big = Usage::tick(u32::MAX, u64::MAX, u32::MAX);
        assert_eq!(Usage::sum([&big, &big]), big);
    }

    #[test]
    fn old_json_without_new_fields_loads() {
        let u: Usage = serde_json::from_str("{}").unwrap();
        assert_eq!(u, Usage::default());
    }

    #[test]
    fn each_process_gets_its_own_share() {
        let mut t = UsageTracker::new();
        let first = [p(10, None, None, 1000, 5), p(11, Some(10), None, 0, 5)];
        t.observe(0, &first, 4);
        assert_eq!(t.share_of(10), 0, "the first reading only sets the baseline");
        // 1 s later on 4 cores: process 10 used 2 s of CPU (500 per mille of the machine), 11 used 1 s, 12 is new.
        let second = [
            p(10, None, None, 3000, 5),
            p(11, Some(10), None, 1000, 5),
            p(12, Some(10), None, 400, 5),
        ];
        let u = t.observe(1000, &second, 4);
        assert_eq!(t.share_of(10), 500);
        assert_eq!(t.share_of(11), 250);
        assert_eq!(t.share_of(12), 100);
        assert_eq!(t.share_of(99), 0);
        assert_eq!(u.cpu_permille, 850, "the shares add up to the total");
        t.reset();
        assert_eq!(t.share_of(10), 0);
    }
}
