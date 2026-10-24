use crate::{Debouncer, Due};
use paddock_jobs::CancelToken;
use std::fmt;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Real time between looks at the cancel token and the debouncer when no event arrives. The debounce itself is decided
/// on the debouncer's clock; this only bounds how late a due action can run.
const WAKE: Duration = Duration::from_millis(50);

/// Why a watch could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchError(pub String);

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WatchError {}

/// Something that tells us a folder changed.
pub trait ChangeSource: Send + Sync {
    /// Starts watching `dir` (and below it when `recursive`): every change (a file added, changed, removed or renamed)
    /// sends one `()` on `tx`. The returned guard stops the watch when dropped.
    fn watch(
        &self,
        dir: &Path,
        recursive: bool,
        tx: Sender<()>,
    ) -> Result<Box<dyn Send>, WatchError>;
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

    /// Whether the watcher thread has ended (stopped, or every sender is gone).
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
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

/// Watches `dir` (created if missing) on a background thread: events go to `debouncer`, and `on_due` runs on that thread
/// each time the debouncer says [`Due::Now`]. A panic in `on_due` is caught so the watch goes on.
pub fn spawn(
    debouncer: Arc<Debouncer>,
    source: &dyn ChangeSource,
    dir: &Path,
    recursive: bool,
    mut on_due: impl FnMut() + Send + 'static,
) -> Result<WatchHandle, WatchError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        WatchError(format!(
            "could not create the folder to watch: {}",
            e.kind()
        ))
    })?;
    let (tx, rx) = mpsc::channel();
    let guard = source.watch(dir, recursive, tx)?;
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let thread = std::thread::Builder::new()
        .name("folder-watch".into())
        .spawn(move || run(&debouncer, &rx, &token, guard, &mut on_due))
        .map_err(|e| WatchError(format!("could not start the folder watcher: {}", e.kind())))?;
    Ok(WatchHandle {
        cancel,
        thread: Some(thread),
    })
}

fn run(
    debouncer: &Debouncer,
    rx: &Receiver<()>,
    cancel: &CancelToken,
    _guard: Box<dyn Send>,
    on_due: &mut dyn FnMut(),
) {
    while !cancel.is_cancelled() {
        match rx.recv_timeout(WAKE) {
            Ok(()) => {
                debouncer.note_change();
                while rx.try_recv().is_ok() {
                    debouncer.note_change();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if debouncer.poll() == Due::Now {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut *on_due));
        }
    }
}

/// A [`ChangeSource`] for tests: the test sends the changes.
#[derive(Default, Clone)]
pub struct FakeSource {
    tx: Arc<Mutex<Option<Sender<()>>>>,
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
    fn watch(
        &self,
        _dir: &Path,
        _recursive: bool,
        tx: Sender<()>,
    ) -> Result<Box<dyn Send>, WatchError> {
        *self.tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        Ok(Box::new(()))
    }
}
