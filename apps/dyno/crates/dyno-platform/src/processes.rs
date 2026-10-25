//! The process table through `sysinfo`, kept cheap: each tick refreshes only CPU, memory and disk counters (exe
//! path and user are read once per process). Details refresh one process with its command line and working
//! directory, and an OS-specific [`DetailsProbe`] adds priority, modules, connections, signature and so on.

use crate::parse::time::{same_start, started_from_unix_secs};
use crate::sensors::rate;
use dyno_core::ports::{PlatformError, ProcessSource};
use dyno_core::process::{ListOrReason, ParentLink, Pid, ProcessDetails, ProcessRef, ProcessSample, RunState, SignatureInfo};
use std::path::Path;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Process, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind, Users};

/// Adds OS-specific details to what the common source read.
pub trait DetailsProbe: Send {
    /// Fills in `details` for `target` (whose executable is `exe`, when known). Failures become unavailable
    /// fields, never errors: the common part is already useful.
    fn enrich(&mut self, target: ProcessRef, exe: Option<&Path>, details: &mut ProcessDetails);
}

/// A probe that adds nothing; lists say why.
pub struct NoDetails;

impl DetailsProbe for NoDetails {
    fn enrich(&mut self, _target: ProcessRef, _exe: Option<&Path>, _details: &mut ProcessDetails) {}
}

/// The deepest parent chain reported (guards against cycles from reused pids).
const MAX_PARENT_DEPTH: usize = 64;
/// The user list is re-read at most this often when an unknown user id shows up.
const USERS_REFRESH: Duration = Duration::from_secs(60);

/// The common process source.
pub struct SysinfoProcesses {
    sys: System,
    users: Users,
    users_read: Instant,
    last: Option<Instant>,
    cores: f32,
    probe: Box<dyn DetailsProbe>,
}

impl SysinfoProcesses {
    /// A source with an OS probe.
    pub fn new(probe: Box<dyn DetailsProbe>) -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_list(CpuRefreshKind::nothing());
        let cores = sys.cpus().len().max(1) as f32;
        SysinfoProcesses {
            sys,
            users: Users::new_with_refreshed_list(),
            users_read: Instant::now(),
            last: None,
            cores,
            probe,
        }
    }

    fn tick_kind() -> ProcessRefreshKind {
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet)
            .without_tasks()
    }

    fn user_name(&self, p: &Process) -> Option<String> {
        let uid = p.user_id()?;
        self.users
            .get_user_by_id(uid)
            .map(|u| u.name().to_string())
            .or_else(|| well_known_account(&uid.to_string()).map(str::to_string))
    }

    fn sample_of(&self, p: &Process, secs: f64) -> ProcessSample {
        let disk = p.disk_usage();
        ProcessSample {
            pid: p.pid().as_u32(),
            parent: p.parent().map(|pp| pp.as_u32()),
            name: p.name().to_string_lossy().into_owned(),
            exe: p.exe().map(|e| e.to_string_lossy().into_owned()).filter(|e| !e.is_empty()),
            user: self.user_name(p),
            cpu: normalise_cpu(p.cpu_usage(), self.cores),
            memory: p.memory(),
            disk_read_bps: rate(disk.read_bytes, secs),
            disk_write_bps: rate(disk.written_bytes, secs),
            started: started_from_unix_secs(p.start_time()),
            state: run_state(p.status()),
        }
    }

    fn parents(&self, p: &Process) -> Vec<ParentLink> {
        let mut out = Vec::new();
        let mut child_start = p.start_time();
        let mut next = p.parent();
        while let Some(pp) = next {
            if out.len() >= MAX_PARENT_DEPTH || out.iter().any(|l: &ParentLink| l.pid == pp.as_u32()) {
                break;
            }
            let Some(parent) = self.sys.process(pp) else { break };
            // A "parent" that started after its child is a reused pid, not the real parent.
            if parent.start_time() > child_start {
                break;
            }
            out.push(ParentLink {
                pid: pp.as_u32(),
                name: parent.name().to_string_lossy().into_owned(),
            });
            child_start = parent.start_time();
            next = parent.parent();
        }
        out
    }
}

impl ProcessSource for SysinfoProcesses {
    fn processes(&mut self) -> Vec<ProcessSample> {
        let now = Instant::now();
        let secs = self.last.map(|l| now.duration_since(l).as_secs_f64()).unwrap_or(0.0);
        self.last = Some(now);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, Self::tick_kind());
        let unknown_user = self
            .sys
            .processes()
            .values()
            .filter_map(Process::user_id)
            .any(|u| self.users.get_user_by_id(u).is_none() && well_known_account(&u.to_string()).is_none());
        if unknown_user && self.users_read.elapsed() >= USERS_REFRESH {
            self.users.refresh();
            self.users_read = Instant::now();
        }
        let mut out: Vec<ProcessSample> = self.sys.processes().values().map(|p| self.sample_of(p, secs)).collect();
        out.sort_by_key(|p| p.pid);
        out
    }

    fn details(&mut self, target: ProcessRef) -> Result<ProcessDetails, PlatformError> {
        let pid = sysinfo::Pid::from_u32(target.pid);
        let kind = ProcessRefreshKind::nothing()
            .with_memory()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet)
            .with_tasks();
        self.sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
        let p = self.sys.process(pid).ok_or_else(|| gone(target.pid))?;
        // sysinfo lists Linux threads as processes; a thread id is never a target.
        if p.thread_kind().is_some() || !same_start(started_from_unix_secs(p.start_time()), target.started) {
            return Err(gone(target.pid));
        }
        let mut sample = self.sample_of(p, 0.0);
        // Keep the last tick's rates rather than zeros.
        sample.cpu = normalise_cpu(p.cpu_usage(), self.cores);
        let exe = p.exe().map(Path::to_path_buf);
        let not_here = || "Not available on this OS".to_string();
        let mut details = ProcessDetails {
            process: sample,
            command_line: p.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect(),
            cwd: p.cwd().map(|c| c.to_string_lossy().into_owned()).filter(|c| !c.is_empty()),
            priority: None,
            affinity: None,
            efficiency_mode: None,
            threads: p.tasks().map(|t| u32::try_from(t.len()).unwrap_or(u32::MAX)),
            handles: p.open_files().map(|n| u32::try_from(n).unwrap_or(u32::MAX)),
            parents: self.parents(p),
            signature: SignatureInfo::Unknown(not_here()),
            open_files: ListOrReason::Unavailable(not_here()),
            connections: ListOrReason::Unavailable(not_here()),
            modules: ListOrReason::Unavailable(not_here()),
            startup_source: None,
        };
        self.probe.enrich(target, exe.as_deref(), &mut details);
        Ok(details)
    }
}

/// The error for a process that exited (or whose pid now belongs to another process).
pub(crate) fn gone(pid: Pid) -> PlatformError {
    PlatformError::NotFound(format!("Process {pid} has exited."))
}

/// sysinfo's per-core CPU percentage as a percentage of the whole machine.
pub fn normalise_cpu(per_core_pct: f32, cores: f32) -> f32 {
    if !per_core_pct.is_finite() || cores <= 0.0 {
        return 0.0;
    }
    (per_core_pct / cores).clamp(0.0, 100.0)
}

/// Maps sysinfo's status.
pub fn run_state(s: ProcessStatus) -> RunState {
    match s {
        ProcessStatus::Run => RunState::Running,
        ProcessStatus::Sleep | ProcessStatus::Idle => RunState::Sleeping,
        ProcessStatus::Stop => RunState::Suspended,
        _ => RunState::Other,
    }
}

/// Names of Windows service accounts that are not in the local user list.
pub fn well_known_account(sid: &str) -> Option<&'static str> {
    match sid {
        "S-1-5-18" => Some("SYSTEM"),
        "S-1-5-19" => Some("LOCAL SERVICE"),
        "S-1-5-20" => Some("NETWORK SERVICE"),
        "0" => Some("root"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mappings() {
        assert_eq!(normalise_cpu(400.0, 8.0), 50.0);
        assert_eq!(normalise_cpu(f32::NAN, 8.0), 0.0);
        assert_eq!(normalise_cpu(10.0, 0.0), 0.0);
        assert_eq!(normalise_cpu(1e6, 2.0), 100.0);
        assert_eq!(run_state(ProcessStatus::Run), RunState::Running);
        assert_eq!(run_state(ProcessStatus::Sleep), RunState::Sleeping);
        assert_eq!(run_state(ProcessStatus::Idle), RunState::Sleeping);
        assert_eq!(run_state(ProcessStatus::Stop), RunState::Suspended);
        assert_eq!(run_state(ProcessStatus::Zombie), RunState::Other);
        assert_eq!(well_known_account("S-1-5-18"), Some("SYSTEM"));
        assert_eq!(well_known_account("S-1-5-19"), Some("LOCAL SERVICE"));
        assert_eq!(well_known_account("S-1-5-20"), Some("NETWORK SERVICE"));
        assert_eq!(well_known_account("0"), Some("root"));
        assert_eq!(well_known_account("S-1-5-21-1"), None);
    }

    #[test]
    fn lists_this_machine_and_details_of_itself() {
        let mut s = SysinfoProcesses::new(Box::new(NoDetails));
        let first = s.processes();
        assert!(first.len() > 3);
        let me = std::process::id();
        let mine = first.iter().find(|p| p.pid == me).expect("own process listed");
        assert!(mine.started.0 > 0);
        assert!(mine.exe.is_some());
        let second = s.processes();
        assert!(second.iter().all(|p| (0.0..=100.0).contains(&p.cpu)));
        let d = s.details(mine.reference()).unwrap();
        assert_eq!(d.process.pid, me);
        assert!(!d.command_line.is_empty());
        assert!(!d.parents.is_empty(), "the test runner has a parent");
        // A reused pid (another start time) is not this process.
        let wrong = ProcessRef {
            pid: me,
            started: mine.started.plus(60_000),
        };
        assert!(matches!(s.details(wrong), Err(PlatformError::NotFound(_))));
        let missing = ProcessRef {
            pid: u32::MAX - 7,
            started: mine.started,
        };
        assert!(matches!(s.details(missing), Err(PlatformError::NotFound(_))));
    }
}
