//! Events-grid page benchmark (WP-1 of the Debrief 1.2 foundation; budget: interactive < 0.5 s, ADR 0010 and 0014).
//! Off unless `GRID_MODE` is set. Invented corpora only. Run with `--release --test bench_grid_page -- --nocapture`.
//!
//! - `GRID_MODE=build`: `GRID_STORE` (new folder) imports `GRID_MAIN`, waits for the index and the catalogue, stops. The
//!   folder is the "first import done" snapshot; clone it per run (`cp -cR`).
//! - `GRID_MODE=quiet`: opens `GRID_STORE`; with `GRID_EXTRA` imports that folder as the second import and waits for the
//!   index and the catalogue; then `GRID_RUNS` (default 21) page queries one after another.
//! - `GRID_MODE=during`: opens `GRID_STORE` and imports `GRID_EXTRA` while a thread runs the page query every 250 ms.
//! - `GRID_MODE=explain`: opens the catalogue of `GRID_STORE` with a plain connection (the store's memory limit and
//!   thread count) and prints `EXPLAIN ANALYZE` of the statements the page runs (`GRID_SQL` names one more).
//!
//! The query: `GRID_Q_FROM`/`GRID_Q_TO` (ms) and `GRID_Q_FIELD` = `GRID_Q_VALUE`. Every result line starts with `grid:`.

mod common;

#[global_allocator]
static GLOBAL: debrief_alloc::MiMalloc = debrief_alloc::MiMalloc;

use debrief_core::ports::{Filter, Ingest, PageQuery, Query};
use debrief_core::query::FieldFilter;
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"))
}

fn filter() -> Filter {
    Filter {
        from: Some(env("GRID_Q_FROM").parse().unwrap()),
        to: Some(env("GRID_Q_TO").parse().unwrap()),
        filters: vec![FieldFilter {
            field: env("GRID_Q_FIELD"),
            op: "eq".into(),
            value: Some(env("GRID_Q_VALUE")),
        }],
        ..Filter::default()
    }
}

fn page(s: &DuckStore, f: &Filter) -> (f64, usize) {
    let t = Instant::now();
    let r = s
        .page(&PageQuery {
            query: f.clone(),
            offset: 0,
            limit: 500,
            ..PageQuery::default()
        })
        .unwrap();
    (t.elapsed().as_secs_f64() * 1000.0, r.total)
}

fn stats(label: &str, mut ms: Vec<f64>, rows: usize) {
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if ms.is_empty() {
        println!("grid: {label} queries=0");
        return;
    }
    let p = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    println!(
        "grid: {label} queries={} p50_ms={:.1} p95_ms={:.1} max_ms={:.1} rows_last={rows}",
        ms.len(),
        p(0.5),
        p(0.95),
        ms[ms.len() - 1]
    );
}

struct Stderr;

impl log::Log for Stderr {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.target().starts_with("debrief_store_duck")
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) && std::env::var("GRID_TRACE").is_ok() {
            println!("grid: log {}", r.args());
        }
    }

    fn flush(&self) {}
}

fn open(dir: &std::path::Path) -> DuckStore {
    let _ = log::set_logger(&Stderr).map(|()| log::set_max_level(log::LevelFilter::Debug));
    DuckStore::open(dir, Profile::default_profile(), PluginRegistry::new()).unwrap()
}

#[test]
fn grid_page() {
    match std::env::var("GRID_MODE").as_deref() {
        Ok("build") => build(),
        Ok("quiet") => quiet(),
        Ok("during") => during(),
        Ok("explain") => explain(),
        _ => eprintln!("skipped: set GRID_MODE=build|quiet|during|explain"),
    }
}

fn build() {
    debrief_alloc::tune();
    let dir = PathBuf::from(env("GRID_STORE"));
    assert!(!dir.join(debrief_store_duck::DB_FILE).exists(), "GRID_STORE must be a new folder");
    let store = DuckStore::create(&dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
    let t = Instant::now();
    let r = store.open(&[PathBuf::from(env("GRID_MAIN"))]).unwrap().wait().unwrap();
    println!("grid: build import events={} s={:.1}", r.events, t.elapsed().as_secs_f64());
    store.wait_for_index(Duration::from_secs(7200)).unwrap();
    store.wait_for_catalogue(Duration::from_secs(7200)).unwrap();
    println!("grid: build done s={:.1}", t.elapsed().as_secs_f64());
}

fn quiet() {
    debrief_alloc::tune();
    let store = open(&PathBuf::from(env("GRID_STORE")));
    if let Ok(extra) = std::env::var("GRID_EXTRA") {
        store.open(&[PathBuf::from(extra)]).unwrap().wait().unwrap();
        store.wait_for_index(Duration::from_secs(7200)).unwrap();
        store.wait_for_catalogue(Duration::from_secs(7200)).unwrap();
    }
    let f = filter();
    let runs: usize = std::env::var("GRID_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(21);
    let (cold, rows) = page(&store, &f);
    println!("grid: first_query_ms={cold:.1} rows={rows}");
    let mut ms = Vec::new();
    let mut last = 0;
    for _ in 0..runs {
        let (m, r) = page(&store, &f);
        ms.push(m);
        last = r;
    }
    stats("quiet", ms, last);
}

fn during() {
    debrief_alloc::tune();
    let store = Arc::new(open(&PathBuf::from(env("GRID_STORE"))));
    let f = filter();
    let stop = Arc::new(AtomicBool::new(false));
    let samples = Arc::new(Mutex::new((Vec::new(), 0usize)));
    let probe = {
        let (s, stop, samples, f) = (store.clone(), stop.clone(), samples.clone(), f.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let (m, r) = page(&s, &f);
                let mut g = samples.lock().unwrap();
                g.0.push(m);
                g.1 = r;
                drop(g);
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    };
    let t = Instant::now();
    let r = store.open(&[PathBuf::from(env("GRID_EXTRA"))]).unwrap().wait().unwrap();
    stop.store(true, Ordering::Relaxed);
    probe.join().unwrap();
    println!("grid: during import events={} s={:.1}", r.events, t.elapsed().as_secs_f64());
    let g = samples.lock().unwrap();
    stats("during", g.0.clone(), g.1);
}

fn explain() {
    use duckdb::Connection;
    let dir = PathBuf::from(env("GRID_STORE"));
    // Opening the store points the `events` views at this folder's segments; then only the catalogue is used.
    drop(open(&dir));
    let threads = std::env::var("GRID_THREADS").unwrap_or_else(|_| "4".into());
    let limit = std::env::var("GRID_LIMIT").unwrap_or_else(|_| "192MB".into());
    let mut cfg = duckdb::Config::default()
        .max_memory(&limit)
        .and_then(|c| c.threads(threads.parse().unwrap()))
        .unwrap();
    // The store's other settings (open_db), each switchable: GRID_ASYNC (default 4, "none" leaves DuckDB's), GRID_FLUSH.
    let asy = std::env::var("GRID_ASYNC").unwrap_or_else(|_| "4".into());
    if asy != "none" {
        cfg = cfg.with("async_threads", asy).unwrap();
    }
    if let Ok(flush) = std::env::var("GRID_FLUSH") {
        cfg = cfg.with("allocator_flush_threshold", flush).unwrap();
    }
    let conn = Connection::open_with_flags(dir.join(debrief_store_duck::DB_FILE), cfg).unwrap();
    conn.execute_batch("SET GLOBAL parquet_metadata_cache = true").unwrap();
    if let Ok(set) = std::env::var("GRID_SET") {
        conn.execute_batch(&set).unwrap();
    }
    let text = std::fs::read_to_string(env("GRID_SQL")).unwrap();
    for stmt in text.split(";;") {
        let stmt = stmt.trim();
        if stmt.is_empty() {
            continue;
        }
        for i in 0..5 {
            let t = Instant::now();
            let rows: Vec<String> = if i == 4 {
                let mut st = conn.prepare(&format!("EXPLAIN ANALYZE {stmt}")).unwrap();
                st.query_map([], |r| r.get::<_, String>(1)).unwrap().map(|r| r.unwrap()).collect()
            } else {
                let mut st = conn.prepare(stmt).unwrap();
                let n = st.query_map([], |_| Ok(())).unwrap().count();
                vec![format!("{n} rows")]
            };
            println!(
                "grid: sql run={i} ms={:.1} threads={threads} limit={limit} :: {}",
                t.elapsed().as_secs_f64() * 1000.0,
                stmt[..stmt.len().min(90)].replace('\n', " ")
            );
            if i == 4 {
                println!("{}", rows.join("\n"));
            }
        }
    }
}
