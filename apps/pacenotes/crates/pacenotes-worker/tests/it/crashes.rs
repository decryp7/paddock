//! The worker is a process: a crash, an exit, a stack overflow, a spin and a block each cost one file and nothing else.

use std::sync::Arc;
use std::time::Duration;

use pacenotes_index::{SourceError, SymbolSource};
use pacenotes_worker::{OsProbe, ParseWorkers};
use paddock_jobs::CancelToken;

use crate::support::{FixedProbe, TickProbe, WORKER, p, settings, workers};

const GOOD: &[u8] =
    b"pub struct Shape;\n\nimpl Shape {\n    pub fn area(&self) -> u32 { helper(2) }\n}\n\npub fn helper(x: u32) -> u32 { x }\n";

fn extract(w: &ParseWorkers, path: &str, bytes: &[u8]) -> Result<Option<pacenotes_index::FileFacts>, SourceError> {
    w.extract(&p(path), bytes, &CancelToken::new())
}

#[test]
fn a_file_is_parsed_in_the_worker_and_the_worker_is_reused() {
    let w = workers();
    let facts = extract(&w, "src/lib.rs", GOOD).unwrap().unwrap();
    let names: Vec<_> = facts.symbols.iter().map(|s| s.qualified_name.as_str()).collect();
    assert_eq!(names, ["Shape", "Shape::area", "helper"]);
    assert_eq!(facts.language.id(), "rust");
    assert!(
        facts
            .calls
            .iter()
            .any(|c| c.callee == "helper" && c.caller.as_deref() == Some("Shape::area"))
    );
    assert!(extract(&w, "src/lib.rs", GOOD).unwrap().is_some());
    assert_eq!(w.started(), 1, "one process served both files");
    assert_eq!(
        extract(&w, "data.json", b"{}").unwrap(),
        None,
        "a file in no known language has no facts"
    );
    assert!(!w.started().eq(&0));
}

#[test]
fn the_worker_answers_a_bad_path_and_a_cancelled_call_with_typed_errors_and_stays_alive() {
    let w = workers();
    assert!(extract(&w, "src/lib.rs", GOOD).is_ok());
    let dead = CancelToken::new();
    dead.cancel();
    assert_eq!(w.extract(&p("a.rs"), GOOD, &dead), Err(SourceError::Cancelled));
    assert_eq!(w.started(), 1);
}

#[test]
fn a_worker_that_aborts_exits_or_overflows_its_stack_costs_that_file_only() {
    for crash in ["bad.abort", "bad.exit", "bad.overflow"] {
        let w = workers();
        match extract(&w, crash, b"x") {
            Err(SourceError::Crashed(why)) => assert!(why.contains("worker"), "{crash}: {why}"),
            other => panic!("{crash}: {other:?}"),
        }
        // the next file goes to a fresh worker and is parsed
        assert!(extract(&w, "src/ok.rs", GOOD).unwrap().is_some(), "after {crash}");
        assert!(w.started() >= 2, "the crash was followed by a new process: {}", w.started());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_spinning_worker_is_judged_stuck_by_cpu_time_and_a_blocked_one_by_ticks() {
    let w = workers();
    match extract(&w, "loop.spin", b"x") {
        Err(SourceError::Crashed(why)) => assert!(why.contains("stuck") && why.contains("Spinning"), "{why}"),
        other => panic!("{other:?}"),
    }
    match extract(&w, "wait.block", b"x") {
        Err(SourceError::Crashed(why)) => assert!(why.contains("stuck") && why.contains("Blocked"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(extract(&w, "src/ok.rs", GOOD).unwrap().is_some());
}

#[test]
fn cancel_ends_a_stuck_call_and_a_paused_worker_is_never_judged_stuck() {
    // a probe that says "stopped by the system" forever: the judge counts nothing, the call would wait for ever, so only Cancel
    // ends it, and the pause was counted so the job can say so
    let (probe, ticks) = TickProbe::new();
    let w = Arc::new(ParseWorkers::new(settings(), Arc::new(probe)));
    let token = CancelToken::new();
    let t = token.clone();
    let w2 = w.clone();
    let handle = std::thread::spawn(move || w2.extract(&p("wait.block"), b"x", &t));
    // wait on the monitor's own ticks (one message per sample), not on a clock: the first sample is the baseline, so seven samples are six
    // judged ticks
    for _ in 0..7 {
        ticks.recv().unwrap();
    }
    assert!(w.paused_ticks() >= 5, "the monitor saw the pause: {}", w.paused_ticks());
    assert!(!handle.is_finished(), "a paused worker is waited for, however many ticks go by");
    token.cancel();
    assert_eq!(handle.join().unwrap(), Err(SourceError::Cancelled));
    let _ = OsProbe;
}

#[test]
fn the_worker_program_answers_its_self_test() {
    let out = std::process::Command::new(WORKER).arg("--self-test").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("pacenotes-parse-worker") && text.contains("ok parse"), "{text}");
}

#[test]
fn a_program_that_does_not_exist_is_a_failure_of_the_file_not_a_panic() {
    let mut s = settings();
    s.program = "/definitely/not/a/worker".into();
    let w = ParseWorkers::new(s, Arc::new(OsProbe));
    match w.extract(&p("a.rs"), GOOD, &CancelToken::new()) {
        Err(SourceError::Failed(why)) => assert!(why.contains("could not be started"), "{why}"),
        other => panic!("{other:?}"),
    }
}

/// Regression (found by measuring, rule 48): a call that finished at once waited for the monitor's first beat, because the monitor
/// sampled the worker before it looked at `done`, so each file cost a beat (a second by default) and a `ps` run. The beat here is
/// an hour: only a call that does not wait for the monitor can return. The guard thread is a hang guard, not a measured time.
#[test]
fn a_call_that_finishes_does_not_wait_for_the_monitor() {
    let samples = Arc::new(FixedProbe::stopped());
    let w = ParseWorkers::new(
        pacenotes_worker::WorkerSettings {
            beat: Duration::from_secs(3600),
            ..settings()
        },
        samples.clone(),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let hung = std::thread::spawn(move || {
        for _ in 0..3 {
            assert!(extract(&w, "src/lib.rs", GOOD).unwrap().is_some());
        }
        tx.send(()).unwrap();
    });
    assert!(
        rx.recv_timeout(Duration::from_secs(60)).is_ok(),
        "a finished call is waiting for the monitor's beat"
    );
    hung.join().unwrap();
    assert_eq!(samples.count(), 0, "a call that ends before the first beat asks the probe nothing");
}

/// Cancel ends a stuck call at once, not at the monitor's next beat: the beat here is an hour, so only a monitor that is woken by the
/// cancel can end the call. The guard is a hang guard, not a measured time.
#[test]
fn cancel_wakes_the_monitor_instead_of_waiting_for_the_next_beat() {
    let w = Arc::new(ParseWorkers::new(
        pacenotes_worker::WorkerSettings {
            beat: Duration::from_secs(3600),
            ..settings()
        },
        Arc::new(FixedProbe::stopped()),
    ));
    let token = CancelToken::new();
    let (t, w2) = (token.clone(), w.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tx.send(w2.extract(&p("wait.block"), b"x", &t)).unwrap();
    });
    while w.started() < 1 {
        std::thread::yield_now();
    }
    token.cancel();
    let result = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the cancel waited for the monitor's beat");
    assert_eq!(result, Err(SourceError::Cancelled));
}

#[test]
fn imports_come_back_and_a_file_the_parser_refuses_or_the_wire_cannot_carry_fails_that_file_only() {
    let w = workers();
    let facts = extract(&w, "src/lib.rs", b"use std::fmt;\n\npub fn a() {}\n").unwrap().unwrap();
    assert!(facts.imports.iter().any(|i| i.target.contains("std")), "{:?}", facts.imports);

    // over the parser's own size limit: a typed answer, the worker stays
    let over = vec![b'a'; 3 * 1024 * 1024];
    match extract(&w, "big.rs", &over) {
        Err(SourceError::Failed(why)) => assert!(why.contains("at most"), "{why}"),
        other => panic!("{other:?}"),
    }
    // over what one message can carry: the file fails, the worker stays
    // (a long beat: the host spends a while turning the file into a message, and that is not the worker being stuck)
    let w = ParseWorkers::new(
        pacenotes_worker::WorkerSettings {
            beat: Duration::from_secs(3600),
            ..settings()
        },
        Arc::new(OsProbe),
    );
    let huge = vec![b'a'; 20 * 1024 * 1024];
    match extract(&w, "huge.rs", &huge) {
        Err(SourceError::Failed(why)) => assert!(why.contains("does not fit"), "{why}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    assert!(extract(&w, "src/ok.rs", GOOD).unwrap().is_some());
    assert_eq!(w.started(), 1, "none of this cost a process");
}
