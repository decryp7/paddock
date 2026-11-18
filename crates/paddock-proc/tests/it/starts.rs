//! The process-start counter through this crate's face (`paddock_proc::starts`, the bulk-API redesign's G3): a real run through
//! [`paddock_proc::run_bounded`] is one start, and a per-item loop of runs fails a budget the bulk form meets.

use paddock_proc::starts::{assert_starts, at_most, count_starts};
use paddock_proc::{Limits, command, run_bounded};
use std::time::Duration;

/// A program every OS has that exits at once: `cmd /c exit 0` on Windows, `true` elsewhere.
fn quick() -> std::process::Command {
    if cfg!(windows) {
        let mut c = command("cmd");
        c.args(["/c", "exit", "0"]);
        c
    } else {
        command("true")
    }
}

fn run_each(items: usize) {
    for _ in 0..items {
        run_bounded(quick(), Limits::new(Duration::from_secs(30))).expect("the quick program runs");
    }
}

#[test]
fn a_bounded_run_is_one_start() {
    let counter = count_starts();
    run_each(1);
    assert_eq!(counter.starts(), 1);
}

#[test]
fn a_per_item_loop_of_runs_breaks_the_budget_the_bulk_form_meets() {
    assert_starts(1, || run_each(1));
    let failed = std::panic::catch_unwind(|| assert_starts(at_most(1), || run_each(3)));
    assert!(failed.is_err(), "three runs passed a budget of one");
}
