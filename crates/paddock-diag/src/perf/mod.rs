//! The "Performance" section of the diagnostics bundle (`performance.json` and `performance-summary.txt`): how long the start
//! took by phase, the slowest recent operations and what machine this ran on. It holds names, durations and hardware facts,
//! never user content: names are identifiers ([`ring::valid_name`]), anything else becomes `invalid-name`.
//!
//! An app does three things:
//!
//! 1. at start, [`init`] with its data folder (reads the ring the last run left, so a crash does not lose it) and
//!    [`startup_phase`] after each step of the start (`logging`, `settings`, `window-ready`);
//! 2. around work a person waits for (an import, a query, a page load, a job), [`record`] or [`record_phase`] with the
//!    duration, or [`time`] for a guard that records on drop;
//! 3. add [`PerformanceSection`] to the report's sections, and call [`flush`] now and then (the section flushes before it reads).
//!
//! Cost: [`record`] takes no lock for an operation too short for the full ring (one atomic read), and the ring holds at most
//! [`RING_CAPACITY`] entries of 24 hours, so it stays a few KB in memory and in its file (`performance-ring.json` in the data
//! folder, atomically replaced; a damaged file is replaced at the next flush and never stops the app). The format follows the
//! startup and summary views of VS Code ("Startup Performance"), Chrome's trace summary and Windows Performance Recorder: a
//! list of phases and a list of the slowest things, no tracing.
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
use ring::{Phase, Ring, RingError, Slow, Startup};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
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
/// [`Ring::floor_ms`], readable without the lock.
static FLOOR: AtomicU64 = AtomicU64::new(0);

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        let now = Instant::now();
        Mutex::new(State {
            ring: Ring::new(),
            startup: Startup::new(),
            path: None,
            dirty: false,
            began: now,
            mark: now,
        })
    })
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

/// Names the data folder, reads the ring the last run left there and merges it in. Call once, early.
pub fn init(data_dir: &Path) -> Loaded {
    let path = data_dir.join(RING_FILE);
    let now = now_ms();
    let outcome = read_ring(&path, now);
    let mut st = lock();
    st.path = Some(path);
    let loaded = match outcome {
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
    };
    FLOOR.store(st.ring.floor_ms(), Ordering::Relaxed);
    loaded
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

/// Records that `op` took `duration` (as one piece, phase [`DEFAULT_PHASE`]).
pub fn record(op: &str, duration: Duration) {
    record_phase(op, DEFAULT_PHASE, duration);
}

/// Records that the `phase` of `op` took `duration`. Cheap: a duration too short for the full ring returns after one atomic read.
pub fn record_phase(op: &str, phase: &str, duration: Duration) {
    let ms = millis(duration);
    if ms <= FLOOR.load(Ordering::Relaxed) {
        return;
    }
    let now = now_ms();
    let mut st = lock();
    if st.ring.insert(Slow::new(op, phase, ms, now), now) {
        st.dirty = true;
    }
    FLOOR.store(st.ring.floor_ms(), Ordering::Relaxed);
}

/// Ends the startup phase `name`: its duration runs from the previous phase's end (or from the start).
pub fn startup_phase(name: &str) {
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
    op: String,
    phase: String,
    since: Instant,
}

/// Times `op` until the returned guard is dropped.
pub fn time(op: &str) -> Timer {
    time_since(op, Instant::now())
}

/// Times `op` from `since` (work that began before the guard could be made) until the returned guard is dropped.
pub fn time_since(op: &str, since: Instant) -> Timer {
    Timer {
        op: op.into(),
        phase: DEFAULT_PHASE.into(),
        since,
    }
}

impl Timer {
    /// Names the phase this guard times.
    pub fn phase(mut self, phase: &str) -> Self {
        self.phase = phase.into();
        self
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        record_phase(&self.op, &self.phase, self.since.elapsed());
    }
}

/// Writes the ring to its file when it changed (atomically: a temporary file, then a rename). Does nothing before [`init`].
pub fn flush() -> std::io::Result<()> {
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
    let tmp = path.with_extension("json.tmp");
    let written = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &path));
    if written.is_err() {
        lock().dirty = true;
        let _ = std::fs::remove_file(&tmp);
    }
    written
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
                memory_total_bytes: 1 << 30,
                memory_free_bytes: 1 << 29,
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
