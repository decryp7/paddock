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
    repair_done: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
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

    /// Registers the end of the start repair: the returned sender is used by its thread when it finishes, so Quit
    /// can wait for it (it never leaves a repair writing the record beside the stop marker).
    pub fn track_repair(&self) -> std::sync::mpsc::Sender<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        *self.repair_done.lock().unwrap_or_else(|p| p.into_inner()) = Some(rx);
        tx
    }

    /// Waits up to `bound` for the start repair to end (after [`quit`](Self::quit)); `true` when it ended or never
    /// ran.
    pub fn join_repair(&self, bound: std::time::Duration) -> bool {
        let rx = self.repair_done.lock().unwrap_or_else(|p| p.into_inner()).take();
        rx.is_none_or(|rx| !matches!(rx.recv_timeout(bound), Err(std::sync::mpsc::RecvTimeoutError::Timeout)))
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
    fn quit_cancels_the_running_operations_and_refuses_new_ones() {
        let ops = ServiceOps::default();
        let running = ops.begin().unwrap();
        let ended = ops.begin().unwrap();
        drop(ended);
        ops.quit();
        assert!(running.is_cancelled());
        assert!(ops.begin().is_none(), "nothing starts after Quit");
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
        let (stat, atomic) = (["sta", "tic "].concat(), ["Atomic", "Bool"].concat());
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                assert!(
                    !(code.contains(&stat) && code.contains(&atomic)),
                    "{}:{}: a process-wide flag; give the operation its own `OpCancel` (ADR 0056)",
                    file.display(),
                    n + 1
                );
            }
        }
    }
}
