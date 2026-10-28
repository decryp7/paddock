//! The process table through `sysinfo`, kept cheap: each tick refreshes only CPU, memory and disk counters (exe
//! path and user are read once per process). Details refresh one process with its command line and working
//! directory, and an OS-specific [`DetailsProbe`] adds priority, modules, connections, signature and so on.

use crate::nt::{ApiCounters, NtError, NtProcess, NtState, NtTracker};
use crate::parse::time::{same_start, started_from_unix_secs};
#[cfg(windows)]
use crate::readability::classify_session;
#[cfg(windows)]
use crate::readability::classify_sid;
#[cfg(unix)]
use crate::readability::classify_uid;
use crate::readability::{apply_nt, as_not_supported, first_user_uid, measure, Os, Raw, SubIds};
use crate::sensors::rate;
use dyno_core::ports::{PlatformError, ProcessSource};
use dyno_core::process::Owner;
use dyno_core::process::{ListOrReason, Measured, ParentLink, Pid, ProcessDetails, ProcessRef, ProcessSample, RunState, SignatureInfo};
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

/// How an account is told apart from Dyno's own: the numeric user id on Unix, the SID text on Windows.
#[cfg(unix)]
type OwnUser = u32;
#[cfg(windows)]
type OwnUser = String;

#[cfg(unix)]
fn own_user_of(u: &sysinfo::Uid) -> OwnUser {
    **u
}
#[cfg(windows)]
fn own_user_of(u: &sysinfo::Uid) -> OwnUser {
    u.to_string()
}

/// The subordinate id ranges of this Linux machine (`/etc/subuid`, `/etc/subgid`); none elsewhere.
fn sub_ids() -> SubIds {
    if !cfg!(target_os = "linux") {
        return SubIds::none();
    }
    let read = |path: &str| {
        use std::io::Read;
        let mut text = String::new();
        // Small files; bounded anyway.
        std::fs::File::open(path).ok()?.take(1 << 20).read_to_string(&mut text).ok()?;
        Some(text)
    };
    let (uids, gids) = (read("/etc/subuid"), read("/etc/subgid"));
    SubIds::parse([uids.as_deref(), gids.as_deref()].into_iter().flatten())
}

/// The text of `/etc/login.defs` on Linux, where it says where ordinary user ids start.
fn login_defs() -> Option<String> {
    if cfg!(target_os = "linux") {
        std::fs::read_to_string("/etc/login.defs").ok()
    } else {
        None
    }
}

/// What macOS still gives for a process whose full BSD info was refused.
#[derive(Debug, Default, Clone, Copy)]
struct BsdFallback {
    uid: Option<u32>,
    parent: Option<u32>,
    start_secs: u64,
}

/// For macOS, the short BSD info and the sysctl start time of a process sysinfo could not describe (another user's); other
/// systems have nothing to add.
fn bsd_fallback(p: &Process) -> BsdFallback {
    #[cfg(target_os = "macos")]
    {
        let pid = p.pid().as_u32();
        let missing = p.user_id().is_none() || p.parent().is_none() || p.start_time() == 0;
        if !missing {
            return BsdFallback::default();
        }
        let short = crate::macos::procinfo::short_info(pid);
        BsdFallback {
            uid: short.map(|s| s.uid),
            parent: short.map(|s| s.parent).filter(|pp| *pp != 0),
            start_secs: if p.start_time() == 0 {
                crate::macos::procinfo::start_secs(pid).unwrap_or(0)
            } else {
                0
            },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = p;
        BsdFallback::default()
    }
}

/// The deepest parent chain reported (guards against cycles from reused pids).
const MAX_PARENT_DEPTH: usize = 64;
/// The user list is re-read at most this often when an unknown user id shows up.
const USERS_REFRESH: Duration = Duration::from_secs(60);

/// Where the system process list comes from (`NtQuerySystemInformation` on Windows; a fake in tests).
pub trait NtSource: Send {
    /// One snapshot of every process.
    fn snapshot(&mut self) -> Result<Vec<NtProcess>, NtError>;
    /// Dyno's own process as the documented per-process APIs report it, for the start-up self-check.
    fn own_counters(&mut self) -> Option<ApiCounters>;
}

struct NtDriver {
    source: Box<dyn NtSource>,
    tracker: NtTracker,
}

/// The common process source.
pub struct SysinfoProcesses {
    sys: System,
    users: Users,
    users_read: Instant,
    last: Option<Instant>,
    cores: f32,
    own_user: Option<OwnUser>,
    #[cfg_attr(windows, allow(dead_code))]
    first_user: u32,
    #[cfg_attr(windows, allow(dead_code))]
    sub_ids: SubIds,
    probe: Box<dyn DetailsProbe>,
    nt: Option<NtDriver>,
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
            own_user: None,
            first_user: first_user_uid(Os::this(), login_defs().as_deref()),
            sub_ids: sub_ids(),
            probe,
            nt: None,
        }
    }

    /// Adds the system process list: it supplies the values of processes this user cannot open, once its fields pass the
    /// self-check against the documented APIs.
    pub fn with_nt(mut self, source: Box<dyn NtSource>) -> Self {
        self.nt = Some(NtDriver {
            source,
            tracker: NtTracker::new(std::process::id()),
        });
        self
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

    #[cfg(unix)]
    fn user_name_of_uid(&self, uid: u32) -> Option<String> {
        self.users.list().iter().find(|u| **u.id() == uid).map(|u| u.name().to_string())
    }

    #[cfg(windows)]
    fn user_name_of_uid(&self, _uid: u32) -> Option<String> {
        None
    }

    fn owner_of(&self, p: &Process, fallback_uid: Option<u32>) -> Owner {
        #[cfg(unix)]
        {
            classify_uid(
                p.user_id().map(|u| **u).or(fallback_uid),
                self.own_user,
                self.first_user,
                &self.sub_ids,
            )
        }
        #[cfg(windows)]
        {
            let _ = fallback_uid;
            if let Some(nt) = &self.nt {
                if let (Some(own), Some(session)) = (nt.tracker.own_session(), nt.tracker.session_of(p.pid().as_u32())) {
                    return classify_session(session, own);
                }
            }
            let sid = p.user_id().map(|u| u.to_string());
            classify_sid(sid.as_deref(), self.own_user.as_deref())
        }
    }

    fn sample_of(&self, p: &Process, secs: f64) -> ProcessSample {
        let disk = p.disk_usage();
        let mut raw = Raw {
            cpu: normalise_cpu(p.cpu_usage(), self.cores),
            memory: p.memory(),
            disk_read: rate(disk.read_bytes, secs),
            disk_write: rate(disk.written_bytes, secs),
            start_secs: p.start_time(),
            user: self.user_name(p),
        };
        let extra = bsd_fallback(p);
        if raw.start_secs == 0 {
            raw.start_secs = extra.start_secs;
        }
        if raw.user.is_none() {
            raw.user = extra.uid.and_then(|uid| self.user_name_of_uid(uid));
        }
        let owner = self.owner_of(p, extra.uid);
        let mut from_list = false;
        if let Some(nt) = &self.nt {
            from_list = apply_nt(&mut raw, owner, nt.tracker.reading(p.pid().as_u32()));
        }
        let mut m = measure(Os::this(), owner, &raw, started_from_unix_secs);
        if !from_list && matches!(self.nt.as_ref().map(|n| n.tracker.state()), Some(NtState::Disabled(_))) {
            m = as_not_supported(m);
        }
        ProcessSample {
            pid: p.pid().as_u32(),
            parent: p.parent().map(|pp| pp.as_u32()).or(extra.parent),
            name: p.name().to_string_lossy().into_owned(),
            exe: p.exe().map(|e| e.to_string_lossy().into_owned()).filter(|e| !e.is_empty()),
            user: m.user,
            cpu: m.cpu,
            memory: m.memory,
            disk_read_bps: m.disk_read,
            disk_write_bps: m.disk_write,
            started: m.started,
            owner,
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
        if let Some(nt) = &mut self.nt {
            let snapshot = nt.source.snapshot();
            let api = (*nt.tracker.state() == NtState::Unchecked)
                .then(|| nt.source.own_counters())
                .flatten();
            if let Some(message) = nt.tracker.update(snapshot, secs, self.cores, api) {
                eprintln!("{message}");
            }
        }
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
        if self.own_user.is_none() {
            self.own_user = sysinfo::get_current_pid()
                .ok()
                .and_then(|me| self.sys.process(me))
                .and_then(|p| p.user_id())
                .map(own_user_of);
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
        if p.thread_kind().is_some()
            || target
                .started
                .is_some_and(|t| !same_start(started_from_unix_secs(p.start_time()), t))
        {
            return Err(gone(target.pid));
        }
        let mut sample = self.sample_of(p, 0.0);
        // Keep the last tick's rates rather than zeros.
        if sample.cpu.copied().is_some() {
            sample.cpu = Measured::Value(normalise_cpu(p.cpu_usage(), self.cores));
        }
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

    /// A process list that answers with a fixed table.
    struct FixedList {
        table: Result<Vec<NtProcess>, NtError>,
        api: Option<ApiCounters>,
    }

    impl NtSource for FixedList {
        fn snapshot(&mut self) -> Result<Vec<NtProcess>, NtError> {
            self.table.clone()
        }
        fn own_counters(&mut self) -> Option<ApiCounters> {
            self.api
        }
    }

    fn own_record(ws: u64) -> NtProcess {
        NtProcess {
            pid: std::process::id(),
            parent: 1,
            session: 1,
            create_time: 1,
            user_time: 0,
            kernel_time: 0,
            working_set: ws,
            working_set_private: ws,
            read_bytes: 0,
            write_bytes: 0,
        }
    }

    #[test]
    fn the_system_process_list_is_checked_against_the_documented_counters_before_it_is_used() {
        let api = ApiCounters {
            cpu_time: 0,
            working_set: 1 << 20,
            read_bytes: 0,
            write_bytes: 0,
        };
        let mut good = SysinfoProcesses::new(Box::new(NoDetails)).with_nt(Box::new(FixedList {
            table: Ok(vec![own_record(1 << 20)]),
            api: Some(api),
        }));
        let listed = good.processes();
        assert!(!listed.is_empty());
        assert_eq!(good.nt.as_ref().unwrap().tracker.state(), &NtState::Active);
        let mut bad = SysinfoProcesses::new(Box::new(NoDetails)).with_nt(Box::new(FixedList {
            table: Ok(vec![own_record(1 << 40)]),
            api: Some(api),
        }));
        let listed = bad.processes();
        assert!(matches!(bad.nt.as_ref().unwrap().tracker.state(), NtState::Disabled(_)));
        let mine = listed.iter().find(|p| p.pid == std::process::id()).unwrap();
        assert!(mine.memory.copied().is_some(), "Dyno's own values come from its handle either way");
        let mut failing = SysinfoProcesses::new(Box::new(NoDetails)).with_nt(Box::new(FixedList {
            table: Err(NtError::Malformed),
            api: None,
        }));
        assert!(!failing.processes().is_empty());
        assert!(matches!(failing.nt.as_ref().unwrap().tracker.state(), NtState::Disabled(_)));
    }

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
        assert_eq!(mine.owner, Owner::Own, "the test process runs as the account that runs it");
        assert!(mine.started.copied().unwrap().0 > 0);
        assert!(mine.exe.is_some());
        let second = s.processes();
        assert!(second.iter().all(|p| p.cpu.copied().is_none_or(|c| (0.0..=100.0).contains(&c))));
        let d = s.details(mine.reference()).unwrap();
        assert_eq!(d.process.pid, me);
        assert!(!d.command_line.is_empty());
        assert!(!d.parents.is_empty(), "the test runner has a parent");
        // A reused pid (another start time) is not this process.
        let wrong = ProcessRef {
            pid: me,
            started: mine.started.copied().map(|t| t.plus(60_000)),
        };
        assert!(matches!(s.details(wrong), Err(PlatformError::NotFound(_))));
        let missing = ProcessRef {
            pid: u32::MAX - 7,
            started: mine.started.copied(),
        };
        assert!(matches!(s.details(missing), Err(PlatformError::NotFound(_))));
    }
}
