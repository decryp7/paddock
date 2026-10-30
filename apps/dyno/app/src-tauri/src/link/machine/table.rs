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
        lock: LockSeen::Free,
        service: None,
        child: None,
        cpu: Cpu::NotRead,
    }
}

fn hint(pid: u32, started_ms: i64, same_build: bool) -> HintSeen {
    HintSeen {
        pid,
        started_ms,
        version: "1.0.0".into(),
        protocol: 1,
        same_build,
    }
}

const SUPERVISING: ServiceState = ServiceState {
    installed: true,
    supervises: true,
};
const NO_SERVICE: ServiceState = ServiceState {
    installed: false,
    supervises: false,
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
            Command::Connect { seq } => Some(*seq),
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
        lock: LockSeen::Held,
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
            cpu: false
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
    let (s, cmds) = first(|l| Look { lock: LockSeen::Held, ..l });
    assert_eq!(launches(&cmds), 0);
    assert_eq!(*s.phase(), Phase::WaitingForHolder);
}

#[test]
fn a_file_of_this_build_beside_a_held_lock_is_connected_to() {
    let (s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, true)),
        lock: LockSeen::Held,
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
    let (mut s, _) = first(|l| Look { lock: LockSeen::Held, ..l });
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
        lock: LockSeen::Held,
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
        lock: LockSeen::Held,
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
        lock: LockSeen::Held,
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
        lock: LockSeen::Held,
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
            lock: LockSeen::Held,
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
    assert_eq!(cmds[0], Command::RemoveHint);
    assert!(matches!(cmds[1], Command::Look { .. }));
    assert_eq!(launches(&cmds), 0);
}

#[test]
fn a_file_of_another_build_with_a_free_lock_is_removed_and_ours_launches() {
    let (_, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, false)),
        ..l
    });
    assert_eq!(cmds[0], Command::RemoveHint);
    assert_eq!(launches(&cmds), 1);
}

#[test]
fn retry_on_an_agent_asked_to_exit_asks_again() {
    let (mut s, cmds) = first(|l| Look {
        hint: Some(hint(9, 500, false)),
        lock: LockSeen::Held,
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
        lock: LockSeen::Held,
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

#[test]
fn a_wait_for_the_service_shows_its_cause_then_retry_starts_the_apps_own() {
    let (mut s, _) = first(|l| Look {
        service: Some(SUPERVISING),
        lock: LockSeen::Held,
        ..l
    });
    run_ticks(&mut s, NOTE_AFTER, |seq, service| Look {
        lock: LockSeen::Held,
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
            lock: if held { LockSeen::Held } else { LockSeen::Free },
            ..look(seq)
        });
        launched += launches(&cmds);
    }
    assert_eq!(launched, 0, "a service that was running was replaced");
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
    let (mut s, _) = first(|l| Look { lock: LockSeen::Held, ..l });
    let seq = look_seq(&s.step(Event::Retry { stall: None }));
    let cmds = s.step(Event::Looked(Look {
        lock: LockSeen::Held,
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
        lock: LockSeen::Held,
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
            lock: LockSeen::Held,
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
            lock: LockSeen::Held,
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
        lock: LockSeen::Held,
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
        ("holder", first(|l| Look { lock: LockSeen::Held, ..l }).0, Stall::OtherAgentHolds),
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
