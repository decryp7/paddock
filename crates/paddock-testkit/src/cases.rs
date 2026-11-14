//! Independent cases of one test run side by side (ADR 0103): a parity check that runs two implementations, or a table of
//! cases that each start programs, waits for the slowest case instead of the sum of all of them. One after the other, the
//! kept-clone parity tests ran 20-24 jobs under one nextest bound and passed it only on a quiet Mac (runs 8054, 8099).
//!
//! - [`both`]: two closures at the same time, both results back (a panic in either is the test's panic, naming the side).
//! - [`run_cases`]: named cases on at most [`case_threads`] threads; every case runs even when one fails, each case's time is
//!   printed (ER-27), and the panic lists every failed case with its message and time.
//!
//! Cases must share nothing they write (each its own folder, port and child processes); a case that needs the result of
//! another is a step of one case, not a case.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Overrides how many cases run at once (a positive integer). Unset: half the cores, at least two.
pub const THREADS_VAR: &str = "PADDOCK_TEST_CASE_THREADS";

/// How many cases [`run_cases`] runs at once: [`THREADS_VAR`], else half the cores, at least two (nextest already runs a test
/// per core; a test's cases share its slot with the tests beside it).
pub fn case_threads() -> usize {
    std::env::var(THREADS_VAR)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| (std::thread::available_parallelism().map_or(2, |n| n.get()) / 2).max(2))
}

/// Runs `a` and `b` at the same time and returns both results. A panic in either is re-raised after both ended, naming the side.
pub fn both<A: Send, B: Send>(
    a: impl FnOnce() -> A + Send,
    b: impl FnOnce() -> B + Send,
) -> (A, B) {
    std::thread::scope(|s| {
        let left = s.spawn(a);
        let right = catch_unwind(AssertUnwindSafe(b));
        let left = left.join();
        match (left, right) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) => panic!("the first of two cases panicked: {}", message(&*e)),
            (_, Err(e)) => panic!("the second of two cases panicked: {}", message(&*e)),
        }
    })
}

/// One named case of [`run_cases`].
pub type Case<'a> = (String, Box<dyn FnOnce() + Send + 'a>);

/// What happened to one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// The case's name.
    pub name: String,
    /// How long it ran.
    pub took: Duration,
    /// Its panic message, when it failed.
    pub failed: Option<String>,
}

/// Runs every case, at most [`case_threads`] at once, and returns what happened to each, in the order given. Never panics on a
/// case's failure: [`run_cases`] is the asserting form.
pub fn run_cases_report(cases: Vec<Case<'_>>) -> Vec<CaseResult> {
    let threads = case_threads().min(cases.len().max(1));
    let queue = Mutex::new(cases.into_iter().enumerate().collect::<Vec<_>>());
    let results = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let next = queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .pop();
                    let Some((at, (name, case))) = next else {
                        return;
                    };
                    let began = Instant::now();
                    let failed = catch_unwind(AssertUnwindSafe(case))
                        .err()
                        .map(|e| message(&*e));
                    let took = began.elapsed();
                    eprintln!(
                        "case {name}: {} in {took:.1?}",
                        if failed.is_some() { "FAILED" } else { "passed" }
                    );
                    results
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((at, CaseResult { name, took, failed }));
                }
            });
        }
    });
    let mut results = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    results.sort_by_key(|(at, _)| *at);
    results.into_iter().map(|(_, r)| r).collect()
}

/// Runs every case side by side ([`run_cases_report`]) and panics, naming `what` and every failed case with its message and time,
/// when any failed.
pub fn run_cases(what: &str, cases: Vec<Case<'_>>) {
    let results = run_cases_report(cases);
    let failed: Vec<String> = results
        .iter()
        .filter_map(|r| {
            r.failed
                .as_ref()
                .map(|m| format!("- {} ({:.1?}): {m}", r.name, r.took))
        })
        .collect();
    assert!(
        failed.is_empty(),
        "{what}: {} of {} cases failed\n{}",
        failed.len(),
        results.len(),
        failed.join("\n")
    );
}

/// A case built from a name and a closure, for [`run_cases`].
pub fn case<'a>(name: impl Into<String>, f: impl FnOnce() + Send + 'a) -> Case<'a> {
    (name.into(), Box::new(f))
}

fn message(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_else(|| "a panic without a message".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Condvar;

    /// A meeting of `n` cases: each arrives and waits for the others, for at most 10 s (a bound that only ends a test whose cases
    /// ran one after the other, where the first to arrive would wait for good).
    struct Meeting {
        n: usize,
        here: Mutex<usize>,
        all: Condvar,
    }

    impl Meeting {
        fn new(n: usize) -> Meeting {
            Meeting {
                n,
                here: Mutex::new(0),
                all: Condvar::new(),
            }
        }

        fn arrive(&self) {
            let mut here = self.here.lock().unwrap();
            *here += 1;
            self.all.notify_all();
            let (here, waited) = self
                .all
                .wait_timeout_while(here, Duration::from_secs(10), |h| *h < self.n)
                .unwrap();
            assert!(
                !waited.timed_out(),
                "only {} of {} cases were running at once: they ran one after the other",
                *here,
                self.n
            );
        }
    }

    #[test]
    fn both_runs_its_two_sides_at_the_same_time() {
        // Each side waits for the other: one after the other, the first side would wait alone.
        let gate = Meeting::new(2);
        let (a, b) = both(
            || {
                gate.arrive();
                1
            },
            || {
                gate.arrive();
                2
            },
        );
        assert_eq!((a, b), (1, 2));
    }

    #[test]
    fn cases_run_side_by_side_so_the_test_takes_the_slowest_not_the_sum() {
        let n = case_threads().min(4);
        assert!(n >= 2, "{n}");
        let gate = Meeting::new(n);
        let cases = (0..n)
            .map(|i| {
                let gate = &gate;
                case(format!("case {i}"), move || gate.arrive())
            })
            .collect();
        run_cases("cases that meet", cases);
    }

    #[test]
    fn every_case_runs_and_every_failure_is_named_with_its_time() {
        let ran = Mutex::new(Vec::new());
        let cases = ["a", "b", "c", "d"]
            .into_iter()
            .map(|name| {
                let ran = &ran;
                case(name, move || {
                    ran.lock().unwrap().push(name);
                    assert!(name != "b" && name != "d", "case {name} is wrong");
                })
            })
            .collect();
        let e = catch_unwind(AssertUnwindSafe(|| run_cases("four cases", cases))).unwrap_err();
        let m = message(&*e);
        let mut ran = ran.into_inner().unwrap();
        ran.sort_unstable();
        assert_eq!(ran, ["a", "b", "c", "d"], "a failure stops no other case");
        assert!(m.contains("four cases: 2 of 4 cases failed"), "{m}");
        assert!(m.contains("- b (") && m.contains("case b is wrong"), "{m}");
        assert!(m.contains("- d (") && m.contains("case d is wrong"), "{m}");
    }
}
