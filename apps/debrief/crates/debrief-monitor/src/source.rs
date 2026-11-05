//! Where process measurements come from. The tracker depends on [`ProcessSource`] only,
//! so tests inject scripted samples; [`SysinfoSource`] is the real adapter.

use crate::config::Targets;
use std::collections::{HashMap, HashSet};
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

/// A process's parent pid and when the process started (seconds since the epoch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub parent: u32,
    pub started: u64,
}

/// Adds descendants of matched processes (`links` maps pid to its parent link) until stable. A pid can be handed on, so a
/// process that started before the present holder of its parent pid is not that holder's child and is not followed
/// (ADR 0033: parent links are read with start times).
pub fn expand_children(matched: &mut HashSet<u32>, links: &HashMap<u32, Link>) {
    loop {
        let before = matched.len();
        for (pid, link) in links {
            if matched.contains(&link.parent) && links.get(&link.parent).is_none_or(|p| link.started >= p.started) {
                matched.insert(*pid);
            }
        }
        if matched.len() == before {
            return;
        }
    }
}

/// [`ProcessSource`] backed by `sysinfo`; refreshes only matched processes in detail.
pub struct SysinfoSource {
    sys: System,
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
            sys: System::new(),
            seen: HashSet::new(),
        }
    }

    fn matched_pids(&mut self, targets: &Targets, include_children: bool) -> Vec<Pid> {
        if let (Some(pids), false) = (targets.only_pids(), include_children) {
            let pids: Vec<Pid> = pids.into_iter().map(Pid::from_u32).collect();
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&pids),
                true,
                ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
            );
            return pids.into_iter().filter(|p| self.sys.process(*p).is_some()).collect();
        }
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        let mut matched: HashSet<u32> = HashSet::new();
        let mut links = HashMap::new();
        for (pid, p) in self.sys.processes() {
            // Linux lists every thread as a process that repeats its process's whole resident size: only real
            // processes are monitored (otherwise a 400-thread process reads as 400 processes of the full size).
            if p.thread_kind().is_some() {
                continue;
            }
            let exe = p.exe().map(|e| e.to_string_lossy());
            if targets.matches(pid.as_u32(), &p.name().to_string_lossy(), exe.as_deref()) {
                matched.insert(pid.as_u32());
            }
            if let Some(parent) = p.parent() {
                links.insert(
                    pid.as_u32(),
                    Link {
                        parent: parent.as_u32(),
                        started: p.start_time(),
                    },
                );
            }
        }
        if include_children {
            expand_children(&mut matched, &links);
        }
        matched.into_iter().map(Pid::from_u32).collect()
    }
}

impl ProcessSource for SysinfoSource {
    fn cpu_count(&self) -> usize {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
    }

    fn host_name(&self) -> String {
        System::host_name().unwrap_or_else(|| "localhost".into())
    }

    fn sample(&mut self, targets: &Targets, include_children: bool) -> Vec<ProcSample> {
        let pids = self.matched_pids(targets, include_children);
        let kind = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet);
        self.sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, kind);
        let mut out = Vec::new();
        let mut alive = HashSet::new();
        for pid in pids {
            let Some(p) = self.sys.process(pid) else { continue };
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

    #[test]
    fn children_are_found_transitively() {
        let links = HashMap::from([
            (1, link(0, 10)),
            (2, link(1, 20)),
            (3, link(2, 30)),
            (4, link(99, 40)),
            (5, link(4, 50)),
        ]);
        let mut m = HashSet::from([1]);
        expand_children(&mut m, &links);
        assert_eq!(m, HashSet::from([1, 2, 3]));
    }

    fn link(parent: u32, started: u64) -> Link {
        Link { parent, started }
    }

    /// A process that started before the pid it names as parent was handed to its present holder is not that holder's child.
    #[test]
    fn children_older_than_their_parent_are_skipped() {
        // 2's parent pid 1 now belongs to a process that started (at 100) after 2 did (at 20): the pid was handed on.
        let links = HashMap::from([(1, link(0, 100)), (2, link(1, 20)), (3, link(1, 150)), (4, link(3, 160))]);
        let mut m = HashSet::from([1]);
        expand_children(&mut m, &links);
        assert_eq!(m, HashSet::from([1, 3, 4]));
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
