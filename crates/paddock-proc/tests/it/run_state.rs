//! A SIGSTOP-stopped process is read as stopped and a start watch waits for it instead of calling it
//! idle (ADR 0031). Unix only: the test child is `sleep`, stopped and killed with signals.
#![cfg(unix)]

use paddock_proc::{
    Liveness, Pace, Patience, ProcessId, RunState, StallCause, StartProbe, StartState, StartWatch,
    Watched, command, liveness_of, run_state,
};
use std::process::Child;
use std::time::{Duration, Instant};

/// A child that sleeps; killed and reaped when dropped.
struct Sleeper(Child);

impl Sleeper {
    fn new() -> Sleeper {
        Sleeper(command("sleep").arg("60").spawn().unwrap())
    }
    fn id(&self) -> ProcessId {
        ProcessId::of(self.0.id()).unwrap()
    }
    #[allow(
        clippy::disallowed_methods,
        reason = "stopping (not ending) our own unreaped child, which has no ProcessId API"
    )]
    fn signal(&self, sig: libc::c_int) {
        // SAFETY: kill takes plain values; the pid is our unreaped child.
        assert_eq!(unsafe { libc::kill(self.0.id() as libc::pid_t, sig) }, 0);
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits for the condition (the signal is delivered asynchronously); fails after a generous bound.
fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !ok() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_child_is_running_then_stopped_then_running_and_a_killed_one_is_exited() {
    let child = Sleeper::new();
    let id = child.id();
    assert_eq!(run_state(id), RunState::Running);
    child.signal(libc::SIGSTOP);
    until("stopped", || run_state(id) == RunState::Stopped);
    child.signal(libc::SIGCONT);
    until("running again", || run_state(id) == RunState::Running);
    child.signal(libc::SIGKILL);
    drop(child);
    assert_eq!(run_state(id), RunState::Exited);
}

#[test]
fn a_pid_that_names_another_process_is_exited() {
    let me = ProcessId::of(std::process::id()).unwrap();
    let other = ProcessId::new(
        me.pid(),
        paddock_proc::StartTime::from_micros(me.started().as_micros() + 1_000_000),
    );
    assert_eq!(run_state(other), RunState::Exited);
}

/// Names only the process: the run state comes from the default `run_state`. A constant CPU time, so only a pause can keep
/// the idle count from reaching its limit.
struct ByPid(ProcessId);

impl StartProbe for ByPid {
    fn liveness(&mut self) -> Liveness {
        Liveness::Running
    }
    fn ready(&mut self) -> bool {
        false
    }
    fn pid(&self) -> Option<ProcessId> {
        Some(self.0)
    }
    fn cpu_time(&mut self) -> Option<Duration> {
        Some(Duration::from_secs(1))
    }
}

/// A pace that does not sleep and ends the watch after `.1` waits.
struct Fake(u32, u32);

impl Pace for Fake {
    fn wait(&mut self, _tick: Duration) -> bool {
        self.0 += 1;
        self.0 < self.1
    }
}

#[test]
fn a_watch_over_a_stopped_child_says_paused_once_and_never_calls_it_stuck() {
    let child = Sleeper::new();
    let id = child.id();
    child.signal(libc::SIGSTOP);
    until("stopped", || run_state(id) == RunState::Stopped);
    let patience = Patience {
        idle: Duration::from_millis(5),
        tick: Duration::from_millis(1),
        cpu_every: 1,
        ..Patience::default()
    };
    let mut watch = StartWatch::new(patience, "the child", ByPid(id), Fake(0, 200));
    let mut stalls = Vec::new();
    loop {
        match watch.wait() {
            Watched::State(StartState::Stalled(r)) => stalls.push(r),
            Watched::State(other) => panic!("unexpected {other:?}"),
            Watched::Cancelled => break,
        }
    }
    assert_eq!(stalls.len(), 1, "{stalls:?}");
    assert_eq!(stalls[0].cause, StallCause::Paused);
    assert!(!stalls[0].cause.is_stuck());
    child.signal(libc::SIGKILL);
}

#[test]
fn the_same_watch_over_a_running_idle_child_does_call_it_idle() {
    let child = Sleeper::new();
    let patience = Patience {
        idle: Duration::from_millis(5),
        tick: Duration::from_millis(1),
        cpu_every: 1,
        ..Patience::default()
    };
    let mut watch = StartWatch::new(patience, "the child", ByPid(child.id()), Fake(0, 200));
    match watch.wait() {
        Watched::State(StartState::Stalled(r)) => {
            assert!(matches!(r.cause, StallCause::Idle(_)), "{r:?}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn liveness_of_a_stopped_child_is_paused_and_of_a_running_one_running() {
    let child = Sleeper::new();
    let id = child.id();
    assert_eq!(liveness_of(id, None), Liveness::Running);
    child.signal(libc::SIGSTOP);
    until("stopped", || run_state(id) == RunState::Stopped);
    assert_eq!(liveness_of(id, None), Liveness::Paused);
}
