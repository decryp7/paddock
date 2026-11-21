use crate::{Debouncer, Due};
use paddock_jobs::CancelToken;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// The first wait before a watch that ended is started again; each failed start doubles it, up to [`RESTART_MAX`].
pub const RESTART_FIRST: Duration = Duration::from_secs(1);
/// The longest wait between two starts of a watch that keeps failing.
pub const RESTART_MAX: Duration = Duration::from_secs(60);
/// How often a watch whose folder is gone looks for it to come back.
pub const ROOT_POLL: Duration = Duration::from_secs(5);

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

/// What a [`ChangeSource`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Something in the folder changed (a file added, changed, removed or renamed).
    Changed,
    /// Changes may have been lost while the watch goes on: the OS dropped events (a full inotify queue, an FSEvents
    /// dropped-events notice, a `ReadDirectoryChangesW` overflow). Everything under the folder must be looked at again (Apple's
    /// FSEvents guide and Microsoft's `ReadDirectoryChangesW` page both say so). The text says why, for the log.
    Rescan(String),
    /// The watch reported an error and may have stopped: everything is looked at again and the watch is started anew.
    Failed(String),
}

/// Something that tells us a folder changed.
pub trait ChangeSource: Send + Sync {
    /// Starts watching `dir` (and below it when `recursive`): every change sends one [`Change::Changed`] on `tx`, a sign that
    /// changes were lost a [`Change::Rescan`], an error a [`Change::Failed`]. The returned guard stops the watch when dropped;
    /// a watch that ends by itself drops `tx`, and the watcher thread starts it again.
    fn watch(
        &self,
        dir: &Path,
        recursive: bool,
        tx: Sender<Change>,
    ) -> Result<Box<dyn Send>, WatchError>;

    /// The same source, owned: the watcher thread keeps it to start the watch again after it ended.
    fn boxed(&self) -> Box<dyn ChangeSource>;
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

    /// Whether the watcher thread has ended (it ends only when stopped).
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
///
/// `on_due` must look at the whole folder (every caller re-reads it), because it also runs when changes may have been
/// lost: after a [`Change::Rescan`] (logged as a warning with its cause), once the debouncer's resync time has passed with no
/// event at all ([`Debouncer::resync_due`], logged), and after the watch was started again.
///
/// A watch that ends (a [`Change::Failed`], or the source drops its sender) is started again after [`RESTART_FIRST`], doubling
/// up to [`RESTART_MAX`] while starts fail; each attempt is logged. When the folder itself is gone, that is logged, `on_due`
/// runs once, and the thread looks for the folder every [`ROOT_POLL`] and starts the watch again when it is back (it never
/// makes the folder again: who removed it may have meant it). All waits are on the debouncer's clock.
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
    let mut watch = Watch {
        source: source.boxed(),
        dir: dir.to_path_buf(),
        recursive,
        state: State::Watching { rx, _guard: guard },
    };
    let thread = std::thread::Builder::new()
        .name("folder-watch".into())
        .spawn(move || run(&debouncer, &mut watch, &token, &mut on_due))
        .map_err(|e| WatchError(format!("could not start the folder watcher: {}", e.kind())))?;
    Ok(WatchHandle {
        cancel,
        thread: Some(thread),
    })
}

/// Where a watch stands.
enum State {
    /// The source reports on `rx`; dropping the guard stops it.
    Watching {
        rx: Receiver<Change>,
        _guard: Box<dyn Send>,
    },
    /// The watch ended: start it again at `at` (on the debouncer's clock); `attempt` counts the starts that failed so far.
    Restarting { at: Duration, attempt: u32 },
    /// The folder is gone: look for it again at `at`.
    Gone { at: Duration },
}

struct Watch {
    source: Box<dyn ChangeSource>,
    dir: PathBuf,
    recursive: bool,
    state: State,
}

/// The wait before start number `attempt + 1`: [`RESTART_FIRST`] doubled per failed start, at most [`RESTART_MAX`].
pub fn restart_delay(attempt: u32) -> Duration {
    RESTART_FIRST
        .saturating_mul(1u32 << attempt.min(16))
        .min(RESTART_MAX)
}

impl Watch {
    /// Leaves `Watching` for a restart (the guard is dropped, which stops what is left of the old watch).
    fn ended(&mut self, debouncer: &Debouncer, why: &str) {
        log::warn!(
            "folder watch: the watch of {} ended ({why}); starting it again in {:?}",
            self.dir.display(),
            restart_delay(0)
        );
        debouncer.note_change();
        self.state = State::Restarting {
            at: debouncer.now() + restart_delay(0),
            attempt: 0,
        };
    }

    fn gone(&mut self, debouncer: &Debouncer) {
        log::warn!(
            "folder watch: the watched folder {} is gone; looking for it every {:?}",
            self.dir.display(),
            ROOT_POLL
        );
        debouncer.note_change();
        self.state = State::Gone {
            at: debouncer.now() + ROOT_POLL,
        };
    }

    /// Starts the watch again (state `Restarting`, due).
    fn restart(&mut self, debouncer: &Debouncer, attempt: u32) {
        if !self.dir.is_dir() {
            self.gone(debouncer);
            return;
        }
        let (tx, rx) = mpsc::channel();
        match self.source.watch(&self.dir, self.recursive, tx) {
            Ok(guard) => {
                log::info!(
                    "folder watch: the watch of {} started again (after {} failed start(s)); looking at the whole folder again",
                    self.dir.display(),
                    attempt
                );
                debouncer.note_change();
                self.state = State::Watching { rx, _guard: guard };
            }
            Err(e) => {
                let wait = restart_delay(attempt + 1);
                log::warn!(
                    "folder watch: could not start the watch of {} again ({e}); trying again in {wait:?}",
                    self.dir.display()
                );
                self.state = State::Restarting {
                    at: debouncer.now() + wait,
                    attempt: attempt + 1,
                };
            }
        }
    }

    /// One turn of the thread: take what the source sent, or move a restart or a gone folder on.
    fn turn(&mut self, debouncer: &Debouncer) {
        let now = debouncer.now();
        match &self.state {
            State::Watching { rx, .. } => {
                let mut failed = None;
                let first = match rx.recv_timeout(WAKE) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => {
                        self.ended(debouncer, "the source stopped reporting");
                        return;
                    }
                };
                let Some(first) = first else { return };
                let mut changes = vec![first];
                changes.extend(rx.try_iter());
                for change in changes {
                    match change {
                        Change::Changed => debouncer.note_change(),
                        Change::Rescan(cause) => {
                            log::warn!(
                                "folder watch: changes in {} may have been missed ({cause}); looking at the whole folder again",
                                self.dir.display()
                            );
                            debouncer.note_change();
                        }
                        Change::Failed(cause) => failed = Some(cause),
                    }
                }
                if !self.dir.is_dir() {
                    self.gone(debouncer);
                } else if let Some(cause) = failed {
                    self.ended(debouncer, &cause);
                }
            }
            State::Restarting { at, attempt } => {
                let (at, attempt) = (*at, *attempt);
                if now >= at {
                    self.restart(debouncer, attempt);
                } else {
                    std::thread::sleep(WAKE);
                }
            }
            State::Gone { at } => {
                let at = *at;
                if now < at {
                    std::thread::sleep(WAKE);
                } else if self.dir.is_dir() {
                    log::info!(
                        "folder watch: the watched folder {} is back",
                        self.dir.display()
                    );
                    self.restart(debouncer, 0);
                } else {
                    self.state = State::Gone {
                        at: now + ROOT_POLL,
                    };
                }
            }
        }
    }
}

fn run(debouncer: &Debouncer, watch: &mut Watch, cancel: &CancelToken, on_due: &mut dyn FnMut()) {
    while !cancel.is_cancelled() {
        watch.turn(debouncer);
        let act = if debouncer.poll() == Due::Now {
            true
        } else if debouncer.resync_due() {
            log::info!(
                "folder watch: no change reported in {} for the resync time; looking at the whole folder again",
                watch.dir.display()
            );
            true
        } else {
            false
        };
        if act {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut *on_due));
        }
    }
}

/// A [`ChangeSource`] for tests: the test sends the changes, ends the watch, and makes starts fail.
#[derive(Default, Clone)]
pub struct FakeSource {
    tx: Arc<Mutex<Option<Sender<Change>>>>,
    watches: Arc<std::sync::atomic::AtomicUsize>,
    refuse: Arc<std::sync::atomic::AtomicBool>,
}

impl FakeSource {
    /// A source nobody has started watching yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports one change; `false` when no watch is running.
    pub fn change(&self) -> bool {
        self.send(Change::Changed)
    }

    /// Reports that changes were lost, with its cause (as an OS queue overflow would); `false` when no watch is running.
    pub fn rescan(&self, cause: &str) -> bool {
        self.send(Change::Rescan(cause.to_owned()))
    }

    /// Reports an error of the watch, with its cause; `false` when no watch is running.
    pub fn fail(&self, cause: &str) -> bool {
        self.send(Change::Failed(cause.to_owned()))
    }

    /// Ends the running watch as a dead watcher would: its sender is dropped.
    pub fn end(&self) {
        *self.tx.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// How many times a watch was started.
    pub fn watches(&self) -> usize {
        self.watches.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Makes the following starts fail (`true`) or succeed (`false`).
    pub fn refuse(&self, refuse: bool) {
        self.refuse
            .store(refuse, std::sync::atomic::Ordering::SeqCst);
    }

    fn send(&self, change: Change) -> bool {
        let g = self.tx.lock().unwrap_or_else(|p| p.into_inner());
        g.as_ref().is_some_and(|t| t.send(change).is_ok())
    }
}

impl ChangeSource for FakeSource {
    fn watch(
        &self,
        _dir: &Path,
        _recursive: bool,
        tx: Sender<Change>,
    ) -> Result<Box<dyn Send>, WatchError> {
        self.watches
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.refuse.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(WatchError("the fake refuses to start".into()));
        }
        *self.tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        Ok(Box::new(()))
    }

    fn boxed(&self) -> Box<dyn ChangeSource> {
        Box::new(self.clone())
    }
}
