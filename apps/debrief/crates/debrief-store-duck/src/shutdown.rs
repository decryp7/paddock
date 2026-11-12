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
//!
//! That still left a window: a static DuckDB first builds after the last registration is destroyed before the drain
//! (CI 7997: SIGBUS in `DistinctStatistics::UpdateSample`, whose two sample selections are function-local statics, while
//! the catalogue thread's append still read one). So on macOS and Linux the drain runs first of all, from a destructor
//! of a main-thread thread-local armed before `main`: `exit()` on the main thread runs that thread's thread-local
//! destructors before any `atexit` handler or C++ static destructor (Apple's libc `exit`, `_tlv_exit` then
//! `__cxa_finalize`; glibc's `__run_exit_handlers`, `__call_tls_dtors` first). A test process and the store worker both
//! end that way, by returning from `main`. The `atexit` registrations stay for an exit from another thread
//! (`std::process::exit` in a signal thread) and for Windows.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

/// Longest the exit handler waits for running statements.
const DRAIN: Duration = Duration::from_secs(5);

static DOWN: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static INSTALL: Once = Once::new();
static BORROWED: AtomicUsize = AtomicUsize::new(0);
/// Interrupts of the connections background threads run their statements on (the indexer, the catalogue): the exit
/// handler fires them, so a step that holds a statement past the drain does not run into DuckDB's destructors.
/// Weak: a closed connection's handle goes with it, and is pruned at the next registration.
static INTERRUPTS: std::sync::Mutex<Vec<std::sync::Weak<duckdb::InterruptHandle>>> = std::sync::Mutex::new(Vec::new());

/// Whether the process is exiting: background steps stop before their next statement.
pub(crate) fn down() -> bool {
    DOWN.load(Ordering::SeqCst)
}

/// Lets the exit handler interrupt the statement running on `conn` (a background thread's own connection).
pub(crate) fn interrupt_on_exit(conn: &duckdb::Connection) {
    let handle = conn.interrupt_handle();
    if let Ok(mut all) = INTERRUPTS.lock() {
        all.retain(|h| h.strong_count() > 0);
        all.push(std::sync::Arc::downgrade(&handle));
    }
}

/// Interrupts the statement every background thread of ours runs now (the indexer's, the catalogue's). Does not mark the
/// process as exiting: the threads are stopped and joined by their owners, then the process ends (see
/// [`crate::DuckStore::stop_for_exit`]).
pub(crate) fn interrupt_all() {
    for handle in crate::lock(&INTERRUPTS).iter().filter_map(std::sync::Weak::upgrade) {
        handle.interrupt();
    }
}

/// One borrowed connection that may run a statement; dropping it lets the exit handler go on.
pub(crate) struct Active(());

#[cfg(test)]
thread_local! {
    /// How many [`Active`] guards the current thread holds (tests check that a statement runs under one).
    static HELD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Whether the current thread holds an [`Active`] guard, so the exit handler would wait for its statement.
#[cfg(test)]
pub(crate) fn held_here() -> bool {
    HELD.with(|h| h.get() > 0)
}

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
        #[cfg(test)]
        HELD.with(|h| h.set(h.get() + 1));
        Some(Active(()))
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        #[cfg(test)]
        HELD.with(|h| h.set(h.get() - 1));
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Runs the exit handler now, as `exit()` would (tests, in a child process of their own: it cannot be undone).
#[cfg(test)]
pub(crate) fn begin_exit() {
    at_exit();
}

extern "C" fn at_exit() {
    DOWN.store(true, Ordering::SeqCst);
    // Never wait for the lock at exit: a thread may hold it (it is only held to push a handle).
    if let Ok(all) = INTERRUPTS.try_lock() {
        for handle in all.iter().filter_map(std::sync::Weak::upgrade) {
            handle.interrupt();
        }
    }
    let started = Instant::now();
    while ACTIVE.load(Ordering::SeqCst) > 0 && started.elapsed() < DRAIN {
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The main thread's exit drain (macOS and Linux): see the module documentation.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod main_thread {
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Drains when the main thread's thread-locals are destroyed: the first thing `exit()` does on the main thread.
    struct MainThreadExit;

    impl Drop for MainThreadExit {
        fn drop(&mut self) {
            super::at_exit();
        }
    }

    thread_local! {
        static MAIN_THREAD_EXIT: MainThreadExit = const { MainThreadExit };
    }

    /// Whether [`arm`] ran (before `main`, on the main thread).
    static ARMED: AtomicBool = AtomicBool::new(false);

    /// Touches the main thread's [`MAIN_THREAD_EXIT`], which registers its destructor; runs as a load-time initializer.
    extern "C" fn arm() {
        if MAIN_THREAD_EXIT.try_with(|_| ()).is_ok() {
            ARMED.store(true, Ordering::SeqCst);
        }
    }

    /// The initializer that arms the drain before `main` runs (the loader runs initializers on the main thread).
    #[used]
    #[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
    #[cfg_attr(target_os = "linux", link_section = ".init_array")]
    pub(super) static ARM: extern "C" fn() = arm;

    /// Whether the drain is armed (tests: the initializer was linked and ran).
    #[cfg(test)]
    pub(super) fn armed() -> bool {
        ARMED.load(Ordering::SeqCst)
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
    // Keeps the initializer's object in the link: an archive member nothing refers to is left out.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    std::hint::black_box(&main_thread::ARM);
    INSTALL.call_once(register);
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    const ORDER_CHILD: &str = "DEBRIEF_SHUTDOWN_ORDER_CHILD";

    /// Writes what the late exit handler saw, with `write(2)`: Rust's stdout is not for use while the process exits.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    extern "C" fn late_handler() {
        let line: &[u8] = if super::down() {
            b"late handler: drained first\n"
        } else {
            b"late handler: ran before the drain\n"
        };
        // SAFETY: writes a static buffer to the standard output descriptor.
        unsafe {
            libc::write(1, line.as_ptr().cast(), line.len());
        }
    }

    /// Runs in the child process only: arms the exit handler as opening a store does, then registers an exit handler
    /// of its own afterwards, as DuckDB does for a function-local static it builds on the first statement that needs it
    /// (the two sample selections of `DistinctStatistics::UpdateSample`, CI 7997). The process then exits normally,
    /// through `main`'s return, as every nextest test process does.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn exit_order_child() {
        if std::env::var_os(ORDER_CHILD).is_none() {
            return;
        }
        super::install();
        // SAFETY: registers a plain `extern "C"` function that only writes to a descriptor.
        assert_eq!(unsafe { libc::atexit(late_handler) }, 0);
    }

    /// The drain of the main thread's exit is armed before `main` (the initializer was linked and ran).
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_main_thread_exit_drain_is_armed_before_main() {
        super::install();
        assert!(super::main_thread::armed(), "the load-time initializer did not run");
    }

    /// Regression (CI 7997, SIGBUS at exit in `DistinctStatistics::UpdateSample`): the drain runs before every exit
    /// handler and C++ static destructor, also those registered after the store opened. `exit()` runs handlers
    /// last-registered first, so DuckDB destroyed a static it had built lazily while the catalogue thread's statement
    /// still read it, and the drain waited only afterwards.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_drain_runs_before_exit_handlers_registered_after_the_store_opened() {
        let out = debrief_core::process::quiet_command(std::env::current_exe().unwrap())
            .args(["--exact", "shutdown::tests::exit_order_child", "--test-threads=1", "--nocapture"])
            .env(ORDER_CHILD, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "child failed: {:?}\n{stdout}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            stdout.contains("late handler: drained first"),
            "an exit handler registered after the store opened ran before the drain:\n{stdout}"
        );
    }
}
