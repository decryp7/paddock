use super::*;
use crate::QuarantinePolicy;
use paddock_jobs::time::FakeClock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const GUARD: Duration = Duration::from_secs(30);
const BUDGET: Duration = Duration::from_secs(5);

fn guard() -> (Guard, FakeClock) {
    let clock = FakeClock::new();
    let q = Arc::new(Quarantine::new(
        QuarantinePolicy::default(),
        Arc::new(clock.clone()),
    ));
    (Guard::new(q, Arc::new(clock.clone())), clock)
}

/// Waits (hang guard only, no timing assertion) until `cond` holds.
fn until(cond: impl Fn() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < GUARD, "gave up waiting");
        std::thread::yield_now();
    }
}

#[test]
fn results_and_typed_errors_pass_through_uncounted() {
    let (g, _) = guard();
    assert_eq!(
        g.call("p", "import", Some(BUDGET), || Ok::<_, String>(7)),
        Ok(7)
    );
    assert_eq!(
        g.call("p", "import", None, || Err::<(), _>("nope".to_string())),
        Err(CallError::Failed("nope".into()))
    );
    assert_eq!(
        g.call("p", "import", Some(BUDGET), || Err::<(), _>(
            "nope".to_string()
        ))
        .unwrap_err()
        .to_string(),
        "nope"
    );
    let local = String::from("borrowed");
    assert_eq!(g.call_here("p", "x", || Ok::<_, ()>(local.len())), Ok(8));
    assert_eq!(g.quarantine().status("p").total, 0);
}

#[test]
fn a_panic_is_a_crash_attributed_to_the_owner_and_counted() {
    let (g, _) = guard();
    let n = std::hint::black_box(1);
    let e = g
        .call("p", "transform", None, move || -> Result<(), String> {
            panic!("exploded {n}")
        })
        .unwrap_err();
    assert_eq!(
        e,
        CallError::Crashed(Crashed::new("p (transform)", "exploded 1"))
    );
    assert_eq!(e.to_string(), "p (transform) crashed: exploded 1");
    let e = g
        .call("p", "import", Some(BUDGET), || -> Result<(), String> {
            std::panic::panic_any(5u8)
        })
        .unwrap_err();
    assert_eq!(e.to_string(), "p (import) crashed: unknown panic");
    let s = g.quarantine().status("p");
    assert_eq!((s.total, s.failures, s.quarantined), (2, 2, false));
    assert!(s.last_error.unwrap().contains("p (import) crashed"));
}

#[test]
fn three_crashes_quarantine_and_refuse_without_running_the_function() {
    let (g, _) = guard();
    for _ in 0..3 {
        let _ = g.call_here("p", "x", || -> Result<(), ()> { panic!("again") });
    }
    assert!(g.quarantine().is_quarantined("p"));
    let ran = Arc::new(AtomicBool::new(false));
    let r = ran.clone();
    let e = g
        .call("p", "x", Some(BUDGET), move || {
            r.store(true, Ordering::SeqCst);
            Ok::<_, String>(())
        })
        .unwrap_err();
    assert_eq!(
        e,
        CallError::Refused(Refusal::Quarantined { owner: "p".into() })
    );
    assert!(e.to_string().contains("disabled after repeated failures"));
    assert!(g.call_here("p", "x", || Ok::<_, ()>(())).is_err());
    assert!(!ran.load(Ordering::SeqCst));
    // Another owner is unaffected; releasing lifts it.
    assert!(g.call_here("q", "x", || Ok::<_, ()>(())).is_ok());
    g.quarantine().release("p");
    assert!(g.call_here("p", "x", || Ok::<_, ()>(())).is_ok());
}

#[test]
fn an_overrun_is_abandoned_counted_and_refuses_calls_until_it_returns() {
    let (g, clock) = guard();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let g2 = g.clone();
    let caller = std::thread::spawn(move || {
        g2.call("p", "import", Some(BUDGET), move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok::<_, String>("late")
        })
    });
    started_rx.recv().unwrap();
    assert!(clock.wait_for_started(1, GUARD));
    clock.advance(BUDGET);
    let e = caller.join().unwrap().unwrap_err();
    assert_eq!(
        e,
        CallError::Hung {
            what: "p (import)".into(),
            budget: BUDGET
        }
    );
    assert!(
        e.to_string()
            .contains("p (import) did not finish within 5s")
    );
    let s = g.quarantine().status("p");
    assert!(s.hung);
    assert_eq!(s.failures, 1);
    let refused = g.call("p", "import", None, || Ok::<_, ()>(())).unwrap_err();
    assert!(matches!(
        refused,
        CallError::Refused(Refusal::NotResponding { .. })
    ));
    // The abandoned call returns: the owner is responsive again.
    release_tx.send(()).unwrap();
    until(|| !g.quarantine().status("p").hung);
    assert!(g.call_here("p", "import", || Ok::<_, ()>(())).is_ok());
}

#[test]
fn repeated_overruns_quarantine() {
    let (g, clock) = guard();
    let mut gates = Vec::new();
    for round in 1..=3u64 {
        let (tx, rx) = mpsc::channel::<()>();
        gates.push(tx);
        let g2 = g.clone();
        let caller = std::thread::spawn(move || {
            g2.call("p", "x", Some(BUDGET), move || {
                let _ = rx.recv();
                Ok::<_, ()>(())
            })
        });
        assert!(clock.wait_for_started(round, GUARD));
        clock.advance(BUDGET);
        assert!(matches!(
            caller.join().unwrap(),
            Err(CallError::Hung { .. })
        ));
        // Let the abandoned call end so the next one is admitted (hung is cleared, quarantine is not).
        if round < 3 {
            gates.pop().unwrap().send(()).unwrap();
            until(|| !g.quarantine().status("p").hung);
        }
    }
    assert!(g.quarantine().is_quarantined("p"));
    drop(gates);
}

#[test]
fn a_call_that_finishes_in_time_leaves_no_timer_behind() {
    let (g, clock) = guard();
    assert_eq!(g.call("p", "x", Some(BUDGET), || Ok::<_, ()>(1)), Ok(1));
    // The timer thread is released by the cancel, so the clock has no waiter left.
    assert!(clock.wait_for_waiters(0, GUARD));
    until(|| clock.waiters() == 0);
    clock.advance(BUDGET * 2);
    assert_eq!(g.quarantine().status("p").total, 0);
}

fn channel_with<T, E>(msgs: Vec<Msg<T, E>>) -> Receiver<Msg<T, E>> {
    let (tx, rx) = mpsc::channel();
    for m in msgs {
        tx.send(m).unwrap();
    }
    rx
}

#[test]
fn a_result_that_beat_the_timeout_is_returned_not_abandoned() {
    let phase = Mutex::new(Phase::Finished);
    let rx = channel_with::<u8, ()>(vec![Msg::Timeout, Msg::Done(Ok(3))]);
    let abandoned = AtomicBool::new(false);
    let s = settle_timeout(&phase, &rx, || abandoned.store(true, Ordering::SeqCst));
    assert!(matches!(s, Settled::Finished(Ok(3))));
    assert!(!abandoned.load(Ordering::SeqCst));
}

#[test]
fn a_finished_call_without_a_result_is_lost_not_hung() {
    let phase = Mutex::new(Phase::Finished);
    let rx = channel_with::<u8, ()>(vec![Msg::Timeout]);
    assert!(matches!(
        settle_timeout(&phase, &rx, || panic!("must not abandon")),
        Settled::Lost
    ));
    let rx = channel_with::<u8, ()>(vec![]);
    let phase = Mutex::new(Phase::Running);
    assert!(matches!(
        settle_timeout(&phase, &rx, || {}),
        Settled::Abandoned
    ));
    assert_eq!(*phase.lock().unwrap(), Phase::Abandoned);
}

#[test]
fn a_poisoned_phase_lock_is_recovered() {
    let phase = Arc::new(Mutex::new(Phase::Running));
    let p = phase.clone();
    let _ = std::thread::spawn(move || {
        let _held = p.lock().unwrap();
        panic!("holder died");
    })
    .join();
    assert!(phase.is_poisoned());
    let rx = channel_with::<u8, ()>(vec![]);
    assert!(matches!(
        settle_timeout(&phase, &rx, || {}),
        Settled::Abandoned
    ));
}

#[test]
fn errors_display_every_variant() {
    let e: CallError<String> = CallError::Unavailable("no thread".into());
    assert_eq!(e.to_string(), "no thread");
    let _: &dyn std::error::Error = &e;
}

#[test]
fn unavailable_texts_name_the_call() {
    let e = std::io::Error::other("no threads left");
    assert_eq!(
        unavailable("p (x)", &e),
        "p (x): could not start a helper thread: no threads left"
    );
    assert_eq!(lost("p (x)"), "p (x) ended without a result");
    assert!(spawn("t", || {}).is_ok());
}

#[test]
fn resolve_covers_every_first_message() {
    let phase = Mutex::new(Phase::Running);
    let rx = channel_with::<u8, ()>(vec![]);
    let r = resolve(
        Msg::Done(Ok(1)),
        &phase,
        &rx,
        || panic!("no"),
        "p (x)",
        BUDGET,
    );
    assert_eq!(r, Ok(1));
    let r = resolve(Msg::Gone, &phase, &rx, || panic!("no"), "p (x)", BUDGET);
    assert_eq!(
        r,
        Err(CallError::Unavailable(
            "p (x) ended without a result".into()
        ))
    );
    let r = resolve(Msg::Timeout, &phase, &rx, || {}, "p (x)", BUDGET);
    assert_eq!(
        r,
        Err(CallError::Hung {
            what: "p (x)".into(),
            budget: BUDGET
        })
    );
    let finished = Mutex::new(Phase::Finished);
    let rx = channel_with::<u8, ()>(vec![Msg::Done(Ok(9))]);
    assert_eq!(
        resolve(
            Msg::Timeout,
            &finished,
            &rx,
            || panic!("no"),
            "p (x)",
            BUDGET
        ),
        Ok(9)
    );
    let rx = channel_with::<u8, ()>(vec![]);
    let r = resolve(
        Msg::Timeout,
        &finished,
        &rx,
        || panic!("no"),
        "p (x)",
        BUDGET,
    );
    assert!(matches!(r, Err(CallError::Unavailable(_))));
}
