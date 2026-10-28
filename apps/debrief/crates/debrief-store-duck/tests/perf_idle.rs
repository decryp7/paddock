//! CPU and memory after an import (research note `docs/research/2026-10-28-cpu-memory.md`): reopens a store that an
//! import wrote (`DEBRIEF_PERF_STORE=<folder>`), then times the background phases and the idle loops one after the
//! other, printing a `PHASE <name> <unix ms>` line at each boundary so an outside sampler (physical footprint and CPU
//! time of this process) can attribute its samples. Without `DEBRIEF_PERF_STORE` it does nothing.
//!
//! `DEBRIEF_PERF_IDLE_S` sets the idle and watcher windows (default 60 s).

mod common;

// The store worker's allocator, so the numbers are the shipped ones.
#[global_allocator]
static GLOBAL: debrief_alloc::MiMalloc = debrief_alloc::MiMalloc;

use debrief_core::catalogue::CatalogueRequest;
use debrief_core::names::Names;
use debrief_core::ports::Query;
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn phase(name: &str) {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    eprintln!("PHASE {name} {ms}");
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[test]
fn background_phases_and_idle_loops() {
    let Some(dir) = std::env::var_os("DEBRIEF_PERF_STORE").map(PathBuf::from) else {
        eprintln!("skipped: set DEBRIEF_PERF_STORE to a store folder an import wrote");
        return;
    };
    let idle = Duration::from_secs(std::env::var("DEBRIEF_PERF_IDLE_S").ok().and_then(|v| v.parse().ok()).unwrap_or(60));
    debrief_alloc::tune();
    eprintln!("PID {}", std::process::id());

    phase("open");
    let t = Instant::now();
    let store = DuckStore::open(&dir, Profile::default_profile(), PluginRegistry::new()).unwrap();
    eprintln!("open: {:.0} ms", ms(t.elapsed()));
    let total = store.summary().unwrap().total;
    eprintln!("events: {total}");

    phase("catalogue");
    let t = Instant::now();
    let built = store.wait_for_catalogue(Duration::from_secs(3600)).unwrap();
    eprintln!("catalogue built={built} in {:.0} ms", ms(t.elapsed()));

    phase("index");
    let t = Instant::now();
    let st = store.wait_for_index(Duration::from_secs(3600)).unwrap();
    eprintln!("index: {} docs in {:.0} ms", st.indexed, ms(t.elapsed()));

    for (name, sql) in [
        ("catalogue rows", "SELECT * FROM range((SELECT count(*) FROM catalogue))"),
        ("catalogue_trigrams rows", "SELECT * FROM range((SELECT count(*) FROM catalogue_trigrams))"),
        ("sources", "SELECT id FROM sources"),
        ("duckdb settings", "SELECT name, value FROM duckdb_settings() WHERE name IN ('memory_limit','threads','temp_directory','max_temp_directory_size','allocator_flush_threshold','allocator_background_threads','checkpoint_threshold')"),
    ] {
        let n = store.bench_sql(sql);
        eprintln!("{name}: {n:?}");
    }
    for row in [
        "SELECT name, value FROM duckdb_settings() WHERE name = 'memory_limit' AND value = '192.0 MiB'",
        "SELECT name, value FROM duckdb_settings() WHERE name = 'threads' AND value = '4'",
    ] {
        eprintln!("setting check {row}: {:?}", store.bench_sql(row));
    }

    // One tick of the catalogue thread when there is nothing to do (catalogue.rs `step`): three anti-join deletes
    // and the pending query, timed in isolation (median of 20).
    let tick_sql = [
        "DELETE FROM catalogue WHERE source NOT IN (SELECT id FROM sources)",
        "DELETE FROM catalogue_trigrams WHERE source NOT IN (SELECT id FROM sources)",
        "DELETE FROM catalogue_sources WHERE source NOT IN (SELECT id FROM sources)",
        "SELECT id FROM sources WHERE ready AND id NOT IN (SELECT source FROM catalogue_sources WHERE version = 1) ORDER BY id",
    ];
    let mut ticks = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        for q in tick_sql {
            store.bench_sql(q).unwrap();
        }
        ticks.push(ms(t.elapsed()));
    }
    ticks.sort_by(f64::total_cmp);
    eprintln!(
        "catalogue idle tick (3 deletes + pending): median {:.2} ms, max {:.2} ms",
        ticks[10], ticks[19]
    );

    // Idle: nothing asked of the store; only its own threads (catalogue and full-text loops, 500 ms fallbacks) run.
    phase("idle");
    std::thread::sleep(idle);
    phase("idle-end");

    // The name watcher while the catalogue is "not built" (app names.rs `watch_catalogue`): every 500 ms a summary
    // and the whole catalogue. Timed per call; run for the same window as idle.
    phase("watcher");
    let started = Instant::now();
    let (mut sum_ms, mut cat_ms, mut n, mut cands, mut failed) = (Vec::new(), Vec::new(), 0, 0, 0);
    while started.elapsed() < idle {
        let t = Instant::now();
        let _ = store.summary().unwrap();
        sum_ms.push(ms(t.elapsed()));
        let t = Instant::now();
        match store.catalogue(&CatalogueRequest {
            all: true,
            ..CatalogueRequest::default()
        }) {
            Ok(reply) => cands = reply.candidates.len(),
            Err(e) => {
                failed += 1;
                if failed == 1 {
                    eprintln!("catalogue(all) failed: {e}");
                }
            }
        }
        cat_ms.push(ms(t.elapsed()));
        n += 1;
        std::thread::sleep(Duration::from_millis(500));
    }
    phase("watcher-end");
    sum_ms.sort_by(f64::total_cmp);
    cat_ms.sort_by(f64::total_cmp);
    eprintln!(
        "watcher: {n} ticks ({failed} failed); summary median {:.1} ms; catalogue(all) median {:.1} ms max {:.1} ms; {cands} candidates",
        sum_ms[sum_ms.len() / 2],
        cat_ms[cat_ms.len() / 2],
        cat_ms[cat_ms.len() - 1]
    );

    // The name table rebuild, step by step (`Names::refresh` in debrief-core names/build.rs).
    'steps: {
        use debrief_core::names::build::vocabulary_from;
        use debrief_core::names::group::{group, look_alikes};
        use debrief_core::ports::{Filter, PageQuery};
        use debrief_core::query::TextMode;
        let t = Instant::now();
        let reply = match store.catalogue(&CatalogueRequest {
            all: true,
            ..CatalogueRequest::default()
        }) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("names steps: catalogue(all) failed after {:.0} ms: {e}", ms(t.elapsed()));
                break 'steps;
            }
        };
        let t_cat = t.elapsed();
        let t = Instant::now();
        let vocab = vocabulary_from(&reply.candidates);
        let groups = group(&reply.candidates, &vocab);
        let t_group = t.elapsed();
        let wanted = look_alikes(&groups, 200);
        let mut pages = Vec::new();
        for &i in wanted.iter().take(5) {
            let t = Instant::now();
            let _ = store.page(&PageQuery {
                query: Filter {
                    text: format!("\"{}\"", groups[i].canonical().replace('"', " ")),
                    text_mode: TextMode::Words,
                    ..Filter::default()
                },
                offset: 0,
                limit: 30,
                ..PageQuery::default()
            });
            pages.push(ms(t.elapsed()));
        }
        eprintln!(
            "names steps: catalogue(all) {:.0} ms ({} candidates); vocabulary+group {:.0} ms ({} groups); {} look-alikes; page query per look-alike {:?} ms",
            ms(t_cat), reply.candidates.len(), ms(t_group), groups.len(), wanted.len(), pages
        );
    }
    // The name table rebuild (`Names::refresh`): catalogue, grouping, co-occurrence merge.
    phase("names");
    let names = Names::default();
    let t = Instant::now();
    match names.refresh(&store) {
        Ok(groups) => eprintln!("names refresh: {groups} groups in {:.0} ms", ms(t.elapsed())),
        Err(e) => eprintln!("names refresh failed after {:.0} ms: {e}", ms(t.elapsed())),
    }
    phase("names-end");
}
