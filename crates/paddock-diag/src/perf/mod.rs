//! The "Performance" section of the diagnostics bundle (`performance.json` and `performance-summary.txt`): how long the start
//! took by phase, the slowest recent operations and what machine this ran on. It holds names, durations and hardware facts,
//! never user content: operation and phase names are `&'static str` (a file name or a query text cannot be passed at all), and
//! a name that is not an identifier ([`ring::valid_name`]) still becomes `invalid-name`, also when read back from a ring file.
//!
//! An app does four things:
//!
//! 1. at start, [`init`] with its data folder (once; reads the ring the last run left) and [`startup_phase`] after each step
//!    of the start (`logging`, `settings`, `window-ready`);
//! 2. around work a person waits for (an import, a query, a page load), [`record`] or [`record_phase`] with the duration, or
//!    [`time`] for a guard that records on drop; a finished job goes through [`record_job`];
//! 3. keep the ring on disk: [`flush_in_background`] after a record that changed it (it returns `true`) and [`flush`] once at
//!    exit, so a crash or a quit does not lose the history (the section flushes before it reads, too);
//! 4. add [`PerformanceSection`] to the report's sections.
//!
//! Cost: a record takes no lock for an operation too short for the full ring (one atomic read, one clock read), and the ring
//! holds at most [`RING_CAPACITY`] entries of 24 hours, a few KB in memory and in its file (`performance-ring.json` in the data
//! folder, replaced atomically; a damaged file is replaced at the next flush and never stops the app). The format follows the
//! startup and summary views of VS Code ("Startup Performance"), Chrome's trace summary and Windows Performance Recorder: a
//! list of phases and a list of the slowest things, no tracing.
//!
//! The JSON writes no number of 13 or more digits (times as RFC 3339 or an age in seconds, sizes in MiB): the text redactor
//! every file passes masks such a run that passes the Luhn check, and would break the file.
//!
//! - [`ring`]: names, the ring of the slowest operations, the startup phases (pure).
//! - [`machine`]: the machine facts and the data folder's volume (`sysinfo` at the edge).
//! - [`document`]: `performance.json` and the summary (pure).

pub mod document;
pub mod machine;
pub mod ring;

use crate::section::{Content, Context, Section, SectionFile, SectionOutput};
use document::Document;
use machine::{MachineSource, SystemMachine};
pub use ring::{DEFAULT_PHASE, INVALID_NAME, MAX_NAME, RING_CAPACITY, WINDOW_MS};
use ring::{Floor, Phase, Ring, RingError, Slow, Startup};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The ring file in the data folder.
pub const RING_FILE: &str = "performance-ring.json";

/// What the recorder holds: the ring, the startup phases and where the ring is saved.
struct State {
    ring: Ring,
    startup: Startup,
    path: Option<PathBuf>,
    dirty: bool,
    began: Instant,
    mark: Instant,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static INIT: OnceLock<Loaded> = OnceLock::new();
/// [`Ring::floor`], readable without the lock.
static FLOOR_MS: AtomicU64 = AtomicU64::new(0);
static FLOOR_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
/// One flush at a time: the write and the rename of the ring file.
static FLUSH: Mutex<()> = Mutex::new(());
/// A background flush is queued and has not started yet.
static FLUSH_QUEUED: AtomicBool = AtomicBool::new(false);

impl State {
    fn new() -> State {
        let now = Instant::now();
        State {
            ring: Ring::new(),
            startup: Startup::new(),
            path: None,
            dirty: false,
            began: now,
            mark: now,
        }
    }
}

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::new()))
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn publish_floor(st: &State, now: u64) {
    let f = st.ring.floor(now);
    FLOOR_MS.store(f.ms, Ordering::Relaxed);
    FLOOR_UNTIL_MS.store(f.until_ms, Ordering::Relaxed);
}

/// What [`init`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// A ring file with this many entries from the last 24 hours.
    Entries(usize),
    /// No ring file yet.
    Fresh,
    /// The ring file could not be used (and is replaced at the next flush); the app carries on with an empty ring.
    Unusable(RingError),
}

/// Starts the startup clock (the first call fixes time zero; every other function does it too, so the earlier in `main` the
/// better).
pub fn startup_begin() {
    drop(lock());
}

/// Names the data folder, reads the ring the last run left there and merges it in. Runs once per process: a second call
/// (from another place that also starts the app) returns the first call's outcome and changes nothing, so nothing is merged
/// twice.
pub fn init(data_dir: &Path) -> Loaded {
    INIT.get_or_init(|| init_once(data_dir)).clone()
}

fn init_once(data_dir: &Path) -> Loaded {
    let now = now_ms();
    let mut st = lock();
    let loaded = load_into(&mut st, data_dir, now);
    publish_floor(&st, now);
    loaded
}

/// Names the ring file in `data_dir` and merges what it holds into `st`.
fn load_into(st: &mut State, data_dir: &Path, now: u64) -> Loaded {
    let path = data_dir.join(RING_FILE);
    let outcome = read_ring(&path, now);
    st.path = Some(path);
    match outcome {
        Ok(None) => Loaded::Fresh,
        Ok(Some(ring)) => {
            let n = ring.len();
            for e in ring.snapshot(now) {
                st.ring.insert(e, now);
            }
            Loaded::Entries(n)
        }
        Err(e) => {
            log::warn!("the performance ring file is not used: {e}");
            st.dirty = true;
            Loaded::Unusable(e)
        }
    }
}

fn read_ring(path: &Path, now: u64) -> Result<Option<Ring>, RingError> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(RingError::Damaged(format!(
                "cannot be read: {:?}",
                e.kind()
            )));
        }
    };
    if meta.len() > ring::MAX_RING_FILE {
        return Err(RingError::TooLarge(meta.len()));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| RingError::Damaged(format!("cannot be read: {:?}", e.kind())))?;
    Ring::parse(&bytes, now).map(Some)
}

/// Records that `op` took `duration` (as one piece, phase [`DEFAULT_PHASE`]). Returns whether the ring kept it (and so
/// changed: see [`flush_in_background`]).
pub fn record(op: &'static str, duration: Duration) -> bool {
    record_phase(op, DEFAULT_PHASE, duration)
}

/// Records that the `phase` of `op` took `duration`; see [`record`]. Cheap: a duration too short for the full ring returns
/// after one atomic read and one clock read.
pub fn record_phase(op: &'static str, phase: &'static str, duration: Duration) -> bool {
    record_with(duration, || (op.to_string(), phase.to_string()))
}

/// Records that a job of `kind` ended as `outcome` (`done`, `failed`, `cancelled`) after `duration`, as `job:<kind>`.
pub fn record_job(kind: &'static str, outcome: &'static str, duration: Duration) -> bool {
    record_with(duration, || (format!("job:{kind}"), outcome.to_string()))
}

fn record_with(duration: Duration, names: impl FnOnce() -> (String, String)) -> bool {
    let ms = millis(duration);
    let now = now_ms();
    let floor = Floor {
        ms: FLOOR_MS.load(Ordering::Relaxed),
        until_ms: FLOOR_UNTIL_MS.load(Ordering::Relaxed),
    };
    if floor.turns_away(ms, now) {
        return false;
    }
    let (op, phase) = names();
    let mut st = lock();
    let kept = st.ring.insert(Slow::new(&op, &phase, ms, now), now);
    st.dirty |= kept;
    publish_floor(&st, now);
    kept
}

/// Ends the startup phase `name`: its duration runs from the previous phase's end (or from the start).
pub fn startup_phase(name: &'static str) {
    let mut st = lock();
    let now = Instant::now();
    let (at, took) = (
        millis(now.duration_since(st.began)),
        millis(now.duration_since(st.mark)),
    );
    st.mark = now;
    st.startup.push(name, at, took);
}

/// A guard that records `op` with the time between [`time`] and its drop.
#[must_use = "the duration is recorded when the guard is dropped"]
pub struct Timer {
    op: &'static str,
    phase: &'static str,
    since: Instant,
}

/// Times `op` until the returned guard is dropped.
pub fn time(op: &'static str) -> Timer {
    time_since(op, Instant::now())
}

/// Times `op` from `since` (work that began before the guard could be made) until the returned guard is dropped.
pub fn time_since(op: &'static str, since: Instant) -> Timer {
    Timer {
        op,
        phase: DEFAULT_PHASE,
        since,
    }
}

impl Timer {
    /// Names the phase this guard times.
    pub fn phase(mut self, phase: &'static str) -> Self {
        self.phase = phase;
        self
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        record_phase(self.op, self.phase, self.since.elapsed());
    }
}

/// The temporary file a flush writes before the rename: one per process.
fn tmp_path(path: &Path, pid: u32) -> PathBuf {
    path.with_extension(format!("json.{pid}.tmp"))
}

/// Writes the ring to its file when it changed (a temporary file, then a rename; one flush at a time, so two callers never
/// share the temporary file). Does nothing before [`init`].
pub fn flush() -> std::io::Result<()> {
    let _one_at_a_time = FLUSH.lock().unwrap_or_else(PoisonError::into_inner);
    let (path, bytes) = {
        let mut st = lock();
        let Some(path) = st.path.clone() else {
            return Ok(());
        };
        if !st.dirty {
            return Ok(());
        }
        st.dirty = false;
        (path, st.ring.to_bytes(now_ms()))
    };
    let tmp = tmp_path(&path, std::process::id());
    let written = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &path));
    if written.is_err() {
        lock().dirty = true;
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Flushes on a thread of its own (never on the UI thread) and logs a failure. Calls made while one is queued join it: a
/// second call returns `None`. The handle is for tests and for an exit that wants to wait.
pub fn flush_in_background() -> Option<JoinHandle<()>> {
    if FLUSH_QUEUED.swap(true, Ordering::AcqRel) {
        return None;
    }
    let spawned = std::thread::Builder::new()
        .name("perf-flush".into())
        .spawn(|| {
            // Cleared first: a record that lands during the write queues the next flush.
            FLUSH_QUEUED.store(false, Ordering::Release);
            if let Err(e) = flush() {
                log::warn!("the performance ring could not be saved: {:?}", e.kind());
            }
        });
    match spawned {
        Ok(handle) => Some(handle),
        Err(e) => {
            FLUSH_QUEUED.store(false, Ordering::Release);
            log::warn!(
                "the performance ring could not be saved: no thread ({:?})",
                e.kind()
            );
            None
        }
    }
}

/// Where a section reads what was recorded; a test names its own.
pub trait PerfSource: Send + Sync {
    /// Saves what is pending (best effort).
    fn flush(&self) -> std::io::Result<()>;
    /// The startup phases and the slowest operations of the last 24 hours, slowest first.
    fn snapshot(&self, now_ms: u64) -> (Vec<Phase>, Vec<Slow>);
}

/// The process-wide recorder the functions above write to.
pub struct Global;

impl PerfSource for Global {
    fn flush(&self) -> std::io::Result<()> {
        flush()
    }

    fn snapshot(&self, now_ms: u64) -> (Vec<Phase>, Vec<Slow>) {
        let st = lock();
        (st.startup.phases().to_vec(), st.ring.snapshot(now_ms))
    }
}

/// The report section: `performance.json` and `performance-summary.txt`.
pub struct PerformanceSection {
    source: Arc<dyn PerfSource>,
    machine: Arc<dyn MachineSource>,
}

impl PerformanceSection {
    /// The section over the process-wide recorder and this machine.
    pub fn new() -> Self {
        PerformanceSection::with(Arc::new(Global), Arc::new(SystemMachine))
    }

    /// The section over named sources (tests).
    pub fn with(source: Arc<dyn PerfSource>, machine: Arc<dyn MachineSource>) -> Self {
        PerformanceSection { source, machine }
    }
}

impl Default for PerformanceSection {
    fn default() -> Self {
        PerformanceSection::new()
    }
}

impl Section for PerformanceSection {
    fn id(&self) -> &str {
        "performance"
    }

    fn collect(&self, cx: &Context<'_>) -> SectionOutput {
        let mut notes = Vec::new();
        if let Err(e) = self.source.flush() {
            notes.push(format!(
                "The performance history could not be saved just now ({:?}).",
                e.kind()
            ));
        }
        let at = u64::try_from(cx.now.timestamp_millis()).unwrap_or(0);
        let (startup, slowest) = self.source.snapshot(at);
        let doc = Document::new(at, self.machine.facts(cx.dirs.data()), startup, slowest);
        let summary = doc.summary();
        SectionOutput {
            notes,
            files: vec![
                SectionFile::app(
                    "performance-summary.txt",
                    "How long the start took, the slowest recent operations and the machine, in words",
                    Content::Text(summary),
                ),
                SectionFile::app(
                    "performance.json",
                    "The same as data: startup phases, the slowest operations of the last 24 hours (names and durations only), processor, memory, disk type, system and load",
                    // Text, not `Content::Json`: the JSON redactor cuts a nested list longer than 1000 characters to a string, and this list
                    // is the point. The text redactor still masks secrets and personal data in every string.
                    Content::Text(doc.to_json_text()),
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::machine::MachineFacts;
    use super::*;
    use chrono::{TimeZone, Utc};
    use paddock_env::{AppDirs, AppId, MapVars, Os};

    struct FakeMachine;
    impl MachineSource for FakeMachine {
        fn facts(&self, _data_dir: &Path) -> MachineFacts {
            MachineFacts {
                cpu_model: "Test CPU".into(),
                cpu_logical_cores: 4,
                cpu_physical_cores: None,
                memory_total_mib: 1024,
                memory_free_mib: 512,
                data_disk: None,
                os: "TestOS".into(),
                load_average: None,
            }
        }
    }

    struct FakeSource {
        flush: std::io::ErrorKind,
        slowest: Vec<Slow>,
    }
    impl PerfSource for FakeSource {
        fn flush(&self) -> std::io::Result<()> {
            if self.flush == std::io::ErrorKind::Other {
                Ok(())
            } else {
                Err(self.flush.into())
            }
        }
        fn snapshot(&self, _now_ms: u64) -> (Vec<Phase>, Vec<Slow>) {
            (
                vec![Phase {
                    name: "logging".into(),
                    at_ms: 5,
                    duration_ms: 5,
                }],
                self.slowest.clone(),
            )
        }
    }

    fn dirs(dir: &Path) -> AppDirs {
        let id = AppId::new("demo", "Demo").unwrap();
        AppDirs::resolve(
            &id,
            Os::MacOs,
            &MapVars::new().with("DEMO_HOME", dir),
            None,
            true,
        )
        .unwrap()
    }

    fn collect(section: &PerformanceSection) -> SectionOutput {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::AppInfo::new("demo", "Demo", "1.0.0", "");
        let cx = Context {
            app: &app,
            dirs: &dirs(dir.path()),
            now: Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap(),
        };
        section.collect(&cx)
    }

    #[test]
    fn the_section_has_a_summary_and_the_data() {
        let slowest = vec![Slow::new("query", "scan", 900, 1)];
        let s = PerformanceSection::with(
            Arc::new(FakeSource {
                flush: std::io::ErrorKind::Other,
                slowest,
            }),
            Arc::new(FakeMachine),
        );
        assert_eq!(s.id(), "performance");
        let out = collect(&s);
        assert!(out.notes.is_empty());
        let names: Vec<&str> = out.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["performance-summary.txt", "performance.json"]);
        assert!(crate::valid_entry_name(names[0]) && crate::valid_entry_name(names[1]));
        let Content::Text(json) = &out.files[1].content else {
            panic!("text")
        };
        // Every `.json` this section writes still parses after the redactor every file passes (P1-1).
        for f in out.files.iter().filter(|f| f.name.ends_with(".json")) {
            let Content::Text(text) = &f.content else {
                panic!("text")
            };
            let (masked, counts) = crate::Redactor::new().text(text);
            assert_eq!(counts, Default::default(), "{masked}");
            assert!(
                serde_json::from_str::<serde_json::Value>(&masked).is_ok(),
                "{masked}"
            );
        }
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["slowest"][0]["op"], "query");
        let Content::Text(t) = &out.files[0].content else {
            panic!("text")
        };
        assert!(t.contains("Test CPU") && t.contains("query"));
    }

    #[test]
    fn a_failed_flush_is_a_note_and_the_section_still_reports() {
        let s = PerformanceSection::with(
            Arc::new(FakeSource {
                flush: std::io::ErrorKind::PermissionDenied,
                slowest: vec![],
            }),
            Arc::new(FakeMachine),
        );
        let out = collect(&s);
        assert_eq!(out.files.len(), 2);
        assert!(out.notes[0].contains("PermissionDenied"), "{:?}", out.notes);
    }

    #[test]
    fn loading_merges_a_ring_once_and_marks_a_damaged_file_for_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let now = now_ms();
        let mut st = State::new();
        assert_eq!(load_into(&mut st, dir.path(), now), Loaded::Fresh);
        assert_eq!(
            st.path.as_deref(),
            Some(dir.path().join(RING_FILE).as_path())
        );
        assert!(!st.dirty);
        let mut ring = Ring::new();
        ring.insert(Slow::new("a", "b", 10, now), now);
        ring.insert(Slow::new("c", "d", 20, now), now);
        std::fs::write(dir.path().join(RING_FILE), ring.to_bytes(now)).unwrap();
        assert_eq!(load_into(&mut st, dir.path(), now), Loaded::Entries(2));
        assert_eq!(st.ring.len(), 2);
        std::fs::write(dir.path().join(RING_FILE), b"{ damaged").unwrap();
        assert!(matches!(
            load_into(&mut st, dir.path(), now),
            Loaded::Unusable(RingError::Damaged(_))
        ));
        assert!(st.dirty, "the damaged file is replaced at the next flush");
        assert_eq!(st.ring.len(), 2, "what was loaded stays");
    }

    #[test]
    fn the_temporary_file_is_one_per_process_beside_the_ring_file() {
        let p = Path::new("/d/performance-ring.json");
        assert_eq!(tmp_path(p, 7), Path::new("/d/performance-ring.json.7.tmp"));
        assert_ne!(tmp_path(p, 7), tmp_path(p, 8));
    }

    #[test]
    fn a_ring_file_is_read_and_a_damaged_one_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RING_FILE);
        let now = now_ms();
        assert_eq!(read_ring(&path, now), Ok(None));
        let mut ring = Ring::new();
        ring.insert(Slow::new("a", "b", 10, now), now);
        std::fs::write(&path, ring.to_bytes(now)).unwrap();
        assert_eq!(read_ring(&path, now).unwrap().unwrap().len(), 1);
        std::fs::write(&path, b"{ damaged").unwrap();
        assert!(matches!(read_ring(&path, now), Err(RingError::Damaged(_))));
        std::fs::write(&path, vec![b' '; ring::MAX_RING_FILE as usize + 1]).unwrap();
        assert!(matches!(read_ring(&path, now), Err(RingError::TooLarge(_))));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(
            matches!(read_ring(&path, now), Err(RingError::Damaged(_))),
            "a folder where the file should be"
        );
    }
}
