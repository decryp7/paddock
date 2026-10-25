//! Acts only on processes the tests spawn themselves (a shell busy loop or `sleep`), and kills them on drop.

use super::*;
use std::process::{Child, Stdio};

struct Dummy(Child);

impl Dummy {
    fn busy() -> Dummy {
        Dummy(
            crate::process::command("sh")
                .args(["-c", "while :; do :; done"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn sleeper() -> Dummy {
        Dummy(crate::process::command("sleep").arg("60").stdout(Stdio::null()).spawn().unwrap())
    }
    fn target(&self) -> ProcessRef {
        let pid = self.0.id();
        // The child may not be listed in the very first instant.
        for _ in 0..50 {
            if let Some(started) = started_of(pid) {
                return ProcessRef { pid, started };
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("child {pid} never appeared");
    }
}

impl Drop for Dummy {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct NoOs;
impl OsControl for NoOs {
    fn set_efficiency(&mut self, _pid: Pid, _on: bool) -> Result<(), PlatformError> {
        Ok(())
    }
    fn set_affinity(&mut self, _pid: Pid, _mask: u64) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported("no affinity here".into()))
    }
}

fn control() -> UnixControl {
    UnixControl::with_state(ControlState::in_memory(), Box::new(NoOs))
}

fn status(pid: Pid) -> Option<sysinfo::ProcessStatus> {
    let mut sys = System::new();
    let sp = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[sp]), true, ProcessRefreshKind::nothing());
    sys.process(sp).map(|p| p.status())
}

/// Average CPU percent (of one core) of `pid` over `secs`.
fn cpu_over(pid: Pid, secs: u64) -> f32 {
    let mut sys = System::new();
    let sp = sysinfo::Pid::from_u32(pid);
    let kind = ProcessRefreshKind::nothing().with_cpu();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[sp]), true, kind);
    std::thread::sleep(Duration::from_secs(secs));
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[sp]), true, kind);
    sys.process(sp).map(|p| p.cpu_usage()).unwrap_or(0.0)
}

#[test]
fn a_reused_pid_is_not_found_and_pid_one_is_refused() {
    let d = Dummy::sleeper();
    let mut c = control();
    let wrong = ProcessRef {
        started: d.target().started.plus(3_600_000),
        ..d.target()
    };
    assert!(matches!(c.apply(wrong, &ProcessAction::Suspend), Err(PlatformError::NotFound(_))));
    let one = ProcessRef {
        pid: 1,
        started: Timestamp(0),
    };
    assert!(matches!(c.apply(one, &ProcessAction::End), Err(PlatformError::AccessDenied(_))));
    let none = ProcessRef {
        pid: u32::MAX - 3,
        started: Timestamp(0),
    };
    assert!(matches!(c.apply(none, &ProcessAction::End), Err(PlatformError::NotFound(_))));
}

#[test]
fn suspend_and_resume_a_dummy() {
    let d = Dummy::sleeper();
    let t = d.target();
    let mut c = control();
    c.apply(t, &ProcessAction::Suspend).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(status(t.pid), Some(sysinfo::ProcessStatus::Stop));
    assert_eq!(c.state.entries().len(), 1);
    c.apply(t, &ProcessAction::Resume).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_ne!(status(t.pid), Some(sysinfo::ProcessStatus::Stop));
    assert!(c.state.entries().is_empty());
}

#[test]
fn priority_lowers_and_refuses_to_raise_without_rights() {
    let d = Dummy::sleeper();
    let t = d.target();
    let mut c = control();
    c.apply(t, &ProcessAction::SetPriority(Priority::BelowNormal)).unwrap();
    assert_eq!(get_nice(d.0.id() as libc::pid_t).unwrap(), 10);
    // Raising a priority needs root; a normal user is refused with a message that says so.
    if unsafe { libc::geteuid() } != 0 {
        let e = c.apply(t, &ProcessAction::SetPriority(Priority::High)).unwrap_err();
        assert!(matches!(e, PlatformError::AccessDenied(_)), "{e:?}");
    }
}

#[test]
fn efficiency_mode_restores_the_previous_priority() {
    let d = Dummy::sleeper();
    let t = d.target();
    let mut c = control();
    c.apply(t, &ProcessAction::SetPriority(Priority::BelowNormal)).unwrap();
    c.apply(t, &ProcessAction::SetEfficiencyMode(true)).unwrap();
    c.apply(t, &ProcessAction::SetEfficiencyMode(false)).unwrap();
    assert_eq!(get_nice(d.0.id() as libc::pid_t).unwrap(), 10);
}

#[test]
fn affinity_is_the_os_hooks_answer() {
    let d = Dummy::sleeper();
    let mut c = control();
    assert!(matches!(
        c.apply(d.target(), &ProcessAction::SetAffinity(1)),
        Err(PlatformError::Unsupported(_))
    ));
}

#[test]
fn a_cpu_cap_lowers_cpu_and_release_restores_it() {
    let d = Dummy::busy();
    let t = d.target();
    let mut c = control();
    // On a heavily loaded machine a busy loop may not get a core for a second: measure again before giving up.
    let free = (0..4).map(|_| cpu_over(t.pid, 1)).fold(0.0f32, f32::max);
    if free < 40.0 {
        eprintln!("machine too loaded to measure a CPU cap (the dummy got {free}%); skipping");
        return;
    }
    c.apply(t, &ProcessAction::SetCpuCap(Some(20))).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let capped = cpu_over(t.pid, 2);
    assert!(capped < free * 0.6, "capped {capped} vs free {free}");
    assert!(c.state.entries()[0].capped);
    c.apply(t, &ProcessAction::SetCpuCap(None)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_ne!(status(t.pid), Some(sysinfo::ProcessStatus::Stop));
    let again = cpu_over(t.pid, 1);
    assert!(again > capped, "released {again} vs capped {capped}");
}

#[test]
fn release_all_continues_what_a_crashed_run_left_stopped() {
    let d = Dummy::sleeper();
    let t = d.target();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    let mut first = UnixControl::new(file.clone(), Box::new(NoOs));
    first.apply(t, &ProcessAction::Suspend).unwrap();
    std::mem::forget(first); // "crash": nothing is cleaned up
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(status(t.pid), Some(sysinfo::ProcessStatus::Stop));
    let mut second = UnixControl::new(file.clone(), Box::new(NoOs));
    second.release_all();
    std::thread::sleep(Duration::from_millis(200));
    assert_ne!(status(t.pid), Some(sysinfo::ProcessStatus::Stop));
    assert!(ControlState::load(file).entries().is_empty());
    second.release_all(); // idempotent
}

#[test]
fn end_terminates_a_dummy_even_when_stopped() {
    let mut d = Dummy::sleeper();
    let t = d.target();
    let mut c = control();
    c.apply(t, &ProcessAction::Suspend).unwrap();
    c.apply(t, &ProcessAction::End).unwrap();
    let status = d.0.wait().unwrap();
    assert!(!status.success());
    assert!(c.state.entries().is_empty());
    assert!(!process_exists(t.pid));
}
