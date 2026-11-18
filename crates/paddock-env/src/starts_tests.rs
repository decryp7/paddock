//! Tests of [`crate::starts`]: a counter sees the commands its own thread (or a thread or task it is carried into) makes, never
//! another test's; the assertion helpers pass and fail on the count.

use super::{StartBudget, assert_starts, assert_starts_async, at_most, count_starts};
use crate::command::command;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Barrier};
use std::task::{Context, Poll, Waker};

/// Runs a future that never waits (the tests' futures only make commands) to its end on this thread.
fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

/// The code under test: one command per item, and one for all.
fn one_per_item(items: &[&str]) {
    for i in items {
        command("git").arg(i);
    }
}
fn one_for_all(items: &[&str]) {
    command("git").args(items);
}

#[test]
fn a_counter_counts_the_commands_made_on_its_thread_while_it_lives() {
    let counter = count_starts();
    assert_eq!(counter.starts(), 0);
    one_per_item(&["a", "b", "c"]);
    assert_eq!(counter.starts(), 3);
    drop(counter);
    // Nothing counts after the counter is gone, and a new one starts at zero.
    one_per_item(&["d"]);
    assert_eq!(count_starts().starts(), 0);
}

#[test]
fn nested_counters_both_count_and_drop_in_any_order() {
    let outer = count_starts();
    let inner = count_starts();
    one_for_all(&["a"]);
    drop(outer);
    one_for_all(&["b"]);
    assert_eq!(inner.starts(), 2);
}

#[test]
fn parallel_tests_never_see_each_other() {
    // Two threads make commands at the same time; each counter sees only its own (a global counter would see 5 in both).
    let barrier = Arc::new(Barrier::new(2));
    let spawn = |n: usize| {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            let counter = count_starts();
            barrier.wait();
            for _ in 0..n {
                drop(command("git"));
            }
            barrier.wait();
            counter.starts()
        })
    };
    let (a, b) = (spawn(2), spawn(3));
    assert_eq!((a.join().unwrap(), b.join().unwrap()), (2, 3));
}

#[test]
fn a_scope_carries_the_counter_into_a_worker_thread() {
    let counter = count_starts();
    let scope = counter.scope();
    std::thread::spawn(move || {
        let _in = scope.enter();
        one_per_item(&["a", "b"]);
    })
    .join()
    .unwrap();
    // A thread the scope was not carried into is not counted.
    std::thread::spawn(|| one_per_item(&["x"])).join().unwrap();
    assert_eq!(counter.starts(), 2);
}

#[test]
fn a_scope_carries_the_counter_into_a_task_on_every_poll() {
    let counter = count_starts();
    let task = counter
        .scope()
        .wrap(async { one_per_item(&["a", "b", "c"]) });
    // Polled on another thread, as a multi-thread runtime may do.
    std::thread::spawn(move || block_on(task)).join().unwrap();
    assert_eq!(counter.starts(), 3);
}

#[test]
fn assert_starts_passes_on_the_count_and_returns_the_value() {
    assert_eq!(
        assert_starts(1, || {
            one_for_all(&["a", "b", "c"]);
            7
        }),
        7
    );
    assert_starts(at_most(3), || one_per_item(&["a", "b", "c"]));
    assert_starts(0, || ());
    assert_eq!(StartBudget::from(2), StartBudget::Exactly(2));
}

#[test]
fn assert_starts_positive_control_the_per_item_loop_fails_with_the_count() {
    // ER-33: the same assertion that passes on the bulk form fails on the per-item loop.
    let panic = std::panic::catch_unwind(|| assert_starts(1, || one_per_item(&["a", "b", "c"])))
        .unwrap_err();
    let text = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(text.contains("expected 1 process start, got 3"), "{text}");
    let panic =
        std::panic::catch_unwind(|| assert_starts(at_most(2), || one_per_item(&["a", "b", "c"])))
            .unwrap_err();
    let text = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        text.contains("expected at most 2 process starts, got 3"),
        "{text}"
    );
}

#[test]
fn assert_starts_counts_only_the_closure() {
    let _outer = count_starts();
    one_per_item(&["before", "before"]);
    assert_starts(1, || one_for_all(&["a"]));
}

#[test]
fn assert_starts_async_counts_the_future() {
    block_on(assert_starts_async(1, async { one_for_all(&["a", "b"]) }));
    let panic = std::panic::catch_unwind(|| {
        block_on(assert_starts_async(1, async { one_per_item(&["a", "b"]) }))
    })
    .unwrap_err();
    let text = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(text.contains("got 2"), "{text}");
}
