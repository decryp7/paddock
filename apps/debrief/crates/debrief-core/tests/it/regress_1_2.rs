//! Debrief 1.2 regression tests, engine side (docs/plans/debrief-1-2.md, wave 0 package 0.3; ADR 0018, ADR 0051).
//!
//! Each test is written to fail today and is `#[ignore]`d with the failure it shows, until the work package that fixes
//! it removes the marker (3.5 for the CSV, which streams). The inputs are generated at test time into a temp folder
//! (invented rows, never committed, never real data) and are big, so the tests only run when asked for:
//! `cargo test --release -p debrief-core --test it regress_1_2:: -- --ignored` (the nightly/bench workflow does that).

use debrief_core::batch::{PendingBatch, SourceId};
use debrief_core::ingest::{self, BatchSink, ImportEnv, NewSource, SourceReport};
use debrief_core::Profile;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
/// The import budget of the store worker (ADR 0014 section 2): whatever is imported, resident memory stays below it.
const IMPORT_BUDGET_MB: u64 = 768;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> TempDir {
        let d = std::env::temp_dir().join(format!("debrief-regress-1-2-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TempDir(d)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Resident memory of this process in bytes (0 when the OS cannot tell).
fn rss() -> u64 {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_memory(),
    );
    sys.process(pid).map_or(0, |p| p.memory())
}

/// Polls the process's resident memory while a test runs and keeps the peak.
struct Peak {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}
impl Peak {
    fn start() -> Peak {
        let peak = Arc::new(AtomicU64::new(rss()));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (peak.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                p.fetch_max(rss(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        Peak { peak, stop }
    }
    fn mb(&self) -> u64 {
        self.peak.fetch_max(rss(), Ordering::Relaxed);
        self.peak.load(Ordering::Relaxed) >> 20
    }
}
impl Drop for Peak {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A CSV of invented rows (`timestamp,level,service,message`) of at least `bytes` bytes.
fn generate_csv(path: &Path, bytes: u64) -> u64 {
    let mut out = BufWriter::with_capacity(1 << 20, std::fs::File::create(path).unwrap());
    out.write_all(b"timestamp,level,service,message\n").unwrap();
    let (mut written, mut n) = (0u64, 0u64);
    while written < bytes {
        let (s, m) = ((n / 1000) % 60, (n / 60_000) % 60);
        let level = ["INFO", "INFO", "INFO", "WARN", "ERROR"][(n % 5) as usize];
        let line = format!(
            "2026-10-14 10:{m:02}:{s:02}.{:03},{level},svc{},request {n} handled for tenant {} in {} ms\n",
            n % 1000,
            n % 17,
            n % 301,
            n % 997
        );
        out.write_all(line.as_bytes()).unwrap();
        written += line.len() as u64;
        n += 1;
    }
    out.flush().unwrap();
    n
}

/// Imports `paths` into a sink that only counts (the reading side alone); returns the events counted.
fn parse_only(paths: &[PathBuf]) -> usize {
    struct Count(usize, u32);
    impl BatchSink for Count {
        fn begin_source(&mut self, _s: &NewSource) -> anyhow::Result<SourceId> {
            self.1 += 1;
            Ok(SourceId(self.1))
        }
        fn write(&mut self, b: PendingBatch) -> anyhow::Result<()> {
            self.0 += b.len();
            Ok(())
        }
        fn finish_source(&mut self, _r: &SourceReport) -> anyhow::Result<()> {
            Ok(())
        }
        fn abort_source(&mut self, _id: SourceId) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let registry = debrief_core::PluginRegistry::new();
    let parsers = registry
        .compile_parsers(&Profile::default_profile(), &debrief_core::parser::ParserSetup::builtins())
        .unwrap();
    let profile = parsers.base().clone();
    let progress = debrief_core::workspace::ImportProgress::default();
    let env = ImportEnv {
        encodings: &Default::default(),
        importers: registry.importers(),
        parsers: &parsers,
        schema: Arc::new(ingest::schema_for(&profile)),
        cancel: progress.cancel_flag(),
        window: None,
    };
    let canonical: Vec<PathBuf> = paths.iter().map(|p| std::fs::canonicalize(p).unwrap()).collect();
    let files = ingest::files_to_read(&canonical, &canonical, &profile, &Default::default(), &Default::default());
    let mut sink = Count(0, 0);
    ingest::import_files(&env, &files, &Default::default(), &progress, &mut sink);
    sink.0
}

#[test]
#[ignore = "regress-1-2: FAILING TODAY (fixed by plan package 3.5, streaming legacy text): peak RSS 17399 MB while importing a 3072 MB CSV is over the import budget of 768 MB"]
fn a_3_gib_csv_imports_under_the_worker_cap() {
    let dir = TempDir::new("csv");
    let path = dir.0.join("big.csv");
    let rows = generate_csv(&path, 3 * GIB);
    let peak = Peak::start();
    let events = parse_only(std::slice::from_ref(&path));
    let mb = peak.mb();
    assert_eq!(events as u64, rows, "every row of the CSV was imported");
    assert!(
        mb <= IMPORT_BUDGET_MB,
        "peak RSS {mb} MB while importing a {} MB CSV is over the import budget of {IMPORT_BUDGET_MB} MB",
        3 * GIB / MB
    );
}
