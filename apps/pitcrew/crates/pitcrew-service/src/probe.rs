//! What the service looks up about processes and the machine: whether a runner has job processes, whether a runner
//! binary already runs outside Pitcrew, the machine's CPU, memory and free disk, and the service's own cost.
//!
//! One `sysinfo` snapshot is refreshed per supervision tick (every few seconds), not per question.

use pitcrew_core::jobs::is_job_process;
use pitcrew_core::metrics::{permille, MachineSample};
use pitcrew_core::time::Timestamp;
use pitcrew_core::usage::{ProcessInfo, Usage};
use std::path::Path;
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

/// The process and machine snapshot.
pub struct Probe {
    sys: System,
    disks: Disks,
    cpus: usize,
}

impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe {
    /// A probe with a first snapshot.
    pub fn new() -> Self {
        let sys = System::new_with_specifics(RefreshKind::nothing());
        let mut p = Probe {
            sys,
            disks: Disks::new_with_refreshed_list(),
            cpus: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        };
        p.refresh_processes();
        p
    }

    /// Refreshes the process list (names, parents, executables, CPU).
    pub fn refresh_processes(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cpu()
                .with_memory(),
        );
    }

    /// Logical cores of the machine (the denominator of every CPU share).
    pub fn cpus(&self) -> usize {
        self.cpus
    }

    /// The process table as of the last refresh: ids, parents, process groups, CPU time and resident memory.
    pub fn process_table(&self) -> Vec<ProcessInfo> {
        self.sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcessInfo {
                pid: pid.as_u32(),
                ppid: p.parent().map(Pid::as_u32),
                pgid: process_group(pid.as_u32()),
                cpu_time_ms: p.accumulated_cpu_time(),
                rss_bytes: p.memory(),
            })
            .collect()
    }

    /// Whether `pid` has child processes that belong to a job (its console host does not).
    pub fn has_job_children(&self, pid: u32) -> bool {
        let parent = Pid::from_u32(pid);
        self.sys
            .processes()
            .values()
            .any(|p| p.parent() == Some(parent) && is_job_process(&p.name().to_string_lossy()))
    }

    /// A running process (other than `except`) whose executable is `exe`.
    pub fn running_binary(&self, exe: &Path, except: Option<u32>) -> Option<u32> {
        let want = normalise(exe);
        self.sys
            .processes()
            .iter()
            .filter(|(pid, _)| Some(pid.as_u32()) != except)
            .find(|(_, p)| p.exe().is_some_and(|e| normalise(e) == want))
            .map(|(pid, _)| pid.as_u32())
    }

    /// A machine sample (without the runners' share, which the supervisor adds); `disk_of` names a folder on the disk to report.
    pub fn machine(&mut self, now: Timestamp, disk_of: &Path) -> MachineSample {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let cpu = permille(self.sys.global_cpu_usage() / 100.0);
        let (free, total) = self.disk_space(disk_of).unwrap_or((0, 0));
        MachineSample {
            at: now,
            cpu_permille: cpu,
            cpu_peak_permille: cpu,
            mem_used_bytes: self.sys.used_memory(),
            mem_total_bytes: self.sys.total_memory(),
            disk_free_bytes: free,
            disk_total_bytes: total,
            runners: Usage::default(),
        }
    }

    /// Free and total bytes of the disk holding `path`.
    pub fn disk_space(&mut self, path: &Path) -> Option<(u64, u64)> {
        self.disks.refresh(true);
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let target = normalise(&target);
        self.disks
            .list()
            .iter()
            .filter(|d| target.starts_with(&normalise(d.mount_point())))
            .max_by_key(|d| d.mount_point().as_os_str().len())
            .map(|d| (d.available_space(), d.total_space()))
    }

    /// The service's own CPU (per mille of the whole machine) and memory.
    pub fn own_usage(&self) -> (u32, u64) {
        let me = Pid::from_u32(std::process::id());
        let cpus = self.sys.cpus().len().max(1) as f32;
        self.sys
            .process(me)
            .map(|p| (permille(p.cpu_usage() / 100.0 / cpus), p.memory()))
            .unwrap_or((0, 0))
    }
}

/// The process group of `pid` (Unix); `None` when it is gone or the platform has no groups.
#[cfg(unix)]
fn process_group(pid: u32) -> Option<u32> {
    let pid = i32::try_from(pid).ok()?;
    nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(pid)))
        .ok()
        .and_then(|g| u32::try_from(g.as_raw()).ok())
}

/// Windows has no process groups; the tree is the parent-pid tree there.
#[cfg(not(unix))]
fn process_group(_pid: u32) -> Option<u32> {
    None
}

/// A path for comparison: `\\?\` dropped, separators unified, case-insensitive on Windows.
fn normalise(p: &Path) -> String {
    let s = p.display().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_its_own_process_and_the_machine() {
        let mut p = Probe::new();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(p.running_binary(&exe, None), Some(std::process::id()));
        assert_eq!(p.running_binary(&exe, Some(std::process::id())), None);
        assert!(!p.has_job_children(u32::MAX));
        let m = p.machine(Timestamp(1), &std::env::temp_dir());
        assert!(m.mem_total_bytes > 0);
        assert!(m.disk_total_bytes > 0, "{m:?}");
        let (_, mem) = p.own_usage();
        assert!(mem > 0);
        assert!(p.cpus() >= 1);
        let me = std::process::id();
        let mine = p
            .process_table()
            .into_iter()
            .find(|r| r.pid == me)
            .expect("this process is in the table");
        assert!(mine.rss_bytes > 0 && mine.ppid.is_some());
        if cfg!(unix) {
            assert!(mine.pgid.is_some());
        }
    }

    #[test]
    fn paths_compare_loosely() {
        assert_eq!(normalise(Path::new(r"\\?\C:\x\y")), normalise(Path::new("C:/x/y")));
    }
}
