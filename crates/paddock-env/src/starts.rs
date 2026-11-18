//! A counting seam for budget tests (the bulk-API redesign's G3; PRINCIPLES, "Bulk APIs"; ER-20): how many programs a piece of
//! code starts. [`crate::command::command`] is the one constructor of a program's `Command` (clippy forbids `Command::new`
//! elsewhere), so it notes every start here; a test asserts the count in one line, after Django's `assertNumQueries`:
//!
//! ```
//! use paddock_env::starts::{assert_starts, at_most};
//! # fn delete_all(ids: &[&str]) { let _ = paddock_env::command::command("git").args(ids); }
//! assert_starts(1, || delete_all(&["a", "b", "c"])); // N items, one process
//! assert_starts(at_most(2), || delete_all(&["a"]));
//! ```
//!
//! The count belongs to a scope, never to the process: a [`StartCounter`] counts the commands made on the thread that made it
//! (Rust's test harness runs each test on its own thread, so parallel tests never share a count), plus any thread or task the
//! counter's [`StartScope`] is carried into ([`StartScope::enter`], [`StartScope::wrap`]). Work handed to a thread or task the scope
//! was not carried into is not counted; a budget test carries it, or runs the work on its own thread.
//!
//! What is counted is a `Command` made through the constructor, not a successful spawn: one command spawned again after `ETXTBSY`
//! (paddock-proc's retry) is one start, and a command that fails to spawn still counts. The registered copies of the constructor
//! (`docs/known-copies.toml`, entry `no-console-command`) are not counted until they become calls of it. With no counter alive the
//! cost is one thread-local read per command.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

thread_local! {
    /// The counters that see this thread's starts: the thread's own and every scope entered on it.
    static ACTIVE: RefCell<Vec<Arc<AtomicUsize>>> = const { RefCell::new(Vec::new()) };
}

/// Notes one start for every counter of this thread. Called by [`crate::command::command`].
pub(crate) fn note() {
    // `try_with`: a command made while the thread is being torn down is not counted, and never panics.
    let _ = ACTIVE.try_with(|active| {
        for count in active.borrow().iter() {
            count.fetch_add(1, Ordering::Relaxed);
        }
    });
}

fn push(count: &Arc<AtomicUsize>) {
    ACTIVE.with(|active| active.borrow_mut().push(Arc::clone(count)));
}

fn remove(count: &Arc<AtomicUsize>) {
    let _ = ACTIVE.try_with(|active| {
        let mut active = active.borrow_mut();
        if let Some(i) = active.iter().rposition(|c| Arc::ptr_eq(c, count)) {
            active.remove(i);
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
    /// No more than this many starts.
    AtMost(usize),
}

impl From<usize> for StartBudget {
    fn from(n: usize) -> Self {
        StartBudget::Exactly(n)
    }
}

/// A budget of at most `n` starts.
pub fn at_most(n: usize) -> StartBudget {
    StartBudget::AtMost(n)
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
            StartBudget::AtMost(n) if got > n => {
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
