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
//! (`std::process::exit` in a signal thread), for Windows and for Linux with musl (where the order of `exit()` is not
//! the one relied on here; Debrief ships no musl build and no test runs on one).
//!
//! **Windows** (reasoning, not measured: no Windows run was made for this change). libduckdb is a DLL there. `main`
//! returning reaches the CRT's `exit`, which runs the `atexit` handlers of the program (ours: the drain, then the stop and
//! wait for every registered store thread) with the other threads still running; only then `ExitProcess` ends every other
//! thread and the DLL's `DLL_PROCESS_DETACH` destroys its statics. So the destroyed-static crash of the two macOS and
//! Linux reports cannot happen with a thread alive. What stays open is a thread ended by `ExitProcess` inside DuckDB
//! while it holds an internal lock the detach then waits for; the drain closes that too, because it stops and waits for
//! the threads before the process ends, on the same registration as the other platforms. A process that ends in
//! `ExitProcess` or a fast-fail instead of through `main` (a crash, `TerminateProcess`) skips every handler.

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

/// How long a test's guard waits for store threads to leave.
const STOP_BUDGET: Duration = Duration::from_secs(10);

/// One background thread of a store (the catalogue builder, the full-text indexer), registered while it runs.
struct Registered {
    id: u64,
    name: &'static str,
    /// Asks the thread to end; does not wait.
    stop: std::sync::Arc<dyn Fn() + Send + Sync>,
}

static THREADS: std::sync::Mutex<Vec<Registered>> = std::sync::Mutex::new(Vec::new());
static NEXT_THREAD: AtomicUsize = AtomicUsize::new(1);

/// Proof that a store thread is running: it is on the process-wide list until this is dropped, which the thread does as
/// the very last thing it does (declare it first in the thread's closure). The exit drain and [`stop_store_threads`] ask
/// every listed thread to stop and wait for the list to empty, so no thread of ours is inside DuckDB when the process
/// exits, whatever kept its store alive (a worker thread's strong reference, a leaked handle, a test that ended early).
pub(crate) struct StoreThread(u64);

/// Lists the calling thread as a store thread; `stop` asks it to end (flags and wake-ups only, it must not block).
pub(crate) fn register_thread(name: &'static str, stop: std::sync::Arc<dyn Fn() + Send + Sync>) -> StoreThread {
    let id = NEXT_THREAD.fetch_add(1, Ordering::SeqCst) as u64;
    crate::lock(&THREADS).push(Registered { id, name, stop });
    StoreThread(id)
}

impl Drop for StoreThread {
    fn drop(&mut self) {
        // `lock` waits for the list: it is only held for an instant (to push, to list names or to clone the stops, never
        // across a stop call), so the wait is short. A poisoned list is still emptied of this thread.
        THREADS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|t| t.id != self.0);
    }
}

/// The names of the store threads running now. Tests assert this is empty after a store and everything that held it
/// are gone.
pub fn live_store_threads() -> Vec<&'static str> {
    crate::lock(&THREADS).iter().map(|t| t.name).collect()
}

/// Asks every running store thread to stop and interrupts the statements they run, then waits at most `budget` for them
/// to leave. Returns the names of those still running (empty: all stopped). Does not mark the process as exiting.
pub fn stop_store_threads(budget: Duration) -> Vec<&'static str> {
    stop_store_threads_since(0, budget)
}

fn stop_store_threads_since(first: u64, budget: Duration) -> Vec<&'static str> {
    // Clone the stops out: they take the stores' own locks, never ours.
    let stops: Vec<_> = crate::lock(&THREADS)
        .iter()
        .filter(|t| t.id >= first)
        .map(|t| t.stop.clone())
        .collect();
    for stop in &stops {
        stop();
    }
    interrupt_all();
    let started = Instant::now();
    loop {
        let left: Vec<&'static str> = crate::lock(&THREADS).iter().filter(|t| t.id >= first).map(|t| t.name).collect();
        if left.is_empty() || started.elapsed() >= budget {
            return left;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Test guard: when dropped, stops and waits for every store thread started since it was made, and fails the test (a
/// panic, unless the test already panics) if one does not leave within a few seconds. Make it first in a test, so it is
/// dropped last. It stops the threads of every store of the process that began after it, so use it where a test owns the
/// process (nextest runs each test in its own); a `cargo test` binary that shares one process between parallel tests
/// would stop its neighbours' stores.
#[must_use = "bind the guard to a name: dropping it at once stops nothing"]
pub struct StoreThreadsGuard(u64, Duration);

impl StoreThreadsGuard {
    /// Marks now.
    pub fn new() -> StoreThreadsGuard {
        StoreThreadsGuard(NEXT_THREAD.load(Ordering::SeqCst) as u64, STOP_BUDGET)
    }

    /// A guard that waits `budget` for the threads (tests of the guard itself).
    #[cfg(test)]
    pub(crate) fn with_budget(budget: Duration) -> StoreThreadsGuard {
        StoreThreadsGuard(NEXT_THREAD.load(Ordering::SeqCst) as u64, budget)
    }

    /// Stops and waits now; the names of those that stayed.
    pub fn stop(&self) -> Vec<&'static str> {
        stop_store_threads_since(self.0, self.1)
    }
}

impl Default for StoreThreadsGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for StoreThreadsGuard {
    fn drop(&mut self) {
        let left = self.stop();
        if !left.is_empty() && !std::thread::panicking() {
            panic!("store threads outlived their test: {left:?}");
        }
    }
}

/// Test helper: stops and waits for the store threads started from now on when the calling thread ends (the test thread
/// of a nextest test; also after a panic). Call it once at the top of a fixture (the app's `test_app`); a second call on
/// the same thread does nothing. It acts only under nextest (the `NEXTEST` variable is set): there the process is the
/// test's own, so every store thread in it is this test's. A `cargo test` process shares its threads between tests
/// running side by side, and stopping "the threads since" would stop a neighbour's; there the exit drain is what ends
/// them. A store thread that does not leave within ten seconds is named on stderr and the process aborts, as a crash at
/// exit would have done, only visibly and with the thread's name.
pub fn stop_store_threads_when_this_thread_ends() {
    if std::env::var_os("NEXTEST").is_some() {
        stop_at_thread_end(STOP_BUDGET);
    }
}

fn stop_at_thread_end(budget: Duration) {
    /// The mark and the budget (not a [`StoreThreadsGuard`]: its own drop would panic inside this thread-local's drop).
    struct AtThreadEnd(u64, Duration);
    impl Drop for AtThreadEnd {
        fn drop(&mut self) {
            let left = stop_store_threads_since(self.0, self.1);
            if !left.is_empty() {
                eprintln!("store threads outlived their test: {left:?}");
                std::process::abort();
            }
        }
    }
    thread_local! {
        static END: std::cell::RefCell<Option<AtThreadEnd>> = const { std::cell::RefCell::new(None) };
    }
    let _ = END.try_with(|e| {
        let mut e = e.borrow_mut();
        if e.is_none() {
            *e = Some(AtThreadEnd(NEXT_THREAD.load(Ordering::SeqCst) as u64, budget));
        }
    });
}

/// The list of store threads at exit: tried for a few milliseconds (a thread holds it for an instant), never waited for
/// without end. `None` when it stayed busy.
fn threads_at_exit() -> Option<std::sync::MutexGuard<'static, Vec<Registered>>> {
    for _ in 0..50 {
        match THREADS.try_lock() {
            Ok(all) => return Some(all),
            Err(std::sync::TryLockError::Poisoned(p)) => return Some(p.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    None
}

/// Tests: how often the exit drain looked at the list of threads.
#[cfg(test)]
pub(crate) static LOOKS: AtomicUsize = AtomicUsize::new(0);

fn threads_running() -> bool {
    #[cfg(test)]
    LOOKS.fetch_add(1, Ordering::SeqCst);
    threads_at_exit().is_none_or(|all| !all.is_empty())
}

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
    // The registered store threads stop too (the list is tried for a few milliseconds, never waited for without end),
    // then everything is waited for.
    match threads_at_exit() {
        Some(all) => {
            let stops: Vec<_> = all.iter().map(|t| t.stop.clone()).collect();
            drop(all);
            for stop in stops {
                stop();
            }
        }
        None => log::warn!("exit drain: the list of store threads stayed busy; they were not asked to stop"),
    }
    let started = Instant::now();
    while (ACTIVE.load(Ordering::SeqCst) > 0 || threads_running()) && started.elapsed() < DRAIN {
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(1));
    }
    // ER-27: a drain that gave up says what it gave up on.
    let statements = ACTIVE.load(Ordering::SeqCst);
    let threads: Vec<&'static str> = threads_at_exit().map_or_else(Vec::new, |all| all.iter().map(|t| t.name).collect());
    if statements > 0 || !threads.is_empty() {
        log::warn!(
            "exit drain gave up after {:?}: {statements} statement(s) still running, store threads still running: {threads:?}",
            started.elapsed()
        );
    }
}

/// The main thread's exit drain (macOS and Linux): see the module documentation.
#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
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

    /// Whether the calling thread is the process's main thread.
    pub(super) fn is_main_thread() -> bool {
        // SAFETY: both calls only read the caller's own identity.
        #[cfg(target_os = "macos")]
        unsafe {
            libc::pthread_main_np() != 0
        }
        #[cfg(target_os = "linux")]
        unsafe {
            // The main thread's id is the process id.
            libc::syscall(libc::SYS_gettid) == libc::syscall(libc::SYS_getpid)
        }
    }

    /// Whether [`arm`] ran (before `main`, on the main thread).
    static ARMED: AtomicBool = AtomicBool::new(false);

    /// Touches the main thread's [`MAIN_THREAD_EXIT`], which registers its destructor; runs as a load-time initializer.
    extern "C" fn arm() {
        // Only the main thread's thread-locals are destroyed first by `exit()`: an initializer that runs on another
        // thread (a library loaded late) would arm that thread's, which proves nothing.
        if is_main_thread() && MAIN_THREAD_EXIT.try_with(|_| ()).is_ok() {
            ARMED.store(true, Ordering::SeqCst);
        }
    }

    /// The initializer that arms the drain before `main` runs (the loader runs initializers on the main thread).
    #[used]
    #[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
    #[cfg_attr(target_os = "linux", link_section = ".init_array")]
    pub(super) static ARM: extern "C" fn() = arm;

    /// Whether the drain is armed (the initializer was linked and ran on the main thread).
    pub(super) fn armed_now() -> bool {
        ARMED.load(Ordering::SeqCst)
    }

    /// Whether the drain is armed (tests).
    #[cfg(test)]
    pub(super) fn armed() -> bool {
        armed_now()
    }
}

fn register() {
    // SAFETY: `at_exit` is a plain `extern "C" fn()` that only touches atomics and sleeps.
    unsafe {
        libc::atexit(at_exit);
    }
}

/// Whether the main thread's exit drain was armed before `main` in this binary: `Some(true)` armed, `Some(false)` linked
/// out or not run, `None` on a platform that does not use it (Windows, musl). A binary that opens a store checks this in
/// a test, so a linker that drops the initializer's object (the keep-alive in [`install`]) fails a test, not a run.
pub fn main_thread_drain_armed() -> Option<bool> {
    install();
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    return Some(main_thread::armed_now());
    #[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
    None
}

/// Registers the exit handler for the first time (call when the first database is opened).
pub(crate) fn install() {
    // Keeps the initializer's object in the link: an archive member nothing refers to is left out.
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    std::hint::black_box(&main_thread::ARM);
    INSTALL.call_once(register);
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    const ORDER_CHILD: &str = "DEBRIEF_SHUTDOWN_ORDER_CHILD";

    /// Writes what the late exit handler saw, with `write(2)`: Rust's stdout is not for use while the process exits.
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
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
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    #[test]
    fn exit_order_child() {
        if std::env::var_os(ORDER_CHILD).is_none() {
            return;
        }
        super::install();
        // SAFETY: registers a plain `extern "C"` function that only writes to a descriptor.
        assert_eq!(unsafe { libc::atexit(late_handler) }, 0);
        // A store thread that nothing joined (its store leaked, or kept alive by a worker's reference): the drain asks it
        // to stop, and the exit waits until it has left the list.
        *crate::lock(&RUNNING) = Some(super::register_thread(
            "debrief-test-thread",
            std::sync::Arc::new(|| {
                let line = b"store thread asked to stop at exit\n";
                // SAFETY: writes a static buffer to the standard output descriptor.
                unsafe {
                    libc::write(1, line.as_ptr().cast(), line.len());
                }
                // The thread leaves only after the drain looked at the list several times: only a drain that really
                // waits for the list sees "left" before the other handlers.
                std::thread::spawn(|| {
                    paddock_testkit::wait_until("the exit drain to look at the list again", || {
                        super::LOOKS.load(std::sync::atomic::Ordering::SeqCst) >= 3
                    });
                    let line = b"store thread left\n";
                    // SAFETY: writes a static buffer to the standard output descriptor.
                    unsafe {
                        libc::write(1, line.as_ptr().cast(), line.len());
                    }
                    crate::lock(&RUNNING).take();
                });
            }),
        ));
    }

    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    static RUNNING: std::sync::Mutex<Option<super::StoreThread>> = std::sync::Mutex::new(None);

    /// The drain of the main thread's exit is armed before `main` (the initializer was linked and ran).
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    #[test]
    fn the_main_thread_exit_drain_is_armed_before_main() {
        super::install();
        assert!(super::main_thread::armed(), "the load-time initializer did not run");
    }

    /// Regression (CI 7997, SIGBUS at exit in `DistinctStatistics::UpdateSample`): the drain runs before every exit
    /// handler and C++ static destructor, also those registered after the store opened. `exit()` runs handlers
    /// last-registered first, so DuckDB destroyed a static it had built lazily while the catalogue thread's statement
    /// still read it, and the drain waited only afterwards.
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
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
            stdout.contains("store thread asked to stop at exit"),
            "the exit drain did not ask the running store thread to stop:\n{stdout}"
        );
        let at = |what: &str| stdout.find(what).unwrap_or_else(|| panic!("{what:?} missing from:\n{stdout}"));
        assert!(
            at("store thread asked to stop at exit") < at("store thread left") && at("store thread left") < at("late handler"),
            "the drain did not wait for the store thread to leave before the other exit handlers ran:\n{stdout}"
        );
        assert!(
            stdout.contains("late handler: drained first"),
            "an exit handler registered after the store opened ran before the drain:\n{stdout}"
        );
    }

    /// A store thread that outlives its store is stopped by the test guard, and a thread that ignores the stop is named
    /// in the failure (regression: CI 6734 and 7997, threads still inside DuckDB when the test process exited).
    #[test]
    fn the_guard_stops_a_store_thread_whose_store_was_leaked_and_names_one_that_stays() {
        use debrief_core::ports::Ingest;
        let guard = super::StoreThreadsGuard::new();
        let dir = debrief_ipc::testing::scratch_dir("guard-leak");
        let store = crate::DuckStore::create(&dir, debrief_core::Profile::default_profile(), debrief_core::PluginRegistry::new()).unwrap();
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/serilog-text");
        store.open(&[samples]).unwrap().wait().unwrap();
        assert!(
            super::live_store_threads().contains(&"debrief-catalogue"),
            "the catalogue thread is listed while it runs"
        );
        // Nothing drops the store, as when a worker thread still holds the last reference at the end of a test.
        std::mem::forget(store);
        assert_eq!(guard.stop(), Vec::<&str>::new(), "the guard left a store thread running");
        assert!(!super::live_store_threads().contains(&"debrief-catalogue"));
        assert!(!super::live_store_threads().contains(&"debrief-fts"));

        let stubborn = super::register_thread("debrief-stubborn", std::sync::Arc::new(|| {}));
        assert_eq!(
            super::stop_store_threads_since(guard.0, std::time::Duration::from_millis(20)),
            ["debrief-stubborn"]
        );
        drop(stubborn);
        std::mem::forget(guard); // its work is done: dropping it would wait on nothing, but keep the test's end simple
    }

    /// The guard fails the test, naming the thread, when a store thread does not leave (regression: a guard that only
    /// stopped quietly would let the leak through).
    #[test]
    fn the_guard_panics_naming_a_thread_that_does_not_leave() {
        let stubborn = super::register_thread("debrief-stubborn-guard", std::sync::Arc::new(|| {}));
        let guard = super::StoreThreadsGuard::with_budget(std::time::Duration::from_millis(20));
        // The thread registered before the guard is not its own; register another one after it.
        let late = super::register_thread("debrief-stubborn-late", std::sync::Arc::new(|| {}));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(guard)));
        let message = *caught.expect_err("the guard did not panic").downcast::<String>().unwrap();
        assert!(
            message.contains("debrief-stubborn-late") && !message.contains("debrief-stubborn-guard"),
            "{message}"
        );
        drop((stubborn, late));
    }

    /// The thread-end hook aborts the process, naming the thread (runs in a child: an abort cannot be caught).
    #[test]
    fn the_thread_end_hook_aborts_naming_a_thread_that_does_not_leave() {
        if std::env::var_os("DEBRIEF_THREAD_END_CHILD").is_some() {
            std::thread::spawn(|| {
                super::stop_at_thread_end(std::time::Duration::from_millis(20));
                let _stubborn = super::register_thread("debrief-stubborn-end", std::sync::Arc::new(|| {}));
                // The test thread ends here: the hook finds the thread still listed.
                std::mem::forget(_stubborn);
            })
            .join()
            .ok();
            return;
        }
        let out = debrief_core::process::quiet_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "shutdown::tests::the_thread_end_hook_aborts_naming_a_thread_that_does_not_leave",
                "--test-threads=1",
                "--nocapture",
            ])
            .env("DEBRIEF_THREAD_END_CHILD", "1")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "the child did not abort: {:?}", out.status);
        assert!(
            stderr.contains("store threads outlived their test") && stderr.contains("debrief-stubborn-end"),
            "{stderr}"
        );
    }
}
