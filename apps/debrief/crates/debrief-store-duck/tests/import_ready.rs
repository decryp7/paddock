//! Time from "import folder" until search is ready, on a folder of logs (performance.md, "Import of a 7–8 GB folder"):
//!
//! `DEBRIEF_READY_CORPUS=<folder> DEBRIEF_READY_STORE=<new folder> cargo test --release --test import_ready -- --nocapture`
//!
//! Imports the folder into a new store and prints, from the start: the import (events, events/s, peak RSS), the full-text
//! index covering every segment and the catalogue built, and the store's size. Without `DEBRIEF_READY_CORPUS` it does
//! nothing. The store folder is removed afterwards unless `DEBRIEF_READY_KEEP` is set. With `DEBRIEF_READY_REINDEX=1` it
//! times only the index phase on a kept store (`DEBRIEF_READY_STORE`), built again from its segments.

mod common;

// The store worker's allocator (`debrief-store-worker`), so the numbers are the shipped ones.
#[global_allocator]
static GLOBAL: debrief_alloc::MiMalloc = debrief_alloc::MiMalloc;

use common::rss::Sampler;
use debrief_core::ports::{Ingest, Search, SearchRequest};
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// `DEBRIEF_READY_LOG=1`: the store's log (debug and up) on stderr with the seconds since the start, so phases show.
struct Log(Instant);

impl log::Log for Log {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Debug && m.target().starts_with("debrief")
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!(
                "[{:7.1} s] {} {}: {}",
                self.0.elapsed().as_secs_f64(),
                r.level(),
                r.target(),
                r.args()
            );
        }
    }
    fn flush(&self) {}
}

fn dir_mb(p: &Path) -> f64 {
    fn size(p: &Path) -> u64 {
        std::fs::read_dir(p).map_or(0, |rd| {
            rd.flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => size(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        })
    }
    size(p) as f64 / 1e6
}

#[test]
fn import_until_search_is_ready() {
    debrief_alloc::tune();
    let Some(corpus) = std::env::var_os("DEBRIEF_READY_CORPUS").map(PathBuf::from) else {
        eprintln!("skipped: set DEBRIEF_READY_CORPUS (a folder of logs) and DEBRIEF_READY_STORE (a new folder)");
        return;
    };
    let store_dir = PathBuf::from(std::env::var_os("DEBRIEF_READY_STORE").expect("DEBRIEF_READY_STORE: a new store folder"));
    let reindex = std::env::var_os("DEBRIEF_READY_REINDEX").is_some();
    let recatalogue = std::env::var_os("DEBRIEF_READY_RECATALOGUE").is_some();
    assert!(
        reindex || recatalogue || !store_dir.join("data.duckdb").exists(),
        "DEBRIEF_READY_STORE must be a new folder"
    );
    let sampler = Sampler::start();
    let started = Instant::now();
    if std::env::var_os("DEBRIEF_READY_LOG").is_some() {
        let _ = log::set_boxed_logger(Box::new(Log(started))).map(|()| log::set_max_level(log::LevelFilter::Debug));
    }
    let secs = || started.elapsed().as_secs_f64();
    // Knobs: `DEBRIEF_READY_THREADS` (parse threads), `DEBRIEF_READY_INDEX=threads,heap_mb` (the index writer's budget),
    // `DEBRIEF_READY_WAIT=import|index` (stop after the import or after the index; default: both background builds).
    let mut options = debrief_store_duck::StoreOptions::default();
    if let Some(n) = std::env::var("DEBRIEF_READY_THREADS").ok().and_then(|v| v.parse().ok()) {
        options.parse_threads = n;
    }
    if let Some((t, mb)) = std::env::var("DEBRIEF_READY_INDEX").ok().as_deref().and_then(|v| v.split_once(',')) {
        options.index = debrief_store_duck::WriterBudget {
            threads: t.parse().expect("DEBRIEF_READY_INDEX threads"),
            heap: mb.parse::<usize>().expect("DEBRIEF_READY_INDEX heap MB") << 20,
        };
    }
    // `DEBRIEF_READY_CATALOGUE_WORKERS` (sources the catalogue builds at the same time; 0 = automatic).
    if let Some(n) = std::env::var("DEBRIEF_READY_CATALOGUE_WORKERS").ok().and_then(|v| v.parse().ok()) {
        options.catalogue_workers = n;
    }
    // `DEBRIEF_READY_DUCK` (DuckDB threads).
    if let Some(n) = std::env::var("DEBRIEF_READY_DUCK").ok().and_then(|v| v.parse().ok()) {
        options.duck_threads = n;
    }
    let wait = std::env::var("DEBRIEF_READY_WAIT").unwrap_or_default();
    eprintln!(
        "ready: parse threads {}, index writer {:?}, DuckDB threads {}",
        options.parse_threads, options.index, options.duck_threads
    );
    // `DEBRIEF_READY_REINDEX=1`: no import; DEBRIEF_READY_STORE is a kept store whose index is removed and built again
    // from its segments (the index phase alone, the same segments every run; the catalogue builds beside it as after an
    // import).
    // `DEBRIEF_READY_RECATALOGUE=1`: no import; DEBRIEF_READY_STORE is a kept store whose catalogue is forgotten (the
    // `catalogue_sources` rows) and built again from its segments: the catalogue phase alone.
    if recatalogue {
        {
            let conn = duckdb::Connection::open(store_dir.join("data.duckdb")).unwrap();
            conn.execute_batch("DELETE FROM catalogue_sources").unwrap();
        }
        let started = Instant::now();
        let store = DuckStore::open_with(&store_dir, Profile::default_profile(), PluginRegistry::new(), options).unwrap();
        let built = store.wait_for_catalogue(Duration::from_secs(7200)).unwrap();
        let s = started.elapsed().as_secs_f64();
        eprintln!(
            "ready: recatalogue built={built} in {s:.1} s, peak RSS {} MB",
            sampler.take_peak_mb()
        );
        return;
    }
    if reindex {
        let _ = std::fs::remove_dir_all(store_dir.join("index"));
        let started = Instant::now();
        let store = DuckStore::open_with(&store_dir, Profile::default_profile(), PluginRegistry::new(), options).unwrap();
        let st = store.wait_for_index(Duration::from_secs(7200)).unwrap();
        let s = started.elapsed().as_secs_f64();
        let meta = std::fs::read_to_string(store_dir.join("index").join("meta.json")).unwrap_or_default();
        eprintln!(
            "ready: reindex {} of {} docs in {s:.1} s = {:.0} docs/s, peak RSS {} MB, index {:.0} MB, index segments {}",
            st.indexed,
            st.total,
            st.indexed as f64 / s,
            sampler.take_peak_mb(),
            dir_mb(&store_dir.join("index")),
            meta.matches("\"segment_id\"").count(),
        );
        search_times(&store);
        return;
    }
    let store = DuckStore::create_with(&store_dir, Profile::default_profile(), PluginRegistry::new(), options).unwrap();
    let r = store.open(std::slice::from_ref(&corpus)).unwrap().wait().unwrap();
    let import_s = secs();
    eprintln!(
        "ready: import {} events in {import_s:.1} s = {:.0} events/s, peak RSS {} MB; segments {:.0} MB",
        r.events,
        r.events as f64 / import_s,
        sampler.take_peak_mb(),
        dir_mb(&store_dir.join("segments")),
    );
    if wait == "index" {
        let st = store.wait_for_index(Duration::from_secs(7200)).unwrap();
        eprintln!(
            "ready: index {} of {} docs at {:.1} s ({:.0} docs/s after the import), peak RSS {} MB, index {:.0} MB",
            st.indexed,
            st.total,
            secs(),
            st.indexed as f64 / (secs() - import_s),
            sampler.take_peak_mb(),
            dir_mb(&store_dir.join("index")),
        );
        // What more writer threads cost the reads: the index's segments (Tantivy merges them in the background) and
        // word searches (p50 / p95 of 20 each) right after it is ready.
        let meta = std::fs::read_to_string(store_dir.join("index").join("meta.json")).unwrap_or_default();
        eprintln!("ready: index segments {}", meta.matches("\"segment_id\"").count());
        search_times(&store);
    }
    if wait == "import" || wait == "index" {
        drop(store);
        if std::env::var_os("DEBRIEF_READY_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&store_dir);
        }
        return;
    }
    // The index and the catalogue are built at the same time in the background; each is timed from the start.
    let (index, catalogue) = std::thread::scope(|s| {
        let index = s.spawn(|| {
            let st = store.wait_for_index(Duration::from_secs(7200)).unwrap();
            (st, secs())
        });
        let catalogue = s.spawn(|| {
            let built = store.wait_for_catalogue(Duration::from_secs(7200)).unwrap();
            (built, secs())
        });
        (index.join().unwrap(), catalogue.join().unwrap())
    });
    let ready_s = index.1.max(catalogue.1);
    eprintln!(
        "ready: index {} of {} docs at {:.1} s; catalogue built={} at {:.1} s; search ready at {ready_s:.1} s = {:.0} events/s \
         end to end; peak RSS after import {} MB; store {:.0} MB (segments {:.0} MB, index {:.0} MB, data.duckdb {:.0} MB)",
        index.0.indexed,
        index.0.total,
        index.1,
        catalogue.0,
        catalogue.1,
        r.events as f64 / ready_s,
        sampler.take_peak_mb(),
        dir_mb(&store_dir),
        dir_mb(&store_dir.join("segments")),
        dir_mb(&store_dir.join("index")),
        std::fs::metadata(store_dir.join("data.duckdb")).map_or(0.0, |m| m.len() as f64 / 1e6),
    );
    drop(store);
    if std::env::var_os("DEBRIEF_READY_KEEP").is_none() {
        let _ = std::fs::remove_dir_all(&store_dir);
    }
}

/// Word searches (p50 / p95 of 20 each) on a ready index.
fn search_times(store: &DuckStore) {
    for word in ["timeout", "declined", "cap_a0565ebc64cb", "contoso orders"] {
        let mut ms: Vec<f64> = (0..20)
            .map(|_| {
                let t = Instant::now();
                let r = store
                    .search(&SearchRequest {
                        text: word.into(),
                        limit: 0,
                    })
                    .unwrap();
                std::hint::black_box(r.total);
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        let hits = store
            .search(&SearchRequest {
                text: word.into(),
                limit: 0,
            })
            .unwrap()
            .total;
        eprintln!("ready: search {word:?}: {hits} hits, p50 {:.1} ms, p95 {:.1} ms", ms[10], ms[18]);
    }
}
