//! Who may stop a registration, a repair or a wait for the service (ADR 0056, cancel ownership).
//!
//! Every operation owns its own [`OpCancel`]; nothing here is a process-wide flag. The one app-wide signal is Quit:
//! [`ServiceOps`], app state, cancels the operations that are running and refuses new ones.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// The cancel of one operation. Clones share it; the shared service manager takes it as `&AtomicBool`
/// (`paddock_service` checks it between OS steps).
#[derive(Clone, Debug, Default)]
pub struct OpCancel(Arc<AtomicBool>);

impl OpCancel {
    /// The flag the service manager checks.
    pub fn flag(&self) -> &AtomicBool {
        &self.0
    }

    /// Asks the operation to stop at its next step.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether a stop was asked for.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The running service operations and whether the app is quitting. Held as app state.
#[derive(Default)]
pub struct ServiceOps {
    quitting: AtomicBool,
    running: Mutex<Vec<Weak<AtomicBool>>>,
    /// The ends of the operations Quit waits for (the start repair, the resume at start).
    repair_done: Mutex<Vec<std::sync::mpsc::Receiver<()>>>,
    /// Whether the first-run registration was tried in this app (an attempt is not repeated in the same run).
    first_run: crate::login::FirstRunAttempt,
}

impl ServiceOps {
    fn running(&self) -> std::sync::MutexGuard<'_, Vec<Weak<AtomicBool>>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A new operation's own cancel, or `None` once the app is quitting (nothing starts after Quit).
    pub fn begin(&self) -> Option<OpCancel> {
        if self.quitting.load(Ordering::SeqCst) {
            return None;
        }
        let token = OpCancel::default();
        let mut running = self.running();
        running.retain(|w| w.strong_count() > 0);
        running.push(Arc::downgrade(&token.0));
        // Quit may have come between the check and the registration: a token born after it is cancelled at once.
        if self.quitting.load(Ordering::SeqCst) {
            token.cancel();
        }
        Some(token)
    }

    /// Registers the end of an operation Quit must wait for (the start repair, the resume at start): the returned sender is used by
    /// its thread when it finishes, so Quit can wait for it (it never leaves one writing the record beside the stop marker). Every
    /// registered operation is waited for, not only the last.
    pub fn track_repair(&self) -> std::sync::mpsc::Sender<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.repair_done.lock().unwrap_or_else(|p| p.into_inner()).push(rx);
        tx
    }

    /// Waits up to `bound` in all for the tracked operations to end (after [`quit`](Self::quit)); `true` when they all ended or
    /// none ran.
    pub fn join_repair(&self, bound: std::time::Duration) -> bool {
        let all = std::mem::take(&mut *self.repair_done.lock().unwrap_or_else(|p| p.into_inner()));
        let deadline = std::time::Instant::now() + bound;
        all.into_iter().all(|rx| {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            !matches!(rx.recv_timeout(left), Err(std::sync::mpsc::RecvTimeoutError::Timeout))
        })
    }

    /// Runs `work` as an operation of its own that Quit cancels and joins: `None` (and `work` does not run) once the app is
    /// quitting, so a resume that comes after "Quit and stop measuring" cannot register the service again.
    pub fn run_tracked<T>(&self, work: impl FnOnce(OpCancel) -> T) -> Option<T> {
        // Tracked first: a Quit between the two still finds the operation to wait for. The sender is dropped when the operation
        // ends, however it ends (a panic included), which is the end the waiter sees.
        let _done = self.track_repair();
        let cancel = self.begin()?;
        Some(work(cancel))
    }

    /// This app's first-run attempt.
    pub fn first_run_attempt(&self) -> &crate::login::FirstRunAttempt {
        &self.first_run
    }

    /// Quit: refuses new operations and asks the running ones to stop at their next step.
    pub fn quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        for flag in self.running().iter().filter_map(Weak::upgrade) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_operation_has_its_own_cancel() {
        let ops = ServiceOps::default();
        let (first, second) = (ops.begin().unwrap(), ops.begin().unwrap());
        first.cancel();
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled(), "a cancel belongs to one operation");
        assert!(!ops.begin().unwrap().is_cancelled(), "the next operation starts clean");
    }

    #[test]
    fn the_flag_is_the_operations_own() {
        let token = OpCancel::default();
        token.cancel();
        assert!(token.flag().load(Ordering::SeqCst), "the manager checks the flag the operation's cancel sets");
        assert!(!OpCancel::default().flag().load(Ordering::SeqCst));
    }

    #[test]
    fn ended_operations_are_forgotten() {
        let ops = ServiceOps::default();
        let ended: Vec<_> = (0..3).map(|_| ops.begin().unwrap()).collect();
        drop(ended);
        let _live = ops.begin().unwrap();
        assert_eq!(ops.running().len(), 1, "a finished operation stays in the list");
    }

    #[test]
    fn quit_cancels_the_running_operations_and_refuses_new_ones() {
        let ops = ServiceOps::default();
        let running = ops.begin().unwrap();
        let ended = ops.begin().unwrap();
        drop(ended);
        ops.quit();
        assert!(running.is_cancelled());
        assert!(ops.begin().is_none(), "nothing starts after Quit");
    }


    #[test]
    fn a_tracked_operation_is_joined_by_quit_and_none_starts_after_it() {
        let ops = std::sync::Arc::new(ServiceOps::default());
        let ended = std::sync::Arc::new(AtomicBool::new(false));
        let (began, wake) = std::sync::mpsc::channel::<()>();
        let (finish, wait) = std::sync::mpsc::channel::<()>();
        let (worker, flag) = (ops.clone(), ended.clone());
        let op = std::thread::spawn(move || {
            worker.run_tracked(move |cancel| {
                began.send(()).unwrap();
                wait.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                flag.store(true, Ordering::SeqCst);
                cancel.is_cancelled()
            })
        });
        wake.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        ops.quit();
        let joiner = ops.clone();
        let join = std::thread::spawn(move || joiner.join_repair(std::time::Duration::from_secs(5)));
        finish.send(()).unwrap();
        assert!(join.join().unwrap(), "quit's join gave up on a running operation");
        assert!(ended.load(Ordering::SeqCst), "quit's join returned before the operation ended");
        assert_eq!(op.join().unwrap(), Some(true), "the running operation saw Quit's cancel");
        let mut ran = false;
        assert_eq!(ops.run_tracked(|_| ran = true), None, "nothing starts after Quit, so nothing registers again");
        assert!(!ran);
    }

    #[test]
    fn quit_waits_for_every_tracked_operation_not_only_the_last() {
        let ops = ServiceOps::default();
        let first = ops.track_repair();
        let second = ops.track_repair();
        second.send(()).unwrap();
        first.send(()).unwrap();
        assert!(ops.join_repair(std::time::Duration::from_secs(5)));
        let _running = ops.track_repair();
        let finished = ops.track_repair();
        finished.send(()).unwrap();
        assert!(!ops.join_repair(std::time::Duration::from_millis(50)), "the first of two is still running");
    }

    /// The names of the process-wide items in `text`: any `static` item or `thread_local!` (a flag, an atomic, a `OnceLock`, a counter
    /// or a cell), whatever type it is wrapped in. `allowed` names the ones that are the process's by nature.
    fn process_wide_items(text: &str, allowed: &[&str]) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("").trim();
            let code = code.strip_prefix("pub(crate) ").or_else(|| code.strip_prefix("pub ")).unwrap_or(code);
            let code = code.strip_prefix("static mut ").or_else(|| code.strip_prefix("static "));
            if let Some(rest) = code {
                let name = rest.split([':', ' ']).next().unwrap_or("").to_string();
                if !allowed.contains(&name.as_str()) {
                    found.push((n + 1, name));
                }
            } else if line.trim_start().starts_with("thread_local!") {
                found.push((n + 1, "thread_local".to_string()));
            }
        }
        found
    }

    /// The probe has to see the shapes that made this bug: the earlier scan matched `static` and `AtomicBool` on one line only, so a
    /// flag in a wrapper type (`FirstRunAttempt`), a `OnceLock` or a split declaration passed.
    #[test]
    fn the_scan_for_process_wide_flags_sees_every_shape_of_one() {
        let seen = |text: &str| process_wide_items(text, &[]).len();
        assert_eq!(seen("pub static FIRST_RUN_ATTEMPT: FirstRunAttempt = FirstRunAttempt::new();"), 1, "the wrapped flag");
        assert_eq!(seen("static CANCEL: AtomicBool = AtomicBool::new(false);"), 1);
        assert_eq!(seen("static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();"), 1);
        assert_eq!(seen("pub(crate) static COUNT: AtomicUsize ="), 1, "a declaration split over lines");
        assert_eq!(seen("static mut N: u32 = 0;"), 1);
        assert_eq!(seen("thread_local! { static T: Cell<bool> = Cell::new(false); }"), 1);
        assert_eq!(seen("fn f(x: &'static str) -> &'static str { x } // static A: AtomicBool"), 0, "'static and comments are not items");
        assert_eq!(seen("const A: AtomicBool = AtomicBool::new(false);"), 0);
        assert_eq!(process_wide_items("static OK: u8 = 0;", &["OK"]), []);
    }

    /// ER-21: a process-wide cancel or quit flag made this class of bug (one test's or one operation's cancel
    /// reaching another); the sources that decide a service operation hold none.
    #[test]
    fn no_static_cancel_or_quit_flag_is_in_the_service_sources() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = vec![root.join("src/login.rs"), root.join("src/quit.rs")];
        let service = root.join("../../crates/dyno-agent/src/service");
        let mut dirs = vec![service];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    files.push(path);
                }
            }
        }
        // `STOP_BEGUN` is Quit's once-per-process claim and `FIRST_RUN_MARKER_PATH` the one folder the first run's marker is in: neither
        // is the cancel of an operation.
        let allowed = ["STOP_BEGUN", "FIRST_RUN_MARKER_PATH"];
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let tests_at = text.find("#[cfg(test)]").unwrap_or(text.len());
            let found = process_wide_items(&text[..tests_at], &allowed);
            assert!(
                found.is_empty(),
                "{}: {found:?} is process-wide state; give the operation its own `OpCancel` or put it in `ServiceOps` (ADR 0056)",
                file.display()
            );
        }
    }
}
