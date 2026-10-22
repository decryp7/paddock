//! Process control shared by macOS and Linux, through `libc`: end (SIGTERM, then SIGKILL), suspend/resume
//! (SIGSTOP/SIGCONT), priority (`setpriority`) and CPU caps as a **duty cycle** (a thread per capped process that
//! stops and continues it, 100 ms period). What differs per OS (efficiency mode, affinity) is an [`OsControl`].
//!
//! Every action first compares the live process's start time with the [`ProcessRef`], so a reused pid is never hit.
//! What outlives Dyno (suspensions, caps) is kept in a [`ControlState`] file; `release_all` continues those processes,
//! including the ones a crashed earlier run left stopped. If the agent crashes while a capped process is stopped it
//! stays stopped until the next run releases it (ADR 0006).

use crate::control_state::ControlState;
use crate::parse::time::{same_start, started_from_unix_secs};
use dyno_core::ports::{PlatformError, ProcessControl};
use dyno_core::process::{Pid, Priority, ProcessAction, ProcessRef};
use dyno_core::time::Timestamp;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// The duty-cycle period of a CPU cap.
pub const CAP_PERIOD: Duration = Duration::from_millis(100);
/// How long `End` waits after SIGTERM before SIGKILL.
pub const TERM_GRACE: Duration = Duration::from_millis(2_500);

/// The OS-specific parts of process control.
pub trait OsControl: Send {
    /// Turns efficiency mode (background priority) on or off for `pid`.
    fn set_efficiency(&mut self, pid: Pid, on: bool) -> Result<(), PlatformError>;
    /// Restricts `pid` to the cores in `mask`.
    fn set_affinity(&mut self, pid: Pid, mask: u64) -> Result<(), PlatformError>;
}

struct Cap {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Cap {
    fn end(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Process control for Unix-like systems.
pub struct UnixControl {
    os: Box<dyn OsControl>,
    state: ControlState,
    sys: System,
    caps: HashMap<ProcessRef, Cap>,
    saved_nice: HashMap<ProcessRef, i32>,
}

impl UnixControl {
    /// Control that keeps its state in `state_file`.
    pub fn new(state_file: PathBuf, os: Box<dyn OsControl>) -> Self {
        Self::with_state(ControlState::load(state_file), os)
    }

    /// Control with an explicit state (tests use an in-memory one).
    pub fn with_state(state: ControlState, os: Box<dyn OsControl>) -> Self {
        UnixControl {
            os,
            state,
            sys: System::new(),
            caps: HashMap::new(),
            saved_nice: HashMap::new(),
        }
    }

    /// Checks that `target` is still the process it was (start time) and returns its pid as the OS type.
    fn verify(&mut self, target: ProcessRef) -> Result<libc::pid_t, PlatformError> {
        live_check(&mut self.sys, target)
    }
}

fn gone(pid: Pid) -> PlatformError {
    PlatformError::NotFound(format!("Process {pid} has exited."))
}

/// Whether `target` is alive with the same start time.
fn live_check(sys: &mut System, target: ProcessRef) -> Result<libc::pid_t, PlatformError> {
    let pid = libc::pid_t::try_from(target.pid).map_err(|_| gone(target.pid))?;
    if pid <= 1 {
        return Err(PlatformError::AccessDenied("Dyno never changes the system's own process.".into()));
    }
    let sp = sysinfo::Pid::from_u32(target.pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[sp]), true, ProcessRefreshKind::nothing());
    let started = sys.process(sp).map(|p| started_from_unix_secs(p.start_time()));
    match started {
        Some(s) if same_start(s, target.started) => Ok(pid),
        _ => Err(gone(target.pid)),
    }
}

/// Sends `sig`, mapping errno to a message that says what helps.
fn signal(pid: libc::pid_t, sig: libc::c_int) -> Result<(), PlatformError> {
    // SAFETY: `kill` takes plain integers and has no memory effects; a stale pid is caught by the caller's start-time check.
    let r = unsafe { libc::kill(pid, sig) };
    if r == 0 {
        Ok(())
    } else {
        Err(errno_error("signal the process"))
    }
}

fn errno_error(what: &str) -> PlatformError {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => PlatformError::NotFound("The process has exited.".into()),
        Some(libc::EPERM) | Some(libc::EACCES) => PlatformError::AccessDenied(format!(
            "Dyno can't {what}: it belongs to another user or the system. Changing it needs the helper (administrator rights)."
        )),
        _ => PlatformError::Failed(format!("Could not {what}: {e}")),
    }
}

/// Whether `pid` exists (signal 0).
fn exists(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks existence and permission.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The nice value of `pid`.
pub fn get_nice(pid: libc::pid_t) -> Result<i32, PlatformError> {
    // SAFETY: getpriority's errno is cleared first because -1 is also a valid nice value.
    let (v, err) = unsafe {
        *errno_ptr() = 0;
        let v = libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t);
        (v, *errno_ptr())
    };
    if err != 0 {
        return Err(errno_error("read the priority"));
    }
    Ok(v)
}

#[cfg(target_os = "macos")]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__error()
}
#[cfg(not(target_os = "macos"))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__errno_location()
}

/// Sets the nice value of `pid`.
pub fn set_nice(pid: libc::pid_t, nice: i32) -> Result<(), PlatformError> {
    // SAFETY: plain integers; failure is reported through errno.
    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) };
    if r == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    if nice < 0 && matches!(e.raw_os_error(), Some(libc::EPERM) | Some(libc::EACCES)) {
        return Err(PlatformError::AccessDenied(
            "Raising a priority needs administrator rights; Dyno can't do it without the helper.".into(),
        ));
    }
    Err(errno_error("change the priority"))
}

impl UnixControl {
    fn end(&mut self, target: ProcessRef, pid: libc::pid_t) -> Result<(), PlatformError> {
        self.end_cap(target);
        // A stopped process can't act on SIGTERM until it continues.
        let _ = signal(pid, libc::SIGCONT);
        signal(pid, libc::SIGTERM)?;
        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline {
            if !self.alive(target) {
                self.state.update(target, |a| {
                    a.suspended = false;
                    a.capped = false;
                });
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if self.alive(target) {
            signal(pid, libc::SIGKILL)?;
        }
        self.state.update(target, |a| {
            a.suspended = false;
            a.capped = false;
        });
        Ok(())
    }

    /// Alive and not a zombie, with the same start time.
    fn alive(&mut self, target: ProcessRef) -> bool {
        let sp = sysinfo::Pid::from_u32(target.pid);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::Some(&[sp]), true, ProcessRefreshKind::nothing());
        match self.sys.process(sp) {
            Some(p) => {
                !matches!(p.status(), sysinfo::ProcessStatus::Zombie) && same_start(started_from_unix_secs(p.start_time()), target.started)
            }
            None => false,
        }
    }

    fn end_cap(&mut self, target: ProcessRef) {
        if let Some(mut cap) = self.caps.remove(&target) {
            cap.end();
        }
    }

    fn set_cap(&mut self, target: ProcessRef, pid: libc::pid_t, pct: Option<u8>) -> Result<(), PlatformError> {
        self.end_cap(target);
        let Some(pct) = pct else {
            let _ = signal(pid, libc::SIGCONT);
            self.state.update(target, |a| a.capped = false);
            return Ok(());
        };
        let pct = pct.clamp(1, 100);
        // Fail now, not inside the thread, when the process isn't ours to signal.
        signal(pid, libc::SIGCONT)?;
        self.state.update(target, |a| a.capped = true);
        if pct == 100 {
            return Ok(());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name(format!("dyno-cap-{}", target.pid))
            .spawn(move || duty_cycle(target, pid, pct, &flag))
            .map_err(|e| PlatformError::Failed(format!("Could not start the CPU cap: {e}")))?;
        self.caps.insert(
            target,
            Cap {
                stop,
                thread: Some(thread),
            },
        );
        Ok(())
    }
}

/// Runs the process for `pct` ms of every period and stops it for the rest, until `stop` or the process is gone.
/// Always leaves it continued.
fn duty_cycle(target: ProcessRef, pid: libc::pid_t, pct: u8, stop: &AtomicBool) {
    let run = CAP_PERIOD * u32::from(pct) / 100;
    let idle = CAP_PERIOD - run;
    let mut sys = System::new();
    let mut last_check = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if last_check.elapsed() >= Duration::from_secs(1) {
            last_check = Instant::now();
            if live_check(&mut sys, target).is_err() {
                return;
            }
        }
        if signal(pid, libc::SIGCONT).is_err() {
            return;
        }
        std::thread::sleep(run);
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if signal(pid, libc::SIGSTOP).is_err() {
            return;
        }
        std::thread::sleep(idle);
    }
    let _ = signal(pid, libc::SIGCONT);
}

impl ProcessControl for UnixControl {
    fn apply(&mut self, target: ProcessRef, action: &ProcessAction) -> Result<(), PlatformError> {
        let pid = self.verify(target)?;
        match action {
            ProcessAction::End => self.end(target, pid),
            ProcessAction::Suspend => {
                signal(pid, libc::SIGSTOP)?;
                self.state.update(target, |a| a.suspended = true);
                Ok(())
            }
            ProcessAction::Resume => {
                signal(pid, libc::SIGCONT)?;
                self.state.update(target, |a| a.suspended = false);
                Ok(())
            }
            ProcessAction::SetPriority(p) => set_nice(pid, p.nice()),
            ProcessAction::SetAffinity(mask) => self.os.set_affinity(target.pid, *mask),
            ProcessAction::SetEfficiencyMode(on) => {
                if *on {
                    self.saved_nice.entry(target).or_insert(get_nice(pid).unwrap_or(0));
                    self.os.set_efficiency(target.pid, true)
                } else {
                    let back = self.saved_nice.remove(&target).unwrap_or_else(|| Priority::Normal.nice());
                    self.os.set_efficiency(target.pid, false)?;
                    set_nice(pid, back)
                }
            }
            ProcessAction::SetCpuCap(pct) => self.set_cap(target, pid, *pct),
        }
    }

    fn release_all(&mut self) {
        let targets: Vec<ProcessRef> = self.caps.keys().copied().collect();
        for t in targets {
            self.end_cap(t);
        }
        let entries: Vec<ProcessRef> = self.state.entries().iter().map(|e| e.target()).collect();
        for t in entries {
            // Only continue a process that is still the one Dyno stopped; never signal a reused pid.
            if let Ok(pid) = live_check(&mut self.sys, t) {
                let _ = signal(pid, libc::SIGCONT);
            }
        }
        for (t, nice) in std::mem::take(&mut self.saved_nice) {
            if let Ok(pid) = live_check(&mut self.sys, t) {
                let _ = self.os.set_efficiency(t.pid, false);
                let _ = set_nice(pid, nice);
            }
        }
        self.state.clear();
    }
}

impl Drop for UnixControl {
    fn drop(&mut self) {
        // A capped process must never stay stopped because Dyno went away.
        let targets: Vec<ProcessRef> = self.caps.keys().copied().collect();
        for t in targets {
            self.end_cap(t);
        }
    }
}

/// Whether `pid` is a live process of this machine.
pub fn process_exists(pid: Pid) -> bool {
    libc::pid_t::try_from(pid).map(exists).unwrap_or(false)
}

/// The start time sysinfo reports for `pid`, as a [`Timestamp`].
pub fn started_of(pid: Pid) -> Option<Timestamp> {
    let mut sys = System::new();
    let sp = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[sp]), true, ProcessRefreshKind::nothing());
    sys.process(sp).map(|p| started_from_unix_secs(p.start_time()))
}

#[cfg(test)]
mod tests;
