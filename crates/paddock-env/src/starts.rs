//! A counting seam for budget tests (the bulk-API redesign's G3; PRINCIPLES, "Bulk APIs"; ER-20): how many programs a piece of
//! code starts. [`crate::command::command`] and [`crate::command::attached_command`] are the constructors of a program's
//! `Command`, and both note every command they make here; a test asserts the count in one line, after Django's
//! `assertNumQueries`:
//!
//! ```
//! use paddock_env::starts::{assert_starts, at_most};
//! # fn delete_all(ids: &[&str]) { let _ = paddock_env::command::command("git").args(ids); }
//! assert_starts(1, || delete_all(&["a", "b", "c"])); // N items, one process
//! assert_starts(at_most(2), || delete_all(&["a"])); // at least one, at most two
//! ```
//!
//! **Where the constructors are enforced.** Clippy's `disallowed-methods` forbids `std::process::Command::new` (and tokio's) in the
//! shared crates (`crates/clippy.toml`), in every app (`apps/<app>/clippy.toml`, through `lint/clippy.toml`) and in the dev tools.
//! A site that keeps a plain start says so with an `allow` or `expect` and a reason, and is not counted: today the measured
//! baselines of paddock-testkit's `measure_test_command` example, and a few app tests and app build tools with older `allow`s
//! (grep `Command::new` under `apps/`). Code that has no paddock-env dependency cannot be counted until it takes one.
//!
//! **What one start is.** A `Command` made by a constructor, not a spawn: std's `Command` has no hook around `spawn`, `output` and
//! `status`, so the constructor is the one point every start passes through. One command spawned again after `ETXTBSY`
//! (paddock-proc's retry) is one start; the rare code that spawns one `Command` several times counts once, and a command built but
//! never spawned still counts. Counting each spawn would need every one of the repo's spawn calls to go through a wrapper.
//!
//! **Whose count.** The count belongs to a scope, never to the process: a [`StartCounter`] counts the commands made on the thread
//! that made it (Rust's test harness runs each test on its own thread, so parallel tests never share a count), plus any thread or
//! task the counter's [`StartScope`] is carried into ([`StartScope::enter`], [`StartScope::wrap`]); a scope entered where its counter
//! already counts adds nothing. Work handed to a thread or task the scope was not carried into is not counted, which is why
//! [`at_most`] fails on zero ([`at_most_or_none`] says, with a reason, that zero is right). With no counter alive the cost is one
//! thread-local read per command.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

thread_local! {
    /// The counters that see this thread's starts (the thread's own and every scope entered on it), each once, with how many
    /// holders it has here: a scope entered on the thread whose counter is already live adds a holder, never a second count.
    static ACTIVE: RefCell<Vec<(Arc<AtomicUsize>, usize)>> = const { RefCell::new(Vec::new()) };
}

/// Notes one start for every counter of this thread. Called by [`crate::command::command`].
pub(crate) fn note() {
    // `try_with`: a command made while the thread is being torn down is not counted, and never panics.
    let _ = ACTIVE.try_with(|active| {
        for (count, _) in active.borrow().iter() {
            count.fetch_add(1, Ordering::Relaxed);
        }
    });
}

fn push(count: &Arc<AtomicUsize>) {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        match active.iter_mut().find(|(c, _)| Arc::ptr_eq(c, count)) {
            Some((_, holders)) => *holders += 1,
            None => active.push((Arc::clone(count), 1)),
        }
    });
}

fn remove(count: &Arc<AtomicUsize>) {
    let _ = ACTIVE.try_with(|active| {
        let mut active = active.borrow_mut();
        if let Some(i) = active.iter().position(|(c, _)| Arc::ptr_eq(c, count)) {
            active[i].1 -= 1;
            if active[i].1 == 0 {
                active.remove(i);
            }
        }
    });
}

/// Counts the starts of this thread (and of every scope it is carried into) while it lives. Not `Send`: it ends on the thread
/// that made it; carry [`StartCounter::scope`] to other threads.
pub struct StartCounter {
    count: Arc<AtomicUsize>,
    _not_send: PhantomData<*const ()>,
}

/// Starts counting the starts of this thread. Counters nest: every live one counts.
pub fn count_starts() -> StartCounter {
    let count = Arc::new(AtomicUsize::new(0));
    push(&count);
    StartCounter {
        count,
        _not_send: PhantomData,
    }
}

impl StartCounter {
    /// The starts counted so far.
    pub fn starts(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    /// A handle that carries this counter into another thread or task.
    pub fn scope(&self) -> StartScope {
        StartScope {
            count: Arc::clone(&self.count),
        }
    }
}

impl Drop for StartCounter {
    fn drop(&mut self) {
        remove(&self.count);
    }
}

impl fmt::Debug for StartCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartCounter")
            .field("starts", &self.starts())
            .finish()
    }
}

/// A counter carried into another thread or task (`Send`, `Clone`).
#[derive(Clone, Debug)]
pub struct StartScope {
    count: Arc<AtomicUsize>,
}

impl StartScope {
    /// Counts this thread's starts for the counter until the guard drops.
    #[must_use = "the scope ends when the guard drops"]
    pub fn enter(&self) -> Entered {
        push(&self.count);
        Entered {
            count: Arc::clone(&self.count),
            _not_send: PhantomData,
        }
    }

    /// `fut`, with the scope entered on every poll, whichever thread polls it (a task of a multi-thread runtime).
    pub fn wrap<F: Future>(&self, fut: F) -> Counted<F> {
        Counted {
            scope: self.clone(),
            fut: Box::pin(fut),
        }
    }
}

/// The guard of [`StartScope::enter`].
#[derive(Debug)]
pub struct Entered {
    count: Arc<AtomicUsize>,
    _not_send: PhantomData<*const ()>,
}

impl Drop for Entered {
    fn drop(&mut self) {
        remove(&self.count);
    }
}

/// The future of [`StartScope::wrap`].
pub struct Counted<F> {
    scope: StartScope,
    fut: Pin<Box<F>>,
}

impl<F: Future> Future for Counted<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let _in = self.scope.enter();
        self.fut.as_mut().poll(cx)
    }
}

/// The count an assertion expects: exactly `n` (a plain number converts to it) or at most `n` ([`at_most`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartBudget {
    /// Exactly this many starts.
    Exactly(usize),
    /// At least one and no more than this many starts: the work did run here, within its budget. A count of zero fails, because it
    /// is what work moved to a thread or task the counter was not carried into looks like.
    AtMost(usize),
    /// No more than this many starts, zero included ([`at_most_or_none`], which needs a reason).
    AtMostOrNone(usize),
}

impl From<usize> for StartBudget {
    fn from(n: usize) -> Self {
        StartBudget::Exactly(n)
    }
}

/// A budget of at least one and at most `n` starts (zero fails: see [`StartBudget::AtMost`]).
pub fn at_most(n: usize) -> StartBudget {
    StartBudget::AtMost(n)
}

/// A budget of at most `n` starts where none is a right answer too (a warm cache, an early return). `reason` says why zero is
/// expected; it is not stored, it is there for the reader of the test. Panics on an empty reason.
#[track_caller]
pub fn at_most_or_none(n: usize, reason: &str) -> StartBudget {
    assert!(
        !reason.trim().is_empty(),
        "at_most_or_none needs a reason: say why starting nothing is right here"
    );
    StartBudget::AtMostOrNone(n)
}

impl StartBudget {
    #[track_caller]
    fn check(self, got: usize) {
        match self {
            StartBudget::Exactly(n) if got != n => {
                let what = if n == 1 {
                    "process start"
                } else {
                    "process starts"
                };
                panic!("expected {n} {what}, got {got}")
            }
            StartBudget::AtMost(_) if got == 0 => panic!(
                "expected at least one process start, got 0: the work ran on a thread or task this counter was not carried into \
                 (carry `StartCounter::scope`), or it started nothing; if none is right, say so with at_most_or_none(n, reason)"
            ),
            StartBudget::AtMost(n) | StartBudget::AtMostOrNone(n) if got > n => {
                panic!("expected at most {n} process starts, got {got}")
            }
            _ => {}
        }
    }
}

/// Runs `f` on this thread and asserts how many programs it started; returns what `f` returned. Starts before `f` do not count.
#[track_caller]
pub fn assert_starts<T>(expected: impl Into<StartBudget>, f: impl FnOnce() -> T) -> T {
    let counter = count_starts();
    let value = f();
    expected.into().check(counter.starts());
    value
}

/// [`assert_starts`] for a future: the count follows it to whichever thread polls it ([`StartScope::wrap`]).
pub async fn assert_starts_async<F: Future>(expected: impl Into<StartBudget>, fut: F) -> F::Output {
    let expected = expected.into();
    let count = Arc::new(AtomicUsize::new(0));
    let scope = StartScope {
        count: Arc::clone(&count),
    };
    let value = scope.wrap(fut).await;
    expected.check(count.load(Ordering::Relaxed));
    value
}

#[cfg(test)]
#[path = "starts_tests.rs"]
mod tests;
