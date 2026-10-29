//! Who may change the catalog and the full-text index from a background thread (the indexer and the catalogue).
//!
//! - The store's write lock, as every writer: imports, compaction, removals and clears hold it.
//! - While an import only appends (from its sink's creation to the last segment listed, before compaction), its
//!   append window: background steps run beside the parse threads, so search is ready soon after the import ends
//!   instead of starting then. An import adds segments and ready sources and changes nothing a background step
//!   reads; a step in the window only takes segments whose sources are all ready ([`Pass::Appending`]).
//! - The import closes the window (and waits for the steps inside it) before it compacts or renumbers anything.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard};

/// The write lock and the append window of one store.
#[derive(Clone)]
pub(crate) struct Gate {
    pub write: Arc<Mutex<()>>,
    /// Whether the window is open: new steps enter only while it is.
    open: Arc<AtomicBool>,
    /// Held (shared) by each step inside the window; closing takes it exclusively to wait for them. Closing clears
    /// `open` first, so steps that start again at once cannot keep it from closing (they did, with a flag inside the
    /// lock: the import waited for the whole index).
    steps: Arc<RwLock<()>>,
}

/// Permission for one background step.
pub(crate) enum Pass<'a> {
    /// The write lock: nothing else writes.
    Exclusive(#[allow(dead_code)] MutexGuard<'a, ()>),
    /// An import is appending; it waits for this step before it compacts.
    Appending(#[allow(dead_code)] RwLockReadGuard<'a, ()>),
}

impl Pass<'_> {
    /// Whether an import is appending beside this step.
    pub fn appending(&self) -> bool {
        matches!(self, Pass::Appending(_))
    }
}

/// Open while it lives; closing waits for the background steps inside the window.
pub(crate) struct Window<'a>(&'a Gate);

impl Drop for Window<'_> {
    fn drop(&mut self) {
        self.0.open.store(false, Ordering::SeqCst);
        drop(self.0.steps.write().unwrap_or_else(|p| p.into_inner()));
    }
}

impl Gate {
    pub fn new(write: Arc<Mutex<()>>) -> Gate {
        Gate {
            write,
            open: Arc::new(AtomicBool::new(false)),
            steps: Arc::new(RwLock::new(())),
        }
    }

    /// A pass for a background step, without waiting: the write lock if it is free, else the append window if an
    /// import has it open.
    pub fn try_enter(&self) -> Option<Pass<'_>> {
        if let Ok(g) = self.write.try_lock() {
            return Some(Pass::Exclusive(g));
        }
        if !self.open.load(Ordering::SeqCst) {
            return None;
        }
        let g = self.steps.try_read().ok()?;
        // Closing may have begun between the two looks: then this step stays out.
        self.open.load(Ordering::SeqCst).then_some(Pass::Appending(g))
    }

    /// Opens the append window; the caller holds the write lock and only appends until the window is dropped.
    pub fn open_window(&self) -> Window<'_> {
        self.open.store(true, Ordering::SeqCst);
        Window(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_pass_on_the_free_lock_or_in_an_open_window_only() {
        let gate = Gate::new(Arc::new(Mutex::new(())));
        assert!(matches!(gate.try_enter(), Some(Pass::Exclusive(_))));
        let held = gate.write.lock().unwrap();
        assert!(gate.try_enter().is_none(), "the lock is held and no window is open");
        let window = gate.open_window();
        let pass = gate.try_enter().expect("window open");
        assert!(pass.appending());
        drop(pass);
        drop(window);
        assert!(gate.try_enter().is_none(), "the window is closed again");
        drop(held);
    }

    #[test]
    fn closing_refuses_new_steps_while_it_waits_for_the_one_inside() {
        let gate = Gate::new(Arc::new(Mutex::new(())));
        let _held = gate.write.lock().unwrap();
        let window = gate.open_window();
        std::thread::scope(|s| {
            let inside = gate.try_enter().expect("window open");
            let closer = s.spawn(move || drop(window));
            // A step that starts again at once must not keep the window open: once closing has begun, it is refused.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while gate.try_enter().is_some() {
                assert!(std::time::Instant::now() < deadline, "new steps still enter a closing window");
                std::thread::yield_now();
            }
            drop(inside);
            closer.join().unwrap();
        });
        assert!(gate.try_enter().is_none());
    }

    #[test]
    fn closing_the_window_waits_for_the_step_inside() {
        let gate = Gate::new(Arc::new(Mutex::new(())));
        let _held = gate.write.lock().unwrap();
        let window = gate.open_window();
        let closed = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|s| {
            let pass = gate.try_enter().expect("window open");
            let (tx, rx) = std::sync::mpsc::channel();
            let closed = &closed;
            let closer = s.spawn(move || {
                tx.send(()).unwrap();
                drop(window);
                closed.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            rx.recv().unwrap();
            // The closer is blocked on the step's read guard until it is dropped.
            assert!(!closed.load(std::sync::atomic::Ordering::SeqCst));
            drop(pass);
            closer.join().unwrap();
        });
        assert!(closed.load(std::sync::atomic::Ordering::SeqCst));
    }
}
