use paddock_jobs::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn w(n: usize) -> Workers {
    Workers::new(n)
}

#[test]
fn map_keeps_input_order_whatever_the_timing() {
    let items: Vec<u64> = (0..200).collect();
    let counter = AtomicU64::new(0);
    let out = par_map_ordered(items.clone(), w(8), &CancelToken::new(), &counter, |i| {
        // Early items do the most work, so completion order tends to be the reverse of input order.
        for _ in 0..(200 - i) * 200 {
            std::hint::spin_loop();
        }
        Ok(i * 2)
    })
    .unwrap();
    assert_eq!(out, items.iter().map(|i| i * 2).collect::<Vec<_>>());
    assert_eq!(counter.load(Ordering::Relaxed), 200);
}

#[test]
fn map_of_nothing_and_of_one() {
    let none: Vec<u8> = vec![];
    assert!(
        par_map_ordered(none, w(4), &CancelToken::new(), &(), Ok::<u8, JobError>)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        par_map_ordered(vec![1], w(1), &CancelToken::new(), &(), |x| Ok(x + 1)).unwrap(),
        vec![2]
    );
}

#[test]
fn lowest_index_error_wins_deterministically() {
    for _ in 0..20 {
        let r = par_map_ordered(
            (0..100).collect(),
            w(8),
            &CancelToken::new(),
            &(),
            |i: u32| {
                if i % 10 == 7 {
                    Err(JobError::Failed(format!("bad {i}")))
                } else {
                    Ok(i)
                }
            },
        );
        assert_eq!(r.unwrap_err().to_string(), "bad 7");
    }
}

#[test]
fn worker_panic_is_contained() {
    let r = par_map_ordered(
        (0..10).collect(),
        w(3),
        &CancelToken::new(),
        &(),
        |i: u32| {
            if i == 4 {
                panic!("parser bug")
            }
            Ok(i)
        },
    );
    assert!(matches!(r, Err(JobError::Panicked(m)) if m == "parser bug"));
}

#[test]
fn source_panic_is_contained() {
    let src = (0..10u32).map(|i| if i == 3 { panic!("iterator bug") } else { i });
    let mut got = vec![];
    let r = pipeline_ordered(src, w(2), 4, &CancelToken::new(), Ok, |x| {
        got.push(x);
        Ok(())
    });
    assert!(matches!(r, Err(JobError::Panicked(m)) if m == "iterator bug"));
    assert_eq!(
        got,
        vec![0, 1, 2],
        "everything before the failure was delivered, in order"
    );
}

#[test]
fn sink_error_stops_the_run() {
    let r = pipeline_ordered(0..1000u32, w(4), 8, &CancelToken::new(), Ok, |x| {
        if x == 5 {
            Err(JobError::Failed("disk full".into()))
        } else {
            Ok(())
        }
    });
    assert_eq!(r.unwrap_err().to_string(), "disk full");
}

#[test]
fn cancel_stops_promptly_and_reports_the_reason() {
    let cancel = CancelToken::new();
    let started = AtomicUsize::new(0);
    let r = par_map_ordered((0..1_000_000u32).collect(), w(4), &cancel, &(), |i| {
        if started.fetch_add(1, Ordering::SeqCst) == 50 {
            cancel.cancel();
        }
        Ok(i)
    });
    assert!(matches!(r, Err(JobError::Cancelled(CancelReason::User))));
    // Promptness is counted in items, never in wall-clock time (lessons: tests that measure the wall clock fail under load):
    // each worker sees the cancel within its check interval.
    assert!(
        started.load(Ordering::SeqCst) <= 51 + 4 * (CANCEL_CHECK_ITEMS + 1),
        "{}",
        started.load(Ordering::SeqCst)
    );
}

#[test]
fn deadline_cancels_a_pipeline() {
    // The fake clock runs the limit out once the pipeline has delivered some items: no sleep, no wall-clock race.
    let clock = FakeClock::new();
    let cancel = CancelToken::with_deadline_on(Arc::new(clock.clone()), Duration::from_secs(30));
    let r = pipeline_ordered(0u64.., w(2), 4, &cancel, Ok, |i| {
        if i == 100 {
            clock.advance(Duration::from_secs(30));
        }
        Ok(())
    });
    assert!(matches!(
        r,
        Err(JobError::Cancelled(CancelReason::Deadline))
    ));
}

#[test]
fn in_flight_items_are_bounded_by_the_window() {
    let produced = AtomicUsize::new(0);
    let delivered = AtomicUsize::new(0);
    let max_ahead = AtomicUsize::new(0);
    let src = (0..300u32).inspect(|_| {
        let ahead = produced.fetch_add(1, Ordering::SeqCst) + 1 - delivered.load(Ordering::SeqCst);
        max_ahead.fetch_max(ahead, Ordering::SeqCst);
    });
    pipeline_ordered(
        src,
        w(2),
        6,
        &CancelToken::new(),
        |i| {
            if i == 0 {
                // Item 0 is held until the source has filled the window behind it (a hang guard bounds the spin).
                let start = Instant::now();
                while produced.load(Ordering::SeqCst) < 6
                    && start.elapsed() < Duration::from_secs(60)
                {
                    std::thread::yield_now();
                }
            }
            Ok(i)
        },
        |_| {
            delivered.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(delivered.load(Ordering::SeqCst), 300);
    assert!(
        max_ahead.load(Ordering::SeqCst) <= 7,
        "window 6 held, saw {}",
        max_ahead.load(Ordering::SeqCst)
    );
}

#[test]
fn pipeline_overlaps_discovery_and_processing() {
    // The first result is delivered before the (slow) source has produced everything.
    let produced = AtomicUsize::new(0);
    let at_first = AtomicUsize::new(usize::MAX);
    let src = (0..20u32).inspect(|i| {
        produced.fetch_add(1, Ordering::SeqCst);
        if *i == 10 {
            // The source waits for the first delivery: only a pipeline that overlaps the two gets past this
            // (a hang guard bounds the spin).
            let start = Instant::now();
            while at_first.load(Ordering::SeqCst) == usize::MAX
                && start.elapsed() < Duration::from_secs(60)
            {
                std::thread::yield_now();
            }
        }
    });
    pipeline_ordered(src, w(2), 4, &CancelToken::new(), Ok, |x| {
        if x == 0 {
            at_first.store(produced.load(Ordering::SeqCst), Ordering::SeqCst);
        }
        Ok(())
    })
    .unwrap();
    assert!(at_first.load(Ordering::SeqCst) < 20);
}
