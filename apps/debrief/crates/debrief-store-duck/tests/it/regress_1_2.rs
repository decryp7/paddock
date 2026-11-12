//! Debrief 1.2 regression tests, store side (docs/plans/debrief-1-2.md, wave 0 package 0.3; ADR 0018 and 0051).
//!
//! Each test is written to fail today and is `#[ignore]`d with the failure it shows, until the work package that fixes
//! it removes the marker (3.5 for the `.gz`, 4.2 for the exception groups, P3 for the 1.6 M-row catalogue). The inputs are generated at test time into
//! a temp folder (invented rows, never committed, never real data) and the big one is slow, so the tests only run when
//! asked for: `cargo test --release -p debrief-store-duck --test it regress_1_2:: -- --ignored` (nightly/bench workflow).

use crate::common;
use common::rss::Sampler;
use common::TempDir;
use debrief_core::api::QueryRequest;
use debrief_core::catalogue::CatalogueRequest;
use debrief_core::ports::{Analysis, Ingest, Query};
use debrief_core::Profile;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{BufWriter, Write};

/// Worker budget while importing (ADR 0014 section 2).
const IMPORT_BUDGET_MB: u64 = 768;
/// What an exceptions view may return at most (ADR 0018 decision 1: every collection has a limit and a `truncated`
/// marker; 4.2 sets the real default, the test only needs "far below 50 000").
const GROUP_LIMIT: usize = 5_000;

/// A `.gz` of invented log lines that is `bytes` long once uncompressed (it compresses to a few percent of that).
fn generate_gz(path: &std::path::Path, bytes: u64) -> u64 {
    let mut out = BufWriter::new(GzEncoder::new(std::fs::File::create(path).unwrap(), Compression::fast()));
    let (mut written, mut n) = (0u64, 0u64);
    while written < bytes {
        let (s, m) = ((n / 1000) % 60, (n / 60_000) % 60);
        let level = ["INFO", "INFO", "INFO", "WARN", "ERROR"][(n % 5) as usize];
        let line = format!(
            "2026-10-14 10:{m:02}:{s:02}.{:03} {level} svc{} request {n} handled for tenant {} in {} ms\n",
            n % 1000,
            n % 17,
            n % 301,
            n % 997
        );
        out.write_all(line.as_bytes()).unwrap();
        written += line.len() as u64;
        n += 1;
    }
    out.into_inner().unwrap().finish().unwrap();
    n
}

#[test]
#[ignore = "regress-1-2: FAILING TODAY (fixed by plan package 3.5, streaming archives): 0 of 17960344 lines of the .gz were imported (the archive expansion cap stops it)"]
fn a_gz_of_1_5_gb_uncompressed_imports_under_the_import_budget() {
    let data = TempDir::new("regress-gz");
    let path = data.0.join("big.log.gz");
    let rows = generate_gz(&path, 1_500_000_000);
    let dir = TempDir::new("regress-gz-store");
    let sampler = Sampler::start();
    let (store, report) = common::store_with(&dir, Profile::default_profile(), std::slice::from_ref(&path)).unwrap();
    let peak = sampler.take_peak_mb();
    assert_eq!(report.events as u64, rows, "every line of the .gz was imported");
    assert_eq!(store.summary().unwrap().total, report.events);
    assert!(
        peak <= IMPORT_BUDGET_MB,
        "peak RSS {peak} MB importing a 1.5 GB (uncompressed) .gz is over the import budget of {IMPORT_BUDGET_MB} MB"
    );
}

/// Letters only, so the fingerprint (which collapses digits) keeps every type distinct.
fn word(mut n: usize) -> String {
    let mut s = String::new();
    loop {
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
        if n == 0 {
            return s;
        }
    }
}

#[test]
#[ignore = "regress-1-2: FAILING TODAY (fixed by plan package 4.2, bounded engine queries): the exceptions view returned 50000 groups; a view returns at most its limit (5000)"]
fn an_exceptions_view_with_50_000_groups_returns_at_most_its_limit_and_says_truncated() {
    let data = TempDir::new("regress-groups");
    let path = data.0.join("errors.clef");
    let mut out = BufWriter::new(std::fs::File::create(&path).unwrap());
    for i in 0..50_000usize {
        let w = word(i);
        writeln!(
            out,
            r#"{{"@t":"2026-10-14T10:{:02}:{:02}.000Z","@l":"Error","@mt":"Failed {w}","@x":"Acme.{w}Exception: failed {w}\n   at Acme.{w}.Run()"}}"#,
            (i / 60) % 60,
            i % 60
        )
        .unwrap();
    }
    out.flush().unwrap();
    drop(out);
    let dir = TempDir::new("regress-groups-store");
    let (store, _report) = common::store_with(&dir, Profile::default_profile(), std::slice::from_ref(&path)).unwrap();
    let req = QueryRequest { query: Default::default() };
    let answer = store.exception_groups(&req).unwrap();
    assert!(
        answer.result.len() <= GROUP_LIMIT,
        "the exceptions view returned {} groups; a view returns at most its limit ({GROUP_LIMIT})",
        answer.result.len()
    );
    let json = serde_json::to_value(&answer).unwrap();
    assert!(
        !json["truncated"]["limit"].is_null(),
        "the answer says it was truncated: {:?}",
        answer.coverage
    );
}

/// Rows of the permanent catalogue of a store that imported about 7.6 GB (docs/research/2026-10-28-cpu-memory.md, H3).
const CATALOGUE_ROWS: u64 = 1_600_000;

#[test]
fn a_1_6_million_row_catalogue_is_read_within_the_query_budget() {
    let dir = TempDir::new("regress-catalogue");
    let sample = common::log_samples();
    let (store, _report) = common::store_with(&dir, Profile::default_profile(), &sample).unwrap();
    assert!(
        store.wait_for_catalogue(std::time::Duration::from_secs(120)).unwrap(),
        "the samples are catalogued"
    );
    // Invented names spread over every source, standing in for the entities of a very large import: one row per source,
    // scope and name like the rows the catalogue workers write, long names like paths and service names.
    let sources: Vec<u32> = store.summary().unwrap().sources.iter().map(|s| s.id).collect();
    let n = sources.len() as u64;
    let scope = debrief_core::catalogue::Scope::Entity(debrief_core::entities::EntityKind::Executable).encode();
    store.bench_sql("DELETE FROM catalogue").unwrap();
    let list = sources.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
    store
        .bench_sql(&format!(
            "INSERT INTO catalogue SELECT [{list}][1 + i % {n}], '{scope}', 'C:\\Program Files\\Vendor Suite\\Component\\proc' || (i // {n})::VARCHAR || '.exe',
                'cprogramfilesvendorsuitecomponentproc' || (i // {n})::VARCHAR || 'exe', 1 + i % 97, NULL, NULL, i, i FROM range({CATALOGUE_ROWS}) t(i)"
        ))
        .unwrap();
    eprintln!("{n} sources, {CATALOGUE_ROWS} catalogue rows");
    let started = std::time::Instant::now();
    let reply = store.catalogue(&CatalogueRequest {
        all: true,
        ..CatalogueRequest::default()
    });
    let reply = reply.unwrap_or_else(|e| panic!("catalogue(all) over {CATALOGUE_ROWS} rows failed within the 192 MB query budget: {e}"));
    eprintln!(
        "catalogue(all) over {CATALOGUE_ROWS} rows: {} names in {:.2} s",
        reply.candidates.len(),
        started.elapsed().as_secs_f64()
    );
    assert!(reply.built, "the catalogue is built");
    assert!(
        reply.candidates.len() <= debrief_core::catalogue::MAX_NAMES,
        "{} candidates; a read returns at most {}",
        reply.candidates.len(),
        debrief_core::catalogue::MAX_NAMES
    );
}

/// CPU time (user plus system, seconds) of this process so far.
#[cfg(unix)]
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

/// Measurement (docs/performance.md): the statements and CPU time of an idle store, a store of the samples that has
/// built its catalogue and its index and is then left alone for `DEBRIEF_IDLE_S` seconds (default 130, two turns of the
/// background loops' 60 s backstop). Run with `--ignored --nocapture`.
#[cfg(unix)]
#[test]
#[ignore = "measurement: waits DEBRIEF_IDLE_S seconds (default 130); run with --ignored --nocapture"]
fn an_idle_store_statements_and_cpu() {
    let idle: u64 = std::env::var("DEBRIEF_IDLE_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(130)
        .max(10);
    let dir = TempDir::new("regress-idle");
    let (store, _report) = common::store_with(&dir, Profile::default_profile(), &common::log_samples()).unwrap();
    assert!(store.wait_for_catalogue(std::time::Duration::from_secs(120)).unwrap());
    assert!(store.wait_for_index(std::time::Duration::from_secs(120)).unwrap().ready);
    // The catalogue counts as built only once its checkpoint is saved, and the index is ready (both waited for above):
    // nothing is left to settle before the window opens.
    store.bench_sql("CALL enable_logging('QueryLog')").unwrap();
    let cpu = cpu_seconds();
    // conformance: hang-guard — the idle observation window (DEBRIEF_IDLE_S, default 130 s, clamped to at least 10 s) is the measurement itself, not a settle
    std::thread::sleep(std::time::Duration::from_secs(idle));
    let used = cpu_seconds() - cpu;
    let statements = store
        .bench_sql("SELECT message FROM duckdb_logs WHERE type = 'QueryLog' AND message NOT LIKE '%duckdb_logs%'")
        .unwrap();
    eprintln!("IDLE {idle} s: {statements} statements, {used:.3} CPU-s");
}

/// How long the build of [`building_the_catalogue_keeps_its_unsaved_rows_bounded`] may go without progress (ADR 0071,
/// `paddock_testkit::guarded`, as paddock-dev's live scans): the corpus written, the import's files and bytes, the catalogue's
/// committed rows. A slow host stretches the run, never this: 12 s quiet against 37 s locally at a load of about 20 and 46-61 s
/// on the CI Mac (607bb7928, CI 7961). nextest's slow-timeout (`.config/nextest.toml`, its waiver) is only the backstop.
// conformance: hang-guard
const BUILD_STALL: std::time::Duration = std::time::Duration::from_secs(60);

/// The service keeps the high-water mark of the rows committed and not yet checkpointed (a hook, [`RowStats`]), so the
/// bound is read exactly: a sample of the store's memory taken from outside can miss the peak and pass without the fix.
/// Without the mid-run checkpoint every row of the run stays unsaved and the mark is the total.
#[test]
fn building_the_catalogue_keeps_its_unsaved_rows_bounded() {
    let base = TempDir::new("regress-unsaved-corpus");
    let dir = TempDir::new("regress-unsaved");
    let (corpus_dir, store_dir) = (base.0.join("corpus"), dir.0.clone());
    // 100 000 events commit about 340 000 rows, over three times the bound, which is all the probe below needs (ER-33);
    // 300 000 committed ten times the bound and took 46-61 s on the CI Mac (CI 7961). The build is judged on its progress,
    // not on its total time: a loaded host makes it slower, never red.
    let stats = paddock_testkit::guarded("building the catalogue of 100 000 events", BUILD_STALL, move |progress| {
        let corpus = common::corpus::corpus(&corpus_dir, 100_000);
        progress.tick();
        let store =
            debrief_store_duck::DuckStore::create(&store_dir, Profile::default_profile(), debrief_core::PluginRegistry::new()).unwrap();
        let import = store.open(&[corpus]).unwrap();
        // The import's files and bytes; a new phase that starts its counters again counts as progress too.
        progress.follow(|| {
            let s = import.progress();
            let level = [s.done as u64, s.total as u64, s.bytes_done]
                .iter()
                .fold(0u64, |h, v| h.wrapping_mul(1_000_003).wrapping_add(*v));
            (level, import.is_finished())
        });
        import.wait().unwrap();
        progress.tick();
        // The catalogue's committed rows grow while it builds; each look also wakes the catalogue's thread.
        progress.follow(|| {
            let built = store.wait_for_catalogue(std::time::Duration::from_millis(1)).unwrap();
            (store.catalogue_row_stats().map_or(0, |s| s.committed), built)
        });
        store.catalogue_row_stats().expect("the store has a catalogue service")
    });
    eprintln!("{stats:?}");
    // The probe can see the thing (ER-33): the run wrote several times the bound, so an unbounded run would show it.
    assert!(
        stats.committed >= 2 * stats.checkpoint_after,
        "the corpus committed {} rows, too few to tell a bounded run from an unbounded one (bound {})",
        stats.committed,
        stats.checkpoint_after
    );
    // A checkpoint follows the commit that crossed the bound, so one commit more than the bound can be unsaved.
    assert!(
        stats.high_water <= stats.checkpoint_after + stats.largest_commit,
        "the catalogue held {} unsaved rows at once; the bound is {} plus the largest commit ({})",
        stats.high_water,
        stats.checkpoint_after,
        stats.largest_commit
    );
}
