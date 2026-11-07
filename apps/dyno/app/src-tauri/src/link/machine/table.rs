//! Table tests of the pure machine: `(state, event) -> (state', commands)`, one row per arrow. No threads, no files.

use super::*;
use crate::supervise::Machine;

const PID: u32 = 4242;

/// A look with nothing in it: no file, the lock free, nothing asked, no child.
fn look(seq: Seq) -> Look {
    Look {
        seq,
        now_ms: 1_000,
        hint: None,
        lock: Some(LockSeen::Free),
        service: None,
        child: None,
        cpu: Cpu::NotRead,
        app_updated: false,
    }
}

fn hint(pid: u32, started_ms: i64, same_build: bool) -> HintSeen {
    HintSeen {
        pid,
        started_ms,
        version: "1.0.0".into(),
        protocol: 1,
        same_build,
        of_disk_file: false,
    }
}

const SUPERVISING: ServiceState = ServiceState {
    installed: true,
    supervises: true,
    pending: false,
};
const NO_SERVICE: ServiceState = ServiceState {
    installed: false,
    supervises: false,
    pending: false,
};

fn find_look(cmds: &[Command]) -> Option<(Seq, bool)> {
    cmds.iter().find_map(|c| match c {
        Command::Look { seq, service, .. } => Some((*seq, *service)),
        _ => None,
    })
}

/// The seq of the one `Look` command in `cmds`.
fn look_seq(cmds: &[Command]) -> Seq {
    find_look(cmds).unwrap_or_else(|| panic!("no look in {cmds:?}")).0
}

fn connect_seq(cmds: &[Command]) -> Seq {
    cmds.iter()
        .find_map(|c| match c {
            Command::Connect { seq, .. } => Some(*seq),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no connect in {cmds:?}"))
}

fn launches(cmds: &[Command]) -> usize {
    cmds.iter().filter(|c| matches!(c, Command::Launch { .. })).count()
}

fn stall_of(cmds: &[Command]) -> Option<Stall> {
    cmds.iter().find_map(|c| match c {
        Command::Stall { stall, .. } => Some(*stall),
        _ => None,
    })
}

/// A machine that has begun, and the seq of its first look.
fn begun() -> (Supervisor, Seq) {
    let mut s = Supervisor::new();
    let cmds = s.step(Event::Begin);
    let seq = look_seq(&cmds);
    (s, seq)
}

/// The seq of the look a tick issues in a wait.
fn tick_look(s: &mut Supervisor) -> Seq {
    look_seq(&s.step(Event::Tick))
}

/// Answers the first look with `look_with(look)` and returns the machine and the commands.
fn first(look_with: impl FnOnce(Look) -> Look) -> (Supervisor, Vec<Command>) {
    let (mut s, seq) = begun();
    let cmds = s.step(Event::Looked(look_with(look(seq))));
    (s, cmds)
}

/// An app-started agent that is starting: launched (generation 1, pid [`PID`], at 1000 ms) and not yet answering.
fn starting() -> Supervisor {
    let (mut s, _) = first(|l| l);
    s.step(Event::Launched {
        child: 1,
        result: Ok(Launch {
            at_ms: 1_000,
            pid: Some(PID),
        }),
    });
    s
}

/// Connected to the agent this app started.
fn connected_own() -> Supervisor {
    let mut s = starting();
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        hint: Some(hint(PID, 1_000, true)),
        child: Some((1, Child::Running { pid: PID })),
        ..look(seq)
    }));
    let c = connect_seq(&cmds);
    s.step(Event::Connected {
        seq: c,
        peer: 7,
        pid: Some(PID),
    });
    assert_eq!(
        *s.phase(),
        Phase::Connected {
            peer: 7,
            owner: Owner::ThisApp(1)
        }
    );
    s
}

/// Connected to an agent somebody else started, with the service view `service`.
fn connected_other(service: ServiceState) -> Supervisor {
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        lock: Some(LockSeen::Held),
        service: Some(service),
        ..l
    });
    let c = connect_seq(&cmds);
    s.step(Event::Connected {
        seq: c,
        peer: 8,
        pid: Some(9),
    });
    assert_eq!(
        *s.phase(),
        Phase::Connected {
            peer: 8,
            owner: Owner::Other
        }
    );
    s
}

/// Answers the look a `ServiceChanged` issues with `service`.
fn service_now(s: &mut Supervisor, service: ServiceState) -> Vec<Command> {
    let seq = look_seq(&s.step(Event::ServiceChanged));
    s.step(Event::Looked(Look {
        service: Some(service),
        ..look(seq)
    }))
}

fn ticks(s: &mut Supervisor, n: u32) -> Vec<Command> {
    let mut all = Vec::new();
    for _ in 0..n {
        all.extend(s.step(Event::Tick));
    }
    all
}

/// Ticks and answers every look it issues with `answer`, `n` times; returns every command seen.
fn run_ticks(s: &mut Supervisor, n: u32, answer: impl Fn(Seq, bool) -> Look) -> Vec<Command> {
    let mut all = Vec::new();
    for _ in 0..n {
        let cmds = s.step(Event::Tick);
        let look = find_look(&cmds);
        all.extend(cmds);
        if let Some((seq, service)) = look {
            all.extend(s.step(Event::Looked(answer(seq, service))));
        }
    }
    all
}

// ------------------------------------------------------------------------------------------------ beginning and deciding

#[test]
fn begin_reports_starting_and_looks_once() {
    let mut s = Supervisor::new();
    let cmds = s.step(Event::Begin);
    assert!(matches!(cmds[0], Command::Report(ref r) if r.state == AgentState::Starting));
    assert!(matches!(
        cmds[1],
        Command::Look {
            seq: 1,
            service: true,
            cpu: false,
            lock: true,
            deciding: true,
            recheck: false
        }
    ));
    assert_eq!(*s.phase(), Phase::Looking { seq: 1 });
    assert!(s.step(Event::Begin).is_empty(), "a second begin does nothing");
}

#[test]
fn no_agent_and_a_free_lock_launches_one_child_of_generation_one() {
    let (s, cmds) = first(|l| l);
    assert_eq!(cmds, [Command::Launch { child: 1, anyway: false }]);
    assert_eq!(*s.phase(), Phase::Starting { child: 1 });
    assert_eq!(s.held(), Some(1));
}

#[test]
fn an_answer_to_a_superseded_look_changes_nothing() {
    let (mut s, seq) = begun();
    assert!(s.step(Event::Looked(look(seq + 5))).is_empty());
    assert_eq!(*s.phase(), Phase::Looking { seq });
}

#[test]
fn a_held_lock_with_no_file_is_waited_for_and_nothing_is_launched() {
    let (s, cmds) = first(|l| Look {
        lock: Some(LockSeen::Held),
        ..l
    });
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
}

#[test]
fn a_file_of_this_build_beside_a_held_lock_is_connected_to() {
    let (s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        lock: Some(LockSeen::Held),
        ..l
    });
    assert!(matches!(cmds[0], Command::Connect { .. }));
    assert!(matches!(s.phase(), Phase::Connecting { purpose: Purpose::Use, .. }));
}

#[test]
fn a_stale_file_with_a_free_lock_is_not_the_agent_and_not_connected_to() {
    // The file is older than the wait and nothing holds the lock: nothing can answer. It launches, it connects to nothing.
    let (s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        ..l
    });
    assert_eq!(launches(&cmds), 1);
    assert!(!cmds.iter().any(|c| matches!(c, Command::Connect { .. })));
    assert!(!s.snapshot().connected());
}

#[test]
fn a_file_written_after_the_wait_began_is_worth_a_connection_even_with_a_free_lock() {
    let (mut s, _) = first(|l| Look {
        lock: Some(LockSeen::Held),
        ..l
    });
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        hint: Some(hint(9, 1_001, true)),
        ..look(seq)
    }));
    assert!(matches!(cmds[0], Command::Connect { .. }));
}

#[test]
fn a_failed_connect_decides_again_from_the_same_look_without_the_file() {
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        lock: Some(LockSeen::Held),
        ..l
    });
    let c = connect_seq(&cmds);
    let cmds = s.step(Event::ConnectFailed { seq: c, why: "no".into() });
    assert_eq!(launches(&cmds), 0, "the lock is held: a launch would be refused");
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
    // With the lock free the same failure launches.
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        ..l
    });
    assert_eq!(launches(&cmds), 1, "an old file with a free lock is never connected to");
    // (a file newer than the wait does get a connection; when it fails, the free lock launches)
    let (mut s2, seq) = begun();
    s2.step(Event::Looked(Look {
        lock: Some(LockSeen::Held),
        ..look(seq)
    }));
    let seq = tick_look(&mut s2);
    let cmds = s2.step(Event::Looked(Look {
        hint: Some(hint(9, 1_001, true)),
        ..look(seq)
    }));
    let c = connect_seq(&cmds);
    let cmds = s2.step(Event::ConnectFailed { seq: c, why: "no".into() });
    assert_eq!(launches(&cmds), 1);
    let _ = &mut s;
}

#[test]
fn a_connection_made_after_it_was_given_up_is_closed() {
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        lock: Some(LockSeen::Held),
        ..l
    });
    let c = connect_seq(&cmds);
    let cmds = s.step(Event::Connected {
        seq: c + 10,
        peer: 3,
        pid: None,
    });
    assert_eq!(cmds, [Command::CloseConnection { peer: 3 }]);
}

#[test]
fn an_agent_of_another_build_is_asked_to_exit_and_awaited_until_the_lock_is_free() {
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, false)),
        lock: Some(LockSeen::Held),
        ..l
    });
    let c = connect_seq(&cmds);
    assert!(matches!(
        s.phase(),
        Phase::Connecting {
            purpose: Purpose::AskToExit,
            ..
        }
    ));
    let cmds = s.step(Event::Connected {
        seq: c,
        peer: 5,
        pid: Some(9),
    });
    assert_eq!(cmds, [Command::AskToExit { peer: 5 }]);
    assert_eq!(*s.phase(), Phase::Replacing { peer: 5 });
    // It has been asked and still holds the folder: nothing is launched, however many looks.
    for _ in 0..3 {
        let seq = tick_look(&mut s);
        let cmds = s.step(Event::Looked(Look {
            hint: Some(hint(9, 500, false)),
            lock: Some(LockSeen::Held),
            ..look(seq)
        }));
        assert_eq!(launches(&cmds), 0);
    }
    assert_eq!(s.phase(), &Phase::Replacing { peer: 5 });
    // The lock is free: the stale file goes and the machine decides afresh (a new look), still no launch before it.
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        hint: Some(hint(9, 500, false)),
        ..look(seq)
    }));
    assert!(matches!(cmds[0], Command::RemoveHint { .. }), "{cmds:?}");
    assert!(matches!(cmds[1], Command::Look { .. }));
    assert_eq!(launches(&cmds), 0);
}

#[test]
fn a_file_of_another_build_with_a_free_lock_is_removed_and_ours_launches() {
    let (_, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, false)),
        ..l
    });
    assert!(matches!(cmds[0], Command::RemoveHint { look: 1 }), "{cmds:?}");
    assert_eq!(launches(&cmds), 1);
}

#[test]
fn retry_on_an_agent_asked_to_exit_asks_again() {
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, false)),
        lock: Some(LockSeen::Held),
        ..l
    });
    let c = connect_seq(&cmds);
    s.step(Event::Connected {
        seq: c,
        peer: 5,
        pid: Some(9),
    });
    let cmds = s.step(Event::Retry { stall: None });
    let seq = look_seq(&cmds);
    let cmds = s.step(Event::Looked(Look {
        hint: Some(hint(9, 500, false)),
        lock: Some(LockSeen::Held),
        ..look(seq)
    }));
    assert!(
        matches!(
            s.phase(),
            Phase::Connecting {
                purpose: Purpose::AskToExit,
                ..
            }
        ),
        "{cmds:?}"
    );
}

// ------------------------------------------------------------------------------------------------ the login service

#[test]
fn a_supervising_service_is_waited_for_and_the_app_starts_nothing() {
    let (s, cmds) = first(|l| Look {
        service: Some(SUPERVISING),
        ..l
    });
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::WaitingForService);
}

/// The VM's login (2026-10-31): launchd starts the agent before the app, the agent writes `agent.json` a few seconds
/// later (older than the app's wait, so not "fresh"), and the lock is held throughout. The app must connect on the first
/// look that sees the file, within `LOCK_EVERY` ticks, long before the wait's note or the fallback.
#[test]
fn the_app_connects_to_the_services_agent_as_soon_as_its_file_appears_while_the_lock_is_held() {
    let (mut s, _) = first(|l| Look {
        service: Some(SUPERVISING),
        lock: Some(LockSeen::Held),
        ..l
    });
    assert_eq!(*s.phase(), Phase::WaitingForService);
    // Held lock, no file yet: it keeps waiting and starts nothing.
    let cmds = run_ticks(&mut s, 20, |seq, service| Look {
        lock: Some(LockSeen::Held),
        service: service.then_some(SUPERVISING),
        ..look(seq)
    });
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::WaitingForService);
    // The file appears (written before the wait began, so not fresh): connected to within one lock probe.
    let cmds = run_ticks(&mut s, LOCK_EVERY, |seq, service| Look {
        hint: Some(hint(9, 1, true)),
        lock: Some(LockSeen::Held),
        service: service.then_some(SUPERVISING),
        ..look(seq)
    });
    assert!(
        cmds.iter().any(|c| matches!(c, Command::Connect { purpose: Purpose::Use, .. })),
        "{cmds:?}"
    );
    assert_eq!(launches(&cmds), 0);
}

#[test]
fn a_wait_for_the_service_shows_its_cause_then_retry_starts_the_apps_own() {
    let (mut s, _) = first(|l| Look {
        service: Some(SUPERVISING),
        lock: Some(LockSeen::Held),
        ..l
    });
    run_ticks(&mut s, NOTE_AFTER, |seq, service| Look {
        lock: Some(LockSeen::Held),
        service: service.then_some(SUPERVISING),
        ..look(seq)
    });
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::WaitingForService));
    let id = s.snapshot().stall.map(|(i, _)| i);
    let cmds = s.step(Event::Retry { stall: id });
    assert!(cmds.contains(&Command::ClearStall));
    let seq = look_seq(&cmds);
    // Retry chose the app's own agent, although the service says it supervises, and the lock is free.
    let cmds = s.step(Event::Looked(Look {
        service: Some(SUPERVISING),
        ..look(seq)
    }));
    assert_eq!(launches(&cmds), 1);
}

#[test]
fn a_service_that_starts_nothing_is_replaced_by_the_apps_own_without_retry() {
    // The lock stays free across SERVICE_IDLE_RECHECKS rechecks in a row: the service starts nothing.
    let (mut s, _) = first(|l| Look {
        service: Some(SUPERVISING),
        ..l
    });
    let cmds = run_ticks(&mut s, SERVICE_RECHECK * (SERVICE_IDLE_RECHECKS + 1), |seq, service| Look {
        service: service.then_some(SUPERVISING),
        ..look(seq)
    });
    assert_eq!(launches(&cmds), 1, "the app never started its own agent");
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Report(r) if r.message.as_deref().is_some_and(|m| m.contains("did not start")))),
        "no note said the service started nothing"
    );
}

#[test]
fn a_lock_held_resets_the_count_of_a_service_that_starts_nothing() {
    let (mut s, _) = first(|l| Look {
        service: Some(SUPERVISING),
        ..l
    });
    let mut launched = 0;
    for round in 0..(SERVICE_IDLE_RECHECKS * 3) {
        let held = round % 2 == 0; // held every other recheck: never four free ones in a row
        let cmds = run_ticks(&mut s, SERVICE_RECHECK, |seq, service| Look {
            service: service.then_some(SUPERVISING),
            lock: if held { Some(LockSeen::Held) } else { Some(LockSeen::Free) },
            ..look(seq)
        });
        launched += launches(&cmds);
    }
    assert_eq!(launched, 0, "a service that was running was replaced");
}

// ------------------------------------------------------------------------------------------------ a registration is pending (ADR 0056)

/// The job is absent: a removal of the old registration or the OS has not loaded the new one yet.
const PENDING_NO_JOB: ServiceState = ServiceState {
    installed: false,
    supervises: false,
    pending: true,
};
/// The job runs, and the registration still waits for the service's agent to answer.
const PENDING_JOB_RUNS: ServiceState = ServiceState {
    installed: true,
    supervises: true,
    pending: true,
};
/// A job the OS refuses: registered, nothing runs, and nothing will (the attempts are not pending).
const REFUSED_JOB: ServiceState = ServiceState {
    installed: true,
    supervises: false,
    pending: false,
};

/// Ticks and answers the looks with `service` and a free lock (nobody holds the data folder).
fn tick_with(s: &mut Supervisor, n: u32, service: ServiceState) -> Vec<Command> {
    run_ticks(s, n, |seq, asked| Look {
        service: asked.then_some(service),
        ..look(seq)
    })
}

#[test]
fn a_pending_registration_is_waited_for_with_its_cause_at_once_and_the_app_starts_nothing() {
    let (s, cmds) = first(|l| Look {
        service: Some(PENDING_NO_JOB),
        ..l
    });
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::WaitingForService);
    assert_eq!(stall_of(&cmds), Some(Stall::RegisteringService), "a silent wait: {cmds:?}");
}

#[test]
fn a_pending_registration_that_then_answers_is_connected_to_without_a_launch_however_long_it_took() {
    let (mut s, _) = first(|l| Look {
        service: Some(PENDING_NO_JOB),
        ..l
    });
    // Longer than the old fallback (SERVICE_IDLE_RECHECKS rechecks of SERVICE_RECHECK ticks) and than the repair's wait.
    let cmds = tick_with(&mut s, SERVICE_RECHECK * (SERVICE_IDLE_RECHECKS + 2), PENDING_NO_JOB);
    assert_eq!(
        launches(&cmds),
        0,
        "the app started its own agent while the service was registering"
    );
    let cmds = tick_with(&mut s, SERVICE_RECHECK * 2, PENDING_JOB_RUNS);
    assert_eq!(launches(&cmds), 0, "the app started its own agent while the registered job ran");
    // The service's agent answers: the lock is held and its file is there.
    let cmds = run_ticks(&mut s, PENDING_RECHECK + LOCK_EVERY, |seq, service| Look {
        hint: Some(hint(9, 1, true)),
        lock: Some(LockSeen::Held),
        service: service.then_some(SUPERVISING),
        ..look(seq)
    });
    assert_eq!(launches(&cmds), 0);
    assert!(
        cmds.iter().any(|c| matches!(c, Command::Connect { purpose: Purpose::Use, .. })),
        "{cmds:?}"
    );
}

#[test]
fn a_pending_registration_that_ends_with_no_service_falls_back_at_once_not_after_the_old_delay() {
    let (mut s, _) = first(|l| Look {
        service: Some(PENDING_NO_JOB),
        ..l
    });
    tick_with(&mut s, SERVICE_RECHECK, PENDING_NO_JOB);
    // The registration ended (switched off, approval pending, the attempts used up): nothing is coming.
    let cmds = tick_with(&mut s, PENDING_RECHECK, NO_SERVICE);
    assert_eq!(launches(&cmds), 1, "the app waited for a service that is not coming: {cmds:?}");
    assert!(matches!(s.phase(), Phase::Starting { .. }));
}

#[test]
fn a_job_the_os_refuses_after_a_pending_registration_falls_back_at_once() {
    let (mut s, _) = first(|l| Look {
        service: Some(PENDING_JOB_RUNS),
        ..l
    });
    let cmds = tick_with(&mut s, PENDING_RECHECK, REFUSED_JOB);
    assert_eq!(launches(&cmds), 1, "{cmds:?}");
}

#[test]
fn a_refused_job_with_no_pending_registration_starts_the_apps_own_agent_at_the_first_look() {
    let (s, cmds) = first(|l| Look {
        service: Some(REFUSED_JOB),
        ..l
    });
    assert_eq!(launches(&cmds), 1);
    assert!(matches!(s.phase(), Phase::Starting { .. }));
}

#[test]
fn a_slow_service_whose_job_runs_but_has_not_answered_is_not_given_up_on_while_the_registration_is_pending() {
    let (mut s, _) = first(|l| Look {
        service: Some(PENDING_JOB_RUNS),
        ..l
    });
    // Free lock on every recheck, SERVICE_IDLE_RECHECKS times over: the count of a service that starts nothing must not run.
    let cmds = tick_with(&mut s, PENDING_RECHECK * SERVICE_IDLE_RECHECKS * 4, PENDING_JOB_RUNS);
    assert_eq!(launches(&cmds), 0, "{cmds:?}");
    // The registration ends with a running job: the old count (the service starts nothing) starts only now.
    let cmds = tick_with(&mut s, SERVICE_RECHECK * (SERVICE_IDLE_RECHECKS - 2), SUPERVISING);
    assert_eq!(launches(&cmds), 0, "the count began before the registration ended");
    let cmds = tick_with(&mut s, SERVICE_RECHECK * 3, SUPERVISING);
    assert_eq!(launches(&cmds), 1, "a service that starts nothing is still replaced after the wait");
}

#[test]
fn the_cause_of_a_pending_wait_gives_way_to_the_ordinary_one_when_the_registration_ends_and_the_wait_goes_on() {
    let (mut s, _) = first(|l| Look {
        service: Some(PENDING_NO_JOB),
        ..l
    });
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::RegisteringService));
    // Ends with a supervising service that does not answer yet: the agent is awaited, said in the usual words.
    run_ticks(&mut s, PENDING_RECHECK + 1, |seq, service| Look {
        lock: Some(LockSeen::Held),
        service: service.then_some(SUPERVISING),
        ..look(seq)
    });
    assert_ne!(
        s.snapshot().stall.map(|(_, k)| k),
        Some(Stall::RegisteringService),
        "the registration is over"
    );
    run_ticks(&mut s, NOTE_AFTER, |seq, service| Look {
        lock: Some(LockSeen::Held),
        service: service.then_some(SUPERVISING),
        ..look(seq)
    });
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::WaitingForService));
}

#[test]
fn retry_in_a_pending_wait_looks_again_and_never_starts_an_agent_beside_the_registering_service() {
    let (mut s, _) = first(|l| Look {
        service: Some(PENDING_NO_JOB),
        ..l
    });
    let id = s.snapshot().stall.map(|(i, _)| i);
    let cmds = s.step(Event::Retry { stall: id });
    let seq = look_seq(&cmds);
    let cmds = s.step(Event::Looked(Look {
        service: Some(PENDING_NO_JOB),
        ..look(seq)
    }));
    assert_eq!(launches(&cmds), 0, "{cmds:?}");
    assert_eq!(*s.phase(), Phase::WaitingForService);
}

#[test]
fn a_pending_registration_that_never_ends_is_given_up_on_after_the_longest_wait_and_the_app_starts_its_own_agent() {
    let tuning = Tuning {
        pending_max: 40,
        ..Tuning::default()
    };
    let mut s = Supervisor::with_tuning(tuning);
    let seq = look_seq(&s.step(Event::Begin));
    s.step(Event::Looked(Look {
        service: Some(PENDING_NO_JOB),
        ..look(seq)
    }));
    let cmds = tick_with(&mut s, 39, PENDING_NO_JOB);
    assert_eq!(launches(&cmds), 0, "given up before the longest wait");
    let cmds = tick_with(&mut s, PENDING_RECHECK, PENDING_NO_JOB);
    assert_eq!(launches(&cmds), 1, "waited forever with no agent");
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Report(r) if r.message.as_deref().is_some_and(|m| m.contains("did not finish")))),
        "the fallback said nothing: {cmds:?}"
    );
}

#[test]
fn the_apps_own_agent_is_not_handed_over_while_the_registration_is_pending_and_is_when_it_ends() {
    let mut s = connected_own();
    let cmds = service_now(&mut s, PENDING_JOB_RUNS);
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, Command::AskToExit { .. } | Command::EndHeldChild { .. })),
        "the app's agent was retired for a service that is still registering: {cmds:?}"
    );
    // The registration ends with the service up: the usual hand-over (the service's agent was refused while ours held the folder).
    let cmds = service_now(&mut s, SUPERVISING);
    assert!(cmds.iter().any(|c| matches!(c, Command::EndHeldChild { .. })), "{cmds:?}");
}

#[test]
fn a_connected_link_asks_again_while_a_registration_is_pending_so_the_hand_over_is_not_missed() {
    let mut s = connected_own();
    service_now(&mut s, PENDING_JOB_RUNS);
    let mut asked = 0;
    for _ in 0..PENDING_RECHECK * 3 {
        if let Some((seq, true)) = find_look(&s.step(Event::Tick)) {
            asked += 1;
            s.step(Event::Looked(Look {
                service: Some(PENDING_JOB_RUNS),
                ..look(seq)
            }));
        }
    }
    assert!(asked >= 2, "the connected link never asked the service again ({asked})");
}

#[test]
fn a_connected_link_asks_the_service_nothing_on_its_ticks_when_no_registration_is_pending() {
    let mut s = connected_own();
    service_now(&mut s, NO_SERVICE);
    assert!(matches!(s.phase(), Phase::Connected { .. }));
    let cmds = ticks(&mut s, PENDING_RECHECK * 3);
    assert!(find_look(&cmds).is_none(), "a look with nothing pending: {cmds:?}");
}

#[test]
fn the_cause_of_a_pending_wait_stays_put_while_it_is_asked_about_again() {
    let (mut s, _) = first(|l| Look {
        service: Some(PENDING_NO_JOB),
        ..l
    });
    let id = s.snapshot().stall.map(|(i, _)| i);
    let cmds = tick_with(&mut s, PENDING_RECHECK * 3, PENDING_NO_JOB);
    assert!(
        !cmds.contains(&Command::ClearStall),
        "the cause was cleared and shown again: {cmds:?}"
    );
    assert_eq!(s.snapshot().stall.map(|(i, _)| i), id);
}

#[test]
fn giving_up_on_a_pending_registration_lasts_while_it_stays_pending_and_ends_with_it() {
    let tuning = Tuning {
        pending_max: 40,
        ..Tuning::default()
    };
    let mut s = Supervisor::with_tuning(tuning);
    let seq = look_seq(&s.step(Event::Begin));
    s.step(Event::Looked(Look {
        service: Some(PENDING_NO_JOB),
        ..look(seq)
    }));
    assert!(!s.gave_up_on_pending());
    tick_with(&mut s, 40 + PENDING_RECHECK, PENDING_NO_JOB);
    assert!(s.gave_up_on_pending());
    // Still pending: asked again, still given up on (the wait is not started over).
    service_now(&mut s, PENDING_NO_JOB);
    assert!(s.gave_up_on_pending());
    // The registration ended: the next one is waited for again.
    service_now(&mut s, NO_SERVICE);
    assert!(!s.gave_up_on_pending());
}

#[test]
fn the_services_own_agent_is_not_asked_to_exit_while_the_registration_replaces_the_job() {
    // An ordinary update: the old job is removed (absent for a while) and the service's agent still answers.
    let mut s = connected_other(SUPERVISING);
    let cmds = service_now(&mut s, PENDING_NO_JOB);
    assert!(
        !cmds.iter().any(|c| matches!(c, Command::AskToExit { .. })),
        "the service's agent was taken back during an update: {cmds:?}"
    );
    assert!(matches!(s.phase(), Phase::Connected { .. }));
    // The registration ends and nothing is registered (the user switched it off meanwhile): taken back as usual.
    let cmds = service_now(&mut s, NO_SERVICE);
    assert!(cmds.iter().any(|c| matches!(c, Command::AskToExit { .. })), "{cmds:?}");
}

// ------------------------------------------------------------------------------------------------ starting

#[test]
fn starting_never_launches_again_however_many_ticks_pass() {
    let mut s = starting();
    let cmds = run_ticks(&mut s, NOTE_AFTER * 4, |seq, _| Look {
        child: Some((1, Child::Running { pid: PID })),
        ..look(seq)
    });
    assert_eq!(launches(&cmds), 0, "a second agent was launched beside the first");
    let stalls = cmds.iter().filter(|c| matches!(c, Command::Stall { .. })).count();
    assert_eq!(stalls, 1, "one note, not one per look");
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::AgentNotAnswering));
}

#[test]
fn a_start_is_judged_on_the_process_and_its_discovery_file_not_on_a_clock() {
    let judged = |child: Child, file: Option<HintSeen>| {
        let mut s = starting();
        let seq = tick_look(&mut s);
        let cmds = s.step(Event::Looked(Look {
            hint: file,
            child: Some((1, child)),
            ..look(seq)
        }));
        (s, cmds)
    };
    let running = Child::Running { pid: PID };
    // Alive without a file: still starting.
    let (s, cmds) = judged(running, None);
    assert!(cmds.is_empty() && matches!(s.phase(), Phase::Starting { .. }));
    // A file of another process (a stale one) is not our agent's.
    let (s, cmds) = judged(running, Some(hint(7, 1_000, true)));
    assert!(cmds.is_empty() && matches!(s.phase(), Phase::Starting { .. }));
    // A file naming our process but written before it was launched is an old agent's.
    let (_, cmds) = judged(running, Some(hint(PID, 999, true)));
    assert!(cmds.is_empty());
    // A file naming our process, written since: ready (a connection is made, which is the proof).
    let (s, cmds) = judged(running, Some(hint(PID, 1_000, true)));
    assert!(matches!(cmds[0], Command::Connect { .. }));
    assert!(matches!(s.phase(), Phase::Connecting { .. }));
    // Exited because another agent holds the folder: waits for it.
    let (s, cmds) = judged(
        Child::Exited {
            code: Some(dyno_agent::cli::EXIT_ALREADY_RUNNING),
        },
        None,
    );
    assert!(cmds.is_empty());
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
    assert_eq!(s.held(), None);
    // Exited for another reason, even with a file that names it: a failure with its code.
    let (s, cmds) = judged(Child::Exited { code: Some(1) }, Some(hint(PID, 1_000, true)));
    assert!(
        matches!(&cmds[0], Command::Report(r) if r.message.as_deref().is_some_and(|m| m.contains("exit code 1"))),
        "{cmds:?}"
    );
    assert!(matches!(s.phase(), Phase::BackingOff { .. }));
    // Ended by a signal.
    let (_, cmds) = judged(Child::Exited { code: None }, None);
    assert!(matches!(&cmds[0], Command::Report(r) if r.message.as_deref().is_some_and(|m| m.contains("ended while starting"))));
}

#[test]
fn a_file_that_arrives_before_the_launch_is_confirmed_is_not_ready_yet() {
    let (mut s, _) = first(|l| l);
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        hint: Some(hint(PID, 1_000, true)),
        child: Some((1, Child::Running { pid: PID })),
        ..look(seq)
    }));
    assert!(!cmds.iter().any(|c| matches!(c, Command::Connect { .. })));
}

#[test]
fn a_failed_launch_backs_off_and_says_why() {
    let (mut s, _) = first(|l| l);
    let cmds = s.step(Event::Launched {
        child: 1,
        result: Err("no agent program".into()),
    });
    assert!(
        matches!(&cmds[0], Command::Report(r) if r.state == AgentState::Restarting && r.message.as_deref() == Some("no agent program"))
    );
    assert!(
        matches!(s.phase(), Phase::BackingOff { failures: 1, ticks_left } if *ticks_left == u32::try_from(backoff(1).as_secs()).unwrap() * TICKS_PER_SECOND)
    );
    assert_eq!(s.held(), None, "a launch that failed holds nothing");
}

#[test]
fn retry_while_starting_ends_the_child_and_launches_nothing_until_it_is_gone() {
    let mut s = starting();
    let cmds = s.step(Event::Retry { stall: None });
    assert_eq!(cmds, [Command::EndHeldChild { child: 1 }]);
    assert_eq!(*s.phase(), Phase::Ending { child: 1 });
    assert_eq!(s.held(), Some(1), "the child is held until it is gone");
    for cmd in ticks(&mut s, 5) {
        assert!(!matches!(cmd, Command::Launch { .. } | Command::Look { .. }), "{cmd:?}");
    }
    let cmds = s.step(Event::ChildEnded { child: 1, code: None });
    let seq = look_seq(&cmds);
    assert_eq!(s.held(), None);
    let cmds = s.step(Event::Looked(look(seq)));
    assert_eq!(cmds, [Command::Launch { child: 2, anyway: false }]);
}

#[test]
fn the_end_of_an_old_child_never_touches_the_current_one() {
    let mut s = starting();
    s.step(Event::Retry { stall: None });
    let cmds = s.step(Event::ChildEnded { child: 1, code: None });
    let seq = look_seq(&cmds);
    s.step(Event::Looked(look(seq)));
    assert_eq!(s.held(), Some(2));
    // A second, late report of generation 1's end arrives while generation 2 runs.
    assert!(s.step(Event::ChildEnded { child: 1, code: None }).is_empty());
    assert_eq!(s.held(), Some(2));
    assert_eq!(*s.phase(), Phase::Starting { child: 2 });
}

#[test]
fn a_retry_for_a_stall_that_is_over_does_nothing() {
    let mut s = starting();
    assert!(s.step(Event::Retry { stall: Some(99) }).is_empty());
    assert_eq!(*s.phase(), Phase::Starting { child: 1 });
}

#[test]
fn retry_on_a_holder_launches_anyway_once() {
    let (mut s, _) = first(|l| Look {
        lock: Some(LockSeen::Held),
        ..l
    });
    let seq = look_seq(&s.step(Event::Retry { stall: None }));
    let cmds = s.step(Event::Looked(Look {
        lock: Some(LockSeen::Held),
        ..look(seq)
    }));
    assert_eq!(cmds, [Command::Launch { child: 1, anyway: true }]);
}

// ------------------------------------------------------------------------------------------------ connected

#[test]
fn a_connection_to_the_child_we_hold_is_ours_any_other_is_not() {
    let s = connected_own();
    assert!(s.snapshot().started_by_this_app());
    let s = connected_other(NO_SERVICE);
    assert!(s.snapshot().connected() && !s.snapshot().started_by_this_app());
}

#[test]
fn connecting_publishes_the_peer_reports_running_and_looks_at_the_service() {
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        lock: Some(LockSeen::Held),
        ..l
    });
    let c = connect_seq(&cmds);
    let cmds = s.step(Event::Connected {
        seq: c,
        peer: 8,
        pid: Some(9),
    });
    assert_eq!(cmds[0], Command::UsePeer(Some(8)));
    assert!(matches!(&cmds[1], Command::Report(r) if r.state == AgentState::Running));
    assert!(matches!(cmds[2], Command::Look { service: true, .. }));
}

#[test]
fn a_connection_lost_after_it_worked_looks_again_but_one_lost_at_once_backs_off() {
    let mut s = connected_own();
    ticks(&mut s, CONNECTED_FOR);
    let cmds = s.step(Event::PeerClosed { peer: 7 });
    assert_eq!(cmds[0], Command::UsePeer(None));
    assert!(matches!(&cmds[1], Command::Report(r) if r.state == AgentState::Restarting));
    assert!(matches!(s.phase(), Phase::Looking { .. }));
    // The held child still runs: it is starting again, not launched again.
    let seq = look_seq(&cmds);
    let cmds = s.step(Event::Looked(Look {
        child: Some((1, Child::Running { pid: PID })),
        ..look(seq)
    }));
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::Starting { child: 1 });

    let mut s = connected_own();
    s.step(Event::PeerClosed { peer: 7 });
    assert!(matches!(s.phase(), Phase::BackingOff { failures: 1, .. }), "a crash loop backs off");
}

#[test]
fn a_connection_that_is_not_the_current_one_closing_changes_nothing() {
    let mut s = connected_own();
    assert!(s.step(Event::PeerClosed { peer: 1234 }).is_empty());
    assert!(s.snapshot().connected());
}

#[test]
fn a_backoff_counts_ticks_then_looks_and_a_long_run_of_failures_is_shown() {
    let (mut s, mut seq) = begun();
    let mut shown = None;
    for n in 1..=FAILED_AFTER {
        let cmds = s.step(Event::Looked(look(seq)));
        assert_eq!(launches(&cmds), 1);
        let cmds = s.step(Event::Launched {
            child: s.held().unwrap(),
            result: Err("nope".into()),
        });
        shown = shown.or(stall_of(&cmds));
        let Phase::BackingOff { failures, ticks_left } = *s.phase() else {
            panic!("{:?}", s.phase())
        };
        assert_eq!(failures, n);
        let mut next = None;
        for _ in 0..ticks_left {
            next = next.or(find_look(&s.step(Event::Tick)));
        }
        seq = next.expect("the backoff ended without a look").0;
    }
    assert_eq!(shown, Some(Stall::AgentKeepsFailing));
}

// ------------------------------------------------------------------------------------------------ hand-over and take-back

#[test]
fn a_service_that_now_supervises_takes_over_from_the_agent_this_app_started() {
    let mut s = connected_own();
    let cmds = service_now(&mut s, SUPERVISING);
    assert_eq!(
        cmds,
        [
            Command::UsePeer(None),
            Command::AskToExit { peer: 7 },
            Command::EndHeldChild { child: 1 }
        ]
    );
    assert_eq!(*s.phase(), Phase::Ending { child: 1 });
    // Only when the child is gone does the machine look; the service supervises, so it waits and launches nothing.
    let seq = look_seq(&s.step(Event::ChildEnded { child: 1, code: Some(0) }));
    let cmds = s.step(Event::Looked(Look {
        service: Some(SUPERVISING),
        ..look(seq)
    }));
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::WaitingForService);
}

#[test]
fn nothing_is_handed_over_once_the_service_has_shown_it_starts_nothing() {
    let mut s = connected_own();
    s.fallback = true;
    assert!(service_now(&mut s, SUPERVISING).is_empty());
    assert!(s.snapshot().connected());
}

#[test]
fn a_supervising_service_never_makes_the_app_end_an_agent_it_did_not_start() {
    let mut s = connected_other(SUPERVISING);
    assert!(service_now(&mut s, SUPERVISING).is_empty(), "rule 53: nothing is ended or asked");
}

#[test]
fn a_removed_service_is_asked_to_exit_and_nothing_launches_until_the_lock_is_free() {
    let mut s = connected_other(SUPERVISING);
    let cmds = service_now(&mut s, NO_SERVICE);
    assert_eq!(cmds, [Command::UsePeer(None), Command::AskToExit { peer: 8 }]);
    assert_eq!(*s.phase(), Phase::TakingBack { peer: 8 });
    // The connection closes as the agent exits; the lock is not yet released (Windows): nothing launches.
    assert!(s.step(Event::PeerClosed { peer: 8 }).is_empty());
    for _ in 0..3 {
        let seq = tick_look(&mut s);
        let cmds = s.step(Event::Looked(Look {
            lock: Some(LockSeen::Held),
            hint: Some(hint(9, 500, true)),
            ..look(seq)
        }));
        assert_eq!(launches(&cmds), 0);
        assert!(
            !cmds.iter().any(|c| matches!(c, Command::Connect { .. })),
            "reconnected to the agent it asked to exit"
        );
    }
    // Released: a fresh look decides, and only then does it launch.
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(look(seq)));
    assert_eq!(launches(&cmds), 0);
    let seq = look_seq(&cmds);
    let cmds = s.step(Event::Looked(look(seq)));
    assert_eq!(launches(&cmds), 1);
}

// ------------------------------------------------------------------------------------------------ quit

#[test]
fn quit_decides_once_and_a_later_exit_cannot_change_it() {
    let mut s = connected_own();
    let cmds = s.step(Event::Quit);
    assert_eq!(
        cmds,
        [
            Command::UsePeer(None),
            Command::AskToExit { peer: 7 },
            Command::EndHeldChild { child: 1 }
        ]
    );
    assert_eq!(
        *s.phase(),
        Phase::Quitting {
            outcome: QuitOutcome { stopped_own: true }
        }
    );
    assert!(s.step(Event::Quit).is_empty(), "a second quit decides nothing");
    let cmds = s.step(Event::ChildEnded { child: 1, code: Some(0) });
    assert_eq!(cmds, [Command::Done(QuitOutcome { stopped_own: true })]);
    assert_eq!(
        *s.phase(),
        Phase::Stopped {
            outcome: QuitOutcome { stopped_own: true }
        }
    );
    assert!(s.finished());
}

#[test]
fn quit_leaves_an_agent_a_service_keeps_and_one_it_did_not_start() {
    let mut s = connected_own();
    s.step(Event::ServiceInstalled(true));
    let cmds = s.step(Event::Quit);
    assert!(!cmds.iter().any(|c| matches!(c, Command::EndHeldChild { .. })));
    assert_eq!(
        *s.phase(),
        Phase::Stopped {
            outcome: QuitOutcome { stopped_own: false }
        }
    );
    let mut s = connected_other(NO_SERVICE);
    let cmds = s.step(Event::Quit);
    assert_eq!(cmds[1], Command::CloseConnection { peer: 8 });
    assert_eq!(
        *s.phase(),
        Phase::Stopped {
            outcome: QuitOutcome { stopped_own: false }
        }
    );
}

#[test]
fn quit_while_the_launch_is_in_flight_ends_the_child_that_appears() {
    let (mut s, _) = first(|l| l);
    let cmds = s.step(Event::Quit);
    assert!(cmds.contains(&Command::EndHeldChild { child: 1 }));
    assert_eq!(
        *s.phase(),
        Phase::Quitting {
            outcome: QuitOutcome { stopped_own: true }
        }
    );
    let cmds = s.step(Event::Launched {
        child: 1,
        result: Ok(Launch { at_ms: 1, pid: Some(PID) }),
    });
    assert!(cmds.is_empty(), "the end already queued behind the launch ends it");
    s.step(Event::ChildEnded { child: 1, code: None });
    assert!(s.finished());
}

#[test]
fn quit_while_the_launch_fails_has_nothing_left_to_wait_for() {
    let (mut s, _) = first(|l| l);
    s.step(Event::Quit);
    let cmds = s.step(Event::Launched {
        child: 1,
        result: Err("x".into()),
    });
    assert_eq!(cmds, [Command::Done(QuitOutcome { stopped_own: true })]);
}

#[test]
fn quit_while_a_retry_is_ending_the_child_still_stops_it_and_launches_nothing() {
    let mut s = starting();
    s.step(Event::Retry { stall: None });
    let cmds = s.step(Event::Quit);
    assert!(cmds.contains(&Command::EndHeldChild { child: 1 }));
    s.step(Event::ChildEnded { child: 1, code: None });
    assert!(s.finished());
    assert!(s.step(Event::Tick).is_empty());
}

#[test]
fn quit_without_a_child_stops_at_once_and_a_connection_after_the_end_is_closed() {
    let (mut s, _) = begun();
    let cmds = s.step(Event::Quit);
    assert_eq!(cmds, [Command::UsePeer(None), Command::Done(QuitOutcome { stopped_own: false })]);
    assert_eq!(
        s.step(Event::Connected {
            seq: 1,
            peer: 4,
            pid: None
        }),
        [Command::CloseConnection { peer: 4 }]
    );
    assert!(s.step(Event::Tick).is_empty());
}

#[test]
fn quit_before_the_link_began_stops_without_a_word() {
    let mut s = Supervisor::new();
    s.step(Event::Quit);
    assert!(s.finished());
}

// ------------------------------------------------------------------------------------------------ every wait has a cause and a Retry

#[test]
fn every_wait_shows_its_cause_after_the_note_and_retry_acts_on_it() {
    let in_replacing = || {
        let (mut s, cmds) = first(|l| Look {
            hint: Some(hint(9, 500, false)),
            lock: Some(LockSeen::Held),
            ..l
        });
        let c = connect_seq(&cmds);
        s.step(Event::Connected {
            seq: c,
            peer: 5,
            pid: Some(9),
        });
        s
    };
    let in_taking_back = || {
        let mut s = connected_other(SUPERVISING);
        service_now(&mut s, NO_SERVICE);
        s
    };
    let in_ending = || {
        let mut s = starting();
        s.step(Event::Retry { stall: None });
        s
    };
    let in_backoff = || {
        let (mut s, _) = first(|l| l);
        s.step(Event::Launched {
            child: 1,
            result: Err("x".into()),
        });
        s
    };
    let connecting = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        lock: Some(LockSeen::Held),
        ..l
    })
    .0;
    let cases: Vec<(&str, Supervisor, Stall)> = vec![
        ("looking", begun().0, Stall::ServiceNotAnswering),
        ("connecting", connecting, Stall::ServiceNotAnswering),
        ("starting", starting(), Stall::AgentNotAnswering),
        (
            "service",
            first(|l| Look {
                service: Some(SUPERVISING),
                ..l
            })
            .0,
            Stall::WaitingForService,
        ),
        (
            "holder",
            first(|l| Look {
                lock: Some(LockSeen::Held),
                ..l
            })
            .0,
            Stall::OtherAgentHolds,
        ),
        ("replacing", in_replacing(), Stall::OtherAgentHolds),
        ("taking back", in_taking_back(), Stall::OtherAgentHolds),
        ("ending", in_ending(), Stall::AgentNotAnswering),
        ("backing off", in_backoff(), Stall::AgentKeepsFailing),
    ];
    for (name, mut s, cause) in cases {
        // No answer to any look: only ticks. The cause must be shown by the time the count reaches the note.
        let mut shown = None;
        for _ in 0..NOTE_AFTER {
            shown = shown.or(stall_of(&s.step(Event::Tick)));
            assert!(!s.overdue(), "{name}: overdue without a cause");
        }
        // (a backoff of 1 s ends long before the note; its failures shown are covered by the backoff test)
        if name == "backing off" {
            continue;
        }
        let shown = shown.unwrap_or_else(|| panic!("{name}: no stall after {NOTE_AFTER} ticks"));
        assert_eq!(shown, cause, "{name}");
    }
}

#[test]
fn retry_is_defined_in_every_phase_and_changes_what_it_should() {
    // Looking: asks again with a new number, so a look that never came is not waited for.
    let (mut s, seq) = begun();
    let new = look_seq(&s.step(Event::Retry { stall: None }));
    assert_ne!(new, seq);
    assert!(s.step(Event::Looked(look(seq))).is_empty(), "the old answer is stale");
    // Backing off: looks at once.
    let (mut s, _) = first(|l| l);
    s.step(Event::Launched {
        child: 1,
        result: Err("x".into()),
    });
    assert!(matches!(s.step(Event::Retry { stall: None })[0], Command::Look { .. }));
    // Connected, Idle: nothing.
    assert!(connected_own().step(Event::Retry { stall: None }).is_empty());
    assert!(Supervisor::new().step(Event::Retry { stall: None }).is_empty());
}

#[test]
fn the_service_changing_asks_the_os_again_and_the_newest_answer_wins() {
    let mut s = connected_own();
    let old = s.look_out.unwrap();
    let cmds = s.step(Event::ServiceChanged);
    assert!(matches!(cmds[..], [Command::Look { service: true, .. }]));
    assert!(service_look_is_stale(&mut s, old), "the look taken before the change is dropped");
}

fn service_look_is_stale(s: &mut Supervisor, seq: Seq) -> bool {
    s.step(Event::Looked(Look {
        service: Some(SUPERVISING),
        ..look(seq)
    }))
    .is_empty()
}

#[test]
fn the_first_request_waits_for_a_connection_a_stall_or_a_failure() {
    let (s, _) = begun();
    assert!(!s.snapshot().settled());
    assert!(Supervisor::new().snapshot().settled(), "a link that never began fails fast");
    assert!(connected_own().snapshot().settled());
    let mut s = starting();
    ticks(&mut s, NOTE_AFTER);
    assert!(s.snapshot().settled(), "a shown stall ends the wait for the first connection");
}

// ------------------------------------------------------------------------------------------------ review: launch failures and old numbers

#[test]
fn a_launch_that_fails_while_retry_is_ending_it_does_not_leave_the_link_stuck() {
    let (mut s, _) = first(|l| l);
    s.step(Event::Retry { stall: None });
    assert_eq!(*s.phase(), Phase::Ending { child: 1 });
    // The launch failed in the meantime: there is no child to wait for, and the end that follows reports nothing to end.
    let cmds = s.step(Event::Launched {
        child: 1,
        result: Err("missing".into()),
    });
    assert_eq!(s.held(), None);
    assert!(matches!(s.phase(), Phase::Looking { .. }), "stuck in {:?}: {cmds:?}", s.phase());
    // The late report of the end changes nothing.
    let seq = look_seq(&cmds);
    assert!(s.step(Event::ChildEnded { child: 1, code: None }).is_empty());
    assert_eq!(*s.phase(), Phase::Looking { seq });
}

#[test]
fn a_launch_answer_about_an_old_child_never_touches_the_current_one() {
    let mut s = starting();
    s.step(Event::Retry { stall: None });
    let seq = look_seq(&s.step(Event::ChildEnded { child: 1, code: None }));
    s.step(Event::Looked(look(seq)));
    assert_eq!(*s.phase(), Phase::Starting { child: 2 });
    // A late "generation 1 could not be launched" while generation 2 starts.
    assert!(s
        .step(Event::Launched {
            child: 1,
            result: Err("old".into())
        })
        .is_empty());
    assert_eq!(*s.phase(), Phase::Starting { child: 2 });
    assert_eq!(s.held(), Some(2));
}

#[test]
fn a_connection_to_an_agent_with_another_pid_is_not_ours_even_while_a_child_is_held() {
    // Rule 53: ours is decided by the pid of the process this app holds, not by "a child is held".
    let mut s = starting();
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        hint: Some(hint(PID, 1_000, true)),
        child: Some((1, Child::Running { pid: PID })),
        ..look(seq)
    }));
    let c = connect_seq(&cmds);
    s.step(Event::Connected {
        seq: c,
        peer: 7,
        pid: Some(PID + 1),
    });
    assert_eq!(
        *s.phase(),
        Phase::Connected {
            peer: 7,
            owner: Owner::Other
        }
    );
}

#[test]
fn the_agent_lock_is_probed_only_by_the_looks_that_need_it() {
    // The lock probe holds the lock for an instant; the agent this app launched, or the one the service starts, claims it at
    // its start, and the two meeting makes that agent's claim fail (exit 3, a second launch). A start in progress never probes.
    let mut s = starting();
    let cmds = s.step(Event::Tick);
    assert!(matches!(cmds[..], [Command::Look { lock: false, .. }]), "{cmds:?}");
    // A wait for a holder probes on every LOCK_EVERY-th tick only; the deciding look and a retired agent's wait always do.
    let (mut s, _) = first(|l| Look {
        lock: Some(LockSeen::Held),
        ..l
    });
    let mut probed = Vec::new();
    for _ in 0..LOCK_EVERY {
        let cmds = s.step(Event::Tick);
        let Some((seq, _)) = find_look(&cmds) else { panic!("{cmds:?}") };
        probed.push(matches!(cmds[..], [Command::Look { lock: true, .. }]));
        s.step(Event::Looked(Look { lock: None, ..look(seq) }));
    }
    assert_eq!(probed.iter().filter(|p| **p).count(), 1, "{probed:?}");
    assert!(probed[(LOCK_EVERY - 1) as usize]);
}

#[test]
fn a_look_that_did_not_probe_the_lock_never_launches_and_asks_again_with_the_lock() {
    // In a wait: it goes on (the file may still be worth a connection, the lock is not known).
    let (mut s, _) = first(|l| Look {
        lock: Some(LockSeen::Held),
        ..l
    });
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look { lock: None, ..look(seq) }));
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
    // In a decision: it probes.
    let (mut s, seq) = begun();
    let cmds = s.step(Event::Looked(Look { lock: None, ..look(seq) }));
    assert_eq!(launches(&cmds), 0);
    assert!(matches!(cmds[..], [Command::Look { lock: true, .. }]), "{cmds:?}");
}

/// ADR 0031: an agent the OS or another app (App Tamer) has stopped is waited for, never ended. The chip says it is paused and
/// has no Retry; a Retry that arrives anyway does nothing; when it runs again the note goes away.
#[test]
fn a_stopped_agent_is_reported_as_paused_and_is_never_ended_or_replaced() {
    let mut s = starting();
    let stopped = |s: &mut Supervisor| {
        let seq = tick_look(s);
        s.step(Event::Looked(Look {
            child: Some((1, Child::Stopped { pid: PID })),
            ..look(seq)
        }))
    };
    let cmds = stopped(&mut s);
    assert_eq!(stall_of(&cmds), Some(Stall::AgentPaused), "{cmds:?}");
    assert!(matches!(s.phase(), Phase::Starting { child: 1 }));
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::AgentPaused));
    // Looks go on for as long as it is stopped (a minute, an hour): nothing is ended, launched or said again.
    for _ in 0..500 {
        let cmds = stopped(&mut s);
        assert!(
            cmds.iter()
                .all(|c| !matches!(c, Command::EndHeldChild { .. } | Command::Launch { .. } | Command::Stall { .. })),
            "{cmds:?}"
        );
    }
    assert!(matches!(s.phase(), Phase::Starting { child: 1 }));
    // A Retry (by the stall's id, or without one) does not end a stopped agent.
    let id = s.snapshot().stall.map(|(i, _)| i);
    for retry in [Event::Retry { stall: id }, Event::Retry { stall: None }] {
        let cmds = s.step(retry);
        assert!(
            cmds.iter()
                .all(|c| !matches!(c, Command::EndHeldChild { .. } | Command::Launch { .. })),
            "{cmds:?}"
        );
        assert!(matches!(s.phase(), Phase::Starting { child: 1 }));
    }
    // It runs again (App Tamer resumed it): the note is withdrawn and the start is judged as before.
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        child: Some((1, Child::Running { pid: PID })),
        ..look(seq)
    }));
    assert!(cmds.iter().any(|c| matches!(c, Command::ClearStall)), "{cmds:?}");
    assert_eq!(s.snapshot().stall, None);
}

#[test]
fn a_stopped_agent_that_is_gone_when_next_looked_at_is_judged_as_exited() {
    let mut s = starting();
    let seq = tick_look(&mut s);
    s.step(Event::Looked(Look {
        child: Some((1, Child::Stopped { pid: PID })),
        ..look(seq)
    }));
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        child: Some((1, Child::Exited { code: Some(1) })),
        ..look(seq)
    }));
    assert!(matches!(s.phase(), Phase::BackingOff { .. }), "{cmds:?}");
}

/// Only the look that starts an attempt is a decision look; the looks of a wait, after a connection and after a service change are
/// not (they always ask the service for what they need).
#[test]
fn only_the_look_that_starts_an_attempt_is_a_decision_look() {
    let is_deciding = |cmds: &[Command]| {
        cmds.iter().find_map(|c| {
            if let Command::Look { deciding, .. } = c {
                Some(*deciding)
            } else {
                None
            }
        })
    };
    assert_eq!(is_deciding(&Supervisor::new().step(Event::Begin)), Some(true));
    let mut waiting = starting();
    let cmds = waiting.step(Event::Tick);
    assert_eq!(
        is_deciding(&cmds),
        Some(false),
        "the look of a wait is not a decision look: {cmds:?}"
    );
    let mut connected = connected_own();
    let cmds = connected.step(Event::ServiceChanged);
    assert_eq!(is_deciding(&cmds), Some(false), "{cmds:?}");
}

/// The decision look no longer asks the login service when an agent is there to connect to, so the machine can be connected to an
/// agent without ever having heard from the service. The service changing right then (the person switched Start at login off) must
/// still take the service's agent back: the change is the evidence, there is no earlier answer to compare with. (A first answer
/// that is not a reply to a change is only a baseline: a foreign agent is never taken back for it.)
#[test]
fn a_service_removed_before_its_first_answer_still_takes_the_agent_back_but_a_baseline_answer_does_not() {
    // Connected to the agent of the file, with the post-connect service look out and unanswered: (machine, that look's seq).
    let connected_without_service = || {
        let (mut s, cmds) = first(|l| Look {
            hint: Some(hint(9, 500, true)),
            lock: Some(LockSeen::Held),
            service: None,
            ..l
        });
        let c = connect_seq(&cmds);
        let after = s.step(Event::Connected {
            seq: c,
            peer: 8,
            pid: Some(9),
        });
        (s, look_seq(&after))
    };
    // The change arrives, and its look says: nothing registered, nothing supervising.
    let (mut s, _) = connected_without_service();
    let cmds = service_now(&mut s, NO_SERVICE);
    assert!(cmds.iter().any(|c| matches!(c, Command::AskToExit { peer: 8 })), "{cmds:?}");
    assert!(matches!(s.phase(), Phase::TakingBack { .. }));
    // The post-connect look's first answer, with no change behind it, is a baseline: nothing is taken back.
    let (mut s, seq) = connected_without_service();
    let cmds = s.step(Event::Looked(Look {
        service: Some(NO_SERVICE),
        ..look(seq)
    }));
    assert!(cmds.iter().all(|c| !matches!(c, Command::AskToExit { .. })), "{cmds:?}");
    assert!(matches!(s.phase(), Phase::Connected { .. }));
}

/// A look of the wait for the login service's agent that asks the service is a recheck look: the adapter may skip the service when
/// the lock is held (an agent is there). The decision look and the looks after a connection or a service change are not.
#[test]
fn the_service_recheck_of_a_wait_is_marked_so_a_held_lock_can_skip_it() {
    let recheck = |cmds: &[Command]| {
        cmds.iter().find_map(|c| {
            if let Command::Look { recheck, service, .. } = c {
                Some((*recheck, *service))
            } else {
                None
            }
        })
    };
    let (mut s, _) = first(|l| Look {
        service: Some(SUPERVISING),
        ..l
    });
    assert_eq!(*s.phase(), Phase::WaitingForService);
    let mut seen_recheck = false;
    for _ in 0..200 {
        let cmds = s.step(Event::Tick);
        if let Some((is_recheck, asks)) = recheck(&cmds) {
            assert_eq!(
                is_recheck, asks,
                "a wait look that asks the service is a recheck, and only that one: {cmds:?}"
            );
            seen_recheck |= is_recheck;
            let seq = look_seq(&cmds);
            s.step(Event::Looked(Look {
                service: asks.then_some(SUPERVISING),
                lock: Some(LockSeen::Held),
                ..look(seq)
            }));
        }
    }
    assert!(seen_recheck, "the wait rechecks the service now and then");
    assert_eq!(
        recheck(&s.step(Event::ServiceChanged)),
        Some((false, true)),
        "a service change always asks"
    );
}

#[test]
fn an_agent_of_the_file_on_disk_that_the_app_cannot_speak_to_is_never_asked_to_exit() {
    // The app was updated while it runs: the agent is the new build (the file on disk) with another protocol. Asking it to exit
    // only makes the service start the same agent again, forever (Dyno lesson 2026-11-07).
    let (s, cmds) = first(|l| Look {
        hint: Some(HintSeen {
            of_disk_file: true,
            ..hint(9, 500, false)
        }),
        lock: Some(LockSeen::Held),
        ..l
    });
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, Command::Connect { .. } | Command::Launch { .. } | Command::RemoveHint { .. })),
        "the agent of the new build was dealt with: {cmds:?}"
    );
    assert!(!matches!(s.phase(), Phase::Connecting { .. }), "{:?}", s.phase());
}

#[test]
fn repeated_replacements_of_agents_that_come_back_give_up_within_a_bound() {
    // A service restarts the agent after each eviction and it is of another build each time.
    let mut s = Supervisor::new();
    let mut seq = look_seq(&s.step(Event::Begin));
    let mut asks = 0;
    for _ in 0..30 {
        let cmds = s.step(Event::Looked(Look {
            hint: Some(hint(9, 500, false)),
            lock: Some(LockSeen::Held),
            ..look(seq)
        }));
        let Some(c) = cmds.iter().find_map(|c| match c {
            Command::Connect { seq, .. } => Some(*seq),
            _ => None,
        }) else {
            break; // gave up: no connection to evict through
        };
        let cmds = s.step(Event::Connected {
            seq: c,
            peer: 5,
            pid: Some(9),
        });
        if cmds.contains(&Command::AskToExit { peer: 5 }) {
            asks += 1;
        }
        // It exits: the lock is free, the file goes, and a new look follows.
        let t = tick_look(&mut s);
        let cmds = s.step(Event::Looked(Look {
            hint: Some(hint(9, 500, false)),
            ..look(t)
        }));
        seq = look_seq(&cmds);
    }
    assert!(asks <= 3, "asked {asks} agents to exit in a row; it never gave up");
}

/// The lines logged so far: a test counts only what comes after this mark (the capture is process-wide and never cleared).
fn log_mark() -> usize {
    crate::logging::testlog::lines().len()
}

/// How many lines logged after `mark` contain `needle`. Use a pid of the test's own in `needle`: tests run in parallel.
fn lines_since(mark: usize, needle: &str) -> usize {
    crate::logging::testlog::lines()
        .iter()
        .skip(mark)
        .filter(|l| l.contains(needle))
        .count()
}

/// [`starting`] for an agent with the pid `pid` (a test that counts log lines names its own).
fn starting_as(pid: u32) -> Supervisor {
    let (mut s, _) = first(|l| l);
    s.step(Event::Launched {
        child: 1,
        result: Ok(Launch {
            at_ms: 1_000,
            pid: Some(pid),
        }),
    });
    s
}

#[test]
fn an_own_agent_of_the_file_on_disk_with_another_protocol_is_waited_for_not_connected_to() {
    // No login service: the app's old agent exited and the app launched the new file, which this app cannot speak to.
    let pid = 880_010;
    let mark = log_mark();
    let mut s = starting_as(pid);
    let cmds = run_ticks(&mut s, NOTE_AFTER * 4, |seq, _| Look {
        hint: Some(HintSeen {
            of_disk_file: true,
            ..hint(pid, 1_500, false)
        }),
        lock: Some(LockSeen::Held),
        child: Some((1, Child::Running { pid })),
        app_updated: true,
        ..look(seq)
    });
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, Command::Connect { .. } | Command::EndHeldChild { .. } | Command::Launch { .. })),
        "a connection that the driver refuses, or the child ended: {:?}",
        cmds.iter().filter(|c| matches!(c, Command::Connect { .. })).count()
    );
    assert_eq!(
        s.snapshot().stall.map(|(_, k)| k),
        Some(Stall::AppUpdated),
        "the update state, not a holder"
    );
    assert_eq!(cmds.iter().filter(|c| matches!(c, Command::Stall { .. })).count(), 1, "one note");
    assert_eq!(
        lines_since(mark, &format!("pid {pid} is the installed build")),
        1,
        "logged on entering the state only"
    );
}

#[test]
fn the_update_wait_shows_the_update_state_at_once_and_logs_once() {
    let mark = log_mark();
    let (s, cmds) = first(|l| Look {
        hint: Some(HintSeen {
            of_disk_file: true,
            ..hint(880_001, 500, false)
        }),
        lock: Some(LockSeen::Held),
        app_updated: true,
        ..l
    });
    assert_eq!(stall_of(&cmds), Some(Stall::AppUpdated), "{cmds:?}");
    let mut s = s;
    run_ticks(&mut s, NOTE_AFTER * 3, |seq, _| Look {
        hint: Some(HintSeen {
            of_disk_file: true,
            ..hint(880_001, 500, false)
        }),
        lock: Some(LockSeen::Held),
        app_updated: true,
        ..look(seq)
    });
    assert_eq!(
        s.snapshot().stall.map(|(_, k)| k),
        Some(Stall::AppUpdated),
        "never the holder's note"
    );
    assert_eq!(lines_since(mark, "pid 880001 is the installed build"), 1);
}

#[test]
fn giving_up_on_replacing_agents_says_so_with_its_own_stall_and_logs_once() {
    let mark = log_mark();
    let mut s = Supervisor::new();
    let mut seq = look_seq(&s.step(Event::Begin));
    let mut gave_up = None;
    for _ in 0..8 {
        let cmds = s.step(Event::Looked(Look {
            hint: Some(hint(880_002, 500, false)),
            lock: Some(LockSeen::Held),
            ..look(seq)
        }));
        if let Some(st) = stall_of(&cmds) {
            gave_up = Some(st);
            break;
        }
        let c = connect_seq(&cmds);
        s.step(Event::Connected {
            seq: c,
            peer: 5,
            pid: Some(880_002),
        });
        let t = tick_look(&mut s);
        let cmds = s.step(Event::Looked(Look {
            hint: Some(hint(880_002, 500, false)),
            ..look(t)
        }));
        seq = look_seq(&cmds);
    }
    assert_eq!(gave_up, Some(Stall::OtherBuildKeepsComing));
    run_ticks(&mut s, NOTE_AFTER * 2, |seq, _| Look {
        hint: Some(hint(880_002, 500, false)),
        lock: Some(LockSeen::Held),
        ..look(seq)
    });
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::OtherBuildKeepsComing));
    assert_eq!(
        lines_since(mark, "pid 880002 keep coming back"),
        1,
        "logged once, not on every look"
    );
}

fn disk_hint(pid: u32) -> Option<HintSeen> {
    Some(HintSeen {
        of_disk_file: true,
        ..hint(pid, 1_500, false)
    })
}

#[test]
fn a_look_without_a_file_does_not_end_the_update_wait_of_a_running_own_agent() {
    let pid = 880_011;
    let mut s = starting_as(pid);
    let cmds = run_ticks(&mut s, 60, |seq, _| Look {
        // Every third look misses the discovery file (it is read while the agent rewrites it).
        hint: if seq % 3 == 0 { None } else { disk_hint(pid) },
        lock: Some(LockSeen::Held),
        child: Some((1, Child::Running { pid })),
        app_updated: true,
        ..look(seq)
    });
    assert_eq!(
        cmds.iter().filter(|c| matches!(c, Command::Stall { .. })).count(),
        1,
        "the note was cleared and shown again: {cmds:?}"
    );
    assert!(!cmds.iter().any(|c| matches!(c, Command::ClearStall)), "{cmds:?}");
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::AppUpdated));
}

#[test]
fn an_agent_of_the_installed_file_is_an_update_only_when_the_app_was_updated_otherwise_a_mismatch() {
    for (updated, want) in [(true, Stall::AppUpdated), (false, Stall::AgentMismatch)] {
        let mut s = starting();
        let cmds = run_ticks(&mut s, 5, |seq, _| Look {
            hint: disk_hint(PID),
            lock: Some(LockSeen::Held),
            child: Some((1, Child::Running { pid: PID })),
            app_updated: updated,
            ..look(seq)
        });
        assert_eq!(stall_of(&cmds), Some(want), "app_updated {updated}");
        assert!(!cmds.iter().any(|c| matches!(c, Command::Connect { .. })));
    }
}

#[test]
fn a_later_stall_of_another_kind_replaces_the_one_shown() {
    let (mut s, cmds) = first(|l| Look {
        hint: disk_hint(880_003),
        lock: Some(LockSeen::Held),
        app_updated: true,
        ..l
    });
    assert_eq!(stall_of(&cmds), Some(Stall::AppUpdated));
    // Eviction attempts were used up meanwhile; the next look sees an ordinary agent of another build.
    s.evictions = MAX_EVICTIONS;
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        hint: Some(hint(880_003, 500, false)),
        lock: Some(LockSeen::Held),
        ..look(seq)
    }));
    assert_eq!(stall_of(&cmds), Some(Stall::OtherBuildKeepsComing), "{cmds:?}");
    assert!(
        cmds.iter().any(|c| matches!(c, Command::ClearStall)),
        "the old note was not cleared first: {cmds:?}"
    );
    assert_eq!(s.snapshot().stall.map(|(_, k)| k), Some(Stall::OtherBuildKeepsComing));
}

#[test]
fn a_look_without_a_file_does_not_end_the_update_wait_when_the_login_service_supervises() {
    let (mut s, cmds) = first(|l| Look {
        hint: disk_hint(880_012),
        lock: Some(LockSeen::Held),
        service: Some(SUPERVISING),
        app_updated: true,
        ..l
    });
    assert_eq!(stall_of(&cmds), Some(Stall::AppUpdated));
    let cmds = run_ticks(&mut s, 60, |seq, _| Look {
        hint: if seq % 3 == 0 { None } else { disk_hint(880_012) },
        lock: Some(LockSeen::Held),
        service: Some(SUPERVISING),
        app_updated: true,
        ..look(seq)
    });
    assert!(
        !cmds.iter().any(|c| matches!(c, Command::ClearStall | Command::Stall { .. })),
        "the note was cleared or shown again: {cmds:?}"
    );
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
}

#[test]
fn the_update_wait_ends_when_the_agent_is_gone_and_the_lock_is_free() {
    let (mut s, _) = first(|l| Look {
        hint: disk_hint(880_013),
        lock: Some(LockSeen::Held),
        app_updated: true,
        ..l
    });
    let seq = tick_look(&mut s);
    let cmds = s.step(Event::Looked(Look {
        hint: None,
        lock: Some(LockSeen::Free),
        ..look(seq)
    }));
    assert_eq!(launches(&cmds), 1, "an agent that is gone is started again: {cmds:?}");
}
