//! Tests of [`crate::starts`]: a counter sees the commands its own thread (or a thread or task it is carried into) makes, never
//! another test's; the assertion helpers pass and fail on the count.

use super::{
    StartBudget, assert_starts, assert_starts_async, at_most, at_most_or_none, count_starts,
};
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

/// The text of a caught panic (a `String` from a formatted message, a `&str` from a plain one).
fn panic_text(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
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
    let text = panic_text(&panic);
    assert!(text.contains("expected 1 process start, got 3"), "{text}");
    let panic =
        std::panic::catch_unwind(|| assert_starts(at_most(2), || one_per_item(&["a", "b", "c"])))
            .unwrap_err();
    let text = panic_text(&panic);
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
    let text = panic_text(&panic);
    assert!(text.contains("got 2"), "{text}");
}

#[test]
fn a_scope_entered_on_its_own_counters_thread_counts_once() {
    // P1-1: the counter is already live here; entering its scope again must not add it twice.
    let counter = count_starts();
    let _in = counter.scope().enter();
    let _again = counter.scope().enter();
    one_per_item(&["a", "b"]);
    assert_eq!(counter.starts(), 2);
    drop(_again);
    one_for_all(&["c"]);
    assert_eq!(
        counter.starts(),
        3,
        "leaving one entry keeps the counter while another holds it"
    );
}

#[test]
fn a_task_polled_on_its_own_counters_thread_counts_once() {
    // P1-1: a current-thread runtime (`#[tokio::test]`) polls the wrapped task on the thread whose counter is live.
    let counter = count_starts();
    block_on(
        counter
            .scope()
            .wrap(async { one_per_item(&["a", "b", "c"]) }),
    );
    assert_eq!(counter.starts(), 3);
    one_for_all(&["d"]);
    assert_eq!(
        counter.starts(),
        4,
        "the counter still counts after the task's scope ended"
    );
}

#[test]
fn at_most_needs_at_least_one_start_positive_control_on_an_uncarried_thread() {
    // P1-2: work moved to a thread the scope was not carried into starts nothing this counter sees; a budget must not pass on that.
    let panic = std::panic::catch_unwind(|| {
        assert_starts(at_most(3), || {
            std::thread::spawn(|| one_per_item(&["a", "b"]))
                .join()
                .unwrap()
        })
    })
    .unwrap_err();
    let text = panic_text(&panic);
    assert!(
        text.contains("got 0") && text.contains("at_most_or_none"),
        "{text}"
    );
    // The same work, carried, meets the budget.
    let counter = count_starts();
    let scope = counter.scope();
    std::thread::spawn(move || {
        let _in = scope.enter();
        one_per_item(&["a", "b"]);
    })
    .join()
    .unwrap();
    assert_eq!(counter.starts(), 2);
}

#[test]
fn at_most_or_none_allows_zero_with_a_reason() {
    assert_starts(at_most_or_none(2, "a warm cache starts nothing"), || ());
    assert_starts(at_most_or_none(2, "a cold cache starts one"), || {
        one_for_all(&["a"])
    });
    let panic = std::panic::catch_unwind(|| {
        assert_starts(at_most_or_none(1, "x y z"), || one_per_item(&["a", "b"]))
    })
    .unwrap_err();
    let text = panic_text(&panic);
    assert!(text.contains("at most 1"), "{text}");
}

#[test]
#[should_panic(expected = "a reason")]
fn at_most_or_none_refuses_an_empty_reason() {
    let _ = at_most_or_none(1, " ");
}

#[test]
fn a_program_with_windows_of_its_own_is_counted_too() {
    // `attached_command` (a CLI's helper in its terminal, the app relaunching itself) sets no creation flags and is still one start.
    assert_starts(2, || {
        crate::command::attached_command("browser");
        command("tool");
    });
}
