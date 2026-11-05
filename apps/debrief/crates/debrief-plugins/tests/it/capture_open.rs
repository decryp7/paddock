//! MEM-1: what opening a large capture costs the app process (the host of the profiling plugin), measured as the
//! process' peak resident memory. `app_holds_the_file` is how the app opened a capture before (read the file, copy it to
//! hand it to a helper thread, lend the bytes); `file_is_lent_by_ranges` is how it does now (the importer gets the path
//! and the plugin's process reads it by ranges). Each runs in a process of its own (nextest), so the peaks do not
//! mix. Ignored: measurements, run with `--ignored --nocapture`; `DEBRIEF_CAPTURE_MB` sets the file's size (default 128).

use crate::common;
use debrief_profile::{CaptureImporter, Limits};
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Peak resident memory of this process so far, in MB.
fn peak_mb() -> u64 {
    let mut u = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage fills the struct it is given; RUSAGE_SELF is always valid.
    let u = unsafe {
        libc::getrusage(libc::RUSAGE_SELF, u.as_mut_ptr());
        u.assume_init()
    };
    // macOS reports bytes, Linux kilobytes.
    let raw = u.ru_maxrss as u64;
    (if cfg!(target_os = "macos") { raw } else { raw * 1024 }) >> 20
}

/// A folded-stacks file of about `mb` megabytes of invented stacks, written line by line.
fn folded(dir: &std::path::Path, mb: u64) -> std::path::PathBuf {
    let path = dir.join("big.folded");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    let mut written = 0u64;
    let mut n = 0u64;
    while written < mb << 20 {
        let line = format!("main;service{};handler{};work{} {}\n", n % 40, n % 997, n % 5, 1 + n % 7);
        out.write_all(line.as_bytes()).unwrap();
        written += line.len() as u64;
        n += 1;
    }
    out.flush().unwrap();
    path
}

fn importer() -> Arc<dyn CaptureImporter> {
    let registry = common::registry(&[common::PROFILING]);
    let service = registry
        .contributions::<dyn CaptureImporter>(debrief_profile::IMPORTERS_POINT)
        .into_iter()
        .find(|s| s.call(|i| i.id().to_string()).ok().as_deref() == Some("folded"))
        .expect("the profiling plugin contributes the folded importer");
    // The guarded service is only a handle here; the import below runs on this thread through it.
    let _ = &service;
    struct Via(debrief_core::plugin::services::Service<dyn CaptureImporter>);
    impl CaptureImporter for Via {
        fn id(&self) -> &str {
            "via"
        }
        fn name(&self) -> &str {
            "via"
        }
        fn sniff(&self, n: &str, h: &[u8]) -> u8 {
            self.0.call(|i| i.sniff(n, h)).unwrap_or(0)
        }
        fn import(&self, n: &str, b: &[u8], l: &Limits) -> debrief_profile::Result<debrief_profile::Capture> {
            let (n, b, l) = (n.to_string(), b.to_vec(), *l);
            self.0
                .call_detached_within(Duration::from_secs(900), move |i| i.import(&n, &b, &l))
                .unwrap()
        }
        fn import_file(&self, n: &str, p: &std::path::Path, l: &Limits) -> debrief_profile::Result<debrief_profile::Capture> {
            let (n, p, l) = (n.to_string(), p.to_path_buf(), *l);
            self.0
                .call_detached_within(Duration::from_secs(900), move |i| i.import_file(&n, &p, &l))
                .unwrap()
        }
    }
    Arc::new(Via(service))
}

fn size() -> u64 {
    std::env::var("DEBRIEF_CAPTURE_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(128)
}

#[test]
#[ignore = "measurement: run with --ignored --nocapture (one test per process)"]
fn app_holds_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = folded(dir.path(), size());
    let importer = importer();
    let before = peak_mb();
    let started = Instant::now();
    // The old flow: the app read the file, copied it for the helper thread, and lent the copy.
    let bytes = std::fs::read(&path).unwrap();
    let capture = importer.import("big.folded", &bytes, &Limits::default());
    drop(bytes);
    eprintln!(
        "OPEN old flow: {} MB file, {:.1} s, peak {} MB (before {} MB), ok={}",
        size(),
        started.elapsed().as_secs_f64(),
        peak_mb(),
        before,
        capture.is_ok()
    );
    assert!(capture.is_ok(), "{:?}", capture.err());
}

#[test]
#[ignore = "measurement: run with --ignored --nocapture (one test per process)"]
fn file_is_lent_by_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let path = folded(dir.path(), size());
    let importer = importer();
    let before = peak_mb();
    let started = Instant::now();
    let capture = importer.import_file("big.folded", &path, &Limits::default());
    eprintln!(
        "OPEN lent by ranges: {} MB file, {:.1} s, peak {} MB (before {} MB), ok={}",
        size(),
        started.elapsed().as_secs_f64(),
        peak_mb(),
        before,
        capture.is_ok()
    );
    assert!(capture.is_ok(), "{:?}", capture.err());
}

/// MEM-2: many large captures open in one window, with and without the app-wide budget. `CAPTURES` files of `MB`
/// megabytes (invented folded stacks) are opened one after the other in a store; the peak resident memory of this process
/// is what matters. Without a budget every capture stays in memory; within one, the least recently used ones are released
/// (and read again when a view asks). One test per process (nextest), so the peaks do not mix. Ignored: measurements.
/// `DEBRIEF_CAPTURES` (default 16) and `DEBRIEF_CAPTURE_MB` (default 24) set the shape; `DEBRIEF_BUDGET_MB` (default
/// 512) the budget.
fn open_many(budget: Option<u64>) {
    let (count, mb) = (
        std::env::var("DEBRIEF_CAPTURES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(16),
        std::env::var("DEBRIEF_CAPTURE_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(24),
    );
    let dir = tempfile::tempdir().unwrap();
    let one = folded(dir.path(), mb);
    let paths: Vec<std::path::PathBuf> = (0..count)
        .map(|i| {
            let p = dir.path().join(format!("capture-{i}.folded"));
            std::fs::copy(&one, &p).unwrap();
            p
        })
        .collect();
    let importers: Vec<Arc<dyn CaptureImporter>> = vec![Arc::new(debrief_plugin_profiling::folded::FoldedImporter)];
    let mut store = debrief_profile::Captures::new(importers, Limits::default());
    let tracker = budget.map(debrief_budget::MemoryBudget::new);
    if let Some(b) = &tracker {
        store = store.with_budget(b.clone());
    }
    let before = peak_mb();
    let started = Instant::now();
    let mut opened = 0;
    let mut refused = None;
    for p in &paths {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        use debrief_profile::CaptureCatalog;
        match store.open(&name, Some(&p.to_string_lossy()), &std::fs::read(p).unwrap()) {
            Ok(_) => opened += 1,
            Err(e) => {
                refused = Some(e.to_string());
                break;
            }
        }
    }
    let resident = tracker.as_ref().map_or(0, |b| b.used() >> 20);
    eprintln!(
        "MANY budget={:?} MB: {count} captures of {mb} MB, opened {opened} in {:.1} s, peak {} MB (before {before} MB), budget holds {resident} MB, refused={:?}",
        budget.map(|b| b >> 20),
        started.elapsed().as_secs_f64(),
        peak_mb(),
        refused.is_some(),
    );
    assert_eq!(opened, count, "{refused:?}");
}

#[test]
#[ignore = "measurement: run with --ignored --nocapture (one test per process)"]
fn many_captures_without_a_budget() {
    open_many(None);
}

#[test]
#[ignore = "measurement: run with --ignored --nocapture (one test per process)"]
fn many_captures_within_a_budget() {
    open_many(Some(
        std::env::var("DEBRIEF_BUDGET_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(512)
            << 20,
    ));
}
