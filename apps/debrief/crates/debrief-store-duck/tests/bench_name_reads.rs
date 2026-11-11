//! Measurement (docs/performance.md, rule 48): the reads the name rebuild makes for a look-alike name, by how often the
//! name occurs, on a large store. A read is the first 30 events that hold the name, by a substring search or by a phrase
//! search through the full-text index (`debrief_core::names` `ReadMode`). Off by default.
//!
//! `DEBRIEF_NAME_BENCH_EVENTS=<n>` (10000000 for the 10M-event store) builds the store, or reuses the one in
//! `DEBRIEF_NAME_BENCH_STORE=<folder>` (made there when it is not): the samples repeated to n events with invented names
//! of 5, 30, 300, 3,000 and 30,000 events spread over all of it (one file per copy of the samples).
//!
//! Run with `--release --test bench_name_reads -- --ignored --nocapture`.

mod common;

// The store worker's allocator (`debrief-store-worker`), so the numbers are the shipped ones.
#[global_allocator]
static GLOBAL: debrief_alloc::MiMalloc = debrief_alloc::MiMalloc;

use common::TempDir;
use debrief_core::ports::{Filter, Ingest, PageQuery, Query};
use debrief_core::query::TextMode;
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The invented names and how many events hold each. The names' words occur nowhere else.
const NAMES: [(&str, u64); 5] = [
    ("zorbquxfive.exe", 5),
    ("zorbquxthirty.exe", 30),
    ("zorbquxthreehundred.exe", 300),
    ("zorbquxthreethousand.exe", 3_000),
    ("zorbquxthirtythousand.exe", 30_000),
];

/// Writes each name's events into the copies of the corpus, spread evenly over them, as CLEF.
fn spread_names(corpus: &std::path::Path) {
    let mut copies: Vec<PathBuf> = std::fs::read_dir(corpus)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    copies.sort();
    for (k, copy) in copies.iter().enumerate() {
        let mut w = std::io::BufWriter::new(std::fs::File::create(copy.join("names-bench.clef")).unwrap());
        for (name, total) in NAMES {
            // Copy k gets the events i (of `total`) whose slot i * copies / total is k.
            let n = copies.len() as u64;
            let mine = (0..total).filter(|i| i * n / total == k as u64).count();
            for j in 0..mine {
                let t = 1_700_000_000_000i64 + (k as i64) * 86_400_000 + j as i64 * 1_000;
                let ts = chrono::DateTime::from_timestamp_millis(t).unwrap().format("%Y-%m-%dT%H:%M:%S%.3fZ");
                writeln!(w, r#"{{"@t":"{ts}","@l":"Information","@m":"Started worker {name} session {j}"}}"#).unwrap();
            }
        }
    }
}

fn try_read(store: &DuckStore, filter: Filter) -> Result<(Duration, usize), String> {
    let started = Instant::now();
    let page = store
        .page(&PageQuery {
            query: filter,
            offset: 0,
            limit: 30,
            ..PageQuery::default()
        })
        .map_err(|e| e.to_string())?;
    Ok((started.elapsed(), page.rows.len()))
}

fn read(store: &DuckStore, filter: Filter) -> (Duration, usize) {
    try_read(store, filter).unwrap()
}

fn substring(name: &str) -> Filter {
    Filter {
        text: name.into(),
        text_mode: TextMode::Substring,
        ..Filter::default()
    }
}

fn phrase(name: &str) -> Filter {
    Filter {
        text: format!("\"{name}\""),
        text_mode: TextMode::Words,
        ..Filter::default()
    }
}

#[test]
#[ignore = "measurement: DEBRIEF_NAME_BENCH_EVENTS=<n> builds a large store; run with --ignored --nocapture"]
fn name_reads_by_how_often_the_name_occurs() {
    debrief_alloc::tune();
    let events: u64 = std::env::var("DEBRIEF_NAME_BENCH_EVENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let kept = std::env::var("DEBRIEF_NAME_BENCH_STORE").ok().map(PathBuf::from);
    assert!(
        events > 0 || kept.is_some(),
        "set DEBRIEF_NAME_BENCH_EVENTS=<n> or DEBRIEF_NAME_BENCH_STORE=<folder>"
    );
    let scratch = TempDir::new("name-reads");
    let store_dir = kept.clone().unwrap_or_else(|| scratch.0.join("store"));
    let store = if store_dir.join(debrief_store_duck::DB_FILE).is_file() {
        DuckStore::open(&store_dir, Profile::default_profile(), PluginRegistry::new()).unwrap()
    } else {
        let corpus = kept
            .as_ref()
            .map_or_else(|| scratch.0.join("corpus"), |k| k.with_extension("corpus"));
        let corpus = common::corpus::corpus(&corpus, events);
        spread_names(&corpus);
        let store = DuckStore::create(&store_dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
        let started = Instant::now();
        let report = store.open(&[corpus]).unwrap().wait().unwrap();
        eprintln!("imported {} events in {:.1} s", report.events, started.elapsed().as_secs_f64());
        store
    };
    let index = store.wait_for_index(Duration::from_secs(7200));
    eprintln!("full-text index: {index:?}");
    assert!(
        store.wait_for_catalogue(Duration::from_secs(7200)).unwrap(),
        "the catalogue was not built"
    );
    eprintln!("{} events", store.summary().unwrap().total);

    // One read of each kind per name, three runs each (the first warms the file cache).
    for (name, total) in NAMES {
        for (label, filter) in [("substring", substring(name)), ("phrase", phrase(name))] {
            let runs: Vec<(Duration, usize)> = (0..3).map(|_| read(&store, filter.clone())).collect();
            let ms: Vec<u128> = runs.iter().map(|r| r.0.as_millis()).collect();
            eprintln!("NAMEREAD name_events={total} mode={label} rows={} ms={ms:?}", runs[0].1);
        }
    }

    // A rare name that occurs only in one day: the scan bounded by the name's own time range (what the catalogue knows).
    let summary = store.summary().unwrap();
    let (from, to) = (summary.from.unwrap_or(0), summary.to.unwrap_or(0));
    let mid = (from + to) / 2;
    let local = "zorbquxlocalday.exe";
    if read(&store, substring(local)).1 == 0 {
        let extra = scratch.0.join("local");
        std::fs::create_dir_all(&extra).unwrap();
        let mut w = std::io::BufWriter::new(std::fs::File::create(extra.join("local-day.clef")).unwrap());
        for j in 0..5i64 {
            let ts = chrono::DateTime::from_timestamp_millis(mid + j * 60_000)
                .unwrap()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ");
            writeln!(w, r#"{{"@t":"{ts}","@l":"Information","@m":"Started worker {local} session {j}"}}"#).unwrap();
        }
        drop(w);
        store.open(&[extra]).unwrap().wait().unwrap();
        store.wait_for_index(Duration::from_secs(3600)).unwrap();
    }
    let bounded = Filter {
        from: Some(mid),
        to: Some(mid + 4 * 60_000),
        ..substring(local)
    };
    for (label, filter) in [("substring", substring(local)), ("substring bounded to its time range", bounded)] {
        let runs: Vec<(Duration, usize)> = (0..3).map(|_| read(&store, filter.clone())).collect();
        let ms: Vec<u128> = runs.iter().map(|r| r.0.as_millis()).collect();
        eprintln!("NAMEREAD name_events=5 span=4min mode={label} rows={} ms={ms:?}", runs[0].1);
    }

    // Reads side by side: 16 substring reads of the 3,000-event name, on 1, 2, 4 and 8 threads.
    for threads in [1usize, 2, 4, 8] {
        let started = Instant::now();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let failures = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    while next.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 16 {
                        if let Err(e) = try_read(&store, substring(NAMES[3].0)) {
                            failures.lock().unwrap().push(e);
                        }
                    }
                });
            }
        });
        let failures = failures.into_inner().unwrap();
        eprintln!(
            "NAMEREAD_PAR threads={threads} reads=16 wall_ms={} failed={} first_error={:?}",
            started.elapsed().as_millis(),
            failures.len(),
            failures.first()
        );
    }
}
