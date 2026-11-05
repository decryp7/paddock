//! Shapes of a real diagnostic bundle (thousands of files, UTF-16 without a byte-order mark, mixed code pages, stray
//! binary bytes, stackless multi-line records, a circular log), read by the streaming import. Every file here is
//! invented; none is a user's data.

use debrief_core::batch::{PendingBatch, SourceId};
use debrief_core::ingest::{self, ImportEnv, NewSource, Outcome, SharedSink, SourceReport};
use debrief_core::Profile;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Counts events and the most file handles open at the time of a write.
#[derive(Default)]
struct Count {
    events: AtomicU64,
    sources: AtomicU64,
    max_fds: AtomicUsize,
    reports: std::sync::Mutex<Vec<SourceReport>>,
}

fn open_fds() -> usize {
    std::fs::read_dir("/dev/fd").map_or(0, |d| d.count())
}

impl SharedSink for Count {
    fn reserve_sources(&self, n: u32) -> anyhow::Result<SourceId> {
        Ok(SourceId(self.sources.fetch_add(n as u64, Ordering::Relaxed) as u32))
    }
    fn begin_source(&self, _s: &NewSource, id: Option<SourceId>) -> anyhow::Result<SourceId> {
        Ok(id.unwrap_or_else(|| SourceId(self.sources.fetch_add(1, Ordering::Relaxed) as u32)))
    }
    fn write(&self, b: PendingBatch) -> anyhow::Result<()> {
        self.events.fetch_add(b.len() as u64, Ordering::Relaxed);
        self.max_fds.fetch_max(open_fds(), Ordering::Relaxed);
        Ok(())
    }
    fn finish_source(&self, r: &SourceReport) -> anyhow::Result<()> {
        self.reports.lock().unwrap().push(r.clone());
        Ok(())
    }
    fn abort_source(&self, _id: SourceId) -> anyhow::Result<()> {
        Ok(())
    }
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("debrief-stream-bundle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn import_dir(root: &Path, threads: usize) -> (Count, Vec<Outcome>) {
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
    let canonical = vec![std::fs::canonicalize(root).unwrap()];
    let files = ingest::files_to_read(&canonical, &canonical, &profile, &Default::default(), &Default::default());
    let sink = Count::default();
    let opts = ingest::ParallelOptions {
        threads,
        ..Default::default()
    };
    let out = ingest::import_files_parallel(&env, &files, &Default::default(), &progress, &opts, &sink);
    (sink, out)
}

fn line(n: u64) -> String {
    format!("2026-10-14 10:{:02}:{:02} [INFO] request {n} handled", (n / 60) % 60, n % 60)
}

fn failures(out: &[Outcome]) -> Vec<String> {
    out.iter()
        .filter_map(|o| match o {
            Outcome::Unrecognized(u) => Some(format!("{}: {}", u.name, u.reason)),
            _ => None,
        })
        .collect()
}

#[test]
fn utf16_without_a_bom_decodes_across_every_read_boundary() {
    // About 9 MiB, so the 8 MiB read ends inside the file; every line holds a character outside the BMP (a surrogate pair).
    for pad in 0..4usize {
        let d = dir(&format!("u16-{pad}"));
        let mut units: Vec<u16> = Vec::new();
        let mut n = 0u64;
        while units.len() * 2 < (9 << 20) {
            let text = format!("{} \u{1F600} caf\u{e9} {}\n", line(n), "x".repeat(pad));
            units.extend(text.encode_utf16());
            n += 1;
        }
        let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        std::fs::write(d.join("u16.log"), &bytes).unwrap();
        let (c, out) = import_dir(&d, 2);
        assert!(failures(&out).is_empty(), "{:?}", failures(&out));
        assert_eq!(c.events.load(Ordering::Relaxed), n, "pad {pad}");
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn code_pages_and_stray_bytes_after_a_clean_head_do_not_fail_the_file() {
    let d = dir("pages");
    let mut bytes = Vec::new();
    let mut n = 0u64;
    // A clean ASCII head larger than the plan's 1 MiB, then lines the head said nothing about.
    while bytes.len() < (2 << 20) {
        bytes.extend_from_slice(line(n).as_bytes());
        bytes.push(b'\n');
        n += 1;
    }
    for k in 0..200 {
        bytes.extend_from_slice(line(n).as_bytes());
        match k % 4 {
            0 => bytes.extend_from_slice(b" caf\xE9 (cp1252)"),
            1 => bytes.extend_from_slice(b" \x83\x8D\x83\x4F (cp932)"),
            2 => bytes.extend_from_slice(b" stray \x00\x01\xFF\xFE bytes"),
            _ => bytes.extend_from_slice(b" \xC3 truncated utf-8"),
        }
        bytes.push(b'\n');
        n += 1;
    }
    std::fs::write(d.join("mixed.log"), &bytes).unwrap();
    let (c, out) = import_dir(&d, 2);
    assert!(failures(&out).is_empty(), "{:?}", failures(&out));
    assert_eq!(c.events.load(Ordering::Relaxed), n);
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn stackless_multi_line_records_hold_across_the_read_boundary() {
    // Records of a stamped line and unstamped continuation lines, laid so the 8 MiB read ends inside one.
    for shift in 0..8usize {
        let d = dir(&format!("multi{shift}"));
        let mut text = String::new();
        let mut events = 0u64;
        while text.len() < (8 << 20) - 150 - shift {
            text.push_str(&line(events));
            text.push('\n');
            events += 1;
        }
        for k in 0..6 {
            text.push_str(&line(events + k));
            text.push_str("\n    at Some.Frame(File.cs:1)\n    at Other.Frame(File.cs:2)\nCaused by: invented\n");
        }
        events += 6;
        std::fs::write(d.join("multi.log"), &text).unwrap();
        let (c, out) = import_dir(&d, 2);
        assert!(failures(&out).is_empty(), "{:?}", failures(&out));
        assert_eq!(
            c.events.load(Ordering::Relaxed),
            events,
            "shift {shift}: continuation lines must not become events"
        );
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn a_circular_log_out_of_time_order_imports_every_line() {
    let d = dir("circular");
    // The newest lines are in the middle: 300 old lines after the wrap point, 500 new ones before it.
    let mut text = String::new();
    for n in 500..800 {
        text.push_str(&line(n));
        text.push('\n');
    }
    for n in 0..500 {
        text.push_str(&line(n + 1000));
        text.push('\n');
    }
    for n in 800..1000 {
        text.push_str(&line(n));
        text.push('\n');
    }
    std::fs::write(d.join("circular.log"), &text).unwrap();
    let (c, out) = import_dir(&d, 2);
    assert!(failures(&out).is_empty(), "{:?}", failures(&out));
    assert_eq!(c.events.load(Ordering::Relaxed), 1000);
    std::fs::remove_dir_all(d).ok();
}

#[cfg(unix)]
#[test]
fn thousands_of_small_files_import_on_a_bounded_pool_with_bounded_handles() {
    let d = dir("many");
    let files = 3000u64;
    for i in 0..files {
        let sub = d.join(format!("d{:02}", i % 30));
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(format!("f{i:05}.log")), format!("{}\n{}\n", line(i), line(i + 1))).unwrap();
    }
    let threads = 4;
    let before = open_fds();
    let (c, out) = import_dir(&d, threads);
    assert!(failures(&out).is_empty(), "{:?}", failures(&out));
    assert_eq!(c.events.load(Ordering::Relaxed), files * 2);
    let most = c.max_fds.load(Ordering::Relaxed);
    assert!(
        most <= before + threads * 8 + 16,
        "{most} handles open at once for {threads} threads (before: {before})"
    );
    std::fs::remove_dir_all(d).ok();
}
