//! What the service looks up about processes and the machine: whether a runner has job processes, whether a runner
//! binary already runs outside Pitcrew, the machine's CPU, memory and free disk, and the service's own cost.
//!
//! One `sysinfo` snapshot is refreshed per supervision tick (every few seconds), not per question.

use pitcrew_core::jobs::is_job_process;
use pitcrew_core::metrics::{permille, MachineSample};
use pitcrew_core::process_track::ProcessMeta;
use pitcrew_core::time::Timestamp;
use pitcrew_core::usage::{ProcessInfo, Usage, UsageTracker};
use std::path::Path;

/// Most processes of one runner described with names and command lines per tick (the rest still count in the usage).
pub const MAX_DESCRIBED: usize = 512;
use pitcrew_core::disk_guard::SpaceCache;
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

/// The process and machine snapshot.
pub struct Probe {
    /// `None` when sysinfo could not start (logged once): the process table is then empty and the machine reads zero.
    sys: Option<System>,
    /// `None` when the volume list could not be read (logged): free space is then unknown.
    disks: Option<Disks>,
    space: SpaceCache,
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
        let sys = System::new_with_specifics(RefreshKind::nothing())
            .inspect_err(|why| {
                eprintln!("pitcrew-service: warning: the OS process table is unavailable (sysinfo): {why}; no process is seen")
            })
            .ok();
        let disks = Disks::new_with_refreshed_list()
            .inspect_err(|why| {
                eprintln!("pitcrew-service: warning: the volume list is unavailable (sysinfo): {why}; free space is unknown")
            })
            .ok();
        let mut p = Probe {
            sys,
            disks,
            space: SpaceCache::default(),
            cpus: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        };
        p.refresh_processes();
        p
    }

    /// Refreshes the process list (names, parents, executables, CPU).
    pub fn refresh_processes(&mut self) {
        let Some(sys) = self.sys.as_mut() else { return };
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cpu()
                .with_memory(),
        );
    }

    /// The processes of the last refresh, without threads: on Linux sysinfo lists every thread (task) as an entry of
    /// its own, with the process's executable and parent, so a thread id could be taken for a process id (and
    /// signalled, adopted or counted twice).
    fn processes(&self) -> impl Iterator<Item = (&Pid, &sysinfo::Process)> {
        // Threads are not processes (Linux lists them), and a zombie has already exited: it only waits for its parent
        // to reap it (in a container without an init it may never be), so it uses nothing and must never be stopped
        // or counted again.
        self.sys
            .iter()
            .flat_map(|sys| sys.processes().iter())
            .filter(|(_, p)| p.thread_kind().is_none() && p.status() != sysinfo::ProcessStatus::Zombie)
    }

    /// Logical cores of the machine (the denominator of every CPU share).
    pub fn cpus(&self) -> usize {
        self.cpus
    }

    /// The process table as of the last refresh: ids, parents, process groups, CPU time and resident memory.
    pub fn process_table(&self) -> Vec<ProcessInfo> {
        self.processes()
            .map(|(pid, p)| ProcessInfo {
                pid: pid.as_u32(),
                ppid: p.parent().map(Pid::as_u32),
                pgid: process_group(pid.as_u32()),
                cpu_time_ms: p.accumulated_cpu_time(),
                rss_bytes: p.memory(),
            })
            .collect()
    }

    /// Names, executables, command lines and start times of the runner's `members` (the runner itself first), at most
    /// [`MAX_DESCRIBED`]. Command lines are read once per process (`OnlyIfNotSet`), for these processes only.
    pub fn process_metas(&mut self, runner_pid: u32, members: &[ProcessInfo], usage: &UsageTracker) -> Vec<ProcessMeta> {
        let mut ordered: Vec<&ProcessInfo> = members.iter().collect();
        ordered.sort_by_key(|p| (p.pid != runner_pid, p.pid));
        ordered.truncate(MAX_DESCRIBED);
        let pids: Vec<Pid> = ordered.iter().map(|p| Pid::from_u32(p.pid)).collect();
        let Some(sys) = self.sys.as_mut() else {
            return Vec::new();
        };
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            false,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );
        ordered
            .into_iter()
            .map(|info| {
                let p = sys.process(Pid::from_u32(info.pid));
                let start = p.map(sysinfo::Process::start_time).filter(|s| *s > 0);
                ProcessMeta {
                    pid: info.pid,
                    ppid: info.ppid,
                    name: p.map_or_else(String::new, |p| p.name().to_string_lossy().into_owned()),
                    exe: p.and_then(|p| p.exe()).map(|e| e.display().to_string()),
                    command: p.map_or_else(String::new, |p| {
                        p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")
                    }),
                    started_ms: start.map(|s| s.saturating_mul(1000)),
                    cpu_permille: usage.share_of(info.pid),
                    mem_bytes: info.rss_bytes,
                }
            })
            .collect()
    }

    /// Whether the process `pid` (started at `started_ms` when known) still runs. A zombie that nobody has reaped
    /// yet does not count: it is dead, only its entry is left.
    // ADR 0033 WP-6: a missing start time counts as a match; compare start times or use paddock_proc::ProcessId
    pub fn alive(&mut self, pid: u32, started_ms: Option<u64>) -> bool {
        let id = Pid::from_u32(pid);
        // Unknown counts as alive: a runner is never restarted because the process table could not be read.
        let Some(sys) = self.sys.as_mut() else {
            return true;
        };
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[id]), true, ProcessRefreshKind::nothing());
        match sys.process(id) {
            None => false,
            Some(p) => {
                if p.status() == sysinfo::ProcessStatus::Zombie {
                    return false;
                }
                let start = p.start_time().saturating_mul(1000);
                // The same process: start times are whole seconds, so allow rounding.
                started_ms.is_none_or(|s| start == 0 || s.abs_diff(start) <= 1500)
            }
        }
    }

    /// Whether `pid` has child processes that belong to a job (its console host does not).
    // ADR 0033 WP-6: children found by parent pid without start times; use paddock_proc::descendants
    pub fn has_job_children(&self, pid: u32) -> bool {
        let parent = Pid::from_u32(pid);
        self.processes()
            .any(|(_, p)| p.parent() == Some(parent) && is_job_process(&p.name().to_string_lossy()))
    }

    /// A running process (other than `except`) whose executable is `exe`.
    pub fn running_binary(&self, exe: &Path, except: Option<u32>) -> Option<u32> {
        let want = normalise(exe);
        self.processes()
            .filter(|(pid, _)| Some(pid.as_u32()) != except)
            .find(|(_, p)| p.exe().is_some_and(|e| normalise(e) == want))
            .map(|(pid, _)| pid.as_u32())
    }

    /// A machine sample (without the runners' share, which the supervisor adds); `disk_of` names a folder on the disk to report.
    pub fn machine(&mut self, now: Timestamp, disk_of: &Path) -> MachineSample {
        let (cpu, mem_used_bytes, mem_total_bytes) = match self.sys.as_mut() {
            Some(sys) => {
                sys.refresh_cpu_usage();
                sys.refresh_memory();
                (permille(sys.global_cpu_usage() / 100.0), sys.used_memory(), sys.total_memory())
            }
            None => (permille(0.0), 0, 0),
        };
        let (free, total) = self.disk_space(disk_of, now).unwrap_or((0, 0));
        MachineSample {
            at: now,
            cpu_permille: cpu,
            cpu_peak_permille: cpu,
            mem_used_bytes,
            mem_total_bytes,
            disk_free_bytes: free,
            disk_total_bytes: total,
            runners: Usage::default(),
        }
    }

    /// Free and total bytes of the disk holding `path`. The OS is asked at most once a minute per disk
    /// ([`SpaceCache`]); in between the last reading is returned.
    pub fn disk_space(&mut self, path: &Path, now: Timestamp) -> Option<(u64, u64)> {
        let disks = self.disks.as_mut()?;
        let key = path.to_string_lossy().to_string();
        self.space.get(&key, now, || {
            disks.refresh(true);
            let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let target = normalise(&target);
            disks
                .iter()
                .filter(|d| target.starts_with(&normalise(d.mount_point())))
                .max_by_key(|d| d.mount_point().as_os_str().len())
                .map(|d| (d.available_space(), d.total_space()))
        })
    }

    /// The service's own CPU (per mille of the whole machine) and memory.
    pub fn own_usage(&self) -> (u32, u64) {
        let me = Pid::from_u32(std::process::id());
        let Some(sys) = self.sys.as_ref() else {
            return (0, 0);
        };
        let cpus = sys.cpus().len().max(1) as f32;
        sys.process(me)
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
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn sees_its_own_process_and_the_machine() {
        let mut p = Probe::new();
        // Other processes may run this test binary at the same time (nextest runs every test in its own process), so
        // the binary is found running, and the excluded process is never the answer.
        let exe = std::env::current_exe().unwrap();
        assert!(p.running_binary(&exe, None).is_some());
        assert_ne!(p.running_binary(&exe, Some(std::process::id())), Some(std::process::id()));
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

    #[cfg(target_os = "linux")]
    #[test]
    fn threads_are_not_processes() {
        // Regression: on Linux sysinfo lists each thread as an entry with the same exe; `running_binary` returned a
        // thread id and the usage tree counted a process once per thread.
        let (stop, rx) = std::sync::mpsc::channel::<()>();
        let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let rx = rx.clone();
                std::thread::spawn(move || {
                    let _ = rx.lock().map(|r| r.recv());
                })
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let me = std::process::id();
        let tids: Vec<u32> = std::fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .filter(|t| *t != me)
            .collect();
        assert!(tids.len() >= 4, "{tids:?}");
        let probe = Probe::new();
        let table = probe.process_table();
        assert_eq!(table.iter().filter(|p| p.pid == me).count(), 1);
        assert!(table.iter().all(|p| !tids.contains(&p.pid)), "a thread is listed as a process");
        let exe = std::env::current_exe().unwrap();
        let found = probe.running_binary(&exe, None).unwrap();
        assert!(!tids.contains(&found), "{found} is a thread id");
        drop(stop);
        for t in threads {
            let _ = t.join();
        }
    }
}
