//! Where process measurements come from. The tracker depends on [`ProcessSource`] only,
//! so tests inject scripted samples; [`SysinfoSource`] is the real adapter.

use crate::config::Targets;
use std::collections::HashSet;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// One measurement of one process.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcSample {
    pub pid: u32,
    pub name: String,
    pub exe: Option<String>,
    /// CPU in "per core" units (200 = two cores busy). `None` until a baseline exists.
    pub cpu_core_percent: Option<f32>,
    pub working_set: u64,
    pub virtual_bytes: u64,
    pub private_bytes: Option<u64>,
    pub threads: Option<u32>,
    pub handles: Option<u32>,
    /// Bytes read / written since the previous sample.
    pub read_bytes: Option<u64>,
    pub write_bytes: Option<u64>,
    pub uptime_sec: u64,
    /// The process exists but its details could not be read.
    pub access_denied: bool,
}

/// Port: yields a sample for every monitored process currently alive.
pub trait ProcessSource {
    /// Logical CPU count, used to normalise CPU to the whole machine.
    fn cpu_count(&self) -> usize;
    /// Name of this machine.
    fn host_name(&self) -> String;
    fn sample(&mut self, targets: &Targets, include_children: bool) -> Vec<ProcSample>;
}

/// Adds descendants of matched processes until stable, following only the links `table` vouches for
/// ([`paddock_proc::parent_of`]): a pid can be handed on, so a process that started before the present holder of its parent pid
/// is not that holder's child (ADR 0033), and a process or parent whose start time is not known is not followed.
pub fn expand_children(matched: &mut HashSet<u32>, table: &paddock_proc::Table) {
    loop {
        let before = matched.len();
        for &pid in table.keys() {
            if paddock_proc::parent_of(table, pid).is_some_and(|parent| matched.contains(&parent)) {
                matched.insert(pid);
            }
        }
        if matched.len() == before {
            return;
        }
    }
}

/// [`ProcessSource`] backed by `sysinfo`; refreshes only matched processes in detail.
pub struct SysinfoSource {
    /// `None` when sysinfo could not start (logged once): nothing is then sampled.
    sys: Option<System>,
    /// PIDs measured before: CPU and IO deltas are meaningless on the first sight.
    seen: HashSet<u32>,
}

impl Default for SysinfoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SysinfoSource {
    pub fn new() -> Self {
        SysinfoSource {
            sys: System::new()
                .inspect_err(|why| log::warn!("the OS process table is unavailable (sysinfo): {why}; no process is sampled"))
                .ok(),
            seen: HashSet::new(),
        }
    }

    fn matched_pids(&mut self, targets: &Targets, include_children: bool) -> Vec<Pid> {
        let Some(sys) = self.sys.as_mut() else {
            return Vec::new();
        };
        if let (Some(pids), false) = (targets.only_pids(), include_children) {
            let pids: Vec<Pid> = pids.into_iter().map(Pid::from_u32).collect();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&pids),
                true,
                ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
            );
            return pids.into_iter().filter(|p| sys.process(*p).is_some()).collect();
        }
        let listed_at = paddock_proc::listing_started();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        let mut matched: HashSet<u32> = HashSet::new();
        for (pid, p) in sys.processes() {
            // Linux lists every thread as a process that repeats its process's whole resident size: only real
            // processes are monitored (otherwise a 400-thread process reads as 400 processes of the full size).
            if p.thread_kind().is_some() {
                continue;
            }
            let exe = p.exe().map(|e| e.to_string_lossy());
            if targets.matches(pid.as_u32(), &p.name().to_string_lossy(), exe.as_deref()) {
                matched.insert(pid.as_u32());
            }
        }
        if include_children {
            // Each process's start time is read once and paired with its listed parent (one start-time read per process, only
            // when children are asked for).
            let table = paddock_proc::table_of_listing(listed_at, sys.processes().values().map(paddock_proc::Listed::of));
            expand_children(&mut matched, &table);
        }
        matched.into_iter().map(Pid::from_u32).collect()
    }
}

impl ProcessSource for SysinfoSource {
    fn cpu_count(&self) -> usize {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
    }

    fn host_name(&self) -> String {
        System::host_name().unwrap_or_else(|_| "localhost".into())
    }

    fn sample(&mut self, targets: &Targets, include_children: bool) -> Vec<ProcSample> {
        let pids = self.matched_pids(targets, include_children);
        let kind = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet);
        let Some(sys) = self.sys.as_mut() else {
            return Vec::new();
        };
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, kind);
        let mut out = Vec::new();
        let mut alive = HashSet::new();
        for pid in pids {
            let Some(p) = sys.process(pid) else { continue };
            let id = pid.as_u32();
            alive.insert(id);
            let known = self.seen.contains(&id);
            let exe = p.exe().map(|e| e.to_string_lossy().into_owned());
            let disk = p.disk_usage();
            out.push(ProcSample {
                pid: id,
                name: p.name().to_string_lossy().into_owned(),
                access_denied: exe.is_none() && p.memory() == 0,
                exe,
                cpu_core_percent: known.then(|| p.cpu_usage()),
                working_set: p.memory(),
                virtual_bytes: p.virtual_memory(),
                private_bytes: None,
                threads: p.tasks().map(|t| t.len() as u32),
                handles: None,
                read_bytes: known.then_some(disk.read_bytes),
                write_bytes: known.then_some(disk.written_bytes),
                uptime_sec: p.run_time(),
            });
        }
        self.seen = alive;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use paddock_proc::{table_from, ProcEntry, StartTime, Table};

    /// `(pid, parent, started in microseconds)` rows; `None` for a start time that is not known.
    fn table(rows: &[(u32, u32, Option<u64>)]) -> Table {
        table_from(rows.iter().map(|&(pid, parent, started)| ProcEntry {
            pid,
            parent: Some(parent),
            rss: 1,
            started: started.map(StartTime::from_micros),
            is_thread: false,
        }))
    }

    #[test]
    fn children_are_found_transitively() {
        let t = table(&[
            (1, 0, Some(10)),
            (2, 1, Some(20)),
            (3, 2, Some(30)),
            (4, 99, Some(40)),
            (5, 4, Some(50)),
        ]);
        let mut m = HashSet::from([1]);
        expand_children(&mut m, &t);
        assert_eq!(m, HashSet::from([1, 2, 3]));
    }

    /// A matched pid whose start time is not known cannot be checked against its children: they are not followed.
    #[test]
    fn children_of_a_parent_without_a_start_time_are_not_followed() {
        let t = table(&[(9, 0, None), (2, 9, Some(20))]);
        let mut m = HashSet::from([9]);
        expand_children(&mut m, &t);
        assert_eq!(m, HashSet::from([9]));
    }

    /// A process that started before the pid it names as parent was handed to its present holder is not that holder's child.
    #[test]
    fn children_older_than_their_parent_are_skipped() {
        // 2's parent pid 1 now belongs to a process that started (at 100) after 2 did (at 20): the pid was handed on.
        let t = table(&[(1, 0, Some(100)), (2, 1, Some(20)), (3, 1, Some(150)), (4, 3, Some(160))]);
        let mut m = HashSet::from([1]);
        expand_children(&mut m, &t);
        assert_eq!(m, HashSet::from([1, 3, 4]));
    }

    /// Regression (ADR 0033, CI run 8694's class): whole seconds hid an orphan that started in the same second as the new holder
    /// of its parent's pid, just before it; the check now compares the exact start times paddock-proc reads.
    #[test]
    fn an_orphan_from_the_same_second_as_the_new_holder_is_not_its_child() {
        let t = table(&[(1, 0, Some(100_400_000)), (2, 1, Some(100_200_000)), (3, 1, Some(100_600_000))]);
        let mut m = HashSet::from([1]);
        expand_children(&mut m, &t);
        assert_eq!(m, HashSet::from([1, 3]));
    }

    /// Linux lists threads as processes; each would repeat the process's whole resident size.
    #[cfg(target_os = "linux")]
    #[test]
    fn threads_are_not_sampled_as_processes() {
        use crate::config::Target;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let threads: Vec<_> = (0..20)
            .map(|_| {
                let s = stop.clone();
                std::thread::spawn(move || {
                    while !s.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                })
            })
            .collect();
        let me = std::process::id();
        let tids: Vec<u32> = std::fs::read_dir("/proc/self/task")
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
            .filter(|t| *t != me)
            .collect();
        let all = Targets::new(vec![Target::Name(globset::Glob::new("*").unwrap().compile_matcher())]);
        let samples = SysinfoSource::new().sample(&all, false);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for t in threads {
            let _ = t.join();
        }
        assert!(samples.iter().any(|p| p.pid == me));
        assert!(samples.iter().all(|p| !tids.contains(&p.pid)), "a thread was sampled as a process");
    }
}
