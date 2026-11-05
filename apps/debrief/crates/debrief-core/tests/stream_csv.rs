//! Streaming import of delimited text (plan package 3.5): the cases a chunked read can get wrong, through the
//! public import path (plan, chunk reader, row folding, batcher). Data is invented and written to a temp folder.

mod common;

use debrief_core::batch::{PendingBatch, SourceId};
use debrief_core::ingest::{self, BatchSink, ImportEnv, NewSource, Outcome, SourceReport};
use debrief_core::Profile;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Counts what the import writes; cancels after `cancel_after` events when set.
struct Count<'a> {
    events: usize,
    sources: u32,
    reports: Vec<SourceReport>,
    cancel_after: Option<(usize, &'a std::sync::atomic::AtomicBool)>,
}

impl BatchSink for Count<'_> {
    fn begin_source(&mut self, _s: &NewSource) -> anyhow::Result<SourceId> {
        self.sources += 1;
        Ok(SourceId(self.sources))
    }
    fn write(&mut self, b: PendingBatch) -> anyhow::Result<()> {
        self.events += b.len();
        if let Some((n, flag)) = self.cancel_after {
            if self.events >= n {
                flag.store(true, Ordering::Relaxed);
            }
        }
        Ok(())
    }
    fn finish_source(&mut self, r: &SourceReport) -> anyhow::Result<()> {
        self.reports.push(r.clone());
        Ok(())
    }
    fn abort_source(&mut self, _id: SourceId) -> anyhow::Result<()> {
        Ok(())
    }
}

fn import(path: &Path, cancel_after: Option<usize>) -> (Count<'static>, Vec<Outcome>) {
    import_with(&Profile::default_profile(), path, cancel_after)
}

fn import_with(profile: &Profile, path: &Path, cancel_after: Option<usize>) -> (Count<'static>, Vec<Outcome>) {
    let registry = debrief_core::PluginRegistry::new();
    let parsers = registry
        .compile_parsers(profile, &debrief_core::parser::ParserSetup::builtins())
        .unwrap();
    let profile = parsers.base().clone();
    let progress = Box::leak(Box::new(debrief_core::workspace::ImportProgress::default()));
    let env = ImportEnv {
        encodings: &Default::default(),
        importers: registry.importers(),
        parsers: &parsers,
        schema: Arc::new(ingest::schema_for(&profile)),
        cancel: progress.cancel_flag(),
        window: None,
    };
    let canonical = vec![std::fs::canonicalize(path).unwrap()];
    let files = ingest::files_to_read(&canonical, &canonical, &profile, &Default::default(), &Default::default());
    let mut sink = Count {
        events: 0,
        sources: 0,
        reports: Vec::new(),
        cancel_after: cancel_after.map(|n| (n, progress.cancel_flag())),
    };
    let out = ingest::import_files(&env, &files, &Default::default(), progress, &mut sink);
    (sink, out)
}

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("debrief-stream-csv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn row(n: u64) -> String {
    format!(
        "2026-10-14 10:{:02}:{:02},INFO,svc{},request {n} handled\n",
        (n / 60) % 60,
        n % 60,
        n % 7
    )
}

#[test]
fn a_byte_order_mark_and_crlf_line_ends_give_the_same_rows() {
    let d = dir("bom");
    let mut text = String::from("timestamp,level,service,message\r\n");
    for n in 0..50 {
        text.push_str(&row(n).replace('\n', "\r\n"));
    }
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(d.join("a.csv"), &bytes).unwrap();
    std::fs::write(d.join("b.csv"), text.as_bytes()).unwrap();
    let (a, _) = import(&d.join("a.csv"), None);
    let (b, _) = import(&d.join("b.csv"), None);
    assert_eq!((a.events, b.events), (50, 50));
    assert_eq!(a.reports[0].events, 50);
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn a_truncated_last_row_is_still_a_row_and_an_empty_file_is_refused() {
    let d = dir("trunc");
    let mut text = String::from("timestamp,level,service,message\n");
    for n in 0..9 {
        text.push_str(&row(n));
    }
    text.push_str("2026-10-14 11:00:00,INFO,svc,cut off in the mid");
    std::fs::write(d.join("a.csv"), &text).unwrap();
    std::fs::write(d.join("empty.csv"), b"").unwrap();
    let (a, _) = import(&d.join("a.csv"), None);
    assert_eq!(a.events, 10);
    let (e, out) = import(&d.join("empty.csv"), None);
    assert_eq!(e.events, 0);
    assert!(matches!(&out[0], Outcome::Unrecognized(u) if u.reason.contains("empty")), "{out:?}");
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn a_quoted_line_break_and_a_multi_byte_character_cross_the_read_boundary_of_the_real_reader() {
    // The reader takes 8 MiB at a time: put a quoted field with a line break and an `é` right on the boundary.
    for shift in 0..6u64 {
        let d = dir(&format!("edge{shift}"));
        let mut text = String::from("timestamp,level,service,message\n");
        let mut n = 0;
        let target = (8usize << 20) - 20 - shift as usize;
        while text.len() < target {
            text.push_str(&row(n));
            n += 1;
        }
        let rows_before = n;
        text.push_str("2026-10-14 12:00:00,INFO,svc,\"quoted é\nsecond line, with a comma\"\n");
        for k in 0..5 {
            text.push_str(&row(n + k));
        }
        std::fs::write(d.join("e.csv"), &text).unwrap();
        let (c, _) = import(&d.join("e.csv"), None);
        assert_eq!(c.events as u64, rows_before + 1 + 5, "shift {shift}");
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn cancel_in_the_middle_of_a_file_stops_it_and_imports_nothing_of_it() {
    let d = dir("cancel");
    let mut text = String::from("timestamp,level,service,message\n");
    for n in 0..400_000 {
        text.push_str(&row(n));
    }
    std::fs::write(d.join("c.csv"), &text).unwrap();
    let (c, out) = import(&d.join("c.csv"), Some(10_000));
    assert!(c.events < 400_000, "read {} of 400000 after the cancel", c.events);
    assert!(c.reports.is_empty());
    assert!(matches!(&out[0], Outcome::Unrecognized(_)), "{out:?}");
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn the_day_order_is_decided_on_the_first_rows_even_when_they_are_long() {
    // About 1.5 KB a row: the 1 MiB head holds only some 700 of them, and the first row that can only be a day (25/04) is past it.
    let d = dir("longrows");
    let pad = "p".repeat(1500);
    let mut text = String::from("timestamp,level,service,message\n");
    text.push_str(&format!("03/04/2026 10:00:00,INFO,svc,{pad}\n"));
    for n in 0..1000 {
        text.push_str(&format!("{:02}/04/2026 10:30:00,INFO,svc,{pad}\n", 5 + n % 7));
    }
    text.push_str(&format!("25/04/2026 11:00:00,INFO,svc,{pad}\n"));
    std::fs::write(d.join("a.csv"), &text).unwrap();
    let (c, _) = import(&d.join("a.csv"), None);
    let (from, to) = (c.reports[0].from.unwrap(), c.reports[0].to.unwrap());
    // 3 April 10:00 to 25 April 11:00; read month first, 03/04 would be 4 March and 25/04 would not be a date.
    assert_eq!(to - from, (22 * 24 * 3600 + 3600) * 1000);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_file_with_a_byte_order_mark_and_no_header_reads_its_first_row() {
    let d = dir("bom-headerless");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    for n in 0..30 {
        bytes.extend_from_slice(row(n).as_bytes());
    }
    std::fs::write(d.join("a.csv"), &bytes).unwrap();
    let (c, _) = import(&d.join("a.csv"), None);
    assert_eq!(c.events, 30);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_48_mib_csv_is_read_in_a_bounded_amount_of_memory() {
    // The whole-file path held about 11 times the file (3.5 GB for 300 MiB); streaming holds a fixed amount.
    let d = dir("memory");
    let mut text = String::from("timestamp,level,service,message\n");
    let mut n = 0u64;
    while text.len() < 48 << 20 {
        text.push_str(&row(n));
        n += 1;
    }
    std::fs::write(d.join("a.csv"), &text).unwrap();
    drop(text);
    let base = common::rss() >> 20;
    let peak = common::Peak::start();
    let (c, _) = import(&d.join("a.csv"), None);
    let mb = peak.mb();
    eprintln!("48 MiB CSV: physical footprint {base} MB before, peak {mb} MB");
    assert_eq!(c.events as u64, n);
    assert!(mb.saturating_sub(base) < 160, "peak {mb} MB over {base} MB for a 48 MiB file");
    std::fs::remove_dir_all(&d).ok();
}
