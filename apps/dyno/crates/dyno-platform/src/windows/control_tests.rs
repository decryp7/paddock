//! Integration tests of Windows process control. They act **only on dummy processes they spawn** (a `cmd` loop that
//! burns one core, or a `ping` that waits) and kill them on drop.

use super::control::{job_name, threads_of, WinControl};
use super::handle::{creation_time, open_process, Owned};
use crate::control_state::ControlState;
use dyno_core::ports::{PlatformError, ProcessControl};
use dyno_core::process::{Priority, ProcessAction, ProcessRef};
use std::process::Child;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::JobObjects::OpenJobObjectW;
use windows::Win32::System::Threading::{
    GetPriorityClass, GetProcessAffinityMask, GetProcessTimes, OpenProcess,
    IDLE_PRIORITY_CLASS, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, BELOW_NORMAL_PRIORITY_CLASS,
};

/// A process the test spawned; killed and reaped on drop.
pub(crate) struct Dummy {
    child: Child,
    pub(crate) target: ProcessRef,
}

impl Dummy {
    /// A process that burns one core.
    pub(crate) fn burner() -> Dummy {
        Self::spawn(&["/d", "/c", "for /L %i in (0,0,1) do @rem"])
    }

    /// A process that waits quietly for a minute.
    pub(crate) fn sleeper() -> Dummy {
        Self::spawn(&["/d", "/c", "ping -n 60 127.0.0.1 >nul"])
    }

    fn spawn(args: &[&str]) -> Dummy {
        let mut cmd = crate::process::command("cmd.exe");
        cmd.args(args);
        let child = cmd.spawn().unwrap();
        let pid = child.id();
        // SAFETY: test-only open of our own child.
        let h = Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.unwrap());
        let started = creation_time(h.raw()).unwrap();
        Dummy {
            child,
            target: ProcessRef { pid, started },
        }
    }

    pub(crate) fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// CPU seconds used so far.
    fn cpu_secs(&self) -> f64 {
        let h = open_process(self.target, Default::default()).unwrap();
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        // SAFETY: test-only query of our own child.
        unsafe { GetProcessTimes(h.raw(), &mut c, &mut e, &mut k, &mut u) }.unwrap();
        (super::handle::ft64(k) + super::handle::ft64(u)) as f64 / 1e7
    }

    /// Its CPU use over `d`, in percent of one core.
    fn cpu_over(&self, d: Duration) -> f64 {
        let a = self.cpu_secs();
        let t = Instant::now();
        std::thread::sleep(d);
        (self.cpu_secs() - a) / t.elapsed().as_secs_f64() * 100.0
    }
}

impl Drop for Dummy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn control() -> WinControl {
    WinControl::with_state(ControlState::in_memory())
}

fn priority_of(d: &Dummy) -> u32 {
    let h = open_process(d.target, Default::default()).unwrap();
    // SAFETY: test-only query of our own child.
    unsafe { GetPriorityClass(h.raw()) }
}

#[test]
fn suspend_and_resume_stop_and_restart_the_cpu() {
    let d = Dummy::burner();
    let mut c = control();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!threads_of(d.target.pid).unwrap().is_empty());
    c.apply(d.target, &ProcessAction::Suspend).unwrap();
    // Suspending twice is idempotent (one resume undoes it).
    c.apply(d.target, &ProcessAction::Suspend).unwrap();
    let paused = d.cpu_over(Duration::from_millis(600));
    assert!(paused < 5.0, "suspended process used {paused:.1}% of a core");
    c.apply(d.target, &ProcessAction::Resume).unwrap();
    let running = d.cpu_over(Duration::from_millis(600));
    assert!(running > 5.0, "resumed process used only {running:.1}% of a core");
}

#[test]
fn priority_affinity_and_efficiency_mode() {
    let d = Dummy::sleeper();
    let mut c = control();
    c.apply(d.target, &ProcessAction::SetPriority(Priority::BelowNormal)).unwrap();
    assert_eq!(priority_of(&d), BELOW_NORMAL_PRIORITY_CLASS.0);
    c.apply(d.target, &ProcessAction::SetAffinity(1)).unwrap();
    let h = open_process(d.target, PROCESS_QUERY_INFORMATION).unwrap();
    let (mut proc_mask, mut sys_mask) = (0usize, 0usize);
    // SAFETY: test-only query of our own child.
    unsafe { GetProcessAffinityMask(h.raw(), &mut proc_mask, &mut sys_mask) }.unwrap();
    assert_eq!(proc_mask, 1);
    assert!(matches!(c.apply(d.target, &ProcessAction::SetAffinity(0)), Err(PlatformError::Failed(_))));

    c.apply(d.target, &ProcessAction::SetEfficiencyMode(true)).unwrap();
    assert_eq!(priority_of(&d), IDLE_PRIORITY_CLASS.0);
    assert_eq!(super::control::efficiency_mode(&h), Some(true), "EcoQoS on");
    c.apply(d.target, &ProcessAction::SetEfficiencyMode(false)).unwrap();
    assert_eq!(super::control::efficiency_mode(&h), Some(false), "EcoQoS off");
    assert_eq!(priority_of(&d), BELOW_NORMAL_PRIORITY_CLASS.0, "priority restored");
}

#[test]
fn a_cpu_cap_throttles_and_releases() {
    let d = Dummy::burner();
    let mut c = control();
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    std::thread::sleep(Duration::from_millis(300));
    let before = d.cpu_over(Duration::from_millis(800));
    // 1% of the machine is `cores`% of one core.
    c.apply(d.target, &ProcessAction::SetCpuCap(Some(1))).unwrap();
    let capped = d.cpu_over(Duration::from_millis(1500));
    eprintln!("burner: {before:.1}% of a core before, {capped:.1}% capped at 1% of {cores} cores");
    // The cap allows `cores`% of one core; the shared test machine may give the burner less than a core anyway.
    assert!(capped <= cores + 12.0, "capped {capped:.1}% of a core; the cap allows {cores}%");
    if before > cores + 20.0 {
        assert!(capped < before - 10.0, "before {before:.1}%, capped {capped:.1}%");
    }
    // Re-capping reuses the job.
    c.apply(d.target, &ProcessAction::SetCpuCap(Some(2))).unwrap();
    c.apply(d.target, &ProcessAction::SetCpuCap(None)).unwrap();
    let released = d.cpu_over(Duration::from_millis(800));
    eprintln!("released: {released:.1}%");
    if before > cores + 20.0 {
        assert!(released > cores + 5.0, "released {released:.1}%");
    }
}

#[test]
fn release_all_undoes_caps_and_suspensions_also_from_a_crashed_run() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state.json");
    let a = Dummy::burner();
    let b = Dummy::burner();
    // A "crashed" run: caps `a`, suspends `b`, and goes away without releasing (its job handles close).
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    std::thread::sleep(Duration::from_millis(300));
    let a_before = a.cpu_over(Duration::from_millis(600));
    let mut crashed = WinControl::new(state.clone());
    crashed.apply(a.target, &ProcessAction::SetCpuCap(Some(1))).unwrap();
    crashed.apply(b.target, &ProcessAction::Suspend).unwrap();
    drop(crashed);
    std::thread::sleep(Duration::from_millis(300));
    assert!(a.cpu_over(Duration::from_millis(800)) <= cores + 12.0);
    assert!(b.cpu_over(Duration::from_millis(500)) < 5.0);

    let mut next = WinControl::new(state.clone());
    next.release_all();
    let a_after = a.cpu_over(Duration::from_millis(800));
    if a_before > cores + 20.0 {
        assert!(a_after > cores + 5.0, "cap released: {a_after:.1}%");
    }
    assert!(b.cpu_over(Duration::from_millis(800)) > 5.0, "resumed");
    assert!(ControlState::load(state).entries().is_empty());
    next.release_all(); // idempotent
}

#[test]
fn a_named_job_survives_its_creator_handle_while_the_process_runs() {
    // release_all after a crash re-opens the job by name; that only works while the name exists.
    let d = Dummy::sleeper();
    let mut c = control();
    c.apply(d.target, &ProcessAction::SetCpuCap(Some(50))).unwrap();
    drop(c); // closes Dyno's job handle without releasing
    // SAFETY: test-only open of a named object.
    let reopened = unsafe { OpenJobObjectW(0x2, false, &HSTRING::from(job_name(d.target))) };
    // The capped process holds a handle to its own job, so the name outlives Dyno's handle.
    drop(Owned(reopened.unwrap()));
}

#[test]
fn end_terminates_and_reused_pids_are_refused() {
    let mut d = Dummy::sleeper();
    let mut c = control();
    let wrong = ProcessRef {
        pid: d.target.pid,
        started: d.target.started.minus(5_000),
    };
    for action in [ProcessAction::End, ProcessAction::Suspend, ProcessAction::SetCpuCap(Some(10)), ProcessAction::SetPriority(Priority::Idle)] {
        assert!(matches!(c.apply(wrong, &action), Err(PlatformError::NotFound(_))), "{action:?}");
    }
    c.apply(d.target, &ProcessAction::End).unwrap();
    let t = Instant::now();
    while d.alive() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!d.alive());
    assert!(matches!(c.apply(d.target, &ProcessAction::Resume), Err(PlatformError::NotFound(_))));
}

#[test]
fn system_processes_are_access_denied() {
    // Observe only: pid 4 ("System") can't be opened for changes by a normal user; nothing is changed.
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(4)]), true);
    let Some(p) = sys.process(sysinfo::Pid::from_u32(4)) else { return };
    let target = ProcessRef {
        pid: 4,
        started: crate::parse::time::started_from_unix_secs(p.start_time()),
    };
    let r = open_process(target, windows::Win32::System::Threading::PROCESS_SET_INFORMATION);
    assert!(matches!(r, Err(PlatformError::AccessDenied(_)) | Err(PlatformError::NotFound(_))), "{r:?}");
}
