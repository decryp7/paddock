//! Acting on processes: end, suspend/resume (every thread, via a Toolhelp snapshot), priority, affinity, efficiency
//! mode (EcoQoS + idle priority, as Task Manager does) and hard CPU caps through a named job object per target with
//! CPU rate control. Every action first opens the process and checks its start time ([`open_process`]).
//!
//! A process can't leave a job, so releasing a cap turns the job's rate control off. What outlives Dyno (caps and
//! suspensions) is recorded in a [`ControlState`] file; `release_all` undoes it, including what a crashed earlier
//! run left behind.

use super::handle::{denied, failed, is_win32, open_process, Owned};
use crate::control_state::ControlState;
use dyno_core::ports::{PlatformError, ProcessControl};
use dyno_core::process::{Pid, Priority, ProcessAction, ProcessRef};
use std::collections::HashMap;
use std::path::PathBuf;
use windows::core::{BOOL, HSTRING};
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_HANDLE_OPTIONS, ERROR_ACCESS_DENIED, HANDLE, HWND, LPARAM, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectCpuRateControlInformation, OpenJobObjectW,
    SetInformationJobObject, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0, JOB_OBJECT_CPU_RATE_CONTROL,
    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetPriorityClass, GetProcessInformation, OpenThread, ProcessPowerThrottling, ResumeThread, SetPriorityClass,
    SetProcessAffinityMask, SetProcessInformation, SuspendThread, TerminateProcess, WaitForSingleObject, ABOVE_NORMAL_PRIORITY_CLASS,
    BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_CREATION_FLAGS,
    PROCESS_DUP_HANDLE, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, REALTIME_PRIORITY_CLASS, THREAD_SUSPEND_RESUME,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE};

/// How long `End` waits for windows to close before terminating.
const CLOSE_GRACE_MS: u32 = 2_000;
/// The most times one thread is resumed to undo stacked suspensions.
const MAX_RESUME_ROUNDS: u32 = 128;
/// Job object access: query.
const JOB_QUERY: u32 = 0x4;
/// Job object access: set attributes.
const JOB_SET_ATTRIBUTES: u32 = 0x2;

/// The Windows priority class for a priority.
pub fn priority_class(p: Priority) -> PROCESS_CREATION_FLAGS {
    match p {
        Priority::Idle => IDLE_PRIORITY_CLASS,
        Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        Priority::Normal => NORMAL_PRIORITY_CLASS,
        Priority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
        Priority::High => HIGH_PRIORITY_CLASS,
    }
}

/// The priority for a Windows priority class (realtime counts as high).
pub fn priority_of_class(class: u32) -> Option<Priority> {
    let c = PROCESS_CREATION_FLAGS(class);
    Some(if c == IDLE_PRIORITY_CLASS {
        Priority::Idle
    } else if c == BELOW_NORMAL_PRIORITY_CLASS {
        Priority::BelowNormal
    } else if c == NORMAL_PRIORITY_CLASS {
        Priority::Normal
    } else if c == ABOVE_NORMAL_PRIORITY_CLASS {
        Priority::AboveNormal
    } else if c == HIGH_PRIORITY_CLASS || c == REALTIME_PRIORITY_CLASS {
        Priority::High
    } else {
        return None;
    })
}

/// The name of the job object that caps `target`.
pub fn job_name(target: ProcessRef) -> String {
    format!("Local\\Dyno-Cap-{}-{}", target.pid, target.started.0)
}

/// The job's CPU rate for a cap in percent of the machine (units of 1/100 %), 1–100 %.
pub fn cpu_rate(pct: u8) -> u32 {
    u32::from(pct.clamp(1, 100)) * 100
}

/// The ids of the threads of `pid`.
pub fn threads_of(pid: Pid) -> Result<Vec<u32>, PlatformError> {
    // SAFETY: a system-wide thread snapshot; the handle is owned below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|e| PlatformError::Failed(format!("Could not list threads: {}", e.message())))?;
    let snap = Owned(snap);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut out = Vec::new();
    // SAFETY: `entry` is a valid THREADENTRY32 with dwSize set; the snapshot handle is live.
    let mut more = unsafe { Thread32First(snap.raw(), &mut entry) }.is_ok();
    while more {
        if entry.th32OwnerProcessID == pid {
            out.push(entry.th32ThreadID);
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snap.raw(), &mut entry) }.is_ok();
    }
    Ok(out)
}

/// Suspends (or resumes) every thread of `pid`. Fails only when no thread could be changed.
fn for_each_thread(pid: Pid, suspend: bool) -> Result<(), PlatformError> {
    let threads = threads_of(pid)?;
    let (mut changed, mut refused) = (0usize, 0usize);
    for tid in threads {
        // SAFETY: OpenThread has no pointer arguments; the handle is owned.
        let Ok(h) = (unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, tid) }) else {
            refused += 1;
            continue;
        };
        let h = Owned(h);
        // SAFETY: `h` is a live thread handle with suspend/resume rights.
        let r = unsafe {
            if suspend {
                SuspendThread(h.raw())
            } else {
                // Resume fully: the previous count says how often the thread was suspended (by anyone).
                let mut prev = ResumeThread(h.raw());
                let mut rounds = 0;
                while prev > 1 && prev != u32::MAX && rounds < MAX_RESUME_ROUNDS {
                    prev = ResumeThread(h.raw());
                    rounds += 1;
                }
                prev
            }
        };
        if r == u32::MAX {
            refused += 1;
        } else {
            changed += 1;
        }
    }
    match (changed, refused) {
        (0, 0) => Err(crate::processes::gone(pid)),
        (0, _) => Err(denied(pid)),
        _ => Ok(()),
    }
}

struct WindowSearch {
    pid: Pid,
    windows: Vec<HWND>,
}

extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the address of the `WindowSearch` passed to EnumWindows, alive for the whole call.
    let search = unsafe { &mut *(lparam.0 as *mut WindowSearch) };
    let mut pid = 0u32;
    // SAFETY: `hwnd` comes from EnumWindows; `pid` is a valid out-pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: as above.
    if pid == search.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() && search.windows.len() < 256 {
        search.windows.push(hwnd);
    }
    BOOL(1)
}

/// Asks the process's visible top-level windows to close. Returns whether it had any.
fn ask_to_close(pid: Pid) -> bool {
    let mut search = WindowSearch { pid, windows: Vec::new() };
    // SAFETY: the callback only touches `search`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(collect_window), LPARAM(&mut search as *mut WindowSearch as isize)) };
    for w in &search.windows {
        // SAFETY: posting WM_CLOSE to a window handle (a stale one fails harmlessly).
        let _ = unsafe { PostMessageW(Some(*w), WM_CLOSE, Default::default(), Default::default()) };
    }
    !search.windows.is_empty()
}

fn set_power_throttling(h: &Owned, pid: Pid, on: bool) -> Result<(), PlatformError> {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        // Off hands the decision back to Windows, as Task Manager does.
        ControlMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
    };
    // SAFETY: `state` is a valid PROCESS_POWER_THROTTLING_STATE of the size passed; `h` has set-information rights.
    unsafe {
        SetProcessInformation(
            h.raw(),
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .map_err(|e| failed(pid, "change efficiency mode of", &e))
}

/// Whether EcoQoS (efficiency mode) is explicitly on for the process behind `h` (needs query rights).
pub fn efficiency_mode(h: &Owned) -> Option<bool> {
    let mut st = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ..Default::default()
    };
    // SAFETY: `st` is a valid struct of the size passed, with its version set as the API requires.
    unsafe {
        GetProcessInformation(
            h.raw(),
            ProcessPowerThrottling,
            &mut st as *mut _ as *mut core::ffi::c_void,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .ok()?;
    Some(st.ControlMask & st.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0)
}

fn set_rate(job: &Owned, pid: Pid, flags: JOB_OBJECT_CPU_RATE_CONTROL, rate: u32) -> Result<(), PlatformError> {
    let info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
        ControlFlags: flags,
        Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { CpuRate: rate },
    };
    // SAFETY: `info` is a valid struct of the size passed; the job handle has set-attributes rights.
    unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectCpuRateControlInformation,
            &info as *const _ as *const core::ffi::c_void,
            size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
        )
    }
    .map_err(|e| failed(pid, "cap the CPU of", &e))
}

/// A job's name exists only while some handle to the job is open, so a crashed Dyno would leave an unreachable
/// cap. Giving the capped process itself a query-only handle to its job keeps the name (and so `release_all` of a
/// later run) working for as long as the process lives. Best effort: without the right, the cap still works.
fn keep_name_alive(target: ProcessRef, job: &Owned) {
    let Ok(t) = open_process(target, PROCESS_DUP_HANDLE) else { return };
    let mut theirs = HANDLE::default();
    // SAFETY: duplicating our live job handle into the target process with query-only access; the target owns the
    // new handle (it is closed when the process exits).
    let _ = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            job.raw(),
            t.raw(),
            &mut theirs,
            JOB_QUERY,
            false,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
    };
}

/// Turns off the cap of a named job left by any run, if the job still exists.
fn release_named_job(target: ProcessRef) {
    // SAFETY: opening a named object; the handle is owned.
    if let Ok(h) = unsafe { OpenJobObjectW(JOB_SET_ATTRIBUTES, false, &HSTRING::from(job_name(target))) } {
        let _ = set_rate(&Owned(h), target.pid, JOB_OBJECT_CPU_RATE_CONTROL(0), 0);
    }
}

/// [`ProcessControl`] for Windows.
pub struct WinControl {
    state: ControlState,
    /// Leftovers of earlier runs, released by the first `release_all`.
    leftovers: Vec<crate::control_state::Applied>,
    jobs: HashMap<ProcessRef, Owned>,
    priority_before_eco: HashMap<ProcessRef, u32>,
}

impl WinControl {
    /// Control keeping its state in `state_file` (what earlier runs left there is released by `release_all`).
    pub fn new(state_file: PathBuf) -> Self {
        Self::with_state(ControlState::load(state_file))
    }

    /// Control with a given state (tests).
    pub fn with_state(state: ControlState) -> Self {
        let leftovers = state.entries().to_vec();
        WinControl {
            state,
            leftovers,
            jobs: HashMap::new(),
            priority_before_eco: HashMap::new(),
        }
    }

    fn end(&mut self, target: ProcessRef) -> Result<(), PlatformError> {
        let h = open_process(target, PROCESS_TERMINATE | PROCESS_SYNCHRONIZE)?;
        if ask_to_close(target.pid) {
            // SAFETY: `h` has SYNCHRONIZE rights.
            if unsafe { WaitForSingleObject(h.raw(), CLOSE_GRACE_MS) } == WAIT_OBJECT_0 {
                self.forget(target);
                return Ok(());
            }
        }
        // SAFETY: `h` has terminate rights.
        match unsafe { TerminateProcess(h.raw(), 1) } {
            Ok(()) => {
                // SAFETY: as above; wait briefly so the caller sees it gone.
                unsafe { WaitForSingleObject(h.raw(), 1_000) };
                self.forget(target);
                Ok(())
            }
            Err(e) if is_win32(&e, ERROR_ACCESS_DENIED) => {
                // Exited between the close request and now?
                // SAFETY: as above, no wait.
                if unsafe { WaitForSingleObject(h.raw(), 0) } == WAIT_OBJECT_0 {
                    self.forget(target);
                    Ok(())
                } else {
                    Err(denied(target.pid))
                }
            }
            Err(e) => Err(failed(target.pid, "end", &e)),
        }
    }

    fn forget(&mut self, target: ProcessRef) {
        self.jobs.remove(&target);
        self.priority_before_eco.remove(&target);
        self.state.update(target, |a| {
            a.capped = false;
            a.suspended = false;
        });
    }

    fn suspend(&mut self, target: ProcessRef, on: bool) -> Result<(), PlatformError> {
        let _h = open_process(target, Default::default())?;
        let already = self.state.entries().iter().any(|a| a.target() == target && a.suspended);
        if on && already {
            return Ok(());
        }
        for_each_thread(target.pid, on)?;
        self.state.update(target, |a| a.suspended = on);
        Ok(())
    }

    fn priority(&mut self, target: ProcessRef, p: Priority) -> Result<(), PlatformError> {
        let h = open_process(target, PROCESS_SET_INFORMATION)?;
        // SAFETY: `h` has set-information rights.
        unsafe { SetPriorityClass(h.raw(), priority_class(p)) }.map_err(|e| failed(target.pid, "change the priority of", &e))
    }

    fn affinity(&mut self, target: ProcessRef, mask: u64) -> Result<(), PlatformError> {
        let mask = usize::try_from(mask).map_err(|_| PlatformError::Unsupported("Too many cores for this build.".into()))?;
        if mask == 0 {
            return Err(PlatformError::Failed("Choose at least one core.".into()));
        }
        let h = open_process(target, PROCESS_SET_INFORMATION)?;
        // SAFETY: `h` has set-information rights.
        unsafe { SetProcessAffinityMask(h.raw(), mask) }.map_err(|e| failed(target.pid, "change the cores of", &e))
    }

    fn efficiency(&mut self, target: ProcessRef, on: bool) -> Result<(), PlatformError> {
        let h = open_process(target, PROCESS_SET_INFORMATION)?;
        if on {
            // SAFETY: `h` has query rights.
            let before = unsafe { GetPriorityClass(h.raw()) };
            set_power_throttling(&h, target.pid, true)?;
            // SAFETY: `h` has set-information rights.
            unsafe { SetPriorityClass(h.raw(), IDLE_PRIORITY_CLASS) }.map_err(|e| failed(target.pid, "change the priority of", &e))?;
            if before != 0 && PROCESS_CREATION_FLAGS(before) != IDLE_PRIORITY_CLASS {
                self.priority_before_eco.entry(target).or_insert(before);
            }
        } else {
            set_power_throttling(&h, target.pid, false)?;
            let restore = self
                .priority_before_eco
                .remove(&target)
                .map(PROCESS_CREATION_FLAGS)
                .unwrap_or(NORMAL_PRIORITY_CLASS);
            // SAFETY: as above.
            unsafe { SetPriorityClass(h.raw(), restore) }.map_err(|e| failed(target.pid, "change the priority of", &e))?;
        }
        Ok(())
    }

    fn cap(&mut self, target: ProcessRef, pct: Option<u8>) -> Result<(), PlatformError> {
        let Some(pct) = pct else {
            let _h = open_process(target, Default::default())?;
            match self.jobs.get(&target) {
                Some(job) => set_rate(job, target.pid, JOB_OBJECT_CPU_RATE_CONTROL(0), 0)?,
                None => release_named_job(target),
            }
            self.jobs.remove(&target);
            self.state.update(target, |a| a.capped = false);
            return Ok(());
        };
        let h = open_process(target, PROCESS_SET_QUOTA | PROCESS_TERMINATE)?;
        if let std::collections::hash_map::Entry::Vacant(slot) = self.jobs.entry(target) {
            // SAFETY: creates (or opens, if a crashed run left it) the named job; the handle is owned.
            let job = unsafe { CreateJobObjectW(None, &HSTRING::from(job_name(target))) }
                .map_err(|e| failed(target.pid, "create a CPU cap for", &e))?;
            let job = Owned(job);
            let mut inside = BOOL(0);
            // SAFETY: both handles are live; `inside` is a valid out-pointer.
            let _ = unsafe { IsProcessInJob(h.raw(), Some(job.raw()), &mut inside) };
            if !inside.as_bool() {
                // SAFETY: the process handle has SET_QUOTA | TERMINATE rights, the job ASSIGN rights (creator).
                unsafe { AssignProcessToJobObject(job.raw(), h.raw()) }.map_err(|e| failed(target.pid, "cap the CPU of", &e))?;
                keep_name_alive(target, &job);
            }
            slot.insert(job);
        }
        let job = self
            .jobs
            .get(&target)
            .ok_or_else(|| PlatformError::Failed("The CPU cap job is missing.".into()))?;
        set_rate(
            job,
            target.pid,
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            cpu_rate(pct),
        )?;
        self.state.update(target, |a| a.capped = true);
        Ok(())
    }
}

impl ProcessControl for WinControl {
    fn apply(&mut self, target: ProcessRef, action: &ProcessAction) -> Result<(), PlatformError> {
        match action {
            ProcessAction::End => self.end(target),
            ProcessAction::Suspend => self.suspend(target, true),
            ProcessAction::Resume => self.suspend(target, false),
            ProcessAction::SetPriority(p) => self.priority(target, *p),
            ProcessAction::SetAffinity(mask) => self.affinity(target, *mask),
            ProcessAction::SetEfficiencyMode(on) => self.efficiency(target, *on),
            ProcessAction::SetCpuCap(pct) => self.cap(target, *pct),
        }
    }

    fn release_all(&mut self) {
        for (target, job) in self.jobs.drain() {
            let _ = set_rate(&job, target.pid, JOB_OBJECT_CPU_RATE_CONTROL(0), 0);
        }
        let mut all: Vec<_> = self.state.entries().to_vec();
        all.append(&mut self.leftovers);
        for a in all {
            let target = a.target();
            // Only the very process Dyno changed: a reused pid fails the identity check.
            if open_process(target, Default::default()).is_err() {
                continue;
            }
            if a.capped {
                release_named_job(target);
            }
            if a.suspended {
                let _ = for_each_thread(target.pid, false);
            }
        }
        self.state.clear();
        self.priority_before_eco.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mappings() {
        for p in [
            Priority::Idle,
            Priority::BelowNormal,
            Priority::Normal,
            Priority::AboveNormal,
            Priority::High,
        ] {
            assert_eq!(priority_of_class(priority_class(p).0), Some(p));
        }
        assert_eq!(priority_of_class(REALTIME_PRIORITY_CLASS.0), Some(Priority::High));
        assert_eq!(priority_of_class(0), None);
        assert_eq!(cpu_rate(25), 2500);
        assert_eq!(cpu_rate(0), 100);
        assert_eq!(cpu_rate(200), 10_000);
        let r = ProcessRef {
            pid: 42,
            started: dyno_core::time::Timestamp(1_700_000_000_000),
        };
        assert_eq!(job_name(r), "Local\\Dyno-Cap-42-1700000000000");
    }
}
