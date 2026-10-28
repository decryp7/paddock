//! The walk of the disk scan: worker threads read folders through the [`DirSource`] port and commit each folder's totals
//! into the shared [`DiskIndex`] in one step, so the index adds up at every moment. Bounded parallelism, background
//! priority, no sleeps: workers wait on a condition for work, Cancel sets a flag and returns at once, and a worker that is
//! stuck inside a call to a dead network share is abandoned, never awaited.

use dyno_core::disk::index::{DiskIndex, NodeId, ScanLimits, Sizes};
use dyno_core::disk::scan::{ScanFailure, ScanProgress, UNREADABLE_LISTED};
use dyno_core::disk::source::{DirEntryInfo, DirSource, EntryKind, ReadError};
use dyno_core::time::Timestamp;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// How a walk ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// Every folder was read.
    Done,
    /// Cancel was called.
    Cancelled,
    /// The walk stopped on an error.
    Failed(ScanFailure),
}

/// What the walk tells whoever started it. Never called with a lock of the walk held.
pub trait WalkHooks: Send + Sync {
    /// Some folders were committed.
    fn progress(&self, walk: &Walk);
    /// The walk ended; called once per pass.
    fn ended(&self, walk: &Walk, end: &End);
    /// A retry began another pass over the folders the first one could not open; called before its threads start.
    fn restarted(&self, walk: &Walk);
}

/// Why a retry did not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryError {
    /// The walk is still running, or it ended by Cancel or an error (only a pass that finished can be retried).
    NotFinished,
    /// Every folder was read.
    NothingToRetry,
    /// No thread could be started; nothing was changed.
    NoThread,
}

/// Counts the walk's threads still alive, so a host with threads stuck in dead calls can be refused more work.
#[derive(Debug, Default)]
pub struct LiveWorkers {
    count: Mutex<usize>,
    zero: Condvar,
}

impl LiveWorkers {
    /// How many are alive.
    pub fn count(&self) -> usize {
        *lock(&self.count)
    }

    /// Blocks until none is alive.
    pub fn wait_zero(&self) {
        let mut c = lock(&self.count);
        while *c > 0 {
            c = self.zero.wait(c).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn enter(self: &Arc<Self>) -> LiveGuard {
        *lock(&self.count) += 1;
        LiveGuard(self.clone())
    }
}

struct LiveGuard(Arc<LiveWorkers>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut c = lock(&self.0.count);
        *c -= 1;
        if *c == 0 {
            self.0.zero.notify_all();
        }
    }
}

/// Starts a named thread running the job; fails when the OS will not give another thread.
pub type Spawn = fn(String, Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()>;

/// The real thread starter.
pub fn os_spawn(name: String, job: Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()> {
    std::thread::Builder::new().name(name).spawn(job).map(|_| ())
}

/// Settings of one walk.
#[derive(Clone)]
pub struct WalkConfig {
    /// Worker threads (at least 1).
    pub threads: usize,
    /// Index bounds.
    pub limits: ScanLimits,
    /// Enter other volumes mounted inside the root (default: show them as mount points and stay on the volume).
    pub cross_volumes: bool,
    /// The bytes expected, when the walk covers a whole volume.
    pub total_bytes: Option<u64>,
    /// Threads of this host that are still alive (shared by every scan of the host).
    pub live: Arc<LiveWorkers>,
    /// How threads are started ([`os_spawn`]; tests give one that fails).
    pub spawn: Spawn,
}

struct Task {
    node: NodeId,
    path: PathBuf,
}

#[derive(Default)]
struct Queue {
    stack: Vec<Task>,
    in_flight: usize,
    finished: bool,
}

#[derive(Default)]
struct Problems {
    unreadable_first: Vec<String>,
    failure: Option<ScanFailure>,
}

/// A running (or finished) walk.
pub struct Walk {
    source: Arc<dyn DirSource>,
    index: Mutex<DiskIndex>,
    queue: Mutex<Queue>,
    wake: Condvar,
    stopped: AtomicBool,
    finishing: AtomicBool,
    /// Worker threads that have not exited yet.
    alive: AtomicUsize,
    /// Every folder was read (set by the worker that saw the queue run dry).
    completed: AtomicBool,
    root_dev: u64,
    /// The folder the walk started from.
    root: PathBuf,
    cfg: WalkConfig,
    /// (device, file id) of every folder entered: a bind mount or firmlink loop is entered once.
    entered: Mutex<HashSet<(u64, u64)>>,
    /// (device, file id) of multi-link files already counted.
    linked: Mutex<HashSet<(u64, u64)>>,
    folders: AtomicU64,
    unreadable: AtomicU64,
    problems: Mutex<Problems>,
    end: (Mutex<Option<End>>, Condvar),
    hooks: Arc<dyn WalkHooks>,
}

/// A consistent copy of the live numbers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// Counts.
    pub progress: ScanProgress,
    /// Whether the node limit left folders out.
    pub truncated: bool,
    /// The first unreadable folders.
    pub unreadable_first: Vec<String>,
}

impl Walk {
    /// Starts walking `root`. Returns at once; the walk runs on its own threads at background priority.
    pub fn start(
        source: Arc<dyn DirSource>,
        root: PathBuf,
        root_name: &str,
        cfg: WalkConfig,
        hooks: Arc<dyn WalkHooks>,
    ) -> Result<Arc<Walk>, ReadError> {
        let me = source.stat(&root)?;
        if me.kind != EntryKind::Dir {
            return Err(ReadError::Other("that is not a folder".into()));
        }
        let index = DiskIndex::new(root_name, cfg.limits);
        let root_node = index.root();
        let mut entered = HashSet::new();
        if me.ino != 0 {
            entered.insert((me.dev, me.ino));
        }
        let walk = Arc::new(Walk {
            root: root.clone(),
            source,
            index: Mutex::new(index),
            queue: Mutex::new(Queue {
                stack: vec![Task {
                    node: root_node,
                    path: root,
                }],
                ..Queue::default()
            }),
            wake: Condvar::new(),
            stopped: AtomicBool::new(false),
            finishing: AtomicBool::new(false),
            alive: AtomicUsize::new(cfg.threads.max(1)),
            completed: AtomicBool::new(false),
            root_dev: me.dev,
            entered: Mutex::new(entered),
            linked: Mutex::new(HashSet::new()),
            folders: AtomicU64::new(0),
            unreadable: AtomicU64::new(0),
            problems: Mutex::new(Problems::default()),
            end: (Mutex::new(None), Condvar::new()),
            hooks,
            cfg: WalkConfig {
                threads: cfg.threads.max(1),
                ..cfg
            },
        });
        walk.launch(walk.cfg.threads)?;
        Ok(walk)
    }

    /// Starts `threads` workers (the caller has set `alive` to the same number). Fails only when not even one starts.
    fn launch(self: &Arc<Self>, threads: usize) -> Result<(), ReadError> {
        for n in 0..threads {
            let w = self.clone();
            let guard = self.cfg.live.enter();
            let spawned = (self.cfg.spawn)(
                format!("dyno-disk-scan-{n}"),
                Box::new(move || {
                    dyno_platform::priority::background_current_thread();
                    // A bug in one reader ends the scan with a message; it never takes the agent down.
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.worker())).is_err() {
                        w.fail(ScanFailure::Internal {
                            detail: "a reader thread stopped unexpectedly".into(),
                        });
                    }
                    // Done is announced by the last worker out, after its thread has left the host's count: a finished scan
                    // never looks like one with threads still stuck in calls.
                    drop(guard);
                    w.worker_exited();
                }),
            );
            if spawned.is_err() {
                if n == 0 {
                    return Err(ReadError::Other("Dyno could not start a thread to read folders.".into()));
                }
                self.alive.fetch_sub(1, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    /// Reads once more the folders the finished pass could not open, and only those, into the same index: what they hold is
    /// added, and the tree completes if nothing else is missing. Returns at once; the pass runs on its own threads and ends
    /// like any other (hooks `restarted`, then `progress`, then `ended`).
    pub fn retry(self: &Arc<Self>) -> Result<(), RetryError> {
        let mut end = lock(&self.end.0);
        // The final event goes out just before the outcome is set, so a caller that reacts to the event at once can get
        // here in between: wait for the outcome of a pass that is ending.
        while end.is_none() && self.finishing.load(Ordering::SeqCst) {
            end = self.end.1.wait(end).unwrap_or_else(|p| p.into_inner());
        }
        if *end != Some(End::Done) {
            return Err(RetryError::NotFinished);
        }
        let ids = lock(&self.index).reopen_unreadable();
        if ids.is_empty() {
            return Err(RetryError::NothingToRetry);
        }
        let tasks: Vec<Task> = {
            let i = lock(&self.index);
            ids.iter()
                .filter_map(|id| i.path_names(*id).ok().map(|names| (*id, names)))
                .map(|(node, names)| Task {
                    node,
                    path: names.iter().skip(1).fold(self.root.clone(), |p, n| p.join(n)),
                })
                .collect()
        };
        let threads = self.cfg.threads.min(tasks.len()).max(1);
        let tasks_len = tasks.len();
        let paths: Vec<PathBuf> = tasks.iter().map(|t| t.path.clone()).collect();
        let again: HashSet<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        *lock(&self.queue) = Queue {
            stack: tasks,
            ..Queue::default()
        };
        self.stopped.store(false, Ordering::SeqCst);
        self.completed.store(false, Ordering::SeqCst);
        self.alive.store(threads, Ordering::SeqCst);
        // The folders about to be read again are no longer counted as refused; any that is refused again is counted anew.
        let taken = u64::try_from(tasks_len).unwrap_or(u64::MAX);
        // No worker is alive between passes, so nothing else changes the count.
        self.unreadable
            .store(self.unreadable.load(Ordering::Relaxed).saturating_sub(taken), Ordering::Relaxed);
        lock(&self.problems).unreadable_first.retain(|p| !again.contains(p));
        self.finishing.store(false, Ordering::SeqCst);
        *end = None;
        drop(end);
        self.hooks.restarted(self);
        if self.launch(threads).is_err() {
            // Nothing could start: leave every folder as it was and say the pass is over.
            let mut i = lock(&self.index);
            for id in ids {
                i.mark_unreadable(id);
                i.finish_listing(id);
            }
            drop(i);
            lock(&self.queue).stack.clear();
            for p in &paths {
                self.note_unreadable(p);
            }
            self.finishing.store(true, Ordering::SeqCst);
            *lock(&self.end.0) = Some(End::Done);
            self.end.1.notify_all();
            return Err(RetryError::NoThread);
        }
        Ok(())
    }

    /// The numbers as of now, consistent with the index.
    pub fn snapshot(&self) -> Snapshot {
        let (bytes, items, truncated) = {
            let i = lock(&self.index);
            let (b, it) = i.totals();
            (b, it, i.truncated())
        };
        Snapshot {
            progress: ScanProgress {
                bytes,
                items,
                folders: self.folders.load(Ordering::Relaxed),
                total_bytes: self.cfg.total_bytes.map(|t| t.max(bytes)),
                unreadable: self.unreadable.load(Ordering::Relaxed),
            },
            truncated,
            unreadable_first: lock(&self.problems).unreadable_first.clone(),
        }
    }

    /// Runs `f` on the index (queries).
    pub fn with_index<R>(&self, f: impl FnOnce(&DiskIndex) -> R) -> R {
        f(&lock(&self.index))
    }

    /// How the walk ended, or `None` while it runs.
    pub fn outcome(&self) -> Option<End> {
        lock(&self.end.0).clone()
    }

    /// Blocks until the walk has ended and says how.
    pub fn wait(&self) -> End {
        let mut g = lock(&self.end.0);
        loop {
            if let Some(e) = g.clone() {
                return e;
            }
            g = self.end.1.wait(g).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Stops the walk and returns at once; whatever was committed stays, consistent. A worker inside a call that does not
    /// return is left behind and its answer discarded.
    pub fn cancel(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.finish(End::Cancelled);
        self.wake_workers();
    }

    /// Wakes every worker waiting for work. The queue lock is taken first: a worker that has just checked `stopped` and
    /// is about to wait holds it, so the wake-up can never slip in between and be lost.
    fn wake_workers(&self) {
        drop(lock(&self.queue));
        self.wake.notify_all();
    }

    fn finish(&self, end: End) {
        if self.finishing.swap(true, Ordering::SeqCst) {
            return;
        }
        // The hook first, then the outcome: whoever sees the outcome can rely on the final event having been sent.
        self.hooks.ended(self, &end);
        *lock(&self.end.0) = Some(end);
        self.end.1.notify_all();
    }

    fn fail(&self, failure: ScanFailure) {
        {
            let mut p = lock(&self.problems);
            if p.failure.is_some() {
                return;
            }
            p.failure = Some(failure.clone());
        }
        self.stopped.store(true, Ordering::SeqCst);
        self.finish(End::Failed(failure));
        self.wake_workers();
    }

    fn worker_exited(&self) {
        if self.alive.fetch_sub(1, Ordering::SeqCst) == 1 && self.completed.load(Ordering::SeqCst) {
            self.finish(End::Done);
        }
    }

    fn worker(&self) {
        loop {
            let task = {
                let mut q = lock(&self.queue);
                loop {
                    if self.stopped.load(Ordering::SeqCst) || q.finished {
                        return;
                    }
                    if let Some(t) = q.stack.pop() {
                        q.in_flight += 1;
                        break t;
                    }
                    if q.in_flight == 0 {
                        q.finished = true;
                        self.completed.store(true, Ordering::SeqCst);
                        drop(q);
                        self.wake.notify_all();
                        return;
                    }
                    q = self.wake.wait(q).unwrap_or_else(|p| p.into_inner());
                }
            };
            let children = self.process(&task);
            {
                let mut q = lock(&self.queue);
                q.in_flight -= 1;
                q.stack.extend(children);
            }
            self.wake.notify_all();
            self.hooks.progress(self);
        }
    }

    fn note_unreadable(&self, path: &std::path::Path) {
        self.unreadable.fetch_add(1, Ordering::Relaxed);
        let mut p = lock(&self.problems);
        if p.unreadable_first.len() < UNREADABLE_LISTED {
            p.unreadable_first.push(path.to_string_lossy().into_owned());
        }
    }

    /// Whether a multi-link file was already counted under another name.
    fn already_counted(&self, e: &DirEntryInfo) -> bool {
        if e.links <= 1 || e.ino == 0 {
            return false;
        }
        let mut seen = lock(&self.linked);
        if seen.len() >= self.cfg.limits.max_hard_links as usize && !seen.contains(&(e.dev, e.ino)) {
            return false;
        }
        !seen.insert((e.dev, e.ino))
    }

    /// Reads one folder and commits it; returns the child folders to read next.
    fn process(&self, task: &Task) -> Vec<Task> {
        let notable = self.cfg.limits.notable_bytes;
        let mut dirs: Vec<DirEntryInfo> = Vec::new();
        let mut mounts: Vec<String> = Vec::new();
        let mut big: Vec<DirEntryInfo> = Vec::new();
        let (mut small, mut small_items, mut small_newest) = (Sizes::default(), 0u64, Timestamp(0));
        let mut unreachable: Option<String> = None;
        let listed = self.source.read_dir(&task.path, &mut |entry| {
            if self.stopped.load(Ordering::Relaxed) {
                return false;
            }
            match entry {
                Ok(e) => match e.kind {
                    EntryKind::Link | EntryKind::Other => {}
                    EntryKind::File => {
                        if self.already_counted(&e) {
                            return true;
                        }
                        if e.bytes >= notable {
                            big.push(e);
                        } else {
                            small.on_disk = small.on_disk.saturating_add(e.bytes);
                            small.logical = small.logical.saturating_add(e.logical);
                            small_items += 1;
                            small_newest = small_newest.max(e.mtime);
                        }
                    }
                    EntryKind::Dir => {
                        if e.dev != self.root_dev && !self.cfg.cross_volumes {
                            mounts.push(e.name);
                        } else if e.ino == 0 || lock(&self.entered).insert((e.dev, e.ino)) {
                            dirs.push(e);
                        }
                    }
                },
                Err(ReadError::PermissionDenied) => self.note_unreadable(&task.path),
                Err(ReadError::Unreachable(d)) => {
                    unreachable = Some(d);
                    return false;
                }
                Err(_) => {}
            }
            true
        });
        if let Some(detail) = unreachable {
            self.fail(ScanFailure::VolumeUnreachable { detail });
            return Vec::new();
        }
        let unreadable_here = match &listed {
            Ok(()) => false,
            Err(ReadError::Unreachable(d)) => {
                self.fail(ScanFailure::VolumeUnreachable { detail: d.clone() });
                return Vec::new();
            }
            Err(ReadError::NotFound) => false,
            Err(_) => {
                self.note_unreadable(&task.path);
                true
            }
        };

        let mut next = Vec::with_capacity(dirs.len());
        {
            let mut i = lock(&self.index);
            if self.stopped.load(Ordering::SeqCst) {
                return Vec::new();
            }
            for d in &dirs {
                if let Some(node) = i.add_folder(task.node, &d.name) {
                    next.push(Task {
                        node,
                        path: task.path.join(&d.name),
                    });
                }
            }
            for m in &mounts {
                i.add_mount_point(task.node, m);
            }
            for f in &big {
                i.add_file(
                    task.node,
                    &f.name,
                    Sizes {
                        on_disk: f.bytes,
                        logical: f.logical,
                    },
                    f.mtime,
                );
            }
            if small_items > 0 {
                i.add_folded_files(task.node, small, small_items, small_newest);
            }
            if unreadable_here {
                i.mark_unreadable(task.node);
            }
            i.finish_listing(task.node);
        }
        self.folders.fetch_add(1, Ordering::Relaxed);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::fs::OsDirSource;
    use crate::disk::testing::{FakeFs, Gate};
    use dyno_core::disk::index::DiskChildren;
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    const MB: u64 = 1024 * 1024;

    #[derive(Default)]
    struct Rec {
        progress: AtomicUsize,
        restarts: AtomicUsize,
        ended: Mutex<Vec<End>>,
    }

    impl WalkHooks for Rec {
        fn progress(&self, _: &Walk) {
            self.progress.fetch_add(1, Ordering::SeqCst);
        }
        fn ended(&self, _: &Walk, end: &End) {
            lock(&self.ended).push(end.clone());
        }
        fn restarted(&self, _: &Walk) {
            self.restarts.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn cfg(threads: usize) -> WalkConfig {
        WalkConfig {
            threads,
            limits: ScanLimits {
                max_nodes: 10_000,
                notable_bytes: MB,
                max_hard_links: 100,
            },
            cross_volumes: false,
            total_bytes: None,
            live: Arc::new(LiveWorkers::default()),
            spawn: os_spawn,
        }
    }

    fn run(fs: Arc<dyn DirSource>, root: PathBuf, cfg: WalkConfig) -> (Arc<Walk>, Arc<Rec>, End) {
        let rec = Arc::new(Rec::default());
        let w = Walk::start(fs, root, "vol", cfg, rec.clone()).unwrap();
        let end = w.wait();
        (w, rec, end)
    }

    fn level(w: &Walk, node: u32) -> DiskChildren {
        w.with_index(|i| i.children(NodeId(node), 0, 256, 0.0).unwrap())
    }

    /// root: a (big 5 MB, small 100), b (c (4 MB) , 2 small), loose 10.
    fn sample() -> FakeFs {
        let fs = FakeFs::new();
        let r = fs.root();
        let a = fs.dir(&r, "a");
        fs.file(&a, "big", 5 * MB);
        fs.file(&a, "small", 100);
        let b = fs.dir(&r, "b");
        let c = fs.dir(&b, "c");
        fs.file(&c, "four", 4 * MB);
        fs.file(&b, "s1", 7);
        fs.file(&b, "s2", 8);
        fs.file(&r, "loose", 10);
        fs
    }

    #[test]
    fn the_totals_add_up_and_the_walk_is_complete() {
        let fs = Arc::new(sample());
        let (w, rec, end) = run(fs.clone(), fs.root(), cfg(1));
        assert_eq!(end, End::Done);
        let snap = w.snapshot();
        assert_eq!(snap.progress.bytes, 9 * MB + 125);
        assert_eq!(snap.progress.items, 9, "folders a, b, c and six files");
        assert_eq!(snap.progress.folders, 4, "root, a, b, c");
        assert_eq!(snap.progress.unreadable, 0);
        assert!(!snap.truncated);
        assert!(w.with_index(DiskIndex::is_complete));
        let top = level(&w, 0);
        assert_eq!(top.children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        let sum: u64 = top.children.iter().map(|c| c.bytes).sum::<u64>() + top.other.bytes + top.loose.bytes;
        assert_eq!(sum, top.node.bytes);
        assert_eq!(top.loose.bytes, 10);
        assert_eq!(rec.progress.load(Ordering::SeqCst), 4, "one progress per folder");
        assert_eq!(*lock(&rec.ended), vec![End::Done], "ended exactly once");
        assert_eq!(w.outcome(), Some(End::Done));
        assert_eq!(fs.reads.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn several_threads_give_the_same_answer() {
        let build = || {
            let fs = FakeFs::new();
            let r = fs.root();
            for i in 0..6 {
                let d = fs.dir(&r, &format!("d{i}"));
                for j in 0..5 {
                    let e = fs.dir(&d, &format!("e{j}"));
                    fs.file(&e, "x", (i * 10 + j) as u64 * MB / 4);
                    let f = fs.dir(&e, "deep");
                    fs.file(&f, "y", 77 + j as u64);
                }
            }
            Arc::new(fs)
        };
        let (one, _, _) = run(build(), PathBuf::from("fake-root"), cfg(1));
        let (four, _, end) = run(build(), PathBuf::from("fake-root"), cfg(4));
        assert_eq!(end, End::Done);
        assert_eq!(one.snapshot().progress, four.snapshot().progress);
        let names = |w: &Walk| level(w, 0).children.iter().map(|c| (c.name.clone(), c.bytes)).collect::<Vec<_>>();
        assert_eq!(names(&one), names(&four));
    }

    #[test]
    fn links_and_special_files_are_not_followed_or_counted() {
        let fs = FakeFs::new();
        let r = fs.root();
        fs.file(&r, "real", 500);
        fs.symlink(&r, "shortcut");
        fs.socket(&r, "sock");
        let fs = Arc::new(fs);
        let (w, _, _) = run(fs, r, cfg(2));
        assert_eq!(w.snapshot().progress.bytes, 500);
        assert_eq!(w.snapshot().progress.items, 1);
    }

    #[test]
    fn another_volume_inside_is_a_mount_point_unless_asked() {
        let build = || {
            let fs = FakeFs::new();
            let r = fs.root();
            let usb = fs.dir_on(&r, "usb", 2, 5000);
            fs.file(&usb, "movie", 50 * MB);
            fs.file(&r, "mine", 9);
            Arc::new(fs)
        };
        let (w, _, _) = run(build(), PathBuf::from("fake-root"), cfg(1));
        assert_eq!(w.snapshot().progress.bytes, 9, "the other volume's bytes are not counted");
        let top = level(&w, 0);
        assert_eq!(top.children[0].kind, dyno_core::disk::index::NodeKind::MountPoint);
        assert!(top.children[0].complete);

        let mut c = cfg(1);
        c.cross_volumes = true;
        let (w, _, _) = run(build(), PathBuf::from("fake-root"), c);
        assert_eq!(w.snapshot().progress.bytes, 50 * MB + 9);
    }

    #[test]
    fn a_folder_that_leads_back_is_entered_once() {
        let fs = FakeFs::new();
        let r = fs.root();
        let a = fs.dir(&r, "a");
        fs.file(&a, "x", 40);
        // `loop` has the root's own (device, file id), as a firmlink or bind mount does.
        fs.dir_on(&a, "loop", 1, 1);
        // `twin` is `a` again under another name.
        let twin_ino = fs.stat(&r.join("a")).unwrap().ino;
        fs.dir_on(&r, "twin", 1, twin_ino);
        let fs = Arc::new(fs);
        let (w, _, end) = run(fs.clone(), r, cfg(2));
        assert_eq!(end, End::Done);
        assert_eq!(w.snapshot().progress.bytes, 40, "counted once");
        assert_eq!(fs.reads.load(Ordering::SeqCst), 2, "the root and a");
    }

    #[test]
    fn a_hard_linked_file_is_counted_once() {
        let fs = FakeFs::new();
        let r = fs.root();
        let a = fs.dir(&r, "a");
        fs.linked_file(&r, "one", 5 * MB, 777);
        fs.linked_file(&a, "two", 5 * MB, 777);
        fs.linked_file(&a, "other", 100, 778);
        let (w, _, _) = run(Arc::new(fs), r, cfg(1));
        assert_eq!(w.snapshot().progress.bytes, 5 * MB + 100);
        assert_eq!(w.snapshot().progress.items, 3, "a, the first name, and the other file");
    }

    #[test]
    fn too_many_linked_files_fall_back_to_counting_each_name() {
        let fs = FakeFs::new();
        let r = fs.root();
        for ino in 0..4u64 {
            fs.linked_file(&r, &format!("f{ino}"), 10, 900 + ino);
            fs.linked_file(&r, &format!("g{ino}"), 10, 900 + ino);
        }
        let mut c = cfg(1);
        c.limits.max_hard_links = 2;
        let (w, _, _) = run(Arc::new(fs), r, c);
        // Two ids are remembered (counted once); the names of the other two ids are all counted, repeats included.
        assert_eq!(w.snapshot().progress.bytes, 2 * 10 + 4 * 10);
    }

    /// root: ok (30 B), Library (refused; 40 B + a 5 MB file once readable), Mail (refused; 7 B once readable).
    fn with_two_refused() -> (Arc<FakeFs>, PathBuf, PathBuf) {
        let fs = FakeFs::new();
        let r = fs.root();
        let ok = fs.dir(&r, "ok");
        fs.file(&ok, "x", 30);
        let lib = fs.dir(&r, "Library");
        fs.file(&lib, "small", 40);
        fs.file(&lib, "big", 5 * MB);
        let mail = fs.dir(&lib, "Mail");
        fs.file(&mail, "inbox", 7);
        fs.fail_open(&lib, ReadError::PermissionDenied);
        (Arc::new(fs), lib, mail)
    }

    #[test]
    fn a_retry_reads_only_the_refused_folders_and_completes_the_tree() {
        let (fs, lib, _) = with_two_refused();
        let (w, rec, end) = run(fs.clone(), fs.root(), cfg(2));
        assert_eq!(end, End::Done);
        assert_eq!(w.snapshot().progress.unreadable, 1);
        assert!(!w.with_index(DiskIndex::is_complete));
        let reads = fs.reads.load(Ordering::SeqCst);

        fs.allow_open(&lib);
        w.retry().unwrap();
        assert_eq!(w.wait(), End::Done);
        assert_eq!(
            fs.reads.load(Ordering::SeqCst),
            reads + 2,
            "Library and the Mail folder it holds, nothing else"
        );
        let s = w.snapshot();
        assert_eq!(s.progress.bytes, 30 + 40 + 5 * MB + 7);
        assert_eq!((s.progress.unreadable, s.unreadable_first.len()), (0, 0));
        assert!(w.with_index(DiskIndex::is_complete), "nothing else was missing");
        assert_eq!(rec.restarts.load(Ordering::SeqCst), 1);
        assert_eq!(*lock(&rec.ended), vec![End::Done, End::Done], "each pass ends once");
        assert_eq!(w.retry(), Err(RetryError::NothingToRetry));
    }

    #[test]
    fn a_folder_refused_again_stays_refused_and_can_be_tried_later() {
        let (fs, lib, _) = with_two_refused();
        let (w, _, _) = run(fs.clone(), fs.root(), cfg(1));
        w.retry().unwrap();
        assert_eq!(w.wait(), End::Done);
        let s = w.snapshot();
        assert_eq!((s.progress.unreadable, s.unreadable_first.len()), (1, 1), "counted once, not twice");
        assert_eq!(s.progress.bytes, 30);
        assert!(!w.with_index(DiskIndex::is_complete));
        fs.allow_open(&lib);
        w.retry().unwrap();
        assert_eq!(w.wait(), End::Done);
        assert!(w.with_index(DiskIndex::is_complete));
    }

    #[test]
    fn a_retry_keeps_the_other_refusals_that_are_not_folders() {
        let (fs, lib, _) = with_two_refused();
        let ok = fs.root().join("ok");
        fs.fail_entries(&ok, vec![ReadError::PermissionDenied]);
        let (w, _, _) = run(fs.clone(), fs.root(), cfg(1));
        assert_eq!(w.snapshot().progress.unreadable, 2, "Library, and an entry of ok");
        fs.allow_open(&lib);
        w.retry().unwrap();
        w.wait();
        let s = w.snapshot();
        assert_eq!(
            s.progress.unreadable, 1,
            "the entry of ok was not read again, so it is still counted"
        );
        assert!(s.unreadable_first.iter().any(|p| p.ends_with("ok")));
    }

    #[test]
    fn only_a_pass_that_finished_can_be_retried() {
        let (fs, _, _) = with_two_refused();
        let gate = Gate::new();
        let g = gate.clone();
        fs.on_read(move |_| g.pass());
        let rec = Arc::new(Rec::default());
        let w = Walk::start(fs.clone(), fs.root(), "vol", cfg(1), rec).unwrap();
        gate.wait_arrived(1);
        assert_eq!(w.retry(), Err(RetryError::NotFinished), "still running");
        w.cancel();
        gate.open();
        assert_eq!(w.wait(), End::Cancelled);
        assert_eq!(
            w.retry(),
            Err(RetryError::NotFinished),
            "cancelled: the partial index is not retried"
        );
    }

    /// Hooks whose `ended` holds the pass in its closing moment (the final event sent, the outcome not yet set).
    struct Closing {
        reached: mpsc::Sender<()>,
        hold: Arc<Gate>,
    }

    impl WalkHooks for Closing {
        fn progress(&self, _: &Walk) {}
        fn ended(&self, _: &Walk, _: &End) {
            self.reached.send(()).unwrap();
            self.hold.pass();
        }
        fn restarted(&self, _: &Walk) {}
    }

    #[test]
    fn a_retry_asked_for_while_a_pass_is_ending_waits_for_its_outcome() {
        let (fs, lib, _) = with_two_refused();
        let (reached, closing) = mpsc::channel();
        let hold = Gate::new();
        let w = Walk::start(
            fs.clone(),
            fs.root(),
            "vol",
            cfg(1),
            Arc::new(Closing {
                reached,
                hold: hold.clone(),
            }),
        )
        .unwrap();
        closing.recv().unwrap();
        assert_eq!(w.outcome(), None, "the final event went out, the outcome is not set yet");
        fs.allow_open(&lib);
        let (asking, asked) = mpsc::channel();
        let w2 = w.clone();
        let retry = std::thread::spawn(move || {
            asking.send(()).unwrap();
            w2.retry()
        });
        asked.recv().unwrap();
        hold.open();
        assert_eq!(retry.join().unwrap(), Ok(()), "not refused as unfinished");
        // The second pass ends through the same hook, which is open now.
        assert_eq!(w.wait(), End::Done);
        assert_eq!(w.snapshot().progress.bytes, 30 + 40 + 5 * MB + 7);
    }

    #[test]
    fn a_clean_walk_has_nothing_to_retry() {
        let fs = Arc::new(sample());
        let (w, _, _) = run(fs.clone(), fs.root(), cfg(1));
        assert_eq!(w.retry(), Err(RetryError::NothingToRetry));
        assert_eq!(w.outcome(), Some(End::Done), "the state is untouched");
    }

    fn no_threads(_: String, _: Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()> {
        Err(std::io::Error::other("no more threads"))
    }

    #[test]
    fn a_retry_that_cannot_start_a_thread_changes_nothing() {
        static OUT_OF_THREADS: AtomicBool = AtomicBool::new(false);
        fn flaky(name: String, job: Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()> {
            if OUT_OF_THREADS.load(Ordering::SeqCst) {
                no_threads(name, job)
            } else {
                os_spawn(name, job)
            }
        }
        let (fs, lib, _) = with_two_refused();
        let mut c = cfg(1);
        c.spawn = flaky;
        let (w, rec, _) = run(fs.clone(), fs.root(), c);
        fs.allow_open(&lib);
        OUT_OF_THREADS.store(true, Ordering::SeqCst);
        assert_eq!(w.retry(), Err(RetryError::NoThread));
        assert_eq!(w.outcome(), Some(End::Done));
        assert!(!w.with_index(DiskIndex::is_complete), "the refused folder is as it was");
        assert_eq!(w.snapshot().progress.bytes, 30);
        assert_eq!(w.snapshot().progress.unreadable, 1, "still counted as refused");
        OUT_OF_THREADS.store(false, Ordering::SeqCst);
        w.retry().unwrap();
        assert_eq!(w.wait(), End::Done);
        assert!(
            w.with_index(DiskIndex::is_complete),
            "the folder could be retried when threads came back"
        );
        assert!(rec.restarts.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn a_walk_that_cannot_start_a_thread_says_so() {
        let fs = Arc::new(sample());
        let mut c = cfg(2);
        c.spawn = no_threads;
        let err = Walk::start(fs.clone(), fs.root(), "vol", c, Arc::new(Rec::default()))
            .err()
            .unwrap();
        assert!(matches!(err, ReadError::Other(m) if m.contains("thread")));
    }

    #[test]
    fn a_failure_to_start_one_of_several_threads_leaves_the_others_to_finish_the_walk() {
        use std::sync::atomic::AtomicBool;
        static FIRST: AtomicBool = AtomicBool::new(true);
        fn once(name: String, job: Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()> {
            if FIRST.swap(false, Ordering::SeqCst) {
                os_spawn(name, job)
            } else {
                Err(std::io::Error::other("no more threads"))
            }
        }
        let fs = Arc::new(sample());
        let mut c = cfg(3);
        c.spawn = once;
        let (w, _, end) = run(fs.clone(), fs.root(), c);
        assert_eq!(end, End::Done, "the one thread that started finishes the walk");
        assert_eq!(w.snapshot().progress.bytes, 9 * MB + 125);
    }

    #[test]
    fn both_sizes_are_counted_per_file() {
        let fs = FakeFs::new();
        let r = fs.root();
        fs.file_sized(&r, "sparse.img", 2 * MB, 40 * MB);
        fs.file_sized(&r, "tiny", 4096, 12);
        let (w, _, _) = run(Arc::new(fs), r, cfg(1));
        let top = level(&w, 0);
        assert_eq!((top.node.bytes, top.node.logical_bytes), (2 * MB + 4096, 40 * MB + 12));
        assert_eq!((top.loose.bytes, top.loose.logical_bytes), (4096, 12));
    }

    #[test]
    fn permission_denied_is_counted_and_not_fatal() {
        let fs = FakeFs::new();
        let r = fs.root();
        let ok = fs.dir(&r, "ok");
        fs.file(&ok, "x", 30);
        let locked = fs.dir(&r, "Library");
        fs.file(&locked, "hidden", 99);
        fs.fail_open(&locked, ReadError::PermissionDenied);
        fs.fail_entries(&ok, vec![ReadError::PermissionDenied, ReadError::Other("odd".into())]);
        let (w, _, end) = run(Arc::new(fs), r, cfg(2));
        assert_eq!(end, End::Done);
        let s = w.snapshot();
        assert_eq!(s.progress.bytes, 30);
        assert_eq!(s.progress.unreadable, 2, "the locked folder and the unreadable entry");
        assert_eq!(s.unreadable_first.len(), 2);
        assert!(s.unreadable_first.iter().any(|p| p.ends_with("Library")));
        assert!(!w.with_index(DiskIndex::is_complete), "the volume is incomplete: said, not hidden");
        let top = level(&w, 0);
        let lib = top.children.iter().find(|c| c.name == "Library").unwrap();
        assert_eq!(lib.kind, dyno_core::disk::index::NodeKind::Unreadable);
        assert!(!lib.complete);
    }

    #[test]
    fn only_the_first_unreadable_folders_are_named() {
        let fs = FakeFs::new();
        let r = fs.root();
        for n in 0..15 {
            let d = fs.dir(&r, &format!("locked{n}"));
            fs.fail_open(&d, ReadError::PermissionDenied);
        }
        let (w, _, _) = run(Arc::new(fs), r, cfg(1));
        let s = w.snapshot();
        assert_eq!(s.progress.unreadable, 15);
        assert_eq!(s.unreadable_first.len(), UNREADABLE_LISTED);
    }

    #[test]
    fn a_folder_that_vanished_is_skipped_quietly() {
        let fs = FakeFs::new();
        let r = fs.root();
        let gone = fs.dir(&r, "gone");
        fs.fail_open(&gone, ReadError::NotFound);
        let (w, _, end) = run(Arc::new(fs), r, cfg(1));
        assert_eq!((end, w.snapshot().progress.unreadable), (End::Done, 0));
        assert!(w.with_index(DiskIndex::is_complete));
    }

    #[test]
    fn a_dropped_share_fails_the_scan_and_keeps_what_was_counted() {
        let fs = FakeFs::new();
        let r = fs.root();
        let a = fs.dir(&r, "a");
        fs.file(&a, "x", 12);
        fs.file(&r, "top", 34);
        let b = fs.dir(&r, "b");
        fs.fail_open(&b, ReadError::Unreachable("ENOTCONN".into()));
        let (w, rec, end) = run(Arc::new(fs), r, cfg(1));
        assert_eq!(end, End::Failed(ScanFailure::VolumeUnreachable { detail: "ENOTCONN".into() }));
        assert_eq!(*lock(&rec.ended), vec![end]);
        let top = level(&w, 0);
        assert_eq!(top.node.bytes, 34, "one thread takes b first (it fails), so a was never read");
        let sum: u64 = top.children.iter().map(|c| c.bytes).sum::<u64>() + top.other.bytes + top.loose.bytes;
        assert_eq!(sum, top.node.bytes, "partial results still add up");
        assert!(!w.with_index(DiskIndex::is_complete));
    }

    #[test]
    fn two_readers_failing_at_once_report_one_failure() {
        let fs = FakeFs::new();
        let r = fs.root();
        for n in ["a", "b"] {
            let d = fs.dir(&r, n);
            fs.fail_open(&d, ReadError::Unreachable(format!("lost {n}")));
        }
        let gate = crate::disk::testing::Gate::new();
        let g = gate.clone();
        fs.on_read(move |p| {
            if !p.ends_with("fake-root") {
                g.pass();
            }
        });
        let rec = Arc::new(Rec::default());
        let w = Walk::start(Arc::new(fs), r, "vol", cfg(2), rec.clone()).unwrap();
        gate.wait_arrived(2);
        gate.open();
        assert!(matches!(w.wait(), End::Failed(ScanFailure::VolumeUnreachable { .. })));
        w.cfg.live.wait_zero();
        assert_eq!(lock(&rec.ended).len(), 1, "the second failure changes nothing");
    }

    #[test]
    fn a_share_that_drops_mid_listing_fails_too() {
        let fs = FakeFs::new();
        let r = fs.root();
        fs.file(&r, "x", 5);
        fs.fail_entries(&r, vec![ReadError::Unreachable("ETIMEDOUT".into())]);
        let (_, _, end) = run(Arc::new(fs), r, cfg(2));
        assert!(matches!(end, End::Failed(ScanFailure::VolumeUnreachable { .. })));
    }

    #[test]
    fn cancel_returns_at_once_even_when_a_read_never_returns() {
        let fs = FakeFs::new();
        let r = fs.root();
        let a = fs.dir(&r, "a");
        fs.file(&a, "x", 10);
        fs.file(&r, "top", 7);
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (entered_tx, release_rx) = (Mutex::new(entered_tx), Mutex::new(release_rx));
        fs.on_read(move |p| {
            if p.ends_with("a") {
                let _ = lock(&entered_tx).send(());
                let _ = lock(&release_rx).recv();
            }
        });
        let c = cfg(1);
        let live = c.live.clone();
        let rec = Arc::new(Rec::default());
        let w = Walk::start(Arc::new(fs), r, "vol", c, rec.clone()).unwrap();
        entered_rx.recv().unwrap();
        assert_eq!(live.count(), 1, "one worker, stuck in the read of a");
        w.cancel();
        assert_eq!(w.outcome(), Some(End::Cancelled), "cancelled while the read is still blocked");
        let frozen = w.snapshot();
        assert_eq!(frozen.progress.bytes, 7, "the root was committed before the block");
        w.cancel();
        release_tx.send(()).unwrap();
        live.wait_zero();
        assert_eq!(w.snapshot(), frozen, "the late answer was discarded");
        assert_eq!(*lock(&rec.ended), vec![End::Cancelled], "ended once however often cancel is called");
        assert_eq!(live.count(), 0);
    }

    #[test]
    fn cancel_in_the_middle_keeps_a_consistent_tree() {
        let fs = FakeFs::new();
        let r = fs.root();
        for i in 0..20 {
            let d = fs.dir(&r, &format!("d{i}"));
            fs.file(&d, "x", 100);
        }
        let fs = Arc::new(fs);
        let slot: Arc<Mutex<Option<Arc<Walk>>>> = Arc::new(Mutex::new(None));
        let (s2, count) = (slot.clone(), Arc::new(AtomicUsize::new(0)));
        fs.on_read(move |_| {
            // Cancel from inside the fifth read.
            if count.fetch_add(1, Ordering::SeqCst) == 5 {
                if let Some(w) = lock(&s2).clone() {
                    w.cancel();
                }
            }
        });
        let rec = Arc::new(Rec::default());
        let w = {
            let mut g = lock(&slot);
            let w = Walk::start(fs, r, "vol", cfg(1), rec).unwrap();
            *g = Some(w.clone());
            w
        };
        assert_eq!(w.wait(), End::Cancelled);
        let top = level(&w, 0);
        let sum: u64 = top.children.iter().map(|c| c.bytes).sum::<u64>() + top.other.bytes + top.loose.bytes;
        assert_eq!(sum, top.node.bytes);
        assert!(top.node.bytes < 2000 && !top.node.complete);
    }

    #[test]
    fn the_node_limit_is_reported_not_fatal() {
        let fs = FakeFs::new();
        let r = fs.root();
        for i in 0..10 {
            let d = fs.dir(&r, &format!("d{i}"));
            fs.file(&d, "x", 5);
        }
        let mut c = cfg(1);
        c.limits.max_nodes = 4;
        let (w, _, end) = run(Arc::new(fs), r, c);
        assert_eq!(end, End::Done);
        let s = w.snapshot();
        assert!(s.truncated);
        assert!(s.progress.bytes < 50, "folders past the limit are not walked");
        assert!(w.with_index(|i| i.skipped_folders()) > 0);
    }

    #[test]
    fn the_expected_total_never_falls_below_the_count() {
        let fs = sample();
        let r = fs.root();
        let mut c = cfg(1);
        c.total_bytes = Some(100);
        let (w, _, _) = run(Arc::new(fs), r, c);
        let p = w.snapshot().progress;
        assert_eq!(
            p.total_bytes,
            Some(p.bytes),
            "counted more than the OS said: the total follows, never below"
        );
        let fs = sample();
        let mut c = cfg(1);
        c.total_bytes = Some(100 * MB);
        let (w, _, _) = run(Arc::new(fs), PathBuf::from("fake-root"), c);
        assert_eq!(w.snapshot().progress.total_bytes, Some(100 * MB));
    }

    #[test]
    fn a_bad_root_is_refused() {
        let fs = FakeFs::new();
        fs.file(&fs.root(), "f", 1);
        let fs = Arc::new(fs);
        let hooks: Arc<dyn WalkHooks> = Arc::new(Rec::default());
        let not_dir = Walk::start(fs.clone(), fs.root().join("f"), "x", cfg(1), hooks.clone());
        assert!(matches!(not_dir, Err(ReadError::Other(m)) if m.contains("not a folder")));
        let missing = Walk::start(fs.clone(), Path::new("fake-root").join("nope"), "x", cfg(1), hooks);
        assert!(matches!(missing, Err(ReadError::NotFound)));
    }

    #[test]
    fn a_panicking_reader_fails_the_scan_instead_of_hanging() {
        let fs = FakeFs::new();
        let r = fs.root();
        fs.dir(&r, "boom");
        fs.on_read(|p| {
            if p.ends_with("boom") {
                panic!("a bug in the reader");
            }
        });
        let (_, _, end) = run(Arc::new(fs), r, cfg(2));
        assert!(matches!(end, End::Failed(ScanFailure::Internal { .. })));
    }

    #[test]
    fn zero_threads_means_one() {
        let fs = Arc::new(sample());
        let (_, _, end) = run(fs.clone(), fs.root(), cfg(0));
        assert_eq!(end, End::Done);
    }

    #[test]
    fn a_real_folder_is_walked_without_following_links_or_double_counting() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap();
        std::fs::create_dir_all(root.join("a/deep")).unwrap();
        std::fs::write(root.join("a/x"), vec![1u8; 20_000]).unwrap();
        std::fs::write(root.join("a/deep/y"), vec![1u8; 30_000]).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("huge"), vec![0u8; 500_000]).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.join("shortcut")).unwrap();
            std::fs::hard_link(root.join("a/x"), root.join("a/x-again")).unwrap();
        }
        let (w, _, end) = run(Arc::new(OsDirSource), root, cfg(2));
        assert_eq!(end, End::Done);
        let bytes = w.snapshot().progress.bytes;
        assert!((50_000..120_000).contains(&bytes), "20 KB + 30 KB allocated, once each: {bytes}");
        assert!(w.with_index(DiskIndex::is_complete));
    }
}
