//! The saved-views cache (ADR 0029): a second, rebuildable SQLite file, `cache.db`, in the OS cache folder, written by one
//! `dyno-cache` thread. A save never blocks the walk or the sampler; a load runs on the asking thread with a read-only
//! connection. The app never opens the file.
//!
//! Logs from here carry row ids, scan ids, the target kind (volume or folder, never the path), node counts, bytes, durations and
//! outcomes, never a path or a name (section 6).

pub mod backup;
pub mod db;
#[cfg(test)]
pub(crate) mod testing;

use crate::disk::centre::Clock;
use db::{CacheDb, CacheError, CacheLimits, FreeSpace, Opened, SaveParams, Saved};
use dyno_core::disk::index::DiskIndex;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Instant;

pub use db::{SavedRow, CLEARED_NOTE};

/// The most saves waiting at once; a further one drops the oldest waiting save.
pub const MAX_PENDING_SAVES: usize = 4;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The Settings the cache follows (`disk.savedMaps.enabled`, `disk.savedMaps.maxMb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheSettings {
    /// Whether maps are saved at all.
    pub enabled: bool,
    /// The cap on the packed size of all saved maps, in bytes.
    pub max_bytes: u64,
}

impl From<dyno_core::disk::settings::SavedMapsSettings> for CacheSettings {
    fn from(s: dyno_core::disk::settings::SavedMapsSettings) -> CacheSettings {
        CacheSettings {
            enabled: s.enabled,
            max_bytes: s.max_bytes(),
        }
    }
}

impl Default for CacheSettings {
    fn default() -> Self {
        CacheSettings {
            enabled: true,
            max_bytes: CacheLimits::default().max_bytes,
        }
    }
}

/// How a save ended, told to the one who asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Written as this row; `removed` rows were replaced or evicted for it.
    Saved {
        /// The new row.
        id: i64,
        /// Rows removed (the replaced one and any evicted).
        removed: Vec<i64>,
    },
    /// A newer save of the same target took its place before it started, or the queue was full.
    Superseded,
    /// Larger than the cap: not saved ("Too big to keep; raise the limit in Settings").
    TooBig,
    /// The volume would be left with too little free space.
    NoSpace,
    /// Saving is switched off.
    Disabled,
    /// It failed; the old map (if any) is untouched.
    Failed,
}

/// One save: the tree is a finished snapshot, shared read-only; the writer thread serialises it a chunk at a time.
pub struct SaveJob {
    /// What goes in the row besides the tree.
    pub params: SaveParams,
    /// The finished tree.
    pub index: Arc<DiskIndex>,
    /// The scan, for the log (never the path).
    pub scan: u64,
    /// `volume` or `folder`, for the log.
    pub kind: &'static str,
    /// Told when the save ended.
    pub done: Box<dyn FnOnce(SaveOutcome) + Send>,
}

enum Cmd {
    Save(SaveJob),
    Forget { id: i64, done: Option<Sender<bool>> },
    ForgetAll { done: Option<Sender<()>> },
    Touch(i64),
    View { key: String, format: u32, data: String },
    Barrier(Sender<()>),
    Stop,
}

/// The bounded queue between the savers and the writer thread.
#[derive(Default)]
pub(crate) struct Queue {
    state: Mutex<VecDeque<Cmd>>,
    wake: Condvar,
}

impl Queue {
    /// Adds a save; returns the saves it displaced (an older save of the same target, or the oldest when the queue is full).
    fn push_save(&self, job: SaveJob) -> Vec<SaveJob> {
        let mut q = lock(&self.state);
        let mut dropped = Vec::new();
        if let Some(at) = q
            .iter()
            .position(|c| matches!(c, Cmd::Save(j) if j.params.target_key == job.params.target_key))
        {
            if let Some(Cmd::Save(old)) = q.remove(at) {
                dropped.push(old);
            }
        }
        let saves = q.iter().filter(|c| matches!(c, Cmd::Save(_))).count();
        if saves >= MAX_PENDING_SAVES {
            if let Some(at) = q.iter().position(|c| matches!(c, Cmd::Save(_))) {
                if let Some(Cmd::Save(old)) = q.remove(at) {
                    dropped.push(old);
                }
            }
        }
        q.push_back(Cmd::Save(job));
        self.wake.notify_one();
        dropped
    }

    fn push(&self, c: Cmd) {
        lock(&self.state).push_back(c);
        self.wake.notify_one();
    }

    fn pop(&self) -> Cmd {
        let mut q = lock(&self.state);
        loop {
            if let Some(c) = q.pop_front() {
                return c;
            }
            q = self.wake.wait(q).unwrap_or_else(|p| p.into_inner());
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_saves(&self) -> Vec<String> {
        lock(&self.state)
            .iter()
            .filter_map(|c| match c {
                Cmd::Save(j) => Some(j.params.target_key.clone()),
                _ => None,
            })
            .collect()
    }
}

/// What the writer thread works with.
pub struct CacheDeps {
    /// Wall time (the fake clock in tests).
    pub clock: Arc<dyn Clock>,
    /// The free space of the cache volume.
    pub free: Arc<dyn FreeSpace>,
}

struct Inner {
    path: PathBuf,
    queue: Arc<Queue>,
    settings: Mutex<CacheSettings>,
    clock: Arc<dyn Clock>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// The handle to the cache: cheap to clone, shared by the disk centre and the lists.
#[derive(Clone)]
pub struct Cache {
    inner: Arc<Inner>,
}

/// What opening the cache came to.
pub struct Started {
    /// The cache, or `None` when it is off for this run (busy file, unwritable folder).
    pub cache: Option<Cache>,
    /// A line for the activity log, when there is something the user should know.
    pub note: Option<String>,
}

impl Cache {
    /// Opens the file at `path` and starts the writer thread. A busy file or an unwritable folder means no cache for this run,
    /// said once in `note`; the disk map then works as it did without one.
    pub fn start(path: PathBuf, settings: CacheSettings, deps: CacheDeps) -> Started {
        let opened = match CacheDb::open(&path) {
            Ok(o) => o,
            Err(e) => {
                log::warn!(Cause = e.to_string(); "Saved views database cannot be used; caching is off for this run; next step: check the cache folder is writable");
                return Started {
                    cache: None,
                    note: Some("Saved disk maps are off for this run: the cache folder cannot be written.".into()),
                };
            }
        };
        let (database, note) = match opened {
            Opened::Ready(db, note) => (db, note.map(str::to_string)),
            Opened::Busy => {
                log::warn!("Saved views database is in use by another process; caching is off for this run");
                return Started { cache: None, note: None };
            }
        };
        for target in [path.parent().map(Path::to_path_buf), Some(path.clone())].into_iter().flatten() {
            if let Err(e) = backup::exclude_from_backup(&target) {
                log::info!(Cause = e.to_string(); "Could not mark the cache out of backups");
            }
        }
        let queue = Arc::new(Queue::default());
        let inner = Arc::new(Inner {
            path: path.clone(),
            queue: queue.clone(),
            settings: Mutex::new(settings),
            clock: deps.clock.clone(),
            thread: Mutex::new(None),
        });
        let writer = Writer {
            db: Some(database),
            path,
            queue,
            settings: Arc::downgrade(&inner),
            clock: deps.clock,
            free: deps.free,
        };
        let spawned = std::thread::Builder::new().name("dyno-cache".into()).spawn(move || writer.run());
        match spawned {
            Ok(h) => *lock(&inner.thread) = Some(h),
            Err(e) => {
                log::warn!(Cause = e.to_string(); "Saved views writer thread did not start; caching is off for this run");
                return Started { cache: None, note: None };
            }
        }
        Started {
            cache: Some(Cache { inner }),
            note,
        }
    }

    /// The saved maps (a few rows of JSON; the trees are not read).
    pub fn list(&self) -> Vec<SavedRow> {
        db::list_saved(&self.inner.path).unwrap_or_else(|e| {
            log::warn!(Cause = e.to_string(); "Saved maps could not be listed");
            Vec::new()
        })
    }

    /// Loads a tree on the calling thread. A tree that does not parse is dropped alone and reported as damaged.
    pub fn load(&self, id: i64) -> Result<DiskIndex, CacheError> {
        let started = Instant::now();
        match db::load_disk(&self.inner.path, id) {
            Ok(index) => {
                self.inner.queue.push(Cmd::Touch(id));
                log::info!(Row = id, Nodes = index.len(), Ms = started.elapsed().as_millis() as u64; "Saved disk map loaded");
                Ok(index)
            }
            Err(e) => {
                log::warn!(Row = id, Cause = e.to_string(); "Saved disk map could not be loaded and is dropped; next step: scan again");
                self.forget(id);
                Err(e)
            }
        }
    }

    /// Queues a save. Never blocks: the older save of the same target (or the oldest when the queue is full) is told it was
    /// superseded.
    pub fn save(&self, job: SaveJob) {
        if !lock(&self.inner.settings).enabled {
            (job.done)(SaveOutcome::Disabled);
            return;
        }
        for old in self.inner.queue.push_save(job) {
            log::info!(Scan = old.scan; "Pending disk map save superseded");
            (old.done)(SaveOutcome::Superseded);
        }
    }

    /// Forgets one saved map.
    pub fn forget(&self, id: i64) {
        self.inner.queue.push(Cmd::Forget { id, done: None });
    }

    /// Forgets one saved map and waits until it is gone.
    pub fn forget_now(&self, id: i64) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.queue.push(Cmd::Forget { id, done: Some(tx) });
        rx.recv().unwrap_or(false)
    }

    /// Forgets every saved map: the files are deleted outright and an empty one started, so no path stays in a freed page.
    pub fn forget_all(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.queue.push(Cmd::ForgetAll { done: Some(tx) });
        let _ = rx.recv();
    }

    /// Follows a change of the Settings; switching saving off forgets everything.
    pub fn set_settings(&self, s: CacheSettings) {
        let was = std::mem::replace(&mut *lock(&self.inner.settings), s);
        if was.enabled && !s.enabled {
            self.forget_all();
        }
    }

    /// Stores a small "last known" document (the counter catalog, the software list).
    pub fn put_view(&self, key: &str, format: u32, data: String) {
        if lock(&self.inner.settings).enabled {
            self.inner.queue.push(Cmd::View {
                key: key.into(),
                format,
                data,
            });
        }
    }

    /// A saved document: `(format, saved_at, data)`.
    pub fn view(&self, key: &str) -> Option<(u32, i64, String)> {
        db::get_view(&self.inner.path, key).ok().flatten()
    }

    /// Waits until everything queued before this call was handled (tests, and the end of a run).
    pub fn flush(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.inner.queue.push(Cmd::Barrier(tx));
        let _ = rx.recv();
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Wall time now, on the cache's clock.
    pub fn now(&self) -> i64 {
        self.inner.clock.wall_ms()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.queue.push(Cmd::Stop);
        if let Some(h) = lock(&self.thread).take() {
            let _ = h.join();
        }
    }
}

struct Writer {
    db: Option<CacheDb>,
    path: PathBuf,
    queue: Arc<Queue>,
    settings: std::sync::Weak<Inner>,
    clock: Arc<dyn Clock>,
    free: Arc<dyn FreeSpace>,
}

impl Writer {
    fn limits(&self) -> CacheLimits {
        let max = self
            .settings
            .upgrade()
            .map_or(CacheSettings::default().max_bytes, |i| lock(&i.settings).max_bytes);
        CacheLimits {
            max_bytes: max,
            ..CacheLimits::default()
        }
    }

    fn run(mut self) {
        // Background work: nobody waits for a save (rule 52).
        dyno_platform::priority::lower_current_thread();
        loop {
            match self.queue.pop() {
                Cmd::Stop => break,
                Cmd::Save(job) => self.save(job),
                Cmd::Forget { id, done } => {
                    let gone = self.db.as_mut().map(|d| d.forget(id));
                    let existed = matches!(gone, Some(Ok(true)));
                    if let Some(Err(e)) = &gone {
                        log::warn!(Row = id, Cause = e.to_string(); "Saved disk map could not be forgotten");
                    } else {
                        log::info!(Row = id, Existed = existed; "Saved disk map forgotten");
                    }
                    if let Some(tx) = done {
                        let _ = tx.send(existed);
                    }
                }
                Cmd::ForgetAll { done } => {
                    self.db = None;
                    // Say what really happened: deleted, emptied in place (a file another handle holds open cannot be deleted
                    // on Windows), or neither.
                    let how = match db::delete_files(&self.path) {
                        Ok(()) => "the cache files were deleted",
                        Err(why) => match db::wipe_in_place(&self.path) {
                            Ok(()) => {
                                log::warn!(Cause = why.to_string(); "Cache files could not be deleted");
                                "the cache files could not be deleted, so every table was emptied in place"
                            }
                            Err(e) => {
                                log::error!(Cause = e.to_string(); "Cache files could not be deleted or emptied; next step: restart Dyno");
                                "the saved maps could NOT be removed"
                            }
                        },
                    };
                    self.db = match CacheDb::open(&self.path) {
                        Ok(Opened::Ready(d, _)) => Some(d),
                        _ => None,
                    };
                    log::info!("All saved disk maps forgotten; {how}");
                    if let Some(tx) = done {
                        let _ = tx.send(());
                    }
                }
                Cmd::Touch(id) => {
                    if let Some(d) = &self.db {
                        let _ = d.touch(id, self.clock.wall_ms());
                    }
                }
                Cmd::View { key, format, data } => {
                    if let Some(d) = &self.db {
                        if let Err(e) = d.put_view(&key, format, self.clock.wall_ms(), &data) {
                            log::warn!(Cause = e.to_string(); "A saved view could not be written");
                        }
                    }
                }
                Cmd::Barrier(tx) => {
                    let _ = tx.send(());
                }
            }
        }
    }

    fn save(&mut self, job: SaveJob) {
        let started = Instant::now();
        let limits = self.limits();
        let now = self.clock.wall_ms();
        let outcome = match self.db.as_mut() {
            None => SaveOutcome::Failed,
            Some(d) => match d.save_disk(&job.params, &job.index, now, limits, self.free.as_ref()) {
                Ok(Saved { id, packed_bytes, removed }) => {
                    log::info!(
                        Row = id, Scan = job.scan, Kind = job.kind, Nodes = job.index.len(), PackedBytes = packed_bytes,
                        Removed = removed.len(), Ms = started.elapsed().as_millis() as u64;
                        "Disk map saved"
                    );
                    SaveOutcome::Saved { id, removed }
                }
                Err(e) => {
                    log::warn!(Scan = job.scan, Kind = job.kind, Cause = e.to_string(), Ms = started.elapsed().as_millis() as u64;
                        "Disk map not saved; the earlier saved map (if any) is unchanged");
                    match e {
                        CacheError::TooBig => SaveOutcome::TooBig,
                        CacheError::NoSpace => SaveOutcome::NoSpace,
                        _ => SaveOutcome::Failed,
                    }
                }
            },
        };
        (job.done)(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::db::tests::{params, tree, Roomy};
    use super::testing::FakeClock;
    use super::*;
    use std::sync::mpsc::channel;

    fn start(dir: &Path, clock: Arc<FakeClock>) -> Cache {
        let s = Cache::start(
            dir.join("cache.db"),
            CacheSettings::default(),
            CacheDeps {
                clock,
                free: Arc::new(Roomy),
            },
        );
        s.cache.expect("the cache starts")
    }

    fn job(key: &str, names: &str, tx: Sender<(String, SaveOutcome)>) -> SaveJob {
        let key2 = key.to_string();
        SaveJob {
            params: params(key),
            index: Arc::new(tree(names, 5)),
            scan: 1,
            kind: "folder",
            done: Box::new(move |o| {
                let _ = tx.send((key2, o));
            }),
        }
    }

    #[test]
    fn a_save_goes_through_the_writer_thread_and_is_listed_and_loaded() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let clock = Arc::new(FakeClock::new(1_000));
        let c = start(d.path(), clock.clone());
        let (tx, rx) = channel();
        c.save(job("volume:/", "a", tx));
        let (_, out) = rx.recv().unwrap();
        let SaveOutcome::Saved { id, .. } = out else { panic!("{out:?}") };
        let rows = c.list();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].saved_at, 1_000, "the fake clock's time, not the machine's");
        assert_eq!(c.load(id).unwrap().to_records(), tree("a", 5).to_records());
    }

    #[test]
    fn a_pending_save_of_the_same_target_is_dropped_for_the_newer_one() {
        let q = Queue::default();
        let (tx, rx) = channel();
        assert!(q.push_save(job("folder:/a", "1", tx.clone())).is_empty());
        assert!(q.push_save(job("folder:/b", "2", tx.clone())).is_empty());
        let dropped = q.push_save(job("folder:/a", "3", tx.clone()));
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].params.target_key, "folder:/a");
        assert_eq!(q.pending_saves(), vec!["folder:/b", "folder:/a"]);
        // A full queue drops the oldest waiting save, never blocks.
        for n in 0..MAX_PENDING_SAVES {
            q.push_save(job(&format!("folder:/x{n}"), "4", tx.clone()));
        }
        assert_eq!(q.pending_saves().len(), MAX_PENDING_SAVES);
        drop(rx);
    }

    #[test]
    fn a_save_told_superseded_hears_it() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let c = start(d.path(), Arc::new(FakeClock::new(1)));
        let (tx, rx) = channel();
        c.save(job("folder:/a", "1", tx.clone()));
        c.save(job("folder:/a", "2", tx));
        c.flush();
        let mut outcomes: Vec<_> = rx.try_iter().map(|(_, o)| o).collect();
        outcomes.sort_by_key(|o| matches!(o, SaveOutcome::Superseded));
        assert!(
            outcomes.iter().any(|o| matches!(o, SaveOutcome::Saved { .. })),
            "the last one is written"
        );
        assert!(outcomes.len() <= 2);
    }

    #[test]
    fn forget_all_deletes_the_files_and_leaves_an_empty_cache() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let c = start(d.path(), Arc::new(FakeClock::new(1)));
        let (tx, rx) = channel();
        c.save(job("folder:/a", "1", tx));
        rx.recv().unwrap();
        assert_eq!(c.list().len(), 1);
        c.forget_all();
        assert!(c.list().is_empty());
        let (tx, rx) = channel();
        c.save(job("folder:/b", "2", tx));
        assert!(
            matches!(rx.recv().unwrap().1, SaveOutcome::Saved { .. }),
            "caching goes on after a forget"
        );
    }

    #[test]
    fn switching_the_setting_off_forgets_and_stops_saving() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let c = start(d.path(), Arc::new(FakeClock::new(1)));
        let (tx, rx) = channel();
        c.save(job("folder:/a", "1", tx.clone()));
        rx.recv().unwrap();
        c.set_settings(CacheSettings {
            enabled: false,
            ..CacheSettings::default()
        });
        assert!(c.list().is_empty());
        c.save(job("folder:/b", "2", tx));
        assert_eq!(rx.recv().unwrap().1, SaveOutcome::Disabled);
    }

    #[test]
    fn a_damaged_map_is_dropped_when_it_fails_to_load() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let c = start(d.path(), Arc::new(FakeClock::new(1)));
        let (tx, rx) = channel();
        c.save(job("folder:/a", "1", tx));
        let SaveOutcome::Saved { id, .. } = rx.recv().unwrap().1 else {
            panic!()
        };
        {
            let raw = rusqlite::Connection::open(c.path()).unwrap();
            raw.execute("UPDATE disk_chunks SET data = X'01FFFF'", []).unwrap();
        }
        assert!(c.load(id).is_err());
        c.flush();
        assert!(c.list().is_empty(), "the damaged row is gone");
    }

    #[test]
    fn a_busy_file_means_no_cache_for_this_run() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let p = d.path().join("cache.db");
        drop(start(d.path(), Arc::new(FakeClock::new(1))));
        let holder = rusqlite::Connection::open(&p).unwrap();
        holder.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let s = Cache::start(
            p,
            CacheSettings::default(),
            CacheDeps {
                clock: Arc::new(FakeClock::new(1)),
                free: Arc::new(Roomy),
            },
        );
        assert!(s.cache.is_none());
    }

    #[test]
    fn an_unwritable_folder_means_no_cache_and_one_note() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let blocker = d.path().join("file");
        std::fs::write(&blocker, b"x").unwrap();
        let s = Cache::start(
            blocker.join("cache.db"),
            CacheSettings::default(),
            CacheDeps {
                clock: Arc::new(FakeClock::new(1)),
                free: Arc::new(Roomy),
            },
        );
        assert!(s.cache.is_none());
        assert!(s.note.is_some_and(|n| n.contains("cache folder")));
    }

    #[test]
    fn views_round_trip_through_the_writer() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let clock = Arc::new(FakeClock::new(77));
        let c = start(d.path(), clock.clone());
        c.put_view("counters", 1, "[1,2]".into());
        c.flush();
        assert_eq!(c.view("counters"), Some((1, 77, "[1,2]".into())));
        clock.advance(5);
        c.put_view("counters", 1, "[3]".into());
        c.flush();
        assert_eq!(
            c.view("counters"),
            Some((1, 82, "[3]".into())),
            "replaced, with the time of the second write"
        );
    }
}
