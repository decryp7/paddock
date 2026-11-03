//! Fakes for the disk scan's tests: a file system in memory with injectable failures and hooks, so no test touches a real
//! folder of the machine and none sleeps.

use dyno_core::disk::source::{DirEntryInfo, DirSource, EntryKind, ReadError};
use dyno_core::time::Timestamp;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

type Hook = Arc<dyn Fn(&Path) + Send + Sync>;

/// A folder tree in memory. The root is `fake-root` (a relative path: the fake never leaves memory).
pub struct FakeFs {
    root: PathBuf,
    entries: Mutex<HashMap<PathBuf, Vec<DirEntryInfo>>>,
    open_errors: Mutex<HashMap<PathBuf, ReadError>>,
    stat_errors: Mutex<HashMap<PathBuf, ReadError>>,
    entry_errors: Mutex<HashMap<PathBuf, Vec<ReadError>>>,
    /// Folders whose failing read takes the whole volume down with it (the root then fails its `stat` too).
    drops_volume: Mutex<std::collections::HashSet<PathBuf>>,
    volume_down: std::sync::atomic::AtomicBool,
    hook: Mutex<Option<Hook>>,
    stat_hook: Mutex<Option<Hook>>,
    entry_hook: Mutex<Option<Hook>>,
    next_ino: AtomicU64,
    pub reads: AtomicU64,
}

impl FakeFs {
    pub fn new() -> FakeFs {
        let fs = FakeFs {
            root: PathBuf::from("fake-root"),
            entries: Mutex::new(HashMap::new()),
            open_errors: Mutex::new(HashMap::new()),
            stat_errors: Mutex::new(HashMap::new()),
            entry_errors: Mutex::new(HashMap::new()),
            drops_volume: Mutex::new(std::collections::HashSet::new()),
            volume_down: std::sync::atomic::AtomicBool::new(false),
            hook: Mutex::new(None),
            stat_hook: Mutex::new(None),
            entry_hook: Mutex::new(None),
            next_ino: AtomicU64::new(100),
            reads: AtomicU64::new(0),
        };
        fs.entries.lock().unwrap().insert(fs.root.clone(), Vec::new());
        fs
    }

    pub fn root(&self) -> PathBuf {
        self.root.clone()
    }

    fn ino(&self) -> u64 {
        self.next_ino.fetch_add(1, Ordering::Relaxed)
    }

    fn push(&self, parent: &Path, e: DirEntryInfo) {
        self.entries.lock().unwrap().entry(parent.to_path_buf()).or_default().push(e);
    }

    fn entry(&self, name: &OsStr, kind: EntryKind, bytes: u64, dev: u64, ino: u64, links: u64) -> DirEntryInfo {
        DirEntryInfo {
            name: name.to_os_string(),
            kind,
            bytes,
            logical: bytes,
            dev,
            ino,
            links,
            mtime: Timestamp(1_000),
        }
    }

    /// A folder under `parent` (on device 1); returns its path.
    pub fn dir(&self, parent: &Path, name: impl AsRef<OsStr>) -> PathBuf {
        self.dir_on(parent, name, 1, self.ino())
    }

    /// A folder with an explicit device and file id (a mount point, a loop).
    pub fn dir_on(&self, parent: &Path, name: impl AsRef<OsStr>, dev: u64, ino: u64) -> PathBuf {
        self.push(parent, self.entry(name.as_ref(), EntryKind::Dir, 0, dev, ino, 1));
        let p = parent.join(name.as_ref());
        self.entries.lock().unwrap().entry(p.clone()).or_default();
        p
    }

    pub fn file(&self, parent: &Path, name: impl AsRef<OsStr>, bytes: u64) {
        self.push(parent, self.entry(name.as_ref(), EntryKind::File, bytes, 1, self.ino(), 1));
    }

    /// A file whose allocation and length differ (a sparse or compressed file, or a small one in a whole block).
    pub fn file_sized(&self, parent: &Path, name: impl AsRef<OsStr>, on_disk: u64, logical: u64) {
        let mut e = self.entry(name.as_ref(), EntryKind::File, on_disk, 1, self.ino(), 1);
        e.logical = logical;
        self.push(parent, e);
    }

    /// A file with `links` names sharing one file id.
    pub fn linked_file(&self, parent: &Path, name: impl AsRef<OsStr>, bytes: u64, ino: u64) {
        self.push(parent, self.entry(name.as_ref(), EntryKind::File, bytes, 1, ino, 2));
    }

    pub fn symlink(&self, parent: &Path, name: impl AsRef<OsStr>) {
        self.push(parent, self.entry(name.as_ref(), EntryKind::Link, 0, 1, self.ino(), 1));
    }

    pub fn socket(&self, parent: &Path, name: impl AsRef<OsStr>) {
        self.push(parent, self.entry(name.as_ref(), EntryKind::Other, 0, 1, self.ino(), 1));
    }

    /// Opening this folder fails.
    pub fn fail_open(&self, path: &Path, e: ReadError) {
        self.open_errors.lock().unwrap().insert(path.to_path_buf(), e);
    }

    /// Reading this folder (its open or its entries) fails and takes the whole volume down: the root answers nothing afterwards,
    /// as a dropped share does. Without this a failure is local to the folder.
    pub fn drop_volume_at(&self, path: &Path) {
        self.drops_volume.lock().unwrap().insert(path.to_path_buf());
    }

    /// Opening this folder works again (the person gave the rights).
    pub fn allow_open(&self, path: &Path) {
        self.open_errors.lock().unwrap().remove(path);
    }

    /// Looking at this path fails (a root that cannot be started from).
    pub fn fail_stat(&self, path: &Path, e: ReadError) {
        self.stat_errors.lock().unwrap().insert(path.to_path_buf(), e);
    }

    /// Reading this folder yields these entry errors after its entries.
    pub fn fail_entries(&self, path: &Path, errors: Vec<ReadError>) {
        self.entry_errors.lock().unwrap().insert(path.to_path_buf(), errors);
    }

    /// Runs after every entry of a folder has been handed to the reader, with the entry's path (to stop a listing half way).
    pub fn on_entry(&self, f: impl Fn(&Path) + Send + Sync + 'static) {
        *self.entry_hook.lock().unwrap() = Some(Arc::new(f));
    }

    /// Runs before every `stat` (to hold the first look at a root at a known point).
    pub fn on_stat(&self, f: impl Fn(&Path) + Send + Sync + 'static) {
        *self.stat_hook.lock().unwrap() = Some(Arc::new(f));
    }

    /// Runs before every folder read (to block, count or cancel at a known point).
    pub fn on_read(&self, f: impl Fn(&Path) + Send + Sync + 'static) {
        *self.hook.lock().unwrap() = Some(Arc::new(f));
    }
}

impl DirSource for FakeFs {
    fn stat(&self, path: &Path) -> Result<DirEntryInfo, ReadError> {
        let hook = self.stat_hook.lock().unwrap().clone();
        if let Some(h) = hook {
            h(path);
        }
        if let Some(e) = self.stat_errors.lock().unwrap().get(path) {
            return Err(e.clone());
        }
        if path == self.root && self.volume_down.load(Ordering::SeqCst) {
            return Err(ReadError::Unreachable("Socket is not connected (os error 57)".into()));
        }
        if path == self.root {
            return Ok(self.entry(OsStr::new("fake-root"), EntryKind::Dir, 0, 1, 1, 1));
        }
        let parent = path.parent().ok_or(ReadError::NotFound)?;
        let name = path.file_name().ok_or(ReadError::NotFound)?;
        let entries = self.entries.lock().unwrap();
        entries
            .get(parent)
            .and_then(|v| v.iter().find(|e| e.name == name).cloned())
            .ok_or(ReadError::NotFound)
    }

    fn read_dir(&self, path: &Path, each: &mut dyn FnMut(Result<DirEntryInfo, ReadError>) -> bool) -> Result<(), ReadError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let hook = self.hook.lock().unwrap().clone();
        if let Some(h) = hook {
            h(path);
        }
        if self.drops_volume.lock().unwrap().contains(path) {
            self.volume_down.store(true, Ordering::SeqCst);
        }
        if let Some(e) = self.open_errors.lock().unwrap().get(path) {
            return Err(e.clone());
        }
        let list = self.entries.lock().unwrap().get(path).cloned().ok_or(ReadError::NotFound)?;
        let entry_hook = self.entry_hook.lock().unwrap().clone();
        for e in list {
            let name = e.name.clone();
            if !each(Ok(e)) {
                return Ok(());
            }
            if let Some(h) = &entry_hook {
                h(&path.join(name));
            }
        }
        let errors = self.entry_errors.lock().unwrap().get(path).cloned().unwrap_or_default();
        for e in errors {
            if !each(Err(e)) {
                break;
            }
        }
        Ok(())
    }
}

/// Holds readers at a known point until the test lets them go: tests wait on conditions, never on time.
pub struct Gate {
    state: Mutex<(bool, usize)>,
    cv: Condvar,
}

impl Gate {
    pub fn new() -> Arc<Gate> {
        Arc::new(Gate {
            state: Mutex::new((false, 0)),
            cv: Condvar::new(),
        })
    }

    /// Called by a reader: counts it as arrived and blocks until [`Gate::open`].
    pub fn pass(&self) {
        let mut s = self.state.lock().unwrap();
        s.1 += 1;
        self.cv.notify_all();
        while !s.0 {
            s = self.cv.wait(s).unwrap();
        }
    }

    /// Blocks until `n` readers have arrived.
    pub fn wait_arrived(&self, n: usize) {
        let mut s = self.state.lock().unwrap();
        while s.1 < n {
            s = self.cv.wait(s).unwrap();
        }
    }

    /// Lets every reader go, now and later.
    pub fn open(&self) {
        self.state.lock().unwrap().0 = true;
        self.cv.notify_all();
    }
}
