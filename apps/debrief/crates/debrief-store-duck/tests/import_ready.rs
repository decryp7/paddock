//! Time from "import folder" until search is ready, on a folder of logs (performance.md, "Import of a 7–8 GB folder"):
//!
//! `DEBRIEF_READY_CORPUS=<folder> DEBRIEF_READY_STORE=<new folder> cargo test --release --test import_ready -- --nocapture`
//!
//! Imports the folder into a new store and prints, from the start: the import (events, events/s, peak RSS), the full-text
//! index covering every segment and the catalogue built, and the store's size. Without `DEBRIEF_READY_CORPUS` it does
//! nothing. The store folder is removed afterwards unless `DEBRIEF_READY_KEEP` is set.

mod common;

// The store worker's allocator (`debrief-store-worker`), so the numbers are the shipped ones.
#[global_allocator]
static GLOBAL: debrief_alloc::MiMalloc = debrief_alloc::MiMalloc;

use common::rss::Sampler;
use debrief_core::ports::Ingest;
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    assert!(!store_dir.join("data.duckdb").exists(), "DEBRIEF_READY_STORE must be a new folder");
    let sampler = Sampler::start();
    let started = Instant::now();
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
    // `DEBRIEF_READY_CATALOGUE` (catalogue workers), `DEBRIEF_READY_DUCK` (DuckDB threads).
    if let Some(n) = std::env::var("DEBRIEF_READY_CATALOGUE").ok().and_then(|v| v.parse().ok()) {
        options.catalogue_workers = n;
    }
    if let Some(n) = std::env::var("DEBRIEF_READY_DUCK").ok().and_then(|v| v.parse().ok()) {
        options.duck_threads = n;
    }
    let wait = std::env::var("DEBRIEF_READY_WAIT").unwrap_or_default();
    eprintln!(
        "ready: parse threads {}, index writer {:?}, catalogue workers {}, DuckDB threads {}",
        options.parse_threads, options.index, options.catalogue_workers, options.duck_threads
    );
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
