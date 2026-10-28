//! Quiet DuckDB before the process exits. DuckDB 2.0 keeps process-wide state that C++ destroys during `exit()`
//! (static destructors), while a thread of ours may still be inside a statement: it then dereferenced a destroyed
//! global and the process died with SIGSEGV after its work was done (CI run 6734: every `debrief-app` test that left a
//! live-tail flush running; found with the crash report, `ClientContext::SubmitPreparedStatementInternal`).
//!
//! An `atexit` handler stops new borrowed connections and waits, bounded, for the statements already running;
//! [`Active`] counts them. Handlers run last-registered first, and DuckDB constructs many of its statics lazily, on the
//! first statement that needs them (and registers their destructors then): a handler registered once, before any
//! statement, would run after those. So the handler is registered again whenever the number of borrowed connections
//! reaches a power of two (about 20 times in a million statements); a repeat run finds nothing to wait for.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

/// Longest the exit handler waits for running statements.
const DRAIN: Duration = Duration::from_secs(5);

static DOWN: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static INSTALL: Once = Once::new();
static BORROWED: AtomicUsize = AtomicUsize::new(0);

/// One borrowed connection that may run a statement; dropping it lets the exit handler go on.
pub(crate) struct Active(());

impl Active {
    /// `None` once the process is shutting down: no new statement may start.
    pub(crate) fn enter() -> Option<Active> {
        let n = BORROWED.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_power_of_two() && !DOWN.load(Ordering::SeqCst) {
            register();
        }
        ACTIVE.fetch_add(1, Ordering::SeqCst);
        if DOWN.load(Ordering::SeqCst) {
            ACTIVE.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Active(()))
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

extern "C" fn at_exit() {
    DOWN.store(true, Ordering::SeqCst);
    let started = Instant::now();
    while ACTIVE.load(Ordering::SeqCst) > 0 && started.elapsed() < DRAIN {
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn register() {
    // SAFETY: `at_exit` is a plain `extern "C" fn()` that only touches atomics and sleeps.
    unsafe {
        libc::atexit(at_exit);
    }
}

/// Registers the exit handler for the first time (call when the first database is opened).
pub(crate) fn install() {
    INSTALL.call_once(register);
}
