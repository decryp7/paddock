//! Watching the packs folder. The watcher is a port ([`ChangeSource`]) so the reload logic is tested with a fake and one
//! real-folder test; `NotifySource` (feature `watch`) is the adapter over the `notify` crate.
//!
//! NOTE for X23 (`paddock-watch`): this is the minimal folder watcher the packs need (a change signal, no event
//! details). When `paddock-watch` exists, its debounced folder watch replaces [`ChangeSource`]'s adapter and [`Reloader`]'s
//! debounce; the [`ChangeSource`] trait stays as the seam.

use crate::{PackError, PackKind, Polled, Reloader};
use paddock_jobs::CancelToken;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(feature = "watch")]
mod native;
#[cfg(feature = "watch")]
pub use native::NotifySource;

/// Real time between looks at the cancel token and the debounce when no event arrives. The debounce itself is decided on
/// the reloader's clock; this only bounds how late a due reload can run.
const WAKE: Duration = Duration::from_millis(50);

/// Something that tells us a folder changed.
pub trait ChangeSource: Send + Sync {
    /// Starts watching `dir`: every change in it (a file added, changed, removed or renamed) sends one `()` on `tx`.
    /// The returned guard stops the watch when dropped.
    fn watch(&self, dir: &Path, tx: Sender<()>) -> Result<Box<dyn Send>, PackError>;
}

/// A running watch; dropping it (or [`stop`](Self::stop)) stops the thread and the folder watch.
pub struct WatchHandle {
    cancel: CancelToken,
    thread: Option<JoinHandle<()>>,
}

impl WatchHandle {
    /// Stops the watch and waits for its thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.cancel.cancel();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Watches `dir` (created if missing) and runs `reloader` on a background thread: events are noted, the reloader
/// debounces on its clock, and the store swaps in the new set. The first load is the caller's (`reload_now`) so the
/// app has packs before the first event. A failure to start the watch is returned; the app keeps its packs and may
/// report it as a problem.
pub fn spawn<K: PackKind>(
    reloader: Arc<Reloader<K>>,
    source: &dyn ChangeSource,
    dir: &Path,
) -> Result<WatchHandle, PackError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| PackError::Io(format!("could not create the packs folder: {}", e.kind())))?;
    let (tx, rx) = mpsc::channel();
    let guard = source.watch(dir, tx)?;
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let thread = std::thread::Builder::new()
        .name("pack-watch".into())
        .spawn(move || run(&reloader, &rx, &token, guard))
        .map_err(|e| PackError::Io(format!("could not start the packs watcher: {}", e.kind())))?;
    Ok(WatchHandle {
        cancel,
        thread: Some(thread),
    })
}

fn run<K: PackKind>(
    reloader: &Reloader<K>,
    rx: &Receiver<()>,
    cancel: &CancelToken,
    _guard: Box<dyn Send>,
) {
    while !cancel.is_cancelled() {
        match rx.recv_timeout(WAKE) {
            Ok(()) => {
                reloader.note_change();
                while rx.try_recv().is_ok() {
                    reloader.note_change();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Polled::Reloaded(_) = reloader.poll() {}
    }
}

/// A [`ChangeSource`] for tests: the test sends the changes.
#[derive(Default, Clone)]
pub struct FakeSource {
    tx: Arc<std::sync::Mutex<Option<Sender<()>>>>,
}

impl FakeSource {
    /// A source nobody has started watching yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports one change; `false` when no watch is running.
    pub fn change(&self) -> bool {
        let g = self.tx.lock().unwrap_or_else(|p| p.into_inner());
        g.as_ref().is_some_and(|t| t.send(()).is_ok())
    }
}

impl ChangeSource for FakeSource {
    fn watch(&self, _dir: &Path, tx: Sender<()>) -> Result<Box<dyn Send>, PackError> {
        *self.tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        Ok(Box::new(()))
    }
}
