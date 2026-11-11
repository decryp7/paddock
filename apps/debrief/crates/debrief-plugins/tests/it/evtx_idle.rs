//! What a person saw: Debrief idle after importing an Event Viewer log (user report 2026-10-28, `debrief-1-2-cpu-and-jobs`).
//! A generated `.evtx` (the repeated MIT/Apache-2.0 sample chunk, see `evtx_memory`) is imported through the Windows plugin
//! into the store the app ships, then the phases that follow an import are timed one after the other: the catalogue
//! and the index being built, the name table being rebuilt (what the app's name watcher does once the catalogue is
//! built), and an idle window. Each phase reports this process's CPU time (the plugin's own process is not counted) and,
//! for the idle window, the statements the store ran. Ignored: a measurement, run with `--ignored --nocapture`;
//! `DEBRIEF_EVTX_IDLE_MB` sets the log's size (default 64), `DEBRIEF_EVTX_IDLE_S` the idle window (default 60).

use crate::common;
use crate::evtx_memory::{generate, temp};
use debrief_core::catalogue::CatalogueRequest;
use debrief_core::names::Names;
use debrief_core::ports::{Ingest, Query};
use debrief_core::Profile;
use debrief_store_duck::DuckStore;
use std::time::{Duration, Instant};

/// CPU time (user plus system, seconds) of this process so far.
fn cpu_seconds() -> f64 {
    let mut u = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage fills the struct it is given; RUSAGE_SELF is always valid.
    let u = unsafe {
        libc::getrusage(libc::RUSAGE_SELF, u.as_mut_ptr());
        u.assume_init()
    };
    let t = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1e6;
    t(u.ru_utime) + t(u.ru_stime)
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Runs `f`, printing its wall and CPU time under `label`.
fn phase<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let (cpu, started) = (cpu_seconds(), Instant::now());
    let out = f();
    eprintln!(
        "PHASE {label}: {:.2} s wall, {:.2} CPU-s",
        started.elapsed().as_secs_f64(),
        cpu_seconds() - cpu
    );
    out
}

#[test]
#[ignore = "measurement: imports a generated event log and idles DEBRIEF_EVTX_IDLE_S seconds; run with --ignored --nocapture"]
fn idle_after_an_event_log_import() {
    let (mb, idle) = (env("DEBRIEF_EVTX_IDLE_MB", 64), env("DEBRIEF_EVTX_IDLE_S", 60));
    let data = temp("idle-data");
    let log = generate(&data.0, mb);
    let dir = temp("idle-store");
    let store = DuckStore::create(&dir.0, Profile::default_profile(), common::registry(&[common::WINDOWS])).unwrap();
    let report = phase("import", || store.open(std::slice::from_ref(&log)).unwrap().wait().unwrap());
    eprintln!("EVENTS {} from a {mb} MB log", report.events);
    assert!(report.events > 0, "the log was imported");
    phase("catalogue and index", || {
        assert!(store.wait_for_catalogue(Duration::from_secs(1800)).unwrap());
        assert!(store.wait_for_index(Duration::from_secs(1800)).unwrap().ready);
    });
    let names = Names::default();
    let groups = phase("names rebuild", || {
        // The phases of the rebuild, as the Jobs pane shows them, with the time each one started.
        let started = Instant::now();
        let last = std::cell::Cell::new(None);
        let ctl = debrief_core::names::RebuildControl {
            cancelled: &|| false,
            progress: &|p| {
                if last.replace(Some(p.phase)) != Some(p.phase) {
                    eprintln!(
                        "  REBUILD {:?} ({} items) starts at {:.2} s",
                        p.phase,
                        p.total,
                        started.elapsed().as_secs_f64()
                    );
                }
            },
        };
        match names.rebuild(&store, &ctl).unwrap() {
            debrief_core::names::RebuildEnd::Built { groups } => groups,
            other => panic!("the catalogue is built, yet the rebuild ended with {other:?}"),
        }
    });
    let candidates = store
        .catalogue(&CatalogueRequest {
            all: true,
            ..CatalogueRequest::default()
        })
        .unwrap()
        .candidates
        .len();
    eprintln!("NAMES {candidates} catalogue names, {groups} groups");
    // What one look-alike read costs, by the kind of text search (the rebuild reads the first events of each name).
    {
        use debrief_core::ports::{Filter, PageQuery};
        use debrief_core::query::TextMode;
        let sample = store
            .catalogue(&CatalogueRequest {
                all: true,
                ..CatalogueRequest::default()
            })
            .unwrap()
            .candidates
            .into_iter()
            .take(3)
            .map(|c| c.name)
            .collect::<Vec<_>>();
        for (label, mode, quote) in [("words (phrase)", TextMode::Words, true), ("substring", TextMode::Substring, false)] {
            let started = Instant::now();
            let mut rows = 0;
            for name in &sample {
                let text = if quote { format!("\"{name}\"") } else { name.clone() };
                let page = Query::page(
                    &store,
                    &PageQuery {
                        query: Filter {
                            text,
                            text_mode: mode,
                            ..Filter::default()
                        },
                        offset: 0,
                        limit: 30,
                        ..PageQuery::default()
                    },
                )
                .unwrap();
                rows += page.rows.len();
            }
            eprintln!(
                "READ {label}: {} reads, {rows} rows, {:.0} ms each",
                sample.len(),
                started.elapsed().as_secs_f64() * 1000.0 / sample.len() as f64
            );
        }
    }
    // The catalogue counts as built only once its checkpoint is saved, and the index only when it is ready (both waited
    // for above): nothing is left to settle before the window opens.
    store.bench_sql("CALL enable_logging('QueryLog')").unwrap();
    let cpu = cpu_seconds();
    // conformance: hang-guard — an idle observation window of 60 s or more (DEBRIEF_EVTX_IDLE_S, default 60 s) is the measurement itself, not a settle
    std::thread::sleep(Duration::from_secs(idle));
    let used = cpu_seconds() - cpu;
    let statements = store
        .bench_sql("SELECT message FROM duckdb_logs WHERE type = 'QueryLog' AND message NOT LIKE '%duckdb_logs%'")
        .unwrap();
    eprintln!(
        "IDLE {idle} s: {statements} statements, {used:.3} CPU-s ({:.3} % of a core)",
        used / idle as f64 * 100.0
    );
}
