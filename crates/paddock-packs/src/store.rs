use crate::{PackKind, PackLoader, Snapshot};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

type Observer<P> = Box<dyn Fn(&Arc<Snapshot<P>>) + Send + Sync>;

/// The current [`Snapshot`] of one kind of pack, replaced only by a finished reload (build new data, then swap).
pub struct PackStore<K: PackKind> {
    loader: Mutex<PackLoader<K>>,
    current: RwLock<Arc<Snapshot<K::Pack>>>,
    observers: Mutex<Vec<Observer<K::Pack>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl<K: PackKind> PackStore<K> {
    /// A store over `loader`; nothing is loaded until [`reload`](Self::reload).
    pub fn new(loader: PackLoader<K>) -> Self {
        Self {
            loader: Mutex::new(loader),
            current: RwLock::new(Arc::new(Snapshot::default())),
            observers: Mutex::new(Vec::new()),
        }
    }

    /// The snapshot in use now. Cheap; readers keep theirs while a reload swaps in the next.
    pub fn current(&self) -> Arc<Snapshot<K::Pack>> {
        self.current
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Calls `f` with each new snapshot, after it is in place and only when the set really changed (never for a reload
    /// that found the same files). Observers run on the thread that reloaded; keep them short.
    pub fn observe(&self, f: impl Fn(&Arc<Snapshot<K::Pack>>) + Send + Sync + 'static) {
        lock(&self.observers).push(Box::new(f));
    }

    /// Reads the packs again and swaps the result in when it differs; returns whether it did. Reloads are serialised, so
    /// two threads cannot publish out of order.
    pub fn reload(&self) -> bool {
        let mut loader = lock(&self.loader);
        let Some(snapshot) = loader.reload() else {
            return false;
        };
        let next = Arc::new(snapshot.clone());
        *self.current.write().unwrap_or_else(|p| p.into_inner()) = next.clone();
        drop(loader);
        // An observer that panics is the observer's bug; it must not stop the reload thread or the observers after it.
        for f in lock(&self.observers).iter() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&next)));
        }
        true
    }
}
