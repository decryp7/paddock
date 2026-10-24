use super::*;
use paddock_jobs::time::FakeClock;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fresh() -> (Quarantine, FakeClock) {
    let clock = FakeClock::new();
    (
        Quarantine::new(QuarantinePolicy::default(), Arc::new(clock.clone())),
        clock,
    )
}

#[test]
fn the_default_policy_is_three_in_ten_minutes() {
    let p = QuarantinePolicy::default();
    assert_eq!((p.max, p.window), (3, Duration::from_secs(600)));
    let (q, _) = fresh();
    assert_eq!(q.policy(), p);
    let zero = Quarantine::new(
        QuarantinePolicy {
            max: 0,
            window: p.window,
        },
        Arc::new(FakeClock::new()),
    );
    assert_eq!(zero.policy().max, 1);
}

#[test]
fn repeated_failures_quarantine_and_release_lifts_it() {
    let (q, _) = fresh();
    assert!(!q.record("p", Failure::Panic, "x"));
    assert!(!q.record("p", Failure::Garbled, "x"));
    assert!(q.admit("p").is_ok());
    assert!(q.record("p", Failure::Panic, &"long ".repeat(200)));
    assert!(q.is_quarantined("p"));
    assert_eq!(
        q.admit("p"),
        Err(Refusal::Quarantined { owner: "p".into() })
    );
    let s = q.status("p");
    assert_eq!(
        (s.failures, s.total, s.quarantined, s.hung),
        (3, 3, true, false)
    );
    assert_eq!(s.last_error.unwrap().chars().count(), MAX_ERROR_CHARS);
    // A further failure does not start a second quarantine.
    assert!(!q.record("p", Failure::Panic, "again"));
    q.release("p");
    assert_eq!(q.status("p"), OwnerStatus::default());
    assert!(q.admit("p").is_ok());
    assert!(q.all().is_empty());
}

#[test]
fn failures_older_than_the_window_do_not_count() {
    let (q, clock) = fresh();
    q.record("p", Failure::Panic, "a");
    q.record("p", Failure::Panic, "b");
    clock.advance(Duration::from_secs(599));
    assert_eq!(q.status("p").failures, 2);
    clock.advance(Duration::from_secs(1));
    assert_eq!(q.status("p").failures, 0);
    assert_eq!(q.status("p").total, 2);
    assert!(!q.record("p", Failure::Panic, "c"));
    assert_eq!(q.status("p").failures, 1);
    assert!(!q.is_quarantined("p"));
    // Three inside one window do quarantine.
    assert!(!q.record("p", Failure::Panic, "d"));
    assert!(q.record("p", Failure::Panic, "e"));
    assert_eq!(q.all().keys().collect::<Vec<_>>(), ["p"]);
}

#[test]
fn a_hang_refuses_calls_until_it_ends() {
    let (q, _) = fresh();
    q.record("p", Failure::Hang, "stuck");
    assert!(q.status("p").hung);
    assert_eq!(
        q.admit("p"),
        Err(Refusal::NotResponding {
            owner: "p".into(),
            why: "stuck".into()
        })
    );
    q.hang_ended("p");
    q.hang_ended("p");
    q.hang_ended("never-seen");
    assert!(!q.status("p").hung);
    assert!(q.admit("p").is_ok());
}

#[test]
fn refusals_say_why() {
    assert_eq!(
        Refusal::Quarantined { owner: "p".into() }.to_string(),
        "\"p\" is disabled after repeated failures"
    );
    let r = Refusal::NotResponding {
        owner: "p".into(),
        why: "w".into(),
    };
    assert_eq!(r.to_string(), "\"p\" is not responding: w");
    let _: &dyn std::error::Error = &r;
}

#[test]
fn the_observer_hears_each_start_and_lift_once() {
    let (q, _) = fresh();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    q.set_observer(move |c| s.lock().unwrap().push(c.clone()));
    for _ in 0..5 {
        q.record("p", Failure::Panic, "x");
    }
    q.release("other");
    q.release("p");
    q.release("p");
    let c = |quarantined| Change {
        owner: "p".into(),
        quarantined,
    };
    assert_eq!(*seen.lock().unwrap(), vec![c(true), c(false)]);
}

#[test]
fn a_poisoned_lock_is_recovered() {
    let (q, _) = fresh();
    let q = Arc::new(q);
    q.set_observer(|_| panic!("observer exploded"));
    let q2 = q.clone();
    let crashed = std::thread::spawn(move || {
        for _ in 0..3 {
            q2.record("p", Failure::Panic, "x");
        }
    })
    .join();
    assert!(crashed.is_err());
    // The observer panicked while no lock was held, but poison the owners lock too.
    let q3 = q.clone();
    let _ = std::thread::spawn(move || {
        let _held = q3.owners.lock().unwrap();
        panic!("holder died");
    })
    .join();
    assert!(q.owners.is_poisoned());
    assert!(q.is_quarantined("p"));
    assert!(!q.record("p", Failure::Panic, "y"));
    assert_eq!(q.status("p").total, 4);
    assert!(format!("{q:?}").contains("Quarantine"));
}

fn persistent(dir: &std::path::Path) -> (Quarantine, Option<Problem>, FakeClock) {
    let clock = FakeClock::new();
    let (q, problem) = Quarantine::persistent(
        QuarantinePolicy::default(),
        Arc::new(clock.clone()),
        dir.join("quarantine.json"),
    );
    (q, problem, clock)
}

#[test]
fn a_quarantine_survives_a_restart_and_a_release_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (q, problem, _) = persistent(dir.path());
    assert!(problem.is_none());
    for _ in 0..3 {
        q.record("a", Failure::Panic, "boom");
    }
    q.record("b", Failure::Panic, "only once");
    drop(q);

    let (q, problem, _) = persistent(dir.path());
    assert!(problem.is_none());
    assert!(q.is_quarantined("a"));
    let s = q.status("a");
    assert_eq!(
        (s.total, s.failures, s.last_error.as_deref()),
        (3, 0, Some("boom"))
    );
    assert!(!q.is_quarantined("b"));
    q.release("a");
    drop(q);

    let (q, _, _) = persistent(dir.path());
    assert!(q.all().is_empty());
}

#[test]
fn a_damaged_state_file_gives_an_empty_quarantine_and_a_problem() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("quarantine.json"), b"{ not json").unwrap();
    let (q, problem, _) = persistent(dir.path());
    assert!(matches!(problem, Some(Problem::Damaged { .. })));
    assert!(q.all().is_empty());
    // It still works and saves afterwards.
    for _ in 0..3 {
        q.record("p", Failure::Panic, "x");
    }
    drop(q);
    assert!(persistent(dir.path()).0.is_quarantined("p"));
}

#[test]
fn a_newer_state_file_is_left_alone_and_the_quarantine_still_works_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quarantine.json");
    std::fs::write(&path, br#"{"version": 99, "quarantined": {}}"#).unwrap();
    let (q, problem, _) = persistent(dir.path());
    assert!(matches!(problem, Some(Problem::Newer { found: 99, .. })));
    for _ in 0..3 {
        q.record("p", Failure::Panic, "x");
    }
    assert!(q.is_quarantined("p"));
    assert!(std::fs::read_to_string(&path).unwrap().contains("99"));
}

#[test]
fn concurrent_records_are_all_counted() {
    let (q, _) = fresh();
    let q = Arc::new(q);
    let n = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let (q, n) = (q.clone(), n.clone());
            std::thread::spawn(move || {
                for _ in 0..25 {
                    if q.record("p", Failure::Garbled, "x") {
                        n.fetch_add(1, Ordering::SeqCst);
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(n.load(Ordering::SeqCst), 1);
    assert_eq!(q.status("p").total, 100);
}

#[test]
fn failures_read_as_text() {
    let all = [Failure::Panic, Failure::Hang, Failure::Garbled].map(|f| f.to_string());
    assert_eq!(
        all,
        ["crashed", "overran its time budget", "sent a garbled reply"]
    );
}
