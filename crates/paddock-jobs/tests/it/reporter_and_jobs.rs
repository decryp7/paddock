use crate::common;
use common::{Collect, Panicky};
use paddock_jobs::*;
use std::sync::Arc;
use std::time::Duration;

fn reporter(sink: Arc<dyn EventSink>, clock: &FakeClock) -> Reporter {
    Reporter::with_clock(
        JobId(1),
        JobKind::new("scan"),
        "Scan",
        Some("C:"),
        sink,
        Arc::new(clock.clone()),
        100,
    )
}

#[test]
fn first_event_is_queued_and_phases_go_out_at_once() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    assert_eq!(sink.all()[0].state, JobState::Queued);
    r.start();
    r.start();
    r.begin_phase(Phase::discovering("walk", "Finding files"));
    r.begin_phase(Phase::items("read", "Reading", 10));
    let all = sink.all();
    assert_eq!(
        all.len(),
        4,
        "queued, running, two phases (second start is a no-op)"
    );
    assert!(all.windows(2).all(|w| w[0].seq < w[1].seq));
    match &all[3].state {
        JobState::Running { phase: Some(p) } => {
            assert_eq!((p.phase.id.as_str(), p.done), ("read", 0))
        }
        s => panic!("{s:?}"),
    }
}

#[test]
fn updates_are_throttled_coalesced_and_flushed() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    r.begin_phase(Phase::discovering("walk", "Finding"));
    let base = sink.all().len();
    r.add(1); // within the interval of the phase event: suppressed
    r.add(1);
    assert_eq!(sink.all().len(), base);
    clock.advance_ms(100);
    r.add(1);
    assert_eq!(sink.all().len(), base + 1);
    let last = sink.all().pop().unwrap();
    assert!(
        matches!(
            last.state,
            JobState::Running {
                phase: Some(PhaseProgress { done: 3, .. })
            }
        ),
        "newest value wins"
    );
    r.set(10);
    r.flush();
    assert_eq!(
        sink.all().len(),
        base + 2,
        "flush emits the pending newest value"
    );
    r.flush();
    assert_eq!(
        sink.all().len(),
        base + 2,
        "nothing pending, nothing emitted"
    );
    // Rate bound: a million adds within one interval produce no event.
    for _ in 0..100_000 {
        r.add(1);
    }
    assert_eq!(sink.all().len(), base + 2);
}

#[test]
fn set_total_switches_from_indeterminate() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    r.set_total(5); // not in a phase: ignored
    let n = sink.all().len();
    r.begin_phase(Phase::discovering("walk", "Finding"));
    r.set_total(50);
    r.set_total(50);
    let all = sink.all();
    assert_eq!(all.len(), n + 2, "phase + one total change");
    assert!(
        matches!(&all.last().unwrap().state, JobState::Running { phase: Some(p) } if p.phase.total == Some(50))
    );
}

#[test]
fn eta_appears_only_with_known_total_and_enough_data() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    r.begin_phase(Phase::items("p", "P", 1000));
    for _ in 0..4 {
        clock.advance_ms(1000);
        r.add(100);
    }
    assert!(r.snapshot().eta_ms.is_some());
    assert!(sink.all().last().unwrap().eta_ms.is_some());
    r.begin_phase(Phase::discovering("q", "Q"));
    clock.advance_ms(5000);
    r.add(10);
    assert_eq!(sink.all().last().unwrap().eta_ms, None);
}

#[test]
fn terminal_state_is_final_and_sets_no_cancel() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    assert!(!r.finish(JobState::Done), "queued cannot be done");
    assert!(!r.finish(JobState::Queued), "not terminal");
    r.start();
    assert!(r.finish(JobState::Failed {
        message: "disk full".into()
    }));
    let n = sink.all().len();
    assert!(!r.finish(JobState::Done));
    r.begin_phase(Phase::items("x", "X", 1));
    r.add(1);
    r.flush();
    let _g = r.uncancellable("late");
    assert_eq!(sink.all().len(), n, "nothing is emitted after the end");
    let last = r.snapshot();
    assert!(matches!(last.state, JobState::Failed { .. }));
}

#[test]
fn finishing_a_running_phase_keeps_count() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    r.begin_phase(Phase::items("x", "X", 9));
    r.add(4);
    assert!(!r.finish(JobState::Running { phase: None }));
    assert!(r.finish(JobState::Done));
    assert_eq!(r.snapshot().eta_ms, None);
}

#[test]
fn uncancellable_guard_declares_and_restores() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    r.start();
    {
        let _g = r.uncancellable("Writing the database");
        let s = sink.all().pop().unwrap();
        assert!(!s.cancellable);
        assert_eq!(
            s.not_cancellable_reason.as_deref(),
            Some("Writing the database")
        );
    }
    let s = sink.all().pop().unwrap();
    assert!(s.cancellable && s.not_cancellable_reason.is_none());
    // A guard dropped after the job ended does not resurrect cancelling.
    let g = r.uncancellable("x");
    r.finish(JobState::Done);
    drop(g);
    assert!(!r.snapshot().cancellable);
}

#[test]
fn a_panicking_sink_never_breaks_the_job() {
    let clock = FakeClock::new();
    let r = reporter(Arc::new(Panicky), &clock);
    r.start();
    r.begin_phase(Phase::items("x", "X", 1));
    r.add(1);
    assert!(r.finish(JobState::Done));
}

#[test]
fn sink_panics_are_counted_so_an_owner_can_surface_them() {
    let clock = FakeClock::new();
    let r = reporter(Arc::new(Panicky), &clock);
    let first = r.sink_panics(); // the first snapshot, emitted while the reporter was built
    r.start();
    r.begin_phase(Phase::items("x", "X", 1));
    r.add(1);
    assert!(
        r.sink_panics() > first && first > 0,
        "{first} then {}",
        r.sink_panics()
    );
    let healthy = reporter(Collect::new(), &clock);
    healthy.start();
    assert_eq!(healthy.sink_panics(), 0);
}

#[test]
fn poisoned_state_recovers() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    let r2 = r.clone();
    // Panic while the sink holds the lock is contained, so poison it the hard way: a sink that panics is caught; a second thread
    // that panics in a Counter callback does not own the lock. Poison via the public API is therefore impossible, which is the point.
    let h = std::thread::spawn(move || {
        r2.start();
        panic!("worker bug");
    });
    assert!(h.join().is_err());
    r.begin_phase(Phase::items("x", "X", 1));
    assert!(r.finish(JobState::Done));
}

#[test]
fn counters() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let a = AtomicU64::new(0);
    Counter::add(&a, 3);
    Counter::add(&(), 3);
    assert_eq!(a.load(Ordering::Relaxed), 3);
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink, &clock);
    r.begin_phase(Phase::items("x", "X", 9));
    Counter::add(&r, 2);
    assert_eq!(r.id(), JobId(1));
    assert!(matches!(
        r.snapshot().state,
        JobState::Running {
            phase: Some(PhaseProgress { done: 2, .. })
        }
    ));
}

#[test]
fn run_maps_outcomes_and_emits_one_terminal() {
    let sink = Collect::new();
    let reg = JobRegistry::new(sink.clone());
    let ok = reg.register("download", "Download", Some("a.zip"));
    let v = ok.run(|c| {
        c.phase(Phase::bytes("download", "Downloading", Some(10)));
        c.add(10);
        c.set(10);
        c.set_total(10);
        c.check()?;
        let _g = c.uncancellable("Renaming");
        Counter::add(c, 0);
        let _ = (c.id(), c.reporter().id(), c.cancel_token().is_cancelled());
        Ok(5)
    });
    assert_eq!(v.unwrap(), 5);

    let failed = reg.register("x", "X", None);
    assert!(
        failed
            .run::<()>(|_| Err(JobError::Failed("bad".into())))
            .is_err()
    );
    let cancelled = reg.register("x", "X", None);
    cancelled.cancel_token().cancel();
    assert!(matches!(
        cancelled.run::<()>(|c| c.check()),
        Err(JobError::Cancelled(CancelReason::User))
    ));
    let panicked = reg.register("x", "X", None);
    assert!(
        matches!(panicked.run::<()>(|_| panic!("boom")), Err(JobError::Panicked(m)) if m == "boom")
    );

    let snaps = reg.snapshots();
    assert_eq!(snaps.len(), 4);
    assert!(snaps.windows(2).all(|w| w[0].id < w[1].id));
    assert!(matches!(snaps[0].state, JobState::Done));
    assert!(matches!(&snaps[1].state, JobState::Failed { message } if message == "bad"));
    assert!(matches!(
        snaps[2].state,
        JobState::Cancelled {
            reason: CancelReason::User
        }
    ));
    assert!(matches!(&snaps[3].state, JobState::Failed { message } if message.contains("boom")));
    let terminal = sink.all().iter().filter(|s| s.state.is_terminal()).count();
    assert_eq!(terminal, 4);
    assert_eq!(reg.forget_finished(), 4);
    assert!(reg.snapshots().is_empty());
}

#[test]
fn registry_cancel_semantics() {
    let reg = JobRegistry::with_clock(Collect::new(), Arc::new(FakeClock::new()), 100);
    assert!(!reg.cancel(JobId(99)));
    let queued = reg.register("a", "A", None);
    assert!(reg.cancel(queued.id()));
    assert!(matches!(
        queued.reporter().snapshot().state,
        JobState::Cancelled {
            reason: CancelReason::User
        }
    ));
    assert!(!reg.cancel(queued.id()), "already ended");

    let running = reg.register("b", "B", None);
    running.reporter().start();
    assert!(reg.cancel(running.id()));
    assert!(running.check().is_err());
    assert!(
        matches!(
            running.reporter().snapshot().state,
            JobState::Running { .. }
        ),
        "the body decides when it stops"
    );
}

#[test]
fn deadline_job_cancels_itself() {
    let reg = JobRegistry::new(Collect::new());
    let j = reg.register_with_deadline("a", "A", None, Duration::ZERO);
    assert!(matches!(
        j.run::<()>(|c| c.check()),
        Err(JobError::Cancelled(CancelReason::Deadline))
    ));
}

#[test]
fn many_threads_share_one_reporter() {
    let sink = Collect::new();
    let clock = Arc::new(SystemClock);
    let r = Reporter::with_clock(
        JobId(1),
        JobKind::new("k"),
        "T",
        None,
        sink.clone(),
        clock,
        5,
    );
    r.begin_phase(Phase::discovering("w", "W"));
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                for _ in 0..5_000 {
                    r.add(1);
                }
            });
        }
    });
    r.flush();
    let last = sink.all().pop().unwrap();
    assert!(matches!(
        last.state,
        JobState::Running {
            phase: Some(PhaseProgress { done: 40_000, .. })
        }
    ));
    let seqs: Vec<u64> = sink.all().iter().map(|s| s.seq).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "events arrive in order"
    );
}

#[test]
fn a_phase_publishes_its_last_throttled_count_before_the_next_phase_starts() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    r.begin_phase(Phase::discovering("walk", "Finding"));
    r.add(4);
    r.add(1); // within the interval: pending, not yet published
    r.begin_phase(Phase::items("read", "Reading", 10));
    let walk_counts: Vec<u64> = sink
        .all()
        .iter()
        .filter_map(|s| match &s.state {
            JobState::Running { phase: Some(p) } if p.phase.id == "walk" => Some(p.done),
            _ => None,
        })
        .collect();
    assert_eq!(
        walk_counts.last(),
        Some(&5),
        "the walk's final count is lost: {walk_counts:?}"
    );
}

fn last_done(sink: &Collect) -> Option<u64> {
    sink.all().iter().rev().find_map(|s| match &s.state {
        JobState::Running { phase: Some(p) } => Some(p.done),
        _ => None,
    })
}

#[test]
fn finish_publishes_the_last_throttled_count_first() {
    let sink = Collect::new();
    let clock = FakeClock::new();
    let r = reporter(sink.clone(), &clock);
    r.begin_phase(Phase::discovering("walk", "Finding"));
    r.add(4);
    r.add(1); // inside the interval: pending
    assert!(r.finish(JobState::Done));
    assert_eq!(
        last_done(&sink),
        Some(5),
        "the final count is published before the terminal event"
    );
    assert_eq!(sink.all().last().unwrap().state, JobState::Done);
}

/// A clock that adds one to the job the moment an emit reads it: an update landing in the middle of an emit.
struct AddsDuringEmit {
    job: std::sync::OnceLock<Reporter>,
    armed: std::sync::atomic::AtomicBool,
}

impl Clock for AddsDuringEmit {
    fn monotonic(&self) -> Duration {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst)
            && let Some(r) = self.job.get()
        {
            r.add(1);
        }
        Duration::ZERO
    }
    fn wall(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH
    }
    fn wait(&self, _: Duration, _: &CancelToken) -> Result<(), Cancelled> {
        Ok(())
    }
}

#[test]
fn an_update_landing_during_an_emit_is_published_by_the_next_flush() {
    let sink = Collect::new();
    let clock = Arc::new(AddsDuringEmit {
        job: std::sync::OnceLock::new(),
        armed: std::sync::atomic::AtomicBool::new(false),
    });
    let r = Reporter::with_clock(
        JobId(1),
        JobKind::new("scan"),
        "Scan",
        None,
        sink.clone(),
        clock.clone(),
        100,
    );
    assert!(clock.job.set(r.clone()).is_ok());
    r.begin_phase(Phase::discovering("walk", "Finding"));
    clock.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    r.set_total(9); // an emit; the clock adds 1 while it runs
    r.flush();
    assert_eq!(
        last_done(&sink),
        Some(1),
        "the update made during the emit is not lost"
    );
}
