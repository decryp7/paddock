//! A fake world: a clock that only moves when told to, a process table made by hand, a load made by hand. The files are real
//! (in a temp folder) in [`Rig::new`], so the on-disk layout is what is tested; [`Rig::mem`] keeps the lock folder in memory (the
//! [`MemStore`] behind the [`Store`] port) for a test whose point is a queue, admission or priority rule and that polls many times:
//! on a Windows runner every poll of a real folder costs file-system calls (ADR 0103's size classes; docs/benchmarks.md,
//! "2026-11-28, the throttle tests off the real folder").
#![allow(dead_code)]

use paddock_throttle::clock::{Clock, FakeClock};
use paddock_throttle::config::Config;
use paddock_throttle::fsinfo::FsKind;
use paddock_throttle::gate::FakeLoad;
use paddock_throttle::lock::{Acquirer, Poll, Request, Throttle};
use paddock_throttle::log::Logger;
use paddock_throttle::procs::FakeProcs;
use paddock_throttle::store::{RealStore, SlotLock, Stat, Store};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Rig {
    pub dir: tempfile::TempDir,
    pub clock: Arc<FakeClock>,
    pub procs: Arc<FakeProcs>,
    pub load: Arc<FakeLoad>,
    /// The lock folder's operations: the real folder, or [`MemStore`] in [`Rig::mem`].
    pub store: Arc<dyn Store>,
    in_memory: bool,
}

/// Plants holders in a lock folder (see [`Rig::plant_slot`]).
#[derive(Clone)]
pub struct Planter {
    root: PathBuf,
    store: Arc<dyn Store>,
}

impl Planter {
    /// Takes the slot lock like another process would and writes its info record; the lock is kept while the handle lives.
    pub fn plant_slot(&self, n: u32, owner: &str) -> Box<dyn SlotLock> {
        let held = self
            .store
            .take(&self.root.join(format!("throttle-slot{n}.lock")))
            .expect("lock file")
            .expect("free");
        self.store
            .write(
                &self.root.join(format!("throttle-slot{n}.info")),
                owner.as_bytes(),
            )
            .expect("info");
        held
    }
}

pub struct Agent {
    pub h: Throttle,
    pub said: Arc<Mutex<Vec<String>>>,
}

impl Agent {
    pub fn said(&self) -> String {
        self.said.lock().map(|s| s.join("\n")).unwrap_or_default()
    }
}

impl Rig {
    /// A rig on the real lock folder (a temp folder).
    pub fn new() -> Rig {
        Rig::build(false)
    }

    /// A rig whose lock folder is in memory: the same rules through the [`Store`] port, no file-system call per poll. Anything a test
    /// reads from the folder goes through [`Rig::store`] (the `held`, `count`, `owner_line` and log readers do); a file the run writes
    /// beside the store (its history, its captured output) is still real, in the temp folder.
    pub fn mem() -> Rig {
        Rig::build(true)
    }

    fn build(in_memory: bool) -> Rig {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(1_800_000_000);
        let clock = Arc::new(FakeClock::at_secs(now));
        let store: Arc<dyn Store> = if in_memory {
            Arc::new(MemStore::new(clock.clone()))
        } else {
            Arc::new(RealStore)
        };
        Rig {
            dir: tempfile::tempdir().expect("tempdir"),
            clock,
            procs: Arc::new(FakeProcs::new()),
            load: Arc::new(FakeLoad::new(1.0)),
            store,
            in_memory,
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn cfg(&self) -> Config {
        let (mut c, _) = Config::from_env(&|_| None, &paddock_throttle::places::Host::none());
        c.root = self.root().to_path_buf();
        c.poll = Duration::from_millis(200);
        c.wait_log = Duration::from_secs(1);
        c.unreadable_limit = 3;
        // One at a time unless a test asks for more (slots) on a machine of 16 cores.
        c.slots = 1;
        c.cpus = 16;
        c
    }

    /// One poll after the fake clock has passed the wait-line rate limit (`wait_log`): a wait line is said at most once per `wait_log`, so a
    /// test that reads `said()` right after the first poll sees nothing. Use this, not a hand-picked `advance`, before asserting a wait line.
    pub fn poll_said(&self, a: &mut Acquirer<'_>) -> Poll {
        let wait_log = self.cfg().wait_log;
        self.clock.advance(wait_log + Duration::from_secs(1));
        a.poll()
    }

    pub fn now(&self) -> u64 {
        self.clock.now_secs()
    }

    /// A participant with a made-up pid that "runs" since 100 s ago.
    pub fn agent(&self, pid: u32) -> Agent {
        self.agent_with(pid, self.cfg())
    }

    pub fn agent_with(&self, pid: u32, cfg: Config) -> Agent {
        self.procs.run(pid, self.now().saturating_sub(100));
        let said = Arc::new(Mutex::new(Vec::new()));
        let s2 = Arc::clone(&said);
        let log = Arc::new(Logger::new(
            format!("run-{pid}"),
            pid,
            self.clock.clone(),
            // The in-memory folder keeps no troubleshooting log on disk either.
            (!self.in_memory).then(|| self.root().join(format!("log-{pid}.clef"))),
            false,
        ));
        let mut h = Throttle::new(
            cfg,
            self.clock.clone(),
            self.procs.clone(),
            self.load.clone(),
            log,
            pid,
            Arc::new(move |m: &str| {
                if let Ok(mut v) = s2.lock() {
                    v.push(m.to_string());
                }
            }),
        );
        h.store = self.store.clone();
        if self.in_memory {
            // There is no folder to ask the OS about (on Windows that is a path resolve and a drive query per agent).
            h.fs_kind = Arc::new(|_| FsKind::Local);
        }
        Agent { h, said }
    }

    /// The text of a file of the lock folder (empty when there is none).
    fn text(&self, name: &str) -> String {
        self.store
            .read_to_string(&self.root().join(name))
            .unwrap_or_default()
    }

    /// The names of the files directly in the lock folder.
    fn names(&self) -> Vec<String> {
        self.store
            .list(self.root())
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect()
    }

    /// The info record of slot 1: there while a holder holds the slot, gone after a clean release.
    pub fn lock_dir(&self) -> PathBuf {
        self.root().join("throttle-slot1.info")
    }

    /// How many slots with this file name prefix (`throttle-slot`, `throttle-cheap`, `throttle-`) have a holder's record now.
    pub fn held(&self, prefix: &str) -> usize {
        self.names()
            .iter()
            .filter(|n| n.starts_with(prefix) && n.ends_with(".info"))
            .count()
    }

    pub fn breaks_log(&self) -> String {
        self.text("pitcrew-lock-breaks.log")
    }

    pub fn activity_log(&self) -> String {
        self.text("pitcrew-lock-activity.log")
    }

    /// Entries of the root whose name starts with `prefix`.
    pub fn count(&self, prefix: &str) -> usize {
        self.names()
            .iter()
            .filter(|n| n.starts_with(prefix))
            .count()
    }

    /// Makes slot 1 held by "somebody else" with this info record: the kernel lock is taken here and kept while the returned handle lives.
    pub fn plant_holder(&self, owner: &str) -> Box<dyn SlotLock> {
        self.plant_slot(1, owner)
    }

    /// The same for slot `n` (1-based).
    pub fn plant_slot(&self, n: u32, owner: &str) -> Box<dyn SlotLock> {
        self.clone_paths().plant_slot(n, owner)
    }

    /// A handle to plant holders from a closure that must be `'static` (a fault hook).
    pub fn clone_paths(&self) -> Planter {
        Planter {
            root: self.root().to_path_buf(),
            store: self.store.clone(),
        }
    }

    pub fn owner_line(&self, n: usize) -> String {
        self.text("throttle-slot1.info")
            .lines()
            .nth(n)
            .unwrap_or("")
            .to_string()
    }
}

pub fn req(name: &str) -> Request {
    Request {
        name: name.into(),
        cwd: "/work".into(),
        cmdline: "cargo test".into(),
        high: false,
        expected_secs: None,
        row: None,
    }
}

pub fn req_high(name: &str) -> Request {
    Request {
        high: true,
        ..req(name)
    }
}

/// Polls until held or `max` polls (bounded), sleeping the poll interval between. Returns the last poll.
pub fn acquire(a: &mut Acquirer<'_>, h: &Throttle, max: u32) -> Poll {
    let mut last = Poll::Waiting;
    for _ in 0..max {
        last = a.poll();
        if last != Poll::Waiting {
            return last;
        }
        h.clock.sleep(h.cfg.poll);
    }
    last
}

/// One poll.
pub fn once(a: &mut Acquirer<'_>) -> Poll {
    a.poll()
}

/// An in-memory lock folder (the [`Store`] port): the queue model explores the rules with no disk. A file has bytes and a modification
/// time from the fake clock; a slot lock is a set entry that its guard removes when dropped, like the kernel's lock.
pub struct MemStore {
    clock: Arc<FakeClock>,
    st: Arc<Mutex<MemState>>,
}

#[derive(Default)]
struct MemState {
    files: std::collections::BTreeMap<PathBuf, (Vec<u8>, u64)>,
    locks: std::collections::BTreeSet<PathBuf>,
}

struct MemLock {
    st: Arc<Mutex<MemState>>,
    path: PathBuf,
}

impl Drop for MemLock {
    fn drop(&mut self) {
        if let Ok(mut s) = self.st.lock() {
            s.locks.remove(&self.path);
        }
    }
}

impl SlotLock for MemLock {
    fn release(self: Box<Self>) -> std::io::Result<()> {
        Ok(())
    }
}

fn not_found() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::NotFound)
}

impl MemStore {
    pub fn new(clock: Arc<FakeClock>) -> MemStore {
        MemStore {
            clock,
            st: Arc::default(),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, MemState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The files whose name starts with `prefix` and ends with `suffix`.
    pub fn count(&self, prefix: &str, suffix: &str) -> usize {
        self.state()
            .files
            .keys()
            .filter(|p| {
                let n = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                n.starts_with(prefix) && n.ends_with(suffix)
            })
            .count()
    }
}

impl Store for MemStore {
    fn create_dir_all(&self, _dir: &Path) {}

    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let now = self.clock.now_secs();
        self.state()
            .files
            .insert(path.to_path_buf(), (bytes.to_vec(), now));
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        let mut s = self.state();
        let f = s.files.remove(from).ok_or_else(not_found)?;
        s.files.insert(to.to_path_buf(), f);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.state()
            .files
            .remove(path)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    fn read_bounded(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let mut b = self
            .state()
            .files
            .get(path)
            .ok_or_else(not_found)?
            .0
            .clone();
        b.truncate(paddock_throttle::record::MAX_RECORD);
        Ok(b)
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        let b = self
            .state()
            .files
            .get(path)
            .ok_or_else(not_found)?
            .0
            .clone();
        String::from_utf8(b).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
    }

    fn stat(&self, path: &Path) -> Option<Stat> {
        self.state().files.get(path).map(|(b, t)| Stat {
            len: b.len() as u64,
            mtime: Some(*t),
        })
    }

    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        self.state()
            .files
            .keys()
            .filter(|p| p.parent() == Some(dir))
            .cloned()
            .collect()
    }

    fn append_line(&self, path: &Path, line: &str) {
        let now = self.clock.now_secs();
        let mut s = self.state();
        let e = s
            .files
            .entry(path.to_path_buf())
            .or_insert_with(|| (Vec::new(), now));
        e.0.extend_from_slice(line.as_bytes());
        e.0.push(b'\n');
    }

    fn take(&self, path: &Path) -> std::io::Result<Option<Box<dyn SlotLock>>> {
        let now = self.clock.now_secs();
        let mut s = self.state();
        s.files
            .entry(path.to_path_buf())
            .or_insert_with(|| (Vec::new(), now));
        if s.locks.contains(path) {
            return Ok(None);
        }
        s.locks.insert(path.to_path_buf());
        Ok(Some(Box::new(MemLock {
            st: Arc::clone(&self.st),
            path: path.to_path_buf(),
        })))
    }

    fn is_held(&self, path: &Path) -> std::io::Result<bool> {
        let s = self.state();
        Ok(s.files.contains_key(path) && s.locks.contains(path))
    }
}

/// A [`Store`] that notes which kind of operation touched which kind of file, and forwards to the store it wraps: how a sweep's reach
/// into the folder is measured (the real-folder sweep must reach what the in-memory sweep reaches).
pub struct Recording {
    pub inner: Arc<dyn Store>,
    pub seen: Arc<Mutex<std::collections::BTreeSet<String>>>,
}

/// The kind of a file of the lock folder, from its name.
fn kind_of(p: &Path) -> &'static str {
    let n = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if n.starts_with("throttle-slot") && n.ends_with(".lock") {
        "slot-lock"
    } else if n.starts_with("throttle-slot") {
        "slot-info"
    } else if n.starts_with("throttle-cheap") {
        "cheap"
    } else if n.starts_with("pitcrew-heavy.ticket.") && n.ends_with(".tmp") {
        "ticket-tmp"
    } else if n.starts_with("pitcrew-heavy.ticket.") {
        "ticket"
    } else if n.starts_with("pitcrew-heavy.high.") {
        "high"
    } else if n.starts_with("pitcrew-heavy.drain.") {
        "drain"
    } else if n.starts_with(".plock.") {
        "tmp"
    } else {
        "log"
    }
}

impl Recording {
    fn note(&self, op: &str, p: &Path) {
        if let Ok(mut s) = self.seen.lock() {
            s.insert(format!("{op} {}", kind_of(p)));
        }
    }
}

impl Store for Recording {
    fn create_dir_all(&self, dir: &Path) {
        self.inner.create_dir_all(dir)
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        self.note("write", path);
        self.inner.write(path, bytes)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.note("rename-to", to);
        self.inner.rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.note("remove", path);
        self.inner.remove_file(path)
    }
    fn read_bounded(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.note("read", path);
        self.inner.read_bounded(path)
    }
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        self.inner.read_to_string(path)
    }
    fn stat(&self, path: &Path) -> Option<Stat> {
        self.inner.stat(path)
    }
    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        self.inner.list(dir)
    }
    fn append_line(&self, path: &Path, line: &str) {
        self.inner.append_line(path, line)
    }
    fn take(&self, path: &Path) -> std::io::Result<Option<Box<dyn SlotLock>>> {
        let r = self.inner.take(path)?;
        self.note(if r.is_some() { "take" } else { "take-busy" }, path);
        Ok(r)
    }
    fn is_held(&self, path: &Path) -> std::io::Result<bool> {
        let r = self.inner.is_held(path)?;
        self.note(if r { "probe-held" } else { "probe-free" }, path);
        Ok(r)
    }
}
